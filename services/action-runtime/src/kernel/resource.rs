//! Resource management: the single owner of every long-lived resource.
//!
//! Plugins never hold resources directly — they ask the [`ResourceManager`] for
//! one by id and operate through it. The manager acquires on registration,
//! tracks liveness via heartbeats, and — critically — releases and shuts down
//! everything it owns on [`ResourceManager::shutdown_all`], so nothing leaks
//! when the runtime stops. Terminal sessions, browser sessions, file watchers,
//! microphones, cameras, and Windows handles all flow through here.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use friday_core::Result;
use uuid::Uuid;

use crate::kernel::lifecycle::{LifecycleManager, LifecycleState};

/// Stable identifier for a managed resource: a kind plus a per-kind sequence
/// number (e.g. `TerminalSession #14`). The kind is a `&'static str` so resources
/// are self-describing at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId {
    pub kind: &'static str,
    pub seq: u64,
}

impl ResourceId {
    pub fn new(kind: &'static str, seq: u64) -> Self {
        Self { kind, seq }
    }
}

impl std::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} #{}", self.kind, self.seq)
    }
}

/// Anything the platform must own, monitor, and reclaim. Implementors use
/// interior mutability for the few mutable bits (heartbeat, acquired flag).
#[async_trait]
pub trait ManagedResource: Send + Sync {
    /// Stable id assigned by the manager on registration.
    fn id(&self) -> ResourceId;

    /// Take ownership of the underlying OS/hardware resource.
    async fn acquire(&self) -> Result<()>;

    /// Release the underlying resource (e.g. kill a process, close a handle).
    async fn release(&self) -> Result<()>;

    /// Last time the resource reported liveness. Used by [`ResourceManager::sweep`].
    fn last_heartbeat(&self) -> DateTime<Utc>;

    /// Tear the resource down completely (called during shutdown).
    async fn shutdown(&self) -> Result<()>;
}

/// The sole owner of all long-lived resources.
pub struct ResourceManager {
    resources: Mutex<HashMap<ResourceId, Box<dyn ManagedResource>>>,
    seq: Mutex<HashMap<&'static str, u64>>,
    lifecycle: LifecycleManager,
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceManager {
    pub fn new() -> Self {
        Self {
            resources: Mutex::new(HashMap::new()),
            seq: Mutex::new(HashMap::new()),
            lifecycle: LifecycleManager::new(),
        }
    }

    /// Register and acquire a resource. Assigns the next per-kind sequence
    /// number, moves it to `Running`, and returns its id.
    pub async fn register(&self, resource: Box<dyn ManagedResource>) -> Result<ResourceId> {
        let kind = resource.id().kind;
        let seq = {
            let mut s = self.seq.lock().expect("resource seq poisoned");
            let n = s.entry(kind).or_insert(0);
            *n += 1;
            *n
        };
        let id = ResourceId::new(kind, seq);
        resource.acquire().await?;
        self.lifecycle.create(id.uuid());
        self.lifecycle
            .transition(id.uuid(), LifecycleState::Running)
            .ok();
        self.resources
            .lock()
            .expect("resource map poisoned")
            .insert(id, resource);
        Ok(id)
    }

    /// Apply a closure to a resource by id (read-only access).
    pub fn with_resource<T>(
        &self,
        id: ResourceId,
        f: impl FnOnce(&dyn ManagedResource) -> T,
    ) -> Option<T> {
        let guard = self.resources.lock().expect("resource map poisoned");
        guard.get(&id).map(|r| f(r.as_ref()))
    }

    /// Release a single resource and archive its lifecycle.
    pub async fn release(&self, id: ResourceId) -> Result<()> {
        let resource = self
            .resources
            .lock()
            .expect("resource map poisoned")
            .remove(&id);
        if let Some(r) = resource {
            r.release().await?;
            self.lifecycle
                .transition(id.uuid(), LifecycleState::Archived)
                .ok();
        }
        Ok(())
    }

    /// Release resources whose last heartbeat is older than `older_than`.
    /// Returns the ids that were swept.
    pub async fn sweep(&self, older_than: DateTime<Utc>) -> Result<Vec<ResourceId>> {
        let stale: Vec<ResourceId> = {
            let guard = self.resources.lock().expect("resource map poisoned");
            guard
                .iter()
                .filter(|(_, r)| r.last_heartbeat() < older_than)
                .map(|(id, _)| *id)
                .collect()
        };
        for id in &stale {
            self.release(*id).await?;
        }
        Ok(stale)
    }

    /// Release and shut down every owned resource. Idempotent and safe to call
    /// on shutdown; guarantees no resource is left dangling.
    pub async fn shutdown_all(&self) -> Result<()> {
        let all: Vec<Box<dyn ManagedResource>> = {
            let mut guard = self.resources.lock().expect("resource map poisoned");
            // Take ownership of the boxed resources for teardown.
            guard.drain().map(|(_, r)| r).collect()
        };
        for r in all {
            let _ = r.release().await;
            let _ = r.shutdown().await;
            self.lifecycle
                .transition(r.id().uuid(), LifecycleState::Archived)
                .ok();
        }
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.resources.lock().expect("resource map poisoned").len()
    }
}

/// Helper so a `ResourceId` can be tracked by the [`LifecycleManager`], which keys
/// on [`Uuid`]. We hash kind+seq into a stable uuid.
impl ResourceId {
    pub fn uuid(&self) -> Uuid {
        let mut buf = [0u8; 16];
        let k = self.kind.as_bytes();
        let kind_bytes = crc_bytes(k);
        buf[0..8].copy_from_slice(&kind_bytes);
        buf[8..16].copy_from_slice(&self.seq.to_le_bytes());
        Uuid::from_bytes(buf)
    }
}

fn crc_bytes(input: &[u8]) -> [u8; 8] {
    // Tiny FNV-1a derived 64-bit hash; sufficient to de-duplicate kinds.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in input {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};

    struct TestResource {
        id: ResourceId,
        acquired: Arc<AtomicBool>,
        heartbeat: StdMutex<DateTime<Utc>>,
        released: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ManagedResource for TestResource {
        fn id(&self) -> ResourceId {
            self.id
        }
        async fn acquire(&self) -> Result<()> {
            self.acquired.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn release(&self) -> Result<()> {
            self.released.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn last_heartbeat(&self) -> DateTime<Utc> {
            *self.heartbeat.lock().unwrap()
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    fn make(
        kind: &'static str,
        seq: u64,
        acquired: Arc<AtomicBool>,
        released: Arc<AtomicBool>,
    ) -> Box<dyn ManagedResource> {
        Box::new(TestResource {
            id: ResourceId::new(kind, seq),
            acquired,
            heartbeat: StdMutex::new(Utc::now()),
            released,
        })
    }

    #[tokio::test]
    async fn register_assigns_sequence_and_acquires() {
        let acquired = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let mgr = ResourceManager::new();
        let a = mgr
            .register(make(
                "TerminalSession",
                0,
                acquired.clone(),
                released.clone(),
            ))
            .await
            .unwrap();
        let b = mgr
            .register(make(
                "TerminalSession",
                0,
                acquired.clone(),
                released.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(a.seq, 1);
        assert_eq!(b.seq, 2);
        assert_eq!(mgr.count(), 2);
        assert!(
            acquired.load(Ordering::SeqCst),
            "resource should have been acquired"
        );
    }

    #[tokio::test]
    async fn shutdown_releases_everything() {
        let acquired = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let mgr = ResourceManager::new();
        mgr.register(make("Camera", 0, acquired.clone(), released.clone()))
            .await
            .unwrap();
        mgr.register(make("Microphone", 0, acquired.clone(), released.clone()))
            .await
            .unwrap();
        mgr.shutdown_all().await.unwrap();
        assert_eq!(mgr.count(), 0);
        assert!(
            released.load(Ordering::SeqCst),
            "resources should have been released on shutdown"
        );
    }
}
