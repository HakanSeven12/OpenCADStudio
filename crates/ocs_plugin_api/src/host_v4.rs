//! Host-side V4 snapshot manager.
//!
//! Maintains a per-tab shared-memory document view keyed by the stable
//! `DocumentTab.id`. The manager is used by `src/plugin/v4_support.rs` to
//! open, publish, and close V4 snapshots independently of the V3 reader path.
//!
//! [`manager`] returns a global `Mutex`-guarded [`HostV4SnapshotManager`].
//! [`HostV4SnapshotManager::open`] lazily creates a [`DocumentSnapshotStore`] for
//! a tab; [`HostV4SnapshotManager::publish`] updates an existing store;
//! [`HostV4SnapshotManager::close`] drops it. Segment size defaults to 16 MiB
//! and can be overridden with `OCS_V4_SNAPSHOT_SEGMENT_SIZE`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use acadrust::CadDocument;

use crate::shm::{DocumentSnapshotStore, DocumentViewDataV4, DocumentViewInfo};

static MANAGER: OnceLock<Mutex<HostV4SnapshotManager>> = OnceLock::new();

fn global_manager() -> &'static Mutex<HostV4SnapshotManager> {
    MANAGER.get_or_init(|| Mutex::new(HostV4SnapshotManager::new()))
}

/// Per-tab V4 shared-memory snapshot manager.
pub struct HostV4SnapshotManager {
    stores: HashMap<u64, DocumentSnapshotStore<DocumentViewDataV4>>,
    segment_size: usize,
}

impl HostV4SnapshotManager {
    /// Create a new manager with the default segment size.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            segment_size: default_segment_size(),
        }
    }

    /// Open (or refresh) the V4 snapshot for `tab_id` and return the view info.
    pub fn open(&mut self, tab_id: u64, doc: &CadDocument) -> Option<DocumentViewInfo> {
        let store = match self.stores.get_mut(&tab_id) {
            Some(store) => store,
            None => {
                let store = DocumentSnapshotStore::new(tab_id, self.segment_size).ok()?;
                self.stores.entry(tab_id).or_insert(store)
            }
        };
        if let Err(e) = store.publish(&doc.into()) {
            eprintln!("[host] failed to publish V4 document view for tab {tab_id}: {e}");
            return None;
        }
        Some(DocumentViewInfo {
            path: store.path().to_string_lossy().into_owned(),
            version: store.version(),
        })
    }

    /// Publish the latest document to the existing V4 snapshot for `tab_id`.
    /// Returns the updated view info if the snapshot exists.
    pub fn publish(&mut self, tab_id: u64, doc: &CadDocument) -> Option<DocumentViewInfo> {
        let store = self.stores.get_mut(&tab_id)?;
        if let Err(e) = store.publish(&doc.into()) {
            eprintln!("[host] failed to publish V4 document view for tab {tab_id}: {e}");
            return None;
        }
        Some(DocumentViewInfo {
            path: store.path().to_string_lossy().into_owned(),
            version: store.version(),
        })
    }

    /// Close the V4 snapshot for `tab_id`.
    pub fn close(&mut self, tab_id: u64) {
        self.stores.remove(&tab_id);
    }
}

impl Default for HostV4SnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global accessor for the host V4 snapshot manager.
pub fn manager() -> &'static Mutex<HostV4SnapshotManager> {
    global_manager()
}

fn default_segment_size() -> usize {
    std::env::var("OCS_V4_SNAPSHOT_SEGMENT_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16 * 1024 * 1024)
        .max(1024 * 1024)
}
