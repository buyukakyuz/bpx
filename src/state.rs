//! Client state management

use crate::{DspConfig, DspSession, ResourcePath, SessionId, Version};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trait for managing client state
#[async_trait]
pub trait StateManager: Send + Sync {
    /// Get existing session or create new one
    async fn get_or_create_session(&self, id: Option<SessionId>) -> SessionId;

    /// Get version for a resource in a session
    async fn get_version(&self, session: &SessionId, path: &ResourcePath) -> Option<Version>;

    /// Set version for a resource in a session  
    async fn set_version(&self, session: &SessionId, path: &ResourcePath, version: Version);

    /// Clean up expired sessions
    async fn cleanup_expired(&self);
}

/// In-memory state manager implementation
pub struct InMemoryStateManager {
    sessions: DashMap<SessionId, Arc<RwLock<DspSession>>>,
    config: DspConfig,
}

impl InMemoryStateManager {
    /// Create new in-memory state manager
    pub fn new(config: DspConfig) -> Self {
        Self {
            sessions: DashMap::new(),
            config,
        }
    }
}

#[async_trait]
impl StateManager for InMemoryStateManager {
    async fn get_or_create_session(&self, id: Option<SessionId>) -> SessionId {
        match id {
            Some(session_id) => {
                if self.sessions.contains_key(&session_id) {
                    // Update last accessed time
                    if let Some(session) = self.sessions.get(&session_id) {
                        let mut session = session.write().await;
                        session.touch();
                    }
                    session_id
                } else {
                    // Session expired or doesn't exist, create new one
                    let new_id = SessionId::generate();
                    let session = Arc::new(RwLock::new(DspSession::new(new_id.clone())));
                    self.sessions.insert(new_id.clone(), session);
                    new_id
                }
            }
            None => {
                // First request, create new session
                let new_id = SessionId::generate();
                let session = Arc::new(RwLock::new(DspSession::new(new_id.clone())));
                self.sessions.insert(new_id.clone(), session);
                new_id
            }
        }
    }

    async fn get_version(&self, session_id: &SessionId, path: &ResourcePath) -> Option<Version> {
        let session = self.sessions.get(session_id)?;
        let session = session.read().await;
        session.resources.get(path).map(|v| v.clone())
    }

    async fn set_version(&self, session_id: &SessionId, path: &ResourcePath, version: Version) {
        if let Some(session) = self.sessions.get(session_id) {
            let session = session.read().await;
            session.resources.insert(path.clone(), version);
        }
    }

    async fn cleanup_expired(&self) {
        let ttl = self.config.session_ttl;
        self.sessions.retain(|_, session_arc| {
            let session = tokio::task::block_in_place(|| session_arc.blocking_read());
            !session.is_expired(ttl)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_get_or_create_session_new() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        // First request without session ID should create new session
        let session_id = state_mgr.get_or_create_session(None).await;
        assert!(session_id.to_string().starts_with("sess_"));
        assert!(state_mgr.sessions.contains_key(&session_id));
    }

    #[tokio::test]
    async fn test_get_or_create_session_existing() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        // Create initial session
        let session_id1 = state_mgr.get_or_create_session(None).await;

        // Request with existing session ID should return same session
        let session_id2 = state_mgr
            .get_or_create_session(Some(session_id1.clone()))
            .await;
        assert_eq!(session_id1, session_id2);

        // Should only have one session
        assert_eq!(state_mgr.sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_get_or_create_session_nonexistent() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let fake_session = SessionId::new("fake_session".to_string());

        // Request with non-existent session ID should create new session
        let new_session_id = state_mgr
            .get_or_create_session(Some(fake_session.clone()))
            .await;
        assert_ne!(new_session_id, fake_session);
        assert!(state_mgr.sessions.contains_key(&new_session_id));
    }

    #[tokio::test]
    async fn test_version_tracking() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let session_id = state_mgr.get_or_create_session(None).await;
        let path = ResourcePath::new("/api/test".to_string());
        let version = Version::new("v1".to_string());

        // Initially no version stored
        let stored_version = state_mgr.get_version(&session_id, &path).await;
        assert!(stored_version.is_none());

        // Set version
        state_mgr
            .set_version(&session_id, &path, version.clone())
            .await;

        // Retrieve version
        let stored_version = state_mgr.get_version(&session_id, &path).await;
        assert_eq!(stored_version, Some(version));
    }

    #[tokio::test]
    async fn test_version_tracking_multiple_resources() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let session_id = state_mgr.get_or_create_session(None).await;
        let path1 = ResourcePath::new("/api/users".to_string());
        let path2 = ResourcePath::new("/api/orders".to_string());
        let version1 = Version::new("v1".to_string());
        let version2 = Version::new("v2".to_string());

        // Set versions for different resources
        state_mgr
            .set_version(&session_id, &path1, version1.clone())
            .await;
        state_mgr
            .set_version(&session_id, &path2, version2.clone())
            .await;

        // Both should be retrievable
        assert_eq!(
            state_mgr.get_version(&session_id, &path1).await,
            Some(version1)
        );
        assert_eq!(
            state_mgr.get_version(&session_id, &path2).await,
            Some(version2)
        );
    }

    #[tokio::test]
    async fn test_version_overwrite() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let session_id = state_mgr.get_or_create_session(None).await;
        let path = ResourcePath::new("/api/test".to_string());
        let version1 = Version::new("v1".to_string());
        let version2 = Version::new("v2".to_string());

        // Set initial version
        state_mgr
            .set_version(&session_id, &path, version1.clone())
            .await;
        assert_eq!(
            state_mgr.get_version(&session_id, &path).await,
            Some(version1)
        );

        // Overwrite with new version
        state_mgr
            .set_version(&session_id, &path, version2.clone())
            .await;
        assert_eq!(
            state_mgr.get_version(&session_id, &path).await,
            Some(version2)
        );
    }

    #[tokio::test]
    async fn test_get_version_nonexistent_session() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let fake_session = SessionId::new("fake_session".to_string());
        let path = ResourcePath::new("/api/test".to_string());

        // Should return None for non-existent session
        let version = state_mgr.get_version(&fake_session, &path).await;
        assert!(version.is_none());
    }

    #[tokio::test]
    async fn test_set_version_nonexistent_session() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        let fake_session = SessionId::new("fake_session".to_string());
        let path = ResourcePath::new("/api/test".to_string());
        let version = Version::new("v1".to_string());

        // Setting version for non-existent session should not crash
        state_mgr.set_version(&fake_session, &path, version).await;

        // Session should not be created
        assert!(!state_mgr.sessions.contains_key(&fake_session));
    }

    #[tokio::test]
    async fn test_session_touch_on_access() {
        let config = DspConfig::default();
        let state_mgr = InMemoryStateManager::new(config);

        // Create session
        let session_id = state_mgr.get_or_create_session(None).await;

        // Get initial timestamp
        let initial_time = {
            let session = state_mgr.sessions.get(&session_id).unwrap();
            let session = session.read().await;
            session.last_accessed
        };

        // Wait a bit
        sleep(Duration::from_millis(10)).await;

        // Access session again
        let _same_session = state_mgr
            .get_or_create_session(Some(session_id.clone()))
            .await;

        // Timestamp should be updated
        let updated_time = {
            let session = state_mgr.sessions.get(&session_id).unwrap();
            let session = session.read().await;
            session.last_accessed
        };

        assert!(updated_time > initial_time);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cleanup_expired_sessions() {
        let mut config = DspConfig::default();
        config.session_ttl = Duration::from_millis(50); // Very short TTL for testing
        let state_mgr = InMemoryStateManager::new(config);

        // Create a session
        let session_id = state_mgr.get_or_create_session(None).await;
        assert_eq!(state_mgr.sessions.len(), 1);

        // Wait for session to expire
        sleep(Duration::from_millis(100)).await;

        // Run cleanup
        state_mgr.cleanup_expired().await;

        // Session should be removed
        assert_eq!(state_mgr.sessions.len(), 0);
        assert!(!state_mgr.sessions.contains_key(&session_id));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cleanup_keeps_active_sessions() {
        let mut config = DspConfig::default();
        config.session_ttl = Duration::from_millis(100);
        let state_mgr = InMemoryStateManager::new(config);

        // Create two sessions
        let session_id1 = state_mgr.get_or_create_session(None).await;
        let session_id2 = state_mgr.get_or_create_session(None).await;
        assert_eq!(state_mgr.sessions.len(), 2);

        // Wait a bit, then access one session to keep it active
        sleep(Duration::from_millis(60)).await;
        let _active_session = state_mgr
            .get_or_create_session(Some(session_id1.clone()))
            .await;

        // Wait for the other session to expire
        sleep(Duration::from_millis(60)).await;

        // Run cleanup
        state_mgr.cleanup_expired().await;

        // Only the inactive session should be removed
        assert_eq!(state_mgr.sessions.len(), 1);
        assert!(state_mgr.sessions.contains_key(&session_id1));
        assert!(!state_mgr.sessions.contains_key(&session_id2));
    }

    #[tokio::test]
    async fn test_concurrent_session_creation() {
        let config = DspConfig::default();
        let state_mgr = Arc::new(InMemoryStateManager::new(config));

        let mut handles = vec![];

        // Create multiple concurrent sessions
        for _ in 0..10 {
            let mgr = Arc::clone(&state_mgr);
            let handle = tokio::spawn(async move { mgr.get_or_create_session(None).await });
            handles.push(handle);
        }

        // Wait for all to complete
        let mut session_ids = vec![];
        for handle in handles {
            session_ids.push(handle.await.expect("Task should complete"));
        }

        // All sessions should be unique
        let unique_count = session_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(unique_count, session_ids.len());
        assert_eq!(state_mgr.sessions.len(), session_ids.len());
    }

    #[tokio::test]
    async fn test_concurrent_version_updates() {
        let config = DspConfig::default();
        let state_mgr = Arc::new(InMemoryStateManager::new(config));

        let session_id = state_mgr.get_or_create_session(None).await;
        let path = ResourcePath::new("/api/test".to_string());

        let mut handles = vec![];

        // Create multiple concurrent version updates
        for i in 0..10 {
            let mgr = Arc::clone(&state_mgr);
            let session = session_id.clone();
            let path = path.clone();
            let handle = tokio::spawn(async move {
                let version = Version::new(format!("v{}", i));
                mgr.set_version(&session, &path, version).await;
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        for handle in handles {
            handle.await.expect("Update should complete");
        }

        // Final version should be one of the values (race condition is OK)
        let final_version = state_mgr.get_version(&session_id, &path).await;
        assert!(final_version.is_some());
    }
}
