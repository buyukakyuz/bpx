//! # Binary Patch Exchange (BPX)
//!
//! A bandwidth-optimized HTTP/2 protocol that tracks client state server-side and transmits
//! only binary diffs instead of full payloads, reducing bandwidth usage for
//! frequently-polled resources while maintaining REST's simplicity.
//!
//! ## Core Components
//!
//! - [`BpxServer`] - Main server implementation
//! - [`DiffEngine`] - Binary diff computation and application
//! - [`StateManager`] - Client state tracking and management
//! - [`BpxConfig`] - Configuration options
//!
//! ## Example Usage
//!
//! ```rust
//! use bpx::{BpxServer, BpxConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let server = BpxServer::builder()
//!     .config(BpxConfig::default())
//!     .build()?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

use bytes::Bytes;
use dashmap::DashMap;
use hyper::{Request, Response};
use std::{
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, Instant},
};
use thiserror::Error;

pub mod diff;
pub mod protocol;
pub mod server;
pub mod state;

pub use diff::DiffEngine;
pub use protocol::{BpxRequest, BpxResponse, ResponseBody};
pub use server::{InMemoryResourceStore, ResourceStore};
pub use state::StateManager;

/// Session identifier for tracking client state
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Create a new session ID
    pub fn new(id: String) -> Self {
        Self(id)
    }

    /// Generate a random session ID
    pub fn generate() -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        Self(format!("sess_{:x}", hasher.finish()))
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Resource path for identifying resources within sessions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourcePath(String);

impl ResourcePath {
    /// Create a new resource path
    pub fn new(path: String) -> Self {
        Self(path)
    }
}

impl std::fmt::Display for ResourcePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Version identifier for tracking resource versions
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(String);

impl Version {
    /// Create a new version
    pub fn new(version: String) -> Self {
        Self(version)
    }

    /// Generate version from content hash
    pub fn from_content(content: &[u8]) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Self(format!("v:{:x}", hasher.finish()))
    }

    /// Generate version from timestamp
    pub fn from_timestamp() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self(format!("v:{}", timestamp))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Supported diff formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFormat {
    /// Binary delta format (most efficient)
    BinaryDelta,
    /// JSON patch format (RFC 6902)
    JsonPatch,
    /// BSD diff format
    BsdDiff,
}

impl DiffFormat {
    /// Parse diff format from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "binary-delta" => Some(Self::BinaryDelta),
            "json-patch" => Some(Self::JsonPatch),
            "bsdiff" => Some(Self::BsdDiff),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BinaryDelta => "binary-delta",
            Self::JsonPatch => "json-patch",
            Self::BsdDiff => "bsdiff",
        }
    }
}

/// Client session for tracking resource versions and state
pub struct BpxSession {
    /// Unique session identifier
    pub id: SessionId,
    /// Resource versions tracked for this session
    pub resources: DashMap<ResourcePath, Version>,
    /// Last access time for TTL enforcement
    pub last_accessed: Instant,
    /// Current memory usage in bytes
    pub memory_usage: AtomicUsize,
}

impl BpxSession {
    /// Create a new session
    pub fn new(id: SessionId) -> Self {
        Self {
            id,
            resources: DashMap::new(),
            last_accessed: Instant::now(),
            memory_usage: AtomicUsize::new(0),
        }
    }

    /// Update last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// Check if session has expired
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.last_accessed.elapsed() > ttl
    }
}

/// Configuration for BPX server
#[derive(Debug, Clone)]
pub struct BpxConfig {
    /// Maximum sessions to track concurrently
    pub max_sessions: usize,
    /// Maximum resources tracked per session
    pub max_resources_per_session: usize,
    /// Session TTL
    pub session_ttl: Duration,
    /// Maximum size of resource to diff (larger returns full)
    pub max_diff_size: usize,
    /// Minimum compression ratio to use diff
    pub min_compression_ratio: f32,
    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for BpxConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100_000,
            max_resources_per_session: 1_000,
            session_ttl: Duration::from_secs(24 * 60 * 60), // 24 hours
            max_diff_size: 10 * 1024 * 1024,                // 10MB
            min_compression_ratio: 0.2,                     // 80% savings
            cleanup_interval: Duration::from_secs(5 * 60),  // 5 minutes
        }
    }
}

/// Main BPX errors
#[derive(Debug, Error)]
pub enum BpxError {
    /// Client state not found
    #[error("Client state not found: {client_id}")]
    ClientStateNotFound {
        /// Client identifier
        client_id: SessionId,
    },

    /// Diff computation failed
    #[error("Diff computation failed: {reason}")]
    DiffComputationFailed {
        /// Failure reason
        reason: String,
    },

    /// Resource too large for diffing
    #[error("Resource too large: {size} bytes (max: {max_size})")]
    ResourceTooLarge {
        /// Actual size
        size: usize,
        /// Maximum allowed size
        max_size: usize,
    },

    /// Invalid diff format
    #[error("Invalid diff format: {format}")]
    InvalidDiffFormat {
        /// Requested format
        format: String,
    },

    /// Session capacity exceeded
    #[error("Session capacity exceeded: {current} sessions (max: {max})")]
    SessionCapacityExceeded {
        /// Current session count
        current: usize,
        /// Maximum allowed
        max: usize,
    },
}

/// BPX server implementation
pub struct BpxServer {
    config: BpxConfig,
    state_manager: Arc<dyn StateManager>,
    diff_engine: Arc<dyn DiffEngine>,
}

impl BpxServer {
    /// Create a new BPX server builder
    pub fn builder() -> BpxServerBuilder {
        BpxServerBuilder::new()
    }

    /// Handle a BPX request
    pub async fn handle_request<B, R>(
        &self,
        req: Request<B>,
        resource_store: Arc<R>,
    ) -> Result<Response<Bytes>, BpxError>
    where
        B: http_body::Body + Send + 'static,
        R: ResourceStore + 'static,
    {
        server::handle_bpx_request(
            req,
            &self.config,
            Arc::clone(&self.state_manager),
            Arc::clone(&self.diff_engine),
            resource_store,
        )
        .await
    }

    /// Get server configuration
    pub fn config(&self) -> &BpxConfig {
        &self.config
    }

    /// Get state manager reference
    pub fn state_manager(&self) -> &Arc<dyn StateManager> {
        &self.state_manager
    }

    /// Get diff engine reference
    pub fn diff_engine(&self) -> &Arc<dyn DiffEngine> {
        &self.diff_engine
    }

    /// Perform cleanup of expired sessions
    pub async fn cleanup_expired_sessions(&self) {
        self.state_manager.cleanup_expired().await;
    }
}

/// Builder for configuring BPX server
pub struct BpxServerBuilder {
    config: Option<BpxConfig>,
    state_manager: Option<Arc<dyn StateManager>>,
    diff_engine: Option<Arc<dyn DiffEngine>>,
}

impl BpxServerBuilder {
    fn new() -> Self {
        Self {
            config: None,
            state_manager: None,
            diff_engine: None,
        }
    }

    /// Set server configuration
    pub fn config(mut self, config: BpxConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set state manager implementation
    pub fn state_manager(mut self, state_manager: Arc<dyn StateManager>) -> Self {
        self.state_manager = Some(state_manager);
        self
    }

    /// Set diff engine implementation
    pub fn diff_engine(mut self, diff_engine: Arc<dyn DiffEngine>) -> Self {
        self.diff_engine = Some(diff_engine);
        self
    }

    /// Build the BPX server
    pub fn build(self) -> Result<BpxServer, BpxError> {
        let config = self.config.unwrap_or_default();

        let state_manager = self
            .state_manager
            .ok_or_else(|| BpxError::DiffComputationFailed {
                reason: "State manager not provided".to_string(),
            })?;

        let diff_engine = self
            .diff_engine
            .ok_or_else(|| BpxError::DiffComputationFailed {
                reason: "Diff engine not provided".to_string(),
            })?;

        Ok(BpxServer {
            config,
            state_manager,
            diff_engine,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_generation() {
        let id1 = SessionId::generate();
        let id2 = SessionId::generate();
        assert_ne!(id1, id2);
        assert!(id1.to_string().starts_with("sess_"));
    }

    #[test]
    fn test_version_from_content() {
        let content1 = b"hello world";
        let content2 = b"hello world";
        let content3 = b"hello world!";

        let v1 = Version::from_content(content1);
        let v2 = Version::from_content(content2);
        let v3 = Version::from_content(content3);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
        assert!(v1.to_string().starts_with("v:"));
    }

    #[test]
    fn test_diff_format_parsing() {
        assert_eq!(
            DiffFormat::from_str("binary-delta"),
            Some(DiffFormat::BinaryDelta)
        );
        assert_eq!(
            DiffFormat::from_str("json-patch"),
            Some(DiffFormat::JsonPatch)
        );
        assert_eq!(DiffFormat::from_str("bsdiff"), Some(DiffFormat::BsdDiff));
        assert_eq!(DiffFormat::from_str("invalid"), None);
    }

    #[test]
    fn test_session_expiration() {
        let mut session = BpxSession::new(SessionId::new("test".to_string()));
        let ttl = Duration::from_millis(100);

        assert!(!session.is_expired(ttl));

        // Manually set last_accessed to simulate expiration
        session.last_accessed = Instant::now() - Duration::from_millis(200);
        assert!(session.is_expired(ttl));
    }

    #[test]
    fn test_default_config() {
        let config = BpxConfig::default();
        assert_eq!(config.max_sessions, 100_000);
        assert_eq!(config.max_resources_per_session, 1_000);
        assert_eq!(config.session_ttl, Duration::from_secs(24 * 60 * 60));
        assert_eq!(config.max_diff_size, 10 * 1024 * 1024);
        assert_eq!(config.min_compression_ratio, 0.2);
        assert_eq!(config.cleanup_interval, Duration::from_secs(5 * 60));
    }

    #[test]
    fn test_bpx_server_builder_with_components() {
        use crate::diff::similar::SimilarDiffEngine;
        use crate::state::InMemoryStateManager;

        let config = BpxConfig::default();
        let state_manager: Arc<dyn StateManager> =
            Arc::new(InMemoryStateManager::new(config.clone()));
        let diff_engine: Arc<dyn DiffEngine> = Arc::new(SimilarDiffEngine::new());

        let server = BpxServer::builder()
            .config(config.clone())
            .state_manager(state_manager.clone())
            .diff_engine(diff_engine.clone())
            .build()
            .unwrap();

        assert_eq!(server.config().max_sessions, config.max_sessions);
        assert!(Arc::ptr_eq(server.state_manager(), &state_manager));
        assert!(Arc::ptr_eq(server.diff_engine(), &diff_engine));
    }

    #[test]
    fn test_bpx_server_builder_missing_state_manager() {
        use crate::diff::similar::SimilarDiffEngine;

        let config = BpxConfig::default();
        let diff_engine: Arc<dyn DiffEngine> = Arc::new(SimilarDiffEngine::new());

        let result = BpxServer::builder()
            .config(config)
            .diff_engine(diff_engine)
            .build();

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, BpxError::DiffComputationFailed { .. }));
        }
    }

    #[test]
    fn test_bpx_server_builder_missing_diff_engine() {
        use crate::state::InMemoryStateManager;

        let config = BpxConfig::default();
        let state_manager: Arc<dyn StateManager> =
            Arc::new(InMemoryStateManager::new(config.clone()));

        let result = BpxServer::builder()
            .config(config)
            .state_manager(state_manager)
            .build();

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, BpxError::DiffComputationFailed { .. }));
        }
    }

    #[test]
    fn test_bpx_server_builder_default_config() {
        use crate::diff::similar::SimilarDiffEngine;
        use crate::state::InMemoryStateManager;

        let config = BpxConfig::default();
        let state_manager: Arc<dyn StateManager> = Arc::new(InMemoryStateManager::new(config));
        let diff_engine: Arc<dyn DiffEngine> = Arc::new(SimilarDiffEngine::new());

        // Should use default config when not provided
        let server = BpxServer::builder()
            .state_manager(state_manager)
            .diff_engine(diff_engine)
            .build()
            .unwrap();

        let server_config = server.config();
        let default_config = BpxConfig::default();
        assert_eq!(server_config.max_sessions, default_config.max_sessions);
        assert_eq!(server_config.session_ttl, default_config.session_ttl);
    }

    #[test]
    fn test_bpx_server_builder_custom_config() {
        use crate::diff::similar::SimilarDiffEngine;
        use crate::state::InMemoryStateManager;

        let mut custom_config = BpxConfig::default();
        custom_config.max_sessions = 50_000;
        custom_config.session_ttl = Duration::from_secs(12 * 60 * 60); // 12 hours
        custom_config.min_compression_ratio = 0.3;

        let state_manager: Arc<dyn StateManager> =
            Arc::new(InMemoryStateManager::new(custom_config.clone()));
        let diff_engine: Arc<dyn DiffEngine> = Arc::new(SimilarDiffEngine::new());

        let server = BpxServer::builder()
            .config(custom_config.clone())
            .state_manager(state_manager)
            .diff_engine(diff_engine)
            .build()
            .unwrap();

        let server_config = server.config();
        assert_eq!(server_config.max_sessions, 50_000);
        assert_eq!(server_config.session_ttl, Duration::from_secs(12 * 60 * 60));
        assert_eq!(server_config.min_compression_ratio, 0.3);
    }

    #[test]
    fn test_bpx_session_new_and_touch() {
        let session_id = SessionId::new("test_session".to_string());
        let mut session = BpxSession::new(session_id.clone());

        assert_eq!(session.id, session_id);
        assert_eq!(session.resources.len(), 0);
        assert_eq!(
            session
                .memory_usage
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );

        let initial_time = session.last_accessed;

        // Wait a tiny bit then touch
        std::thread::sleep(Duration::from_millis(1));
        session.touch();

        assert!(session.last_accessed > initial_time);
    }

    #[test]
    fn test_bpx_session_expiration() {
        let session_id = SessionId::new("test_session".to_string());
        let session = BpxSession::new(session_id);
        let very_short_ttl = Duration::from_millis(1);
        let long_ttl = Duration::from_secs(3600);

        // Should not be expired with long TTL
        assert!(!session.is_expired(long_ttl));

        // Wait for very short TTL to pass
        std::thread::sleep(Duration::from_millis(2));

        // Should be expired with very short TTL
        assert!(session.is_expired(very_short_ttl));
    }

    #[test]
    fn test_bpx_session_resource_management() {
        let session_id = SessionId::new("test_session".to_string());
        let session = BpxSession::new(session_id);

        let path1 = ResourcePath::new("/api/users".to_string());
        let path2 = ResourcePath::new("/api/orders".to_string());
        let version1 = Version::new("v1".to_string());
        let version2 = Version::new("v2".to_string());

        // Add resources
        session.resources.insert(path1.clone(), version1.clone());
        session.resources.insert(path2.clone(), version2.clone());

        assert_eq!(session.resources.len(), 2);
        assert_eq!(*session.resources.get(&path1).unwrap(), version1);
        assert_eq!(*session.resources.get(&path2).unwrap(), version2);
    }
}
