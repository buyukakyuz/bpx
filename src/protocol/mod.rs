//! BPX protocol types and wire format definitions

use crate::{DiffFormat, ResourcePath, SessionId, Version};
use bytes::Bytes;
use std::time::Duration;

pub mod headers;
pub mod wire;

/// BPX request containing client state and preferences
#[derive(Debug, Clone)]
pub struct BpxRequest {
    /// Resource path being requested
    pub path: ResourcePath,
    /// Client session ID (None for first request)
    pub session_id: Option<SessionId>,
    /// Version client currently has
    pub base_version: Option<Version>,
    /// Diff formats client supports
    pub accepted_formats: Vec<DiffFormat>,
}

impl BpxRequest {
    /// Create a new BPX request
    pub fn new(path: ResourcePath) -> Self {
        Self {
            path,
            session_id: None,
            base_version: None,
            accepted_formats: vec![DiffFormat::BinaryDelta],
        }
    }

    /// Set session ID
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set base version
    pub fn with_base_version(mut self, version: Version) -> Self {
        self.base_version = Some(version);
        self
    }

    /// Set accepted diff formats
    pub fn with_formats(mut self, formats: Vec<DiffFormat>) -> Self {
        self.accepted_formats = formats;
        self
    }

    /// Check if client has state (session + base version)
    pub fn has_client_state(&self) -> bool {
        self.session_id.is_some() && self.base_version.is_some()
    }

    /// Get preferred diff format
    pub fn preferred_format(&self) -> Option<DiffFormat> {
        self.accepted_formats.first().copied()
    }
}

/// BPX response containing resource data or diff
#[derive(Debug, Clone)]
pub struct BpxResponse {
    /// Current resource version
    pub version: Version,
    /// Response body (full or diff)
    pub body: ResponseBody,
    /// Cache TTL hint for client
    pub cache_ttl: Option<Duration>,
    /// Session ID for client state tracking
    pub session_id: Option<SessionId>,
}

impl BpxResponse {
    /// Create response with full resource content
    pub fn full(version: Version, content: Bytes) -> Self {
        Self {
            version,
            body: ResponseBody::Full(content),
            cache_ttl: None,
            session_id: None,
        }
    }

    /// Create response with diff content
    pub fn diff(version: Version, format: DiffFormat, diff_data: Bytes) -> Self {
        Self {
            version,
            body: ResponseBody::Diff {
                format,
                data: diff_data,
            },
            cache_ttl: None,
            session_id: None,
        }
    }

    /// Set session ID for response
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set cache TTL
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = Some(ttl);
        self
    }

    /// Get the size of the response body
    pub fn body_size(&self) -> usize {
        match &self.body {
            ResponseBody::Full(data) => data.len(),
            ResponseBody::Diff { data, .. } => data.len(),
        }
    }

    /// Check if response contains a diff
    pub fn is_diff(&self) -> bool {
        matches!(self.body, ResponseBody::Diff { .. })
    }
}

/// Response body variants
#[derive(Debug, Clone)]
pub enum ResponseBody {
    /// Complete resource content
    Full(Bytes),
    /// Binary diff with format
    Diff {
        /// Diff format used
        format: DiffFormat,
        /// Diff data
        data: Bytes,
    },
}

impl ResponseBody {
    /// Get the raw bytes of the body
    pub fn as_bytes(&self) -> &Bytes {
        match self {
            Self::Full(data) => data,
            Self::Diff { data, .. } => data,
        }
    }

    /// Get the diff format if this is a diff response
    pub fn diff_format(&self) -> Option<DiffFormat> {
        match self {
            Self::Diff { format, .. } => Some(*format),
            Self::Full(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpx_request_builder() {
        let path = ResourcePath::new("/api/users/123".to_string());
        let session_id = SessionId::new("test_session".to_string());
        let version = Version::new("v1".to_string());

        let request = BpxRequest::new(path.clone())
            .with_session(session_id.clone())
            .with_base_version(version.clone())
            .with_formats(vec![DiffFormat::BinaryDelta, DiffFormat::JsonPatch]);

        assert_eq!(request.path, path);
        assert_eq!(request.session_id, Some(session_id));
        assert_eq!(request.base_version, Some(version));
        assert_eq!(request.accepted_formats.len(), 2);
        assert!(request.has_client_state());
        assert_eq!(request.preferred_format(), Some(DiffFormat::BinaryDelta));
    }

    #[test]
    fn test_bpx_response_creation() {
        let version = Version::new("v2".to_string());
        let content = Bytes::from("test content");
        let session_id = SessionId::new("session123".to_string());

        // Test full response
        let full_response = BpxResponse::full(version.clone(), content.clone())
            .with_session(session_id.clone())
            .with_cache_ttl(Duration::from_secs(300));

        assert_eq!(full_response.version, version);
        assert!(!full_response.is_diff());
        assert_eq!(full_response.body_size(), content.len());
        assert_eq!(full_response.session_id, Some(session_id.clone()));
        assert_eq!(full_response.cache_ttl, Some(Duration::from_secs(300)));

        // Test diff response
        let diff_data = Bytes::from("diff data");
        let diff_response =
            BpxResponse::diff(version.clone(), DiffFormat::BinaryDelta, diff_data.clone());

        assert!(diff_response.is_diff());
        assert_eq!(diff_response.body_size(), diff_data.len());
        assert_eq!(
            diff_response.body.diff_format(),
            Some(DiffFormat::BinaryDelta)
        );
    }

    #[test]
    fn test_request_without_state() {
        let path = ResourcePath::new("/api/test".to_string());
        let request = BpxRequest::new(path);

        assert!(!request.has_client_state());
        assert_eq!(request.preferred_format(), Some(DiffFormat::BinaryDelta));
    }
}
