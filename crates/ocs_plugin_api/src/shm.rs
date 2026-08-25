//! Shared-memory document view for out-of-process plugins.
//!
//! The host owns a memory-mapped file that contains a small, read-only,
//! rkyv-serialized view of the active document. The plugin maps the same file
//! read-only and reads entity/layer data directly from the mapping without
//! copying the full `CadDocument` into its own address space.
//!
//! This module is generic over the payload type so that the V3 simplified view
//! and the V4 bincode-per-entity view can share the same double-buffered
//! control-page logic.
//!
//! Key types:
//!
//! - [`DocumentSnapshotStore<T>`] — host-side, file-backed double buffer. Call
//!   [`DocumentSnapshotStore::publish`] to atomically swap the active segment and
//!   increment the version.
//! - [`SharedDocumentReader<T>`] — plugin-side read-only mapping.
//! - [`DocumentViewInfo`] — path + version returned to the plugin so it can
//!   open the mapping.
//! - [`DocumentViewData`] / [`DocumentViewDataV4`] — concrete snapshot payloads
//!   for V3 and V4.
//!
//! The control page at the start of the mapping stores magic, version, active
//! segment, and length. The two snapshot segments follow it. Publishing writes
//! to the inactive segment, then flips the active segment atomically.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use acadrust::{CadDocument, EntityType, Handle};
use memmap2::{Mmap, MmapMut, MmapOptions};
use rkyv::{check_archived_root, to_bytes, Archive, Deserialize, Serialize};

use crate::host::{DocumentReader, ReaderEntity, ReaderEntityKind, ReaderPoint};

/// Magic number identifying a valid control page.
const CONTROL_MAGIC: u32 = 0x4F_43_53_44; // "OCSD"

/// Size of the control region at the start of the mapping. Must be enough for
/// `ControlPage` and aligned to a typical page boundary so the snapshot segments
/// that follow are naturally aligned for rkyv.
const CONTROL_SIZE: usize = 4096;
const MAX_SNAPSHOT_SIZE: usize = 1024 * 1024 * 1024;

/// Information sent to the plugin so it can open the shared mapping.
#[derive(Debug, Clone)]
pub struct DocumentViewInfo {
    /// Absolute path to the memory-mapped file.
    pub path: String,
    /// Snapshot version at the time the view was opened.
    pub version: u64,
}

/// Marker trait for types that can be placed in a shared-memory snapshot.
///
/// Implementers provide rkyv serialization/deserialization without exposing
/// rkyv's generic serializer types in the public API.
pub trait SnapshotData: Clone + std::fmt::Debug + Send + Sync + 'static {
    /// The archived (zero-copy) type returned by [`Self::check_bytes`].
    type Archived: 'static;

    /// Serialize `self` into rkyv bytes.
    fn to_rkyv_bytes(&self) -> io::Result<Vec<u8>>;

    /// Validate `bytes` and return a reference to the archived type.
    fn check_rkyv_bytes(bytes: &[u8]) -> Option<&Self::Archived>;
}

/// Host-side, file-backed double buffer for the document view.
pub struct DocumentSnapshotStore<T: SnapshotData> {
    path: PathBuf,
    mmap: MmapMut,
    segment_size: usize,
    current_version: u64,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: SnapshotData> DocumentSnapshotStore<T> {
    /// Create a new store for `tab_id`. `segment_size` is the maximum size of one
    /// snapshot buffer; the file is sized to hold two segments plus the control
    /// page.
    pub fn new(tab_id: u64, segment_size: usize) -> io::Result<Self> {
        let segment_size = segment_size.checked_next_multiple_of(4096).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "snapshot segment size overflow")
        })?;
        let total = segment_size
            .checked_mul(2)
            .and_then(|segments| CONTROL_SIZE.checked_add(segments))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot mapping size overflow")
            })?;
        if total > MAX_SNAPSHOT_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "snapshot mapping size {total} exceeds maximum {} MiB",
                    MAX_SNAPSHOT_SIZE / 1024 / 1024
                ),
            ));
        }
        static STORE_ID: AtomicUsize = AtomicUsize::new(0);
        let id = STORE_ID.fetch_add(1, Ordering::Relaxed);
        let path = Self::temp_path(tab_id, id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(Permissions::from_mode(0o600))?;
        }
        file.set_len(total as u64)?;

        let mut mmap = unsafe { MmapOptions::new().len(total).map_mut(&file)? };
        let control = ControlPage::from_bytes_mut(&mut mmap);
        control.magic.store(CONTROL_MAGIC, Ordering::Relaxed);
        control.version.store(0, Ordering::Relaxed);
        control.active_segment.store(0, Ordering::Relaxed);
        control.active_len.store(0, Ordering::Relaxed);
        mmap.flush()?;

        Ok(Self {
            path,
            mmap,
            segment_size,
            current_version: 0,
            _phantom: std::marker::PhantomData,
        })
    }

    fn temp_path(tab_id: u64, id: usize) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ocs_plugin_doc_v4_{}_{}_{}_{}.bin",
            std::process::id(),
            tab_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            id,
        ));
        path
    }

    /// Path the plugin should open to access the mapping.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize `data` into the inactive segment and atomically publish it.
    pub fn publish(&mut self, data: &T) -> io::Result<()> {
        let bytes = data.to_rkyv_bytes()?;
        if bytes.len() > self.segment_size {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!(
                    "document view {} bytes exceeds segment size {}",
                    bytes.len(),
                    self.segment_size
                ),
            ));
        }

        let inactive = {
            let control = ControlPage::from_bytes_mut(&mut self.mmap);
            let active = control.active_segment.load(Ordering::Acquire) as usize;
            1 - active
        };
        let offset = CONTROL_SIZE + inactive * self.segment_size;

        self.mmap[offset..offset + bytes.len()].copy_from_slice(&bytes);
        let control = ControlPage::from_bytes_mut(&mut self.mmap);
        // Ensure the plugin sees the new length before it sees the new version.
        control
            .active_len
            .store(bytes.len() as u64, Ordering::Release);
        control
            .active_segment
            .store(inactive as u8, Ordering::Release);
        self.current_version = self.current_version.wrapping_add(1);
        control
            .version
            .store(self.current_version, Ordering::Release);
        Ok(())
    }

    /// Current published version.
    pub fn version(&self) -> u64 {
        ControlPage::from_bytes(&self.mmap)
            .version
            .load(Ordering::Acquire)
    }
}

impl<T: SnapshotData> Drop for DocumentSnapshotStore<T> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Plugin-side read-only mapping of the host's document view.
pub struct SharedDocumentReader<T: SnapshotData> {
    mmap: Mmap,
    segment_size: usize,
    cached_version: u64,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: SnapshotData> SharedDocumentReader<T> {
    /// Open the file at `path` read-only and map it. The mapping may initially
    /// contain no valid snapshot; the caller should `refresh()` before use.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("snapshot path is not a regular file: {}", path.display()),
            ));
        }
        let file_len = metadata.len();
        if file_len < CONTROL_SIZE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "snapshot file size {file_len} is smaller than control region {CONTROL_SIZE}"
                ),
            ));
        }
        if file_len > MAX_SNAPSHOT_SIZE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "snapshot file size {} exceeds maximum {} MiB; refusing to mmap",
                    file_len,
                    MAX_SNAPSHOT_SIZE / 1024 / 1024
                ),
            ));
        }
        let file_len = usize::try_from(file_len).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "snapshot file size is unsupported")
        })?;
        let mmap = unsafe { MmapOptions::new().len(file_len).map(&file)? };
        let segment_size = (file_len - CONTROL_SIZE) / 2;
        Ok(Self {
            mmap,
            segment_size,
            cached_version: 0,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Check whether the host has published a newer snapshot.
    pub fn has_new_version(&self) -> bool {
        let control = ControlPage::from_bytes(&self.mmap);
        if control.magic.load(Ordering::Acquire) != CONTROL_MAGIC {
            return false;
        }
        control.version.load(Ordering::Acquire) != self.cached_version
    }

    /// Update the cached version after the caller has re-bound to a new snapshot.
    pub fn refresh(&mut self) {
        let control = ControlPage::from_bytes(&self.mmap);
        self.cached_version = control.version.load(Ordering::Acquire);
    }

    fn active_segment_bytes(&self) -> &[u8] {
        let control = ControlPage::from_bytes(&self.mmap);
        let active = control.active_segment.load(Ordering::Acquire) as usize;
        if active > 1 {
            return &[];
        }
        let Ok(len) = usize::try_from(control.active_len.load(Ordering::Acquire)) else {
            return &[];
        };
        let Some(offset) = active
            .checked_mul(self.segment_size)
            .and_then(|offset| CONTROL_SIZE.checked_add(offset))
        else {
            return &[];
        };
        let Some(end) = offset.checked_add(len) else {
            return &[];
        };
        if end > self.mmap.len() {
            return &[];
        }
        &self.mmap[offset..end]
    }

    fn archived(&self) -> Option<&T::Archived> {
        let bytes = self.active_segment_bytes();
        T::check_rkyv_bytes(bytes)
    }
}

impl<T: SnapshotData> SharedDocumentReader<T> {
    /// Public accessor for the archived payload. Used by V4 consumers that
    /// need typed access to `DocumentViewDataV4` entities.
    pub fn payload(&self) -> Option<&T::Archived> {
        self.archived()
    }
}

impl SharedDocumentReader<DocumentViewData> {
    /// V3 convenience accessor for the entity count.
    pub fn entity_count(&self) -> usize {
        self.archived().map(|doc| doc.entities.len()).unwrap_or(0)
    }
}

impl DocumentReader for SharedDocumentReader<DocumentViewData> {
    fn entity_count(&self) -> usize {
        self.archived().map(|doc| doc.entities.len()).unwrap_or(0)
    }

    fn for_each_entity(&self, f: &mut dyn FnMut(ReaderEntity<'_>)) {
        let Some(doc) = self.archived() else { return };
        for entity in doc.entities.iter() {
            let handle = Handle::new(entity.handle);
            let kind = ReaderEntityKind::from_u8(entity.kind);
            let layer_name: &str = entity.layer_name.as_str();
            let point = entity.point.as_ref().map(|p| ReaderPoint {
                x: p.x,
                y: p.y,
                z: p.z,
            });
            f(ReaderEntity {
                handle,
                kind,
                layer_name,
                point,
            });
        }
    }

    fn layer_name(&self, handle: Handle) -> Option<&str> {
        let doc = self.archived()?;
        let handle_val = handle.value();
        doc.layers
            .iter()
            .find(|layer| layer.handle == handle_val)
            .map(|layer| layer.name.as_str())
    }

    fn app_id_name(&self, handle: Handle) -> Option<&str> {
        let doc = self.archived()?;
        let handle_val = handle.value();
        doc.app_ids
            .iter()
            .find(|app| app.handle == handle_val)
            .map(|app| app.name.as_str())
    }
}

/// V3 alias: host-side snapshot store for the simplified document view.
pub type DocumentSnapshotStoreV3 = DocumentSnapshotStore<DocumentViewData>;

/// V3 alias: plugin-side reader for the simplified document view.
pub type SharedDocumentReaderV3 = SharedDocumentReader<DocumentViewData>;

/// Raw control page shared between host and plugin.
#[repr(C, align(8))]
struct ControlPage {
    magic: AtomicU32,
    _pad0: [u8; 4],
    version: AtomicU64,
    active_len: AtomicU64,
    active_segment: AtomicU8,
    _pad1: [u8; 7],
}

impl ControlPage {
    fn from_bytes(mmap: &[u8]) -> &Self {
        assert!(mmap.len() >= std::mem::size_of::<Self>());
        assert_eq!(mmap.as_ptr() as usize % std::mem::align_of::<Self>(), 0);
        unsafe { &*(mmap.as_ptr() as *const Self) }
    }

    fn from_bytes_mut(mmap: &mut [u8]) -> &mut Self {
        assert!(mmap.len() >= std::mem::size_of::<Self>());
        assert_eq!(mmap.as_ptr() as usize % std::mem::align_of::<Self>(), 0);
        unsafe { &mut *(mmap.as_ptr() as *mut Self) }
    }
}

// ── V3 simplified document view ─────────────────────────────────────────────

/// Serializable document view. This is the only data type placed in shared
/// memory, so it must contain no pointers into host memory.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct DocumentViewData {
    pub layers: Vec<LayerView>,
    pub app_ids: Vec<AppIdView>,
    pub entities: Vec<EntityView>,
}

impl From<&CadDocument> for DocumentViewData {
    fn from(doc: &CadDocument) -> Self {
        Self {
            layers: doc.layers.iter().map(LayerView::from).collect(),
            app_ids: doc.app_ids.iter().map(AppIdView::from).collect(),
            entities: doc.entities().map(EntityView::from).collect(),
        }
    }
}

impl SnapshotData for DocumentViewData {
    type Archived = ArchivedDocumentViewData;

    fn to_rkyv_bytes(&self) -> io::Result<Vec<u8>> {
        to_bytes::<_, 256>(self)
            .map(|av| av.into_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    fn check_rkyv_bytes(bytes: &[u8]) -> Option<&Self::Archived> {
        check_archived_root::<Self>(bytes).ok()
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct LayerView {
    pub handle: u64,
    pub name: String,
}

impl From<&acadrust::tables::Layer> for LayerView {
    fn from(layer: &acadrust::tables::Layer) -> Self {
        Self {
            handle: layer.handle.value(),
            name: layer.name.clone(),
        }
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct AppIdView {
    pub handle: u64,
    pub name: String,
}

impl From<&acadrust::tables::AppId> for AppIdView {
    fn from(app_id: &acadrust::tables::AppId) -> Self {
        Self {
            handle: app_id.handle.value(),
            name: app_id.name.clone(),
        }
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct EntityView {
    pub handle: u64,
    pub kind: u8,
    pub layer_name: String,
    pub point: Option<PointView>,
}

impl From<&EntityType> for EntityView {
    fn from(entity: &EntityType) -> Self {
        let handle = entity.common().handle.value();
        let kind = ReaderEntityKind::from_entity(entity).to_u8();
        let layer_name = entity.common().layer.clone();
        let point = match entity {
            EntityType::Point(p) => Some(PointView {
                x: p.location.x,
                y: p.location.y,
                z: p.location.z,
            }),
            _ => None,
        };
        Self {
            handle,
            kind,
            layer_name,
            point,
        }
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy)]
#[archive(check_bytes)]
pub struct PointView {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl ReaderEntityKind {
    /// Convert the simplified kind to a stable `u8` for the shared format.
    pub fn to_u8(self) -> u8 {
        match self {
            ReaderEntityKind::Point => 1,
            ReaderEntityKind::Line => 2,
            ReaderEntityKind::Circle => 3,
            ReaderEntityKind::Arc => 4,
            ReaderEntityKind::Polyline => 5,
            ReaderEntityKind::Text => 6,
            ReaderEntityKind::Other => 0,
        }
    }

    /// Decode a stable `u8` back to the simplified kind.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => ReaderEntityKind::Point,
            2 => ReaderEntityKind::Line,
            3 => ReaderEntityKind::Circle,
            4 => ReaderEntityKind::Arc,
            5 => ReaderEntityKind::Polyline,
            6 => ReaderEntityKind::Text,
            _ => ReaderEntityKind::Other,
        }
    }
}

// ── V4 full-entity document view ────────────────────────────────────────────

/// Serializable V4 document view. The outer structure is rkyv; each entity's
/// `data` is a bincode-encoded `acadrust::EntityType` so that the plugin can
/// reconstruct the full typed entity without relying on acadrust's own rkyv
/// support.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct DocumentViewDataV4 {
    pub layers: Vec<LayerView>,
    pub app_ids: Vec<AppIdView>,
    pub entities: Vec<EntityViewV4>,
}

impl From<&CadDocument> for DocumentViewDataV4 {
    fn from(doc: &CadDocument) -> Self {
        Self {
            layers: doc.layers.iter().map(LayerView::from).collect(),
            app_ids: doc.app_ids.iter().map(AppIdView::from).collect(),
            entities: doc.entities().map(EntityViewV4::from).collect(),
        }
    }
}

impl SnapshotData for DocumentViewDataV4 {
    type Archived = ArchivedDocumentViewDataV4;

    fn to_rkyv_bytes(&self) -> io::Result<Vec<u8>> {
        to_bytes::<_, 256>(self)
            .map(|av| av.into_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    fn check_rkyv_bytes(bytes: &[u8]) -> Option<&Self::Archived> {
        check_archived_root::<Self>(bytes).ok()
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct EntityViewV4 {
    pub handle: u64,
    pub data: Vec<u8>,
}

impl From<&EntityType> for EntityViewV4 {
    fn from(entity: &EntityType) -> Self {
        let handle = entity.common().handle.value();
        let data = bincode::serialize(entity).unwrap_or_default();
        Self { handle, data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{DocumentReader, ReaderEntityKind};
    use acadrust::entities::Point;
    use acadrust::tables::Layer;
    use acadrust::{CadDocument, EntityType};

    fn unique_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ocs_shm_{}_{}_{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn sample_doc() -> CadDocument {
        let mut doc = CadDocument::new();
        doc.layers.add(Layer::new("SURVEY")).unwrap();
        let mut point = Point::from_coords(10.0, 20.0, 5.0);
        point.common.layer = "SURVEY".to_string();
        doc.add_entity(EntityType::Point(point)).unwrap();
        doc
    }

    #[test]
    fn reader_rejects_oversized_snapshot() {
        let path = unique_path("oversized.bin");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(MAX_SNAPSHOT_SIZE as u64 + 1).unwrap();

        let error = match SharedDocumentReader::<DocumentViewData>::open(&path) {
            Ok(_) => panic!("oversized snapshot was mapped"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        drop(file);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reader_rejects_snapshot_without_control_region() {
        let path = unique_path("undersized.bin");
        std::fs::write(&path, vec![0; CONTROL_SIZE - 1]).unwrap();

        let error = match SharedDocumentReader::<DocumentViewData>::open(&path) {
            Ok(_) => panic!("undersized snapshot was mapped"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn store_rejects_oversized_mapping() {
        let error = match DocumentSnapshotStore::<DocumentViewData>::new(0, MAX_SNAPSHOT_SIZE) {
            Ok(_) => panic!("oversized snapshot store was mapped"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn shared_document_reader_roundtrip() {
        let doc = sample_doc();
        let mut store = DocumentSnapshotStore::<DocumentViewData>::new(0, 1024 * 1024).unwrap();
        store.publish(&(&doc).into()).unwrap();

        let reader = SharedDocumentReader::<DocumentViewData>::open(store.path()).unwrap();
        assert_eq!(reader.entity_count(), 1);

        let mut seen = Vec::new();
        reader.for_each_entity(&mut |e| {
            seen.push((e.kind, e.layer_name.to_string(), e.point, e.handle));
        });
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, ReaderEntityKind::Point);
        assert_eq!(seen[0].1, "SURVEY");
        assert_eq!(
            seen[0].2,
            Some(ReaderPoint {
                x: 10.0,
                y: 20.0,
                z: 5.0
            })
        );
        assert!(
            seen[0].3.is_valid(),
            "reader entity should expose a valid handle"
        );
    }

    #[test]
    fn shared_document_reader_updates_after_publish() {
        let doc = sample_doc();
        let mut store = DocumentSnapshotStore::<DocumentViewData>::new(0, 1024 * 1024).unwrap();
        store.publish(&(&doc).into()).unwrap();

        let reader = SharedDocumentReader::<DocumentViewData>::open(store.path()).unwrap();
        assert_eq!(reader.entity_count(), 1);

        let mut doc2 = doc;
        let mut point2 = Point::from_coords(1.0, 2.0, 3.0);
        point2.common.layer = "SURVEY".to_string();
        doc2.add_entity(EntityType::Point(point2)).unwrap();
        store.publish(&(&doc2).into()).unwrap();

        assert_eq!(reader.entity_count(), 2);
    }

    #[test]
    fn layer_name_lookup_by_handle() {
        let doc = sample_doc();
        let mut store = DocumentSnapshotStore::<DocumentViewData>::new(0, 1024 * 1024).unwrap();
        store.publish(&(&doc).into()).unwrap();

        let survey = doc.layers.iter().find(|l| l.name == "SURVEY").unwrap();
        let reader = SharedDocumentReader::<DocumentViewData>::open(store.path()).unwrap();
        assert_eq!(reader.layer_name(survey.handle), Some("SURVEY"));
    }

    #[test]
    fn v4_document_view_roundtrip() {
        let doc = sample_doc();
        let mut store = DocumentSnapshotStore::<DocumentViewDataV4>::new(7, 1024 * 1024).unwrap();
        let view: DocumentViewDataV4 = (&doc).into();
        store.publish(&view).unwrap();

        let reader = SharedDocumentReader::<DocumentViewDataV4>::open(store.path()).unwrap();
        assert_eq!(reader.archived().map(|d| d.entities.len()).unwrap_or(0), 1);

        let archived = reader.archived().unwrap();
        let entity = &archived.entities[0];
        assert_eq!(entity.handle, doc.entities().next().unwrap().common().handle.value());
        let decoded: EntityType = bincode::deserialize(&entity.data).expect("bincode decode");
        assert!(matches!(decoded, EntityType::Point(_)));
    }
}
