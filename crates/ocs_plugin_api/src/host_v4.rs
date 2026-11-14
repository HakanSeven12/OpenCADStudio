//! Host-side V4 snapshot manager.
//!
//! Maintains a per-tab shared-memory document view keyed by the stable
//! `DocumentTab.id`. The manager is used by `src/plugin/v4_support.rs` to
//! open, publish, and close V4 snapshots independently of the V3 reader path.
//!
//! [`manager`] returns a global `Mutex`-guarded [`HostV4SnapshotManager`].
//! [`HostV4SnapshotManager::open`] lazily creates a [`DocumentSnapshotStore`] for
//! a tab; [`HostV4SnapshotManager::publish`] updates an existing store;
//! [`HostV4SnapshotManager::close`] drops it. A segment holds twice the
//! drawing, at least 16 MiB (`OCS_V4_SNAPSHOT_SEGMENT_SIZE` raises that floor).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use document::Document;

use crate::shm::{DocumentSnapshotStore, DocumentViewDataV4, DocumentViewInfo, SnapshotData};

static MANAGER: OnceLock<Mutex<HostV4SnapshotManager>> = OnceLock::new();

fn global_manager() -> &'static Mutex<HostV4SnapshotManager> {
    MANAGER.get_or_init(|| Mutex::new(HostV4SnapshotManager::new()))
}

/// Per-tab V4 shared-memory snapshot manager.
pub struct HostV4SnapshotManager {
    stores: HashMap<u64, DocumentSnapshotStore<DocumentViewDataV4>>,
    /// Stores replaced by a larger mapping (see `publish_document`). Their
    /// mappings stay readable until the process exits so readers of the old
    /// path are never pinned to a frozen, deleted file.
    retired: Vec<DocumentSnapshotStore<DocumentViewDataV4>>,
    segment_size: usize,
}

impl HostV4SnapshotManager {
    /// Create a new manager with the default segment size.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            retired: Vec::new(),
            segment_size: default_segment_size(),
        }
    }

    /// Open (or refresh) the V4 snapshot for `tab_id`. Creates the store when
    /// missing; `Err` reports a real failure, `Ok` the fresh view info.
    pub fn open(&mut self, tab_id: u64, doc: &Document) -> Result<DocumentViewInfo, String> {
        match self.publish_document(tab_id, doc, true) {
            Ok(Some(info)) => Ok(info),
            Ok(None) => Err(format!("the V4 view for tab {tab_id} did not publish")),
            Err(e) => Err(e),
        }
    }
    /// Publish the latest document to the existing V4 snapshot for `tab_id`.
    /// `Ok(None)`: the tab has no open store yet — a legitimate no-op (no V4
    /// consumer), NOT failure; `Ok(Some(info))`: published; `Err`: a real
    /// publication failure the caller must report instead of claiming a
    /// boundary.
    pub fn publish(
        &mut self,
        tab_id: u64,
        doc: &Document,
    ) -> Result<Option<DocumentViewInfo>, String> {
        self.publish_document(tab_id, doc, false)
    }

    /// A drawing that outgrows its mapping moves to a new one at a new path,
    /// twice its size; plugins re-open the view via the fresh path. The
    /// replaced store is RETIRED, not dropped: its mapping (and file) must
    /// stay readable for readers that still hold the old path open — deleting
    /// the file would leave them pinned to a frozen view forever. Retired
    /// stores accumulate only per resize event (rare).
    fn publish_document(
        &mut self,
        tab_id: u64,
        doc: &Document,
        create: bool,
    ) -> Result<Option<DocumentViewInfo>, String> {
        if !create && !self.stores.contains_key(&tab_id) {
            return Ok(None);
        }
        let data: DocumentViewDataV4 = doc.into();
        let bytes = match data.to_rkyv_bytes() {
            Ok(bytes) => bytes,
            Err(e) => {
                let msg = format!(
                    "failed to serialize V4 document view for tab {tab_id}: {e}"
                );
                eprintln!("[host] {msg}");
                return Err(msg);
            }
        };
        let fits = self
            .stores
            .get(&tab_id)
            .is_some_and(|store| bytes.len() <= store.segment_size());
        if !fits {
            let size = bytes
                .len()
                .saturating_mul(2)
                .checked_next_power_of_two()
                .unwrap_or(usize::MAX)
                .max(self.segment_size);
            match DocumentSnapshotStore::new(tab_id, size) {
                Ok(mut store) => {
                    // Publish the new mapping BEFORE it becomes authoritative,
                    // then retire the old store (keep its file alive) instead
                    // of deleting it out from under existing readers.
                    if let Err(e) = store.publish_bytes(&bytes) {
                        let msg = format!(
                            "failed to publish V4 document view for tab {tab_id}: {e}"
                        );
                        eprintln!("[host] {msg}");
                        return Err(msg);
                    }
                    let info = DocumentViewInfo {
                        path: store.path().to_string_lossy().into_owned(),
                        version: store.version(),
                    };
                    if let Some(old) = self.stores.insert(tab_id, store) {
                        self.retired.push(old);
                    }
                    return Ok(Some(info));
                }
                Err(e) => {
                    let msg = format!(
                        "cannot share the V4 document view for tab {tab_id} ({} bytes): {e}",
                        bytes.len()
                    );
                    eprintln!("[host] {msg}");
                    self.stores.remove(&tab_id);
                    return Err(msg);
                }
            }
        }
        // The store must exist here: the `!create` early-return handled the
        // no-store case above, and the resize branch inserts one otherwise.
        let Some(store) = self.stores.get_mut(&tab_id) else {
            let msg = format!("V4 document view for tab {tab_id} vanished mid-publish");
            eprintln!("[host] {msg}");
            return Err(msg);
        };
        if let Err(e) = store.publish_bytes(&bytes) {
            let msg = format!("failed to publish V4 document view for tab {tab_id}: {e}");
            eprintln!("[host] {msg}");
            return Err(msg);
        }
        Ok(Some(DocumentViewInfo {
            path: store.path().to_string_lossy().into_owned(),
            version: store.version(),
        }))
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
