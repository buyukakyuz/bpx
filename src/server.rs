//! HTTP/2 server implementation for BPX

use crate::{
    BpxConfig, BpxError, DiffEngine, DiffFormat, ResourcePath, SessionId, StateManager, Version,
    protocol::{BpxRequest, BpxResponse, ResponseBody, headers::BpxHeaders},
};
use async_trait::async_trait;
use bytes::Bytes;
use hyper::{Request, Response};
use std::sync::Arc;

/// BPX HTTP request handler
pub async fn handle_bpx_request<B, R>(
    req: Request<B>,
    config: &BpxConfig,
    state_mgr: Arc<dyn StateManager>,
    diff_engine: Arc<dyn DiffEngine>,
    resource_store: Arc<R>,
) -> Result<Response<Bytes>, BpxError>
where
    B: http_body::Body + Send + 'static,
    R: ResourceStore + 'static,
{
    // Parse BPX headers from request
    let bpx_request = parse_bpx_request(&req)?;

    // Fetch current resource
    let current_content = resource_store.get_resource(&bpx_request.path).await?;

    let current_version = Version::from_content(&current_content);

    // Get or create session
    let session_id = state_mgr
        .get_or_create_session(bpx_request.session_id.clone())
        .await;

    // Determine if client accepts any server-supported diff format (binary-delta for now)
    let client_accepts_binary = bpx_request
        .accepted_formats
        .iter()
        .any(|f| matches!(f, DiffFormat::BinaryDelta));

    // Check if client has compatible state and we should send diff
    let should_send_diff = if let Some(base_version) = &bpx_request.base_version {
        // Client has state, check if we can compute diff
        if let Some(stored_version) = state_mgr.get_version(&session_id, &bpx_request.path).await {
            // Only send diff if client's base version matches what we have stored
            // AND the current content is actually different
            let versions_match = &stored_version == base_version;
            let content_changed = &stored_version != &current_version;

            versions_match && content_changed && client_accepts_binary
        } else {
            false
        }
    } else {
        false
    };

    let response = if should_send_diff {
        let base_version = bpx_request.base_version.as_ref().unwrap();

        match resource_store
            .get_resource_version(&bpx_request.path, base_version)
            .await
        {
            Ok(base_content) => {
                // Enforce max_diff_size: if either side exceeds threshold, send full
                if base_content.len() > config.max_diff_size
                    || current_content.len() > config.max_diff_size
                {
                    BpxResponse::full(current_version.clone(), current_content.clone())
                        .with_session(session_id.clone())
                } else {
                    // Compute diff between base and current content
                    match diff_engine.compute_diff(&base_content, &current_content) {
                        Ok(diff_data) => {
                            if diff_engine
                                .is_diff_worthwhile(current_content.len(), diff_data.len())
                            {
                                // Negotiated format is binary-delta for now
                                BpxResponse::diff(
                                    current_version.clone(),
                                    DiffFormat::BinaryDelta,
                                    diff_data,
                                )
                                .with_session(session_id.clone())
                            } else {
                                BpxResponse::full(current_version.clone(), current_content.clone())
                                    .with_session(session_id.clone())
                            }
                        }
                        Err(e) => {
                            eprintln!("Diff computation failed: {}", e);
                            BpxResponse::full(current_version.clone(), current_content.clone())
                                .with_session(session_id.clone())
                        }
                    }
                }
            }
            Err(_) => BpxResponse::full(current_version.clone(), current_content.clone())
                .with_session(session_id.clone()),
        }
    } else {
        // Send full content
        BpxResponse::full(current_version.clone(), current_content.clone())
            .with_session(session_id.clone())
    };

    // Update stored version for future requests (store both in state manager and resource store)
    state_mgr
        .set_version(&session_id, &bpx_request.path, current_version.clone())
        .await;

    // Store current content version in resource store for future diff operations
    resource_store.store_version(
        bpx_request.path.clone(),
        current_version.clone(),
        current_content.clone(),
    );

    Ok(build_http_response_with_original_size(
        response,
        current_content.len(),
    ))
}

/// Parse BPX request from HTTP headers
fn parse_bpx_request<B>(req: &Request<B>) -> Result<BpxRequest, BpxError> {
    let path = ResourcePath::new(req.uri().path().to_string());
    let mut bpx_request = BpxRequest::new(path);

    // Parse session header
    if let Some(session_header) = req.headers().get(BpxHeaders::SESSION) {
        if let Ok(session_str) = session_header.to_str() {
            bpx_request = bpx_request.with_session(SessionId::new(session_str.to_string()));
        }
    }

    // Parse base version header
    if let Some(version_header) = req.headers().get(BpxHeaders::BASE_VERSION) {
        if let Ok(version_str) = version_header.to_str() {
            bpx_request = bpx_request.with_base_version(Version::new(version_str.to_string()));
        }
    }

    // Parse accepted diff formats
    if let Some(accept_header) = req.headers().get(BpxHeaders::ACCEPT_DIFF) {
        if let Ok(formats_str) = accept_header.to_str() {
            let formats: Vec<DiffFormat> = formats_str
                .split(',')
                .filter_map(|s| DiffFormat::from_str(s.trim()))
                .collect();
            if !formats.is_empty() {
                bpx_request = bpx_request.with_formats(formats);
            }
        }
    }

    Ok(bpx_request)
}

/// Build HTTP response from BPX response with original size info
fn build_http_response_with_original_size(
    bpx_response: BpxResponse,
    original_size: usize,
) -> Response<Bytes> {
    let mut response = Response::builder().header(
        BpxHeaders::RESOURCE_VERSION,
        bpx_response.version.to_string(),
    );

    if let Some(session_id) = &bpx_response.session_id {
        response = response.header(BpxHeaders::SESSION, session_id.to_string());
    }

    match &bpx_response.body {
        ResponseBody::Full(content) => {
            response = response
                .header(BpxHeaders::DIFF_TYPE, "full")
                .header(BpxHeaders::ORIGINAL_SIZE, content.len().to_string());
        }
        ResponseBody::Diff { format, data } => {
            response = response
                .header(BpxHeaders::DIFF_TYPE, format.as_str())
                .header(BpxHeaders::ORIGINAL_SIZE, original_size.to_string())
                .header(BpxHeaders::DIFF_SIZE, data.len().to_string());
        }
    }

    if let Some(cache_ttl) = bpx_response.cache_ttl {
        response = response.header(BpxHeaders::CACHE_TTL, cache_ttl.as_secs().to_string());
    }

    response
        .body(bpx_response.body.as_bytes().clone())
        .unwrap_or_else(|_| Response::new(Bytes::new()))
}

/// Trait for accessing resource storage
#[async_trait]
pub trait ResourceStore: Send + Sync {
    /// Get current version of a resource
    async fn get_resource(&self, path: &ResourcePath) -> Result<Bytes, BpxError>;

    /// Get specific version of a resource
    async fn get_resource_version(
        &self,
        path: &ResourcePath,
        version: &Version,
    ) -> Result<Bytes, BpxError>;

    /// Store a specific version of a resource
    fn store_version(&self, path: ResourcePath, version: Version, content: Bytes);
}

/// In-memory resource store implementation
pub struct InMemoryResourceStore {
    resources: dashmap::DashMap<String, Bytes>,
    versions: dashmap::DashMap<String, dashmap::DashMap<String, Bytes>>,
}

impl InMemoryResourceStore {
    /// Create a new in-memory resource store
    pub fn new() -> Self {
        Self {
            resources: dashmap::DashMap::new(),
            versions: dashmap::DashMap::new(),
        }
    }

    /// Set a resource's current content
    pub fn set_resource(&self, path: ResourcePath, content: Bytes) {
        self.resources.insert(path.to_string(), content);
    }

    /// Store a specific version of a resource
    pub fn store_version(&self, path: ResourcePath, version: Version, content: Bytes) {
        let path_str = path.to_string();
        let version_str = version.to_string();

        self.versions
            .entry(path_str)
            .or_insert_with(dashmap::DashMap::new)
            .insert(version_str, content);
    }

    /// Get all stored versions for a resource
    pub fn get_versions(&self, path: &ResourcePath) -> Vec<Version> {
        if let Some(versions) = self.versions.get(&path.to_string()) {
            versions
                .iter()
                .map(|entry| Version::new(entry.key().clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Remove a resource and all its versions
    pub fn remove_resource(&self, path: &ResourcePath) {
        let path_str = path.to_string();
        self.resources.remove(&path_str);
        self.versions.remove(&path_str);
    }

    /// Get the total number of resources
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Get the total number of stored versions across all resources
    pub fn version_count(&self) -> usize {
        self.versions.iter().map(|entry| entry.value().len()).sum()
    }

    /// Get current resource content (for demo purposes)
    pub fn get_current_resource(&self, path: &ResourcePath) -> Option<Bytes> {
        self.resources
            .get(&path.to_string())
            .map(|entry| entry.value().clone())
    }
}

impl Default for InMemoryResourceStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResourceStore for InMemoryResourceStore {
    async fn get_resource(&self, path: &ResourcePath) -> Result<Bytes, BpxError> {
        self.resources
            .get(&path.to_string())
            .map(|entry| entry.value().clone())
            .ok_or_else(|| BpxError::ClientStateNotFound {
                client_id: SessionId::new(format!("resource:{}", path)),
            })
    }

    async fn get_resource_version(
        &self,
        path: &ResourcePath,
        version: &Version,
    ) -> Result<Bytes, BpxError> {
        let path_str = path.to_string();
        let version_str = version.to_string();

        if let Some(versions) = self.versions.get(&path_str) {
            versions
                .get(&version_str)
                .map(|entry| entry.value().clone())
                .ok_or_else(|| BpxError::ClientStateNotFound {
                    client_id: SessionId::new(format!("{}@{}", path, version)),
                })
        } else {
            Err(BpxError::ClientStateNotFound {
                client_id: SessionId::new(format!("{}@{}", path, version)),
            })
        }
    }

    fn store_version(&self, path: ResourcePath, version: Version, content: Bytes) {
        Self::store_version(self, path, version, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bpx_request() {
        let req = Request::builder()
            .uri("/api/test")
            .header("X-BPX-Session", "sess_123")
            .header("X-Base-Version", "v:456")
            .header("Accept-Diff", "binary-delta,json-patch")
            .body(())
            .unwrap();

        let bpx_req = parse_bpx_request(&req).unwrap();

        assert_eq!(bpx_req.path.to_string(), "/api/test");
        assert_eq!(bpx_req.session_id.as_ref().unwrap().to_string(), "sess_123");
        assert_eq!(bpx_req.base_version.as_ref().unwrap().to_string(), "v:456");
        assert_eq!(bpx_req.accepted_formats.len(), 2);
        assert_eq!(bpx_req.preferred_format(), Some(DiffFormat::BinaryDelta));
    }

    #[test]
    fn test_parse_bpx_request_minimal() {
        let req = Request::builder().uri("/api/minimal").body(()).unwrap();

        let bpx_req = parse_bpx_request(&req).unwrap();
        assert_eq!(bpx_req.path.to_string(), "/api/minimal");
        assert!(bpx_req.session_id.is_none());
        assert!(bpx_req.base_version.is_none());
        assert_eq!(bpx_req.accepted_formats, vec![DiffFormat::BinaryDelta]); // default
    }

    #[test]
    fn test_parse_bpx_request_invalid_headers() {
        let req = Request::builder()
            .uri("/api/test")
            .header("X-BPX-Session", "sess_123")
            .header("X-Base-Version", "v:456")
            .header("Accept-Diff", "invalid-format,json-patch")
            .body(())
            .unwrap();

        let bpx_req = parse_bpx_request(&req).unwrap();

        // Should ignore invalid format and keep valid ones
        assert_eq!(bpx_req.accepted_formats.len(), 1);
        assert_eq!(bpx_req.preferred_format(), Some(DiffFormat::JsonPatch));
    }

    #[tokio::test]
    async fn test_resource_store_basic_operations() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/users".to_string());
        let content = Bytes::from("user data");

        // Initially empty
        assert_eq!(store.resource_count(), 0);
        assert!(store.get_current_resource(&path).is_none());

        // Set resource
        store.set_resource(path.clone(), content.clone());
        assert_eq!(store.resource_count(), 1);
        assert_eq!(store.get_current_resource(&path), Some(content.clone()));

        // Get via trait method
        let retrieved = store.get_resource(&path).await.unwrap();
        assert_eq!(retrieved, content);
    }
    #[tokio::test]
    async fn test_resource_store_versioning() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/data".to_string());
        let v1_content = Bytes::from("version 1");
        let v2_content = Bytes::from("version 2");
        let version1 = Version::new("v1".to_string());
        let version2 = Version::new("v2".to_string());

        // Store versions
        store.store_version(path.clone(), version1.clone(), v1_content.clone());
        store.store_version(path.clone(), version2.clone(), v2_content.clone());

        assert_eq!(store.version_count(), 2);
        assert_eq!(store.get_versions(&path).len(), 2);

        // Retrieve specific versions
        let retrieved_v1 = store.get_resource_version(&path, &version1).await.unwrap();
        let retrieved_v2 = store.get_resource_version(&path, &version2).await.unwrap();

        assert_eq!(retrieved_v1, v1_content);
        assert_eq!(retrieved_v2, v2_content);
    }

    #[tokio::test]
    async fn test_resource_store_multiple_resources() {
        let store = InMemoryResourceStore::new();
        let path1 = ResourcePath::new("/api/users".to_string());
        let path2 = ResourcePath::new("/api/orders".to_string());
        let content1 = Bytes::from("users data");
        let content2 = Bytes::from("orders data");

        store.set_resource(path1.clone(), content1.clone());
        store.set_resource(path2.clone(), content2.clone());

        assert_eq!(store.resource_count(), 2);
        assert_eq!(store.get_resource(&path1).await.unwrap(), content1);
        assert_eq!(store.get_resource(&path2).await.unwrap(), content2);
    }

    #[tokio::test]
    async fn test_resource_store_overwrite() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/test".to_string());
        let old_content = Bytes::from("old content");
        let new_content = Bytes::from("new content");

        // Set initial content
        store.set_resource(path.clone(), old_content);
        assert_eq!(store.resource_count(), 1);

        // Overwrite with new content
        store.set_resource(path.clone(), new_content.clone());
        assert_eq!(store.resource_count(), 1); // Still one resource
        assert_eq!(store.get_resource(&path).await.unwrap(), new_content);
    }

    #[tokio::test]
    async fn test_resource_store_remove() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/test".to_string());
        let content = Bytes::from("test content");
        let version = Version::new("v1".to_string());

        // Set resource and version
        store.set_resource(path.clone(), content.clone());
        store.store_version(path.clone(), version.clone(), content);

        assert_eq!(store.resource_count(), 1);
        assert_eq!(store.version_count(), 1);

        // Remove resource
        store.remove_resource(&path);

        assert_eq!(store.resource_count(), 0);
        assert_eq!(store.version_count(), 0);
        assert!(store.get_current_resource(&path).is_none());
    }

    #[tokio::test]
    async fn test_resource_store_error_cases() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/nonexistent".to_string());
        let version = Version::new("v1".to_string());

        // Get non-existent resource should error
        let result = store.get_resource(&path).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BpxError::ClientStateNotFound { .. }
        ));

        // Get non-existent version should error
        let result = store.get_resource_version(&path, &version).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BpxError::ClientStateNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_resource_store_version_not_found() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/test".to_string());
        let content = Bytes::from("test content");
        let existing_version = Version::new("v1".to_string());
        let missing_version = Version::new("v2".to_string());

        // Store one version
        store.store_version(path.clone(), existing_version, content);

        // Try to get missing version should error
        let result = store.get_resource_version(&path, &missing_version).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BpxError::ClientStateNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_resource_store_store_version_via_trait() {
        let store = InMemoryResourceStore::new();
        let path = ResourcePath::new("/api/test".to_string());
        let v1 = Version::new("v1".to_string());
        let content = Bytes::from("v1 content");

        // Store via trait method and then retrieve
        ResourceStore::store_version(&store, path.clone(), v1.clone(), content.clone());
        let retrieved = store.get_resource_version(&path, &v1).await.unwrap();
        assert_eq!(retrieved, content);
    }
}
