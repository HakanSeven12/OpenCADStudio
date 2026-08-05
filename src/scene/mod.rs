// Scene modules grouped by role:
//   convert — DXF/ACIS entities → truck solids & tessellated geometry
//   text    — LFF stroke + TrueType font engines and shaping
//   model   — per-entity GPU render models (wire, hatch, mesh, image, object)
//   pick    — hit-testing, selection, grips, spatial index, xclip
//   view    — camera, transforms, viewport, render pipeline driver
//   cache   — block-definition and property caches
pub mod annotative;
pub mod cache;
pub mod convert;
pub mod creation_style;
pub mod model;
pub mod pick;
pub mod pipeline;
pub(crate) mod render_graph;
pub mod text;
pub mod view;

// Topic submodules split out of this root (each contributes `impl Scene`
// blocks and/or free functions). Pure text-move from the original mod.rs.
mod camera_ops;
mod entity;
mod group_layer;
mod layout;
mod limits;
mod modify;
mod mspace;
mod page_setup;
mod paper;
mod preview;
mod project;
mod scene_markers;
mod selection;

// Parallel tessellation free functions live in `convert::tess` (alongside the
// other tessellation code); re-exported here so this root and sibling topic
// modules (each does `use super::*`) keep referencing them unqualified.
pub(crate) use convert::tess::{
    entity_aabb, entity_world_aabb_f64, is_unindexable_entity, tessellate_entity,
    tessellate_entity_dim_text,
};

/// Result of `Scene::entity_index()`. The wire path queries `tree` for
/// view-rect candidates and also always emits `unbounded_handles`
/// (entities with no usable bbox — legacy `UNBOUNDED_AABB` sentinel).
pub(super) struct EntityIndex {
    pub tree: pick::quadtree::QuadTree,
    pub unbounded_handles: Vec<Handle>,
}

#[derive(Clone, Default)]
struct DependencyTargets {
    render_handles: HashSet<Handle>,
    source_handles: HashSet<Handle>,
    touches_block_definition: bool,
}

#[derive(Default)]
struct SceneDependencyIndex {
    layers: HashMap<String, DependencyTargets>,
    text_styles: HashMap<String, DependencyTargets>,
    dim_styles: HashMap<String, DependencyTargets>,
    object_styles: HashMap<Handle, DependencyTargets>,
    points: DependencyTargets,
    text_geometry: DependencyTargets,
    annotation_geometry: DependencyTargets,
}

fn hatch_interaction_aabb(hatch: &model::hatch_model::HatchModel) -> Option<[f64; 4]> {
    let mut aabb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in hatch.boundary.iter().copied() {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let x = hatch.world_origin[0] + x as f64;
        let y = hatch.world_origin[1] + y as f64;
        aabb[0] = aabb[0].min(x);
        aabb[1] = aabb[1].min(y);
        aabb[2] = aabb[2].max(x);
        aabb[3] = aabb[3].max(y);
    }
    aabb.iter().all(|value| value.is_finite()).then_some(aabb)
}

fn mesh_interaction_aabb(set: &model::mesh_model::MeshLodSet) -> Option<[f64; 6]> {
    let mut aabb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut include = |high: [f32; 3], low: [f32; 3]| {
        let point = [
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        ];
        if point.iter().all(|value| value.is_finite()) {
            for axis in 0..3 {
                aabb[axis] = aabb[axis].min(point[axis]);
                aabb[axis + 3] = aabb[axis + 3].max(point[axis]);
            }
        }
    };
    for mesh in &set.lods {
        for (index, &high) in mesh.verts.iter().enumerate() {
            include(high, mesh.verts_low.get(index).copied().unwrap_or([0.0; 3]));
        }
    }
    for (index, &high) in set.edge_verts.iter().enumerate() {
        include(
            high,
            set.edge_verts_low.get(index).copied().unwrap_or([0.0; 3]),
        );
    }
    aabb.iter().all(|value| value.is_finite()).then_some(aabb)
}

pub use model::hatch_model::HatchModel;
pub use model::image_model::ImageModel;
pub use model::mesh_model::MeshLodSet;
pub use model::object::{GripApply, GripDef};
pub use model::wire_model::WireModel;
pub use pick::selection_state::SelectionState;
pub use pipeline::uniforms::Uniforms;
pub use pipeline::viewcube::{
    hit_test, hit_test_cardinal, hover_id, CubeRegion, NudgeDir, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD,
    VIEWCUBE_PX, VIEWCUBE_REGION_PX, VIEWCUBE_RENDER_PX,
};
use view::camera::Camera;
pub use view::camera::Projection;

use crate::command::EntityTransform;
use acadrust::entities::{Block, BlockEnd, Insert as DxfInsert};
use acadrust::entities::{
    BoundaryEdge, BoundaryPath, Hatch as DxfHatch, PolylineEdge, Solid as DxfSolid,
};
use acadrust::objects::ObjectType;
use acadrust::types::Vector2;
use acadrust::{CadDocument, EntityType, Handle, TableEntry};
use glam;
use truck_modeling::{
    base::{BoundedCurve, ParameterDivision1D},
    BSplineCurve as TruckBSpline, KnotVec, NurbsCurve, Point3, Vector4,
};

use iced::time::Duration;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Session-only object visibility used by ISOLATEOBJECTS / HIDEOBJECTS.
///
/// This is deliberately separate from `EntityCommon::invisible`: that DXF
/// property belongs to the drawing (and dynamic-block visibility states),
/// whereas object isolation must never be serialized.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectIsolationState {
    /// Objects explicitly hidden with HIDEOBJECTS.
    pub hidden: HashSet<Handle>,
    /// Objects retained by ISOLATEOBJECTS. `None` means no isolate filter.
    pub keep: Option<HashSet<Handle>>,
}

impl ObjectIsolationState {
    fn hides(&self, handle: Handle) -> bool {
        self.hidden.contains(&handle)
    }

    fn is_active(&self) -> bool {
        !self.hidden.is_empty() || self.keep.is_some()
    }
}

/// Global counter so every Scene and every geometry mutation gets a
/// process-wide unique epoch. This prevents two different tabs (Scenes)
/// from ever sharing the same epoch value, which would cause the shared
/// GPU Pipeline to skip re-uploading geometry when switching tabs.
static GEOMETRY_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Process-wide monotonic id stamped each time the Model wire set is built.
/// The set is held static across camera moves, so the id stays the same every
/// frame until the geometry epoch changes — it uniquely identifies a wire
/// buffer's *content* across frames. The GPU pipeline gates wire re-upload on
/// it: an unchanged id means the world-space wire buffer is not re-sent.
/// Monotonic (never reused) → free of the ABA hazard a raw `Arc` pointer would
/// carry when an address is freed and reallocated.
static WIRE_CONTENT_GEN: AtomicU64 = AtomicU64::new(1);

/// How a single entity changed in a geometry bump. `Modified` covers both a
/// coordinate edit and a property (colour / layer / linetype) change — anything
/// that alters the entity's tessellated output. A property-only recolour that
/// could touch just the GPU WireConst slot is folded into `Modified` for now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// While an entity-only undoable command runs, this captures the pre-mutation
/// image of every entity the five mutation primitives (add / update / erase /
/// transform / copy) touch, so the app can build a cheap **delta**-undo entry
/// instead of cloning the whole ~800k-entity document (~46 ms). The small set
/// of object-map entries changed by ordinary entity operations (groups and
/// raster-image definitions) is captured alongside the entities. `poisoned`
/// remains the fallback for broader structure changes such as a brand-new
/// layer or a `*D` block record.
#[derive(Default)]
pub struct UndoRecording {
    /// First-touch before-image per handle (the first write for a handle wins,
    /// so repeated touches within one command keep the true pre-command state).
    /// A value of `None` marks an entity *added* by the command (no prior
    /// state). `order` preserves first-touch order for a deterministic delta.
    before: HashMap<Handle, Option<Arc<EntityType>>>,
    order: Vec<Handle>,
    /// First-touch before-images for the exact document.objects entries changed
    /// by the command. `None` denotes a newly-created object.
    object_before: HashMap<Handle, Option<ObjectType>>,
    object_order: Vec<Handle>,
    poisoned: bool,
}

impl UndoRecording {
    /// The command touched non-entity state a pure-entity delta can't restore.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// No entity or object entry was recorded (nothing to undo).
    pub fn is_empty(&self) -> bool {
        self.order.is_empty() && self.object_order.is_empty()
    }

    /// Consume both recording directories in deterministic first-touch order.
    /// A `None` image means the entity/object was added by the command.
    pub fn into_recorded_images(
        mut self,
    ) -> (
        Vec<(Handle, Option<Arc<EntityType>>)>,
        Vec<(Handle, Option<ObjectType>)>,
    ) {
        let entities = self
            .order
            .drain(..)
            .map(|h| (h, self.before.remove(&h).flatten()))
            .collect();
        let objects = self
            .object_order
            .drain(..)
            .map(|h| (h, self.object_before.remove(&h).flatten()))
            .collect();
        (entities, objects)
    }

    /// Entity-only convenience used by the focused Scene delta tests.
    #[cfg(test)]
    pub fn into_before_images(self) -> Vec<(Handle, Option<Arc<EntityType>>)> {
        self.into_recorded_images().0
    }
}

/// One geometry mutation's delta record. `changes` names the exact handles that
/// changed; `full` marks a mutation that can't enumerate its handles (undo/redo,
/// file open, style/display change, any bulk op) and forces every consumer that
/// spans it to fall back to a whole-drawing rebuild. Every `geometry_epoch` bump
/// pushes exactly one of these so the journal is never missing a step.
struct GeometryDelta {
    epoch: u64,
    changes: Vec<(Handle, ChangeKind)>,
    /// Cache-category membership captured immediately before a Removed entity
    /// leaves the document. Presence with value 0 means "known unrelated";
    /// absence means the caller removed it outside the tracked primitives and
    /// consumers must remain conservative.
    removed_categories: HashMap<Handle, u16>,
    full: bool,
}

const CACHE_CATEGORY_ANNOTATIVE: u16 = 1 << 0;
const CACHE_CATEGORY_HATCH: u16 = 1 << 1;
const CACHE_CATEGORY_WIPEOUT: u16 = 1 << 2;
const CACHE_CATEGORY_IMAGE: u16 = 1 << 3;
const CACHE_CATEGORY_MESH: u16 = 1 << 4;
const CACHE_CATEGORY_INTERACTION: u16 = 1 << 5;
const CACHE_CATEGORY_TEXT: u16 = 1 << 6;
const CACHE_CATEGORY_INSERT_HATCH: u16 = 1 << 7;

/// Mutable assembly metadata for one resident wire set. Keeping entity ranges
/// beside the flat render Vec lets a one-entity edit splice that run directly;
/// rebuilding this directory by grouping every WireModel was the dominant
/// edit-time CPU cost on dense drawings.
struct ResidentWireSet {
    epoch: u64,
    gen: u64,
    wires: Arc<Vec<WireModel>>,
    layout: Option<ResidentWireLayout>,
}

struct ResidentWireLayout {
    /// Entity handles in final submission order. A temporarily hidden entity
    /// remains here with no range so grip commit can restore it in place.
    order: Vec<Handle>,
    /// Flat wire range `(start, len)` for each currently visible entity.
    ranges: HashMap<Handle, (usize, usize)>,
    /// Blanked ranges retained in the flat Vec. Undo/Redo and grip hide/show can
    /// restore a same-shaped entity in place without shifting every later run.
    vacant: HashMap<Handle, (usize, usize)>,
    /// Number of blank WireModel slots currently retained. Excessive waste
    /// triggers the normal full-build compaction fallback.
    tombstoned_wires: usize,
    /// First synthesized marker wire. Markers have no entity handle and remain
    /// at the tail while entity runs are inserted/removed before them.
    marker_start: usize,
}

#[derive(Clone)]
pub struct PreparedOpenGeometry {
    pub wires: Arc<Vec<WireModel>>,
    pub interaction_index: Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>>,
}

impl std::fmt::Debug for PreparedOpenGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedOpenGeometry")
            .field("wires", &self.wires.len())
            .field("interaction_index", &self.interaction_index.is_some())
            .finish()
    }
}

#[derive(Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct WireGpuPatch {
    pub(crate) changes: Arc<Vec<(Handle, ChangeKind)>>,
    pub(crate) runs: Arc<HashMap<Handle, Arc<Vec<WireModel>>>>,
    pub(crate) index_edits: Arc<Vec<WireIndexEdit>>,
    pub(crate) new_handles_are_suffix: bool,
    /// Whether Face3D / generic fill buffers can differ after this patch.
    pub(crate) face_pass_changed: bool,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct WireIndexEdit {
    pub(crate) handle: Handle,
    pub(crate) start: usize,
    pub(crate) old_len: usize,
    pub(crate) new_len: usize,
    /// Whether the handle should exist in the selection/text slot index after
    /// the physical edit. Tombstoning keeps `old_len == new_len` but removes it
    /// from the index; restoring the vacant range adds it back without a shift.
    pub(crate) visible: bool,
}

#[derive(Clone, Copy)]
struct DrawDepthEntry {
    handle: u64,
    effective: u64,
    label: i32,
    half_label: i32,
}

// Stable order-maintenance labels. Initial builds occupy the middle half of
// this space, leaving 200 million units at either end. New CAD handles are
// monotonic, so ordinary Add extends with the existing label stride without
// changing any sibling's clip-z bias. The large integer domain preserves enough
// f32 separation for nested block children even in very large drawings.
// Deleted entries remain as tombstones so Undo/Redo restores the same slot.
const DRAW_DEPTH_LABEL_MIN: i32 = -1_000_000_000;
const DRAW_DEPTH_LABEL_MAX: i32 = 1_000_000_000;
const DRAW_DEPTH_INITIAL_MIN: i32 = -800_000_000;
const DRAW_DEPTH_INITIAL_MAX: i32 = 800_000_000;
const DRAW_DEPTH_LABEL_SCALE: f32 = 1.0 / 1_073_741_824.0;

fn draw_depth_value(label: i32, half_label: i32) -> [f32; 2] {
    [
        label as f32 * DRAW_DEPTH_LABEL_SCALE,
        half_label as f32 * DRAW_DEPTH_LABEL_SCALE,
    ]
}

fn seed_draw_depth_entries(order: Vec<(u64, u64)>) -> Vec<DrawDepthEntry> {
    let count = order.len();
    let middle_capacity =
        (DRAW_DEPTH_INITIAL_MAX as i64 - DRAW_DEPTH_INITIAL_MIN as i64 - 1) as usize;
    let (low, high) = if count <= middle_capacity {
        (DRAW_DEPTH_INITIAL_MIN, DRAW_DEPTH_INITIAL_MAX)
    } else {
        (DRAW_DEPTH_LABEL_MIN, DRAW_DEPTH_LABEL_MAX)
    };
    let span = high as i64 - low as i64;
    let step = (span / (count as i64 + 1)).max(1);
    order
        .into_iter()
        .enumerate()
        .map(|(index, (handle, effective))| DrawDepthEntry {
            handle,
            effective,
            label: (low as i64 + step * (index as i64 + 1)) as i32,
            half_label: (step / 2).max(1) as i32,
        })
        .collect()
}

fn inserted_draw_depth_label(order: &[DrawDepthEntry], position: usize) -> Option<(i32, i32)> {
    match (
        position.checked_sub(1).and_then(|index| order.get(index)),
        order.get(position),
    ) {
        (None, None) => Some((0, 1)),
        (Some(left), None) => {
            let stride = order
                .get(position.saturating_sub(2))
                .map(|previous| left.label - previous.label)
                .unwrap_or(left.half_label.saturating_mul(2))
                .max(1);
            let label = left.label.checked_add(stride)?;
            (label <= DRAW_DEPTH_LABEL_MAX).then_some((label, (stride / 2).max(1)))
        }
        (None, Some(right)) => {
            let stride = order
                .get(1)
                .map(|next| next.label - right.label)
                .unwrap_or(right.half_label.saturating_mul(2))
                .max(1);
            let label = right.label.checked_sub(stride)?;
            (label >= DRAW_DEPTH_LABEL_MIN).then_some((label, (stride / 2).max(1)))
        }
        (Some(left), Some(right)) if right.label - left.label > 1 => {
            let label = left.label + (right.label - left.label) / 2;
            let nearest = (label - left.label).min(right.label - label);
            Some((label, (nearest / 2).max(1)))
        }
        _ => None,
    }
}

struct DrawDepthCache {
    epoch: u64,
    depths: Arc<HashMap<u64, [f32; 2]>>,
    /// Per-block stable order labels, including deleted tombstones retained for
    /// symmetric Undo/Redo.
    blocks: HashMap<Handle, Vec<DrawDepthEntry>>,
    /// Reverse owner lookup retained across removal for tombstone restoration.
    owners: HashMap<u64, Handle>,
}

struct PaperViewportCache {
    epoch: u64,
    layout: String,
    layout_block: Handle,
    sheet: Handle,
    content: Arc<Vec<Handle>>,
    paper_limits: Option<((f64, f64), (f64, f64))>,
}

struct PaperSheetRenderCache {
    epoch: u64,
    layout: String,
    selected: u64,
    paper_bg: [f32; 4],
    hatches: Arc<Vec<HatchModel>>,
    wipeouts: Arc<Vec<HatchModel>>,
    images: Arc<Vec<ImageModel>>,
}

/// Bound on the geometry-delta ring. A consumer that fell more than this many
/// mutations behind (or predates the oldest retained delta) can't be replayed
/// and does a one-time full rebuild — the safe fallback, not a correctness hole.
const GEOMETRY_JOURNAL_CAP: usize = 256;

/// Whether the persistent per-entity GPU wire arena (`OCS_WIRE_GPU_PATCH`) is
/// enabled — patches one entity's instance slab on an edit instead of rebuilding
/// the whole wire buffer. The render layer selects indexed-storage or packed
/// arena storage from the active device's wire pipeline.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn wire_gpu_patch_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    // On by default; `OCS_WIRE_GPU_PATCH=0` (or off / false / no) reverts to the
    // batched full-upload path as a kill-switch.
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("OCS_WIRE_GPU_PATCH").ok().as_deref(),
            Some("0") | Some("off") | Some("false") | Some("no")
        )
    })
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_gpu_patch_enabled() -> bool {
    true
}

/// Resolve a viewport's paper-to-model scale ratio from its two
/// DXF-derived sources.
///
/// `view_height` (model-space view extent) is the canonical source — it
/// is what AutoCAD actually uses to draw, and what we keep in sync on
/// every write. `custom_scale` is consulted only when `view_height` is
/// missing or zero (some third-party exporters omit it).
#[inline]
pub fn vp_effective_scale(custom_scale: f64, view_height: f64, vp_height: f64) -> f64 {
    if view_height.abs() > 1e-9 {
        return vp_height / view_height;
    }
    if custom_scale.abs() > 1e-9 {
        return custom_scale;
    }
    1.0
}

/// A block name a user may assign: non-empty, no control characters, none of
/// the symbol-table-reserved characters (`*` marks anonymous blocks, `|`
/// xref-dependent ones; the rest are the DXF symbol-name exclusions).
pub(crate) fn valid_block_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | ',' | '=' | '`'
                )
        })
}

/// Pre-built entity caches returned by [`build_derived_caches`].
/// Produced in the file-load background task so the UI thread only assigns.
#[derive(Debug, Clone)]
pub struct DerivedCaches {
    pub read_stats: Option<acadrust::ReadStats>,
    pub source_sha256: Option<String>,
    pub local_extent_max: f32,
    pub local_center: [f64; 2],
    pub hatches: HashMap<Handle, HatchModel>,
    pub images: HashMap<Handle, ImageModel>,
    pub meshes: HashMap<Handle, MeshLodSet>,
    /// Block-definition solid meshes, block-local frame (instanced per INSERT). (#123)
    pub block_meshes: HashMap<Handle, MeshLodSet>,
    /// Non-graphical DWG object relationships and Drawing-property rows.
    /// Prepared during file open so entity selection/deselection never scans
    /// the complete object store on the UI thread.
    pub object_data: crate::entities::object_data::ObjectDataCache,
    /// Number of entities removed by the corrupt-entity guard during load.
    /// Reported back to the UI so the user knows when a file had parser-junk
    /// entities silently dropped.
    pub corrupt_dropped: usize,
    /// Corrupt entities dropped while resolving referenced drawings.
    pub xref_dropped: usize,
    /// XREF resolution results produced by the loader worker. Keeping these in
    /// the open bundle prevents parsing and merging references on the UI thread.
    pub xrefs: Vec<crate::io::xref::XrefInfo>,
    /// Model wire set and its spatial interaction index, prepared on the loader
    /// thread. Installing these prevents the first visible frame from paying a
    /// whole-drawing tessellation/index build while the progress overlay freezes.
    pub prepared_geometry: Option<PreparedOpenGeometry>,
    /// Background-thread open-phase timings in milliseconds (parse, purge,
    /// derived-cache build). Filled in by `open_path_with_phase`; surfaced in
    /// the open-complete breakdown log so open-time regressions are visible.
    pub timings: OpenTimings,
}

/// Wall-clock breakdown of the file-open phases, in milliseconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenTimings {
    pub parse_ms: u32,
    pub purge_ms: u32,
    pub caches_ms: u32,
    pub xref_ms: u32,
}

/// Build hatch / image / mesh caches from a document without needing `&mut Scene`.
/// Intended to run on a background thread during file load.
#[cfg(target_arch = "wasm32")]
pub fn build_derived_caches(doc: &CadDocument) -> DerivedCaches {
    build_derived_caches_impl(doc, None, None)
}

/// Build open-time caches while reporting monotonic progress in 0..=10000.
///
/// The callback is UI-agnostic and may run from Rayon workers. Callers should
/// keep it cheap, normally just updating atomics.
pub fn build_derived_caches_with_progress(
    doc: &CadDocument,
    progress: &(dyn Fn(u16) + Sync),
    material_base_dir: Option<&std::path::Path>,
) -> DerivedCaches {
    build_derived_caches_impl(doc, Some(progress), material_base_dir)
}

fn build_derived_caches_impl(
    doc: &CadDocument,
    progress: Option<&(dyn Fn(u16) + Sync)>,
    material_base_dir: Option<&std::path::Path>,
) -> DerivedCaches {
    // A new drawing must not inherit the previous one's resolved images — drop
    // the memoised set so each reference re-reads / re-fetches once here (and
    // stays cached across this document's later cache rebuilds).
    crate::scene::model::image_model::clear_image_cache();
    let object_data = crate::entities::object_data::build_cache(doc);
    // model-space block handle (same logic as Scene::model_space_block_handle)
    let model_block = doc
        .objects
        .values()
        .find_map(|obj| {
            if let acadrust::objects::ObjectType::Layout(l) = obj {
                if l.name == "Model" && !l.block_record.is_null() {
                    Some(l.block_record)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            doc.block_records
                .get("*Model_Space")
                .map(|br| br.handle)
                .unwrap_or(Handle::NULL)
        });

    // world_offset selection
    //
    // Header `$EXTMIN`/`$EXTMAX` is the fast path, but it's untrustworthy:
    // the sentinel (1e20 / -1e20) when the writer never computed extents,
    // stale values when a drawing was edited and extents weren't refreshed,
    // and top-level extents that span only an Insert's bounding box rather
    // than the actual MSPACE geometry. Any of those
    // leave the precision-preserving offset wrong, so direct MSPACE
    // entities render at huge magnitudes and f32 wires lose precision.
    //
    // Cross-check the header against a per-entity AABB scan of MSPACE
    // (same `bounding_box()` API and same SANE_EXTENT/zero-placeholder
    // filters that `cache::block_cache::build_defn` already uses for block defns)
    // and prefer the entity-scan when the header center drifts more than
    // 10× its own half-span away from the entity centroid.
    use crate::par::prelude::*;

    // Single pass over entities does triple duty: classify cache-kind handle
    // lists (hatch / image / mesh) AND accumulate per-entity centroids for the
    // world_offset median. Folding the offset scan in here collapses what were
    // two O(N) `entities()` walks (offset scan + handle collection) into one.
    // Heavy tessellation runs in parallel below, reading entities via
    // `doc.get_entity(h)` (O(1) HashMap lookup); no clones in this pass.
    let prep = offset_prep(doc, model_block);
    let mut hatch_handles: Vec<Handle> = Vec::new();
    let mut image_handles: Vec<Handle> = Vec::new();
    let mut mesh_handles: Vec<Handle> = Vec::new();
    let mut centers: Vec<[f64; 3]> = Vec::new();
    let entity_total = doc.entity_count().max(1);
    for (index, e) in doc.entities().enumerate() {
        let h = e.common().handle;
        match e {
            EntityType::Hatch(_) | EntityType::Solid(_) => hatch_handles.push(h),
            EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_) => {
                image_handles.push(h)
            }
            EntityType::Solid3D(_)
            | EntityType::Region(_)
            | EntityType::Body(_)
            | EntityType::Surface(_)
            | EntityType::Mesh(_)
            | EntityType::PolygonMesh(_)
            | EntityType::PolyfaceMesh(_) => mesh_handles.push(h),
            _ => {}
        }
        if let Some(c) = offset_centroid(e, model_block, &prep) {
            centers.push(c);
        }
        if index & 0x1fff == 0 {
            if let Some(progress) = progress {
                progress(((index as u64 * 4000) / entity_total as u64) as u16);
            }
        }
    }
    if let Some(progress) = progress {
        progress(4000);
    }
    let (local_center, local_extent_max) = cluster_extent_from_centers(centers, &doc.header);

    // Default bg adaptation target at load: the model background (paper
    // bg is only relevant after the user enters a paper layout, and
    // `synced_hatch_models` re-runs `render_style` per-frame anyway so
    // the per-layout adaptation kicks in later regardless).
    const LOAD_BG: [f32; 4] = [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0];

    let detail_total = hatch_handles
        .len()
        .saturating_add(image_handles.len())
        .saturating_add(mesh_handles.len())
        .max(1);
    let detail_done = std::sync::atomic::AtomicUsize::new(0);
    let report_detail = |done: usize| {
        if let Some(progress) = progress {
            let value = 4000u64 + done as u64 * 6000 / detail_total as u64;
            progress(value.min(10000) as u16);
        }
    };

    // hatches
    let hatches: HashMap<Handle, HatchModel> = hatch_handles
        .par_iter()
        .filter_map(|&handle| {
            let source = doc.get_entity(handle)?;
            let contextual = annotative::entity_for_annotation_context(
                doc,
                source,
                annotative::scale_handle_by_name(doc, &doc.header.current_annotation_scale),
            );
            let e = contextual.as_ref();
            let (raw, ..) = view::render::render_style_for(doc, e);
            let color = view::render::adapt_to_bg(raw, LOAD_BG);
            let model = match e {
                EntityType::Hatch(dxf) => Scene::hatch_model_from_dxf(dxf, color),
                EntityType::Solid(solid) => Some(Scene::solid_hatch_model(solid, color)),
                _ => None,
            };
            let result = model.map(|m| (handle, m));
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();

    // images
    let images: HashMap<Handle, ImageModel> = image_handles
        .par_iter()
        .filter_map(|&handle| {
            let result = match doc.get_entity(handle)? {
                EntityType::RasterImage(img) => {
                    ImageModel::from_raster_image(img).map(|m| (handle, m))
                }
            EntityType::Ole2Frame(ole) => ImageModel::from_ole2frame(ole).map(|m| (handle, m)),
            EntityType::Underlay(u) => match doc.objects.get(&u.definition_handle) {
                Some(acadrust::objects::ObjectType::UnderlayDefinition(def)) => {
                    ImageModel::from_underlay(u, def).map(|m| (handle, m))
                }
                _ => None,
            },
            _ => None,
            };
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();

    // meshes (parallel tessellation). FACETRES (header.facet_resolution)
    // scales the per-LOD segment counts so users with finer drawings get
    // smoother solids; clamped to AutoCAD's [0.01, 10.0] range inside.
    // Top-level (layout-owned) solids are offset into the render frame; block
    // definition solids keep block-local coords for per-INSERT instancing. (#123)
    let facet_res = doc.header.facet_resolution;
    let isolines = doc.header.isolines.max(0) as usize;
    // Real layout blocks come from the Layout objects' block_record handles —
    // `BlockRecord::is_layout()` is unreliable here (it flags ordinary blocks).
    let layout_blocks: std::collections::HashSet<Handle> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            acadrust::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                Some(l.block_record)
            }
            _ => None,
        })
        .collect();
    let built: Vec<(Handle, MeshLodSet, bool)> = mesh_handles
        .par_iter()
        .filter_map(|&handle| {
            let e = doc.get_entity(handle)?;
            let (raw, ..) = view::render::render_style_for(doc, e);
            let color = view::render::adapt_to_bg(raw, LOAD_BG);
            let material = crate::scene::model::material_model::resolve_material_with_base(
                doc,
                e,
                color,
                None,
                material_base_dir,
            );
            let top_level = layout_blocks.contains(&e.common().owner_handle);
            let result = crate::entities::solid3d::tessellate_volume(e, color, facet_res, isolines)
                .map(|mut mesh| {
                material.apply_to_with_face_overrides(
                    &mut mesh,
                    doc,
                    material_base_dir,
                );
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    &mut mesh,
                    doc,
                    e,
                );
                let mesh = if top_level { offset_mesh_lod_set(mesh) } else { mesh };
                (handle, mesh, top_level)
                });
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();
    let mut meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    let mut block_meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    for (handle, mut m, top_level) in built {
        if top_level {
            meshes.insert(handle, m);
        } else {
            m.prepare_instance_source(handle);
            block_meshes.insert(handle, m);
        }
    }

    if let Some(progress) = progress {
        progress(10000);
    }

    DerivedCaches {
        read_stats: None,
        source_sha256: None,
        local_extent_max,
        local_center,
        hatches,
        images,
        meshes,
        block_meshes,
        object_data,
        corrupt_dropped: 0,
        xref_dropped: 0,
        xrefs: Vec::new(),
        prepared_geometry: None,
        timings: OpenTimings::default(),
    }
}

/// Prepare the expensive first Model wire set and spatial interaction index on
/// the loader thread. The temporary `Scene` never crosses threads (it contains
/// `Rc`/`RefCell` state); only its Send-safe document and immutable prepared
/// geometry are returned.
pub fn prepare_open_geometry(
    doc: CadDocument,
    caches: &DerivedCaches,
    model_bg: [f32; 4],
) -> (CadDocument, PreparedOpenGeometry) {
    let mut scene = Scene::new();
    scene.document = doc;
    scene.local_extent_max = caches.local_extent_max;
    scene.local_center = caches.local_center;
    scene.bg_color = model_bg;
    let cannoscale_value = scene.document.header.annotation_scale_value;
    scene.annotation_scale = if cannoscale_value > 1e-9 {
        (1.0 / cannoscale_value) as f32
    } else {
        1.0
    };
    scene.current_layout = "Model".to_string();
    let camera = scene.camera.borrow().clone();
    let wires = scene.model_tile_wires_arc(0, &camera, 1.0, 1.0);
    let interaction_index = if scene.interaction_index_worthwhile(&wires) {
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
        index.prepare_screen();
        Some(index)
    } else {
        None
    };
    let doc = std::mem::replace(&mut scene.document, CadDocument::new());
    (
        doc,
        PreparedOpenGeometry {
            wires,
            interaction_index,
        },
    )
}

/// Mirrors `cache::block_cache::SANE_EXTENT` — wire coords past this magnitude
/// are treated as corruption rather than precision-relevant geometry.
const CLUSTER_SANE_EXTENT: f64 = 1.0e8;

/// MSPACE-membership prep shared by the world-offset centroid scan.
///
/// The filter here MUST agree with `belongs_to_visible_block` (the
/// render-time filter): if rendering treats an entity as MSPACE but we skip
/// it here, our offset misses on-screen geometry and direct WCS-coordinate
/// wires drag f32 precision to its knees. Conversely, including block-defn
/// entities the render path drops would pull the centroid toward block-local
/// origins.
struct OffsetPrep {
    /// `Some` when the model BlockRecord enumerates its entities; the offset
    /// scan uses this set directly. `None` falls back to the legacy
    /// permissive owner-based interpretation.
    mspace_set: Option<rustc_hash::FxHashSet<Handle>>,
    any_enumerated: bool,
    owned_by_other_block: rustc_hash::FxHashSet<Handle>,
}

fn offset_prep(doc: &acadrust::CadDocument, model_block: Handle) -> OffsetPrep {
    let model_br = doc.block_records.iter().find(|br| br.handle == model_block);
    let mspace_set: Option<rustc_hash::FxHashSet<Handle>> = model_br
        .filter(|br| !br.entity_handles.is_empty())
        .map(|br| br.entity_handles.iter().copied().collect());
    let any_enumerated = doc
        .block_records
        .iter()
        .any(|br| !br.entity_handles.is_empty());
    let owned_by_other_block: rustc_hash::FxHashSet<Handle> = if mspace_set.is_none() {
        doc.block_records
            .iter()
            .filter(|br| br.handle != model_block)
            .flat_map(|br| br.entity_handles.iter().copied())
            .collect()
    } else {
        rustc_hash::FxHashSet::default()
    };
    OffsetPrep {
        mspace_set,
        any_enumerated,
        owned_by_other_block,
    }
}

/// Per-entity centroid for the world-offset scan, or `None` if the entity is
/// not MSPACE geometry / has no usable bbox. Single-outlier-robust because
/// the caller takes the median of these per-entity centroids rather than a
/// global min/max midpoint.
fn offset_centroid(e: &EntityType, model_block: Handle, prep: &OffsetPrep) -> Option<[f64; 3]> {
    let c = e.common();
    let h = c.handle;
    let include = if let Some(ref set) = prep.mspace_set {
        set.contains(&h)
    } else if c.owner_handle == model_block {
        true
    } else if !c.owner_handle.is_null() {
        false
    } else if prep.owned_by_other_block.contains(&h) {
        false
    } else {
        // owner null + h not enumerated by any block: legacy permissive
        // when no block enumerated at all, strict drop otherwise (same
        // as belongs_to_visible_block).
        !prep.any_enumerated
    };
    if !include {
        return None;
    }
    // Skip block-defn sentinels and AttributeDefinition — same as
    // cache::block_cache::build_defn. Their bboxes don't represent drawable
    // MSPACE geometry.
    if matches!(
        e,
        EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::AttributeDefinition(_)
    ) {
        return None;
    }
    let (bmin, bmax) = match e {
        EntityType::Insert(ins) => (ins.insert_point, ins.insert_point),
        _ => {
            let bb = e.as_entity().bounding_box();
            (bb.min, bb.max)
        }
    };
    // Empty-entity placeholder (Polyline/Hatch/Spline/Mesh with no
    // vertices). Including these would pull the centroid toward origin
    // and destroy precision on UTM-authored content.
    if bmin.x == 0.0
        && bmin.y == 0.0
        && bmin.z == 0.0
        && bmax.x == 0.0
        && bmax.y == 0.0
        && bmax.z == 0.0
    {
        return None;
    }
    let cx = (bmin.x + bmax.x) * 0.5;
    let cy = (bmin.y + bmax.y) * 0.5;
    let cz = (bmin.z + bmax.z) * 0.5;
    if !cx.is_finite() || !cy.is_finite() || !cz.is_finite() {
        return None;
    }
    if cx.abs() > CLUSTER_SANE_EXTENT || cy.abs() > CLUSTER_SANE_EXTENT {
        return None;
    }
    Some([cx, cy, cz])
}

/// Pick the model-space precision-preserving offset and the `fit_all`
/// outlier-rejection limit from the collected per-entity `centers`.
///
/// Prefers the entity-centroid median; cross-checks against header
/// `$EXTMIN/$EXTMAX` only as a fallback when the entity scan found nothing.
/// `centers` is gathered by the caller's single entity walk (see
/// [`build_derived_caches`]) so no separate AABB pass is needed.
/// Returns `(center, half_span)` of the dense entity cluster. The center is the
/// median of entity centroids — robust against a second, far cluster (e.g. a
/// small-coordinate legend beside a UTM survey), unlike the raw extents centre
/// which would land in the empty gap between them.
fn cluster_extent_from_centers(
    centers: Vec<[f64; 3]>,
    header: &acadrust::document::HeaderVariables,
) -> ([f64; 2], f32) {
    const SANE_EXTENT: f64 = CLUSTER_SANE_EXTENT;
    let entity_ok = !centers.is_empty();

    // 95th-percentile distance from the median × 2 gives the half-span of the
    // dense cluster while leaving room for legitimate outliers (sparse leaders,
    // dimensions, scattered annotations).
    let median = |v: &mut Vec<f64>| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    let percentile = |v: &mut Vec<f64>, frac: f64| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let i = ((v.len() as f64 - 1.0) * frac).round() as usize;
        v[i]
    };
    let (ecenter, espan_max) = if entity_ok {
        let mut xs: Vec<f64> = centers.iter().map(|c| c[0]).collect();
        let mut ys: Vec<f64> = centers.iter().map(|c| c[1]).collect();
        let mx = median(&mut xs);
        let my = median(&mut ys);
        let mut dx: Vec<f64> = centers.iter().map(|c| (c[0] - mx).abs()).collect();
        let mut dy: Vec<f64> = centers.iter().map(|c| (c[1] - my).abs()).collect();
        let p95 = percentile(&mut dx, 0.95).max(percentile(&mut dy, 0.95));
        ([mx, my], (p95 * 2.0).max(1.0) as f32)
    } else {
        ([0.0, 0.0], 0.0)
    };

    // ── Header extents (fallback only) ───────────────────────────────────
    let hmin = header.model_space_extents_min;
    let hmax = header.model_space_extents_max;
    let header_ok = hmin.x < hmax.x
        && hmin.y < hmax.y
        && hmin.x.abs() < SANE_EXTENT
        && hmax.x.abs() < SANE_EXTENT
        && hmin.y.abs() < SANE_EXTENT
        && hmax.y.abs() < SANE_EXTENT;

    // Geometry reaches the GPU as absolute coordinates (the double-single
    // relative-to-eye path keeps it precise at UTM scale), so only the
    // cluster span — for camera fit and cull — is derived from the content.
    if entity_ok {
        (ecenter, espan_max)
    } else if header_ok {
        let hw = ((hmax.x - hmin.x) * 0.5) as f32;
        let hh = ((hmax.y - hmin.y) * 0.5) as f32;
        let hz = ((hmax.z - hmin.z) * 0.5).max(1.0) as f32;
        let hcenter = [(hmin.x + hmax.x) * 0.5, (hmin.y + hmax.y) * 0.5];
        (hcenter, hw.max(hh).max(hz) * 10.0)
    } else {
        ([0.0, 0.0], 1e9_f32)
    }
}

/// One viewport to render this frame — a camera, the screen rectangle it
/// occupies, and the render mode it draws with. The unified renderer
/// produces a `Vec<ViewportInstance>` for both layouts: a Model layout is
/// one full-canvas instance (or several tiled ones), a paper layout is one
/// instance per floating content viewport. The pipeline draws each in its
/// own scissor pass, so a single shader widget covers every case.
#[derive(Clone)]
pub struct ViewportInstance {
    /// Source viewport entity handle, or `Handle::NULL` for the implicit
    /// full-canvas Model view that has no backing entity yet.
    pub handle: Handle,
    /// Source Model-space tile index, or `None` for paper-layout viewports
    /// (they're identified by `handle` instead). Used as the cache key for
    /// `Scene::model_tile_wires_arc` so each pane reuses its own entry on
    /// camera moves instead of accumulating one per camera hash.
    pub tile_idx: Option<usize>,
    /// Screen rectangle (pixels, canvas-relative) this viewport fills.
    pub screen_rect: iced::Rectangle,
    pub camera: Camera,
    pub render_mode: acadrust::entities::ViewportRenderMode,
    /// `true` when this is the viewport receiving cursor input.
    pub active: bool,
    /// `true` when this view's grid is switched on — drives `grid_views`, so the
    /// grid overlay enumerates the exact same sub-views (tile or floating
    /// viewport) the renderer does, instead of a parallel copy.
    pub grid_on: bool,
    /// `true` for the full-canvas paper "sheet" viewport — the layout's own
    /// view (paper-space entities, top-locked), the paper equivalent of the
    /// Model view. Floating content viewports overlay it.
    pub paper_sheet: bool,
}

/// One pane of the Model-space tiled viewport layout: the normalized screen
/// rectangle it fills and the camera it last had. The active tile uses the
/// live `Scene::camera` (so orbit/pan/zoom drive it); inactive tiles keep a
/// snapshot here, swapped in when they become active.
#[derive(Clone)]
pub(crate) struct ModelTile {
    pub(crate) rect: iced::Rectangle,
    pub(crate) camera: Camera,
    /// Visual style for this tile alone — each pane carries its own so
    /// changing one tile's render mode never touches the others.
    pub(crate) render_mode: acadrust::entities::ViewportRenderMode,
    /// Grid display + grid-snap for this viewport alone, round-tripped through
    /// its VPort entry. The app mirrors the *active* tile's pair into the live
    /// grid/snap toggles. (#121)
    pub(crate) grid_on: bool,
    pub(crate) snap_on: bool,
}

/// Gap (pixels) between Model panes — the `pane_grid` spacing and the visible
/// divider width. The renderer derives tile rects through this same spacing so
/// the drawn viewports line up exactly with the pane_grid layout.
pub const TILE_DIVIDER_PX: f32 = 2.0;

/// Minimum used until the viewport render-mode bar reports its natural width.
pub const MODEL_PANE_MIN_FALLBACK_PX: f32 = 50.0;

/// Shift every vertex of a freshly tessellated `MeshLodSet` into the
/// scene's local f32 space by subtracting `world_offset`. ACIS / SAT
/// tessellation hands us WCS coordinates; the wire / hatch / face3d
/// paths run in `(WCS - world_offset)` so meshes at large UTM-scale
/// origins would otherwise float far away from the rest of the
/// geometry. Also recomputes `world_aabb` so per-frame LOD / cull math
/// uses the same space.
fn offset_mesh_lod_set(mut set: MeshLodSet) -> MeshLodSet {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for lod in &mut set.lods {
        // Reconstruct the f64 absolute position from the double-single pair,
        // subtract world_offset in f64, then re-split into (high, low) so the
        // relative-to-eye shader keeps sub-unit precision at UTM scale.
        let has_low = lod.verts_low.len() == lod.verts.len();
        if !has_low {
            lod.verts_low = vec![[0.0; 3]; lod.verts.len()];
        }
        for (v, vl) in lod.verts.iter_mut().zip(lod.verts_low.iter_mut()) {
            let ax = v[0] as f64 + vl[0] as f64;
            let ay = v[1] as f64 + vl[1] as f64;
            let az = v[2] as f64 + vl[2] as f64;
            let hx = ax as f32;
            let hy = ay as f32;
            let hz = az as f32;
            *v = [hx, hy, hz];
            *vl = [
                (ax - hx as f64) as f32,
                (ay - hy as f64) as f32,
                (az - hz as f64) as f32,
            ];
            if hx < min_x {
                min_x = hx;
            }
            if hy < min_y {
                min_y = hy;
            }
            if hx > max_x {
                max_x = hx;
            }
            if hy > max_y {
                max_y = hy;
            }
        }
    }
    // Re-split the feature edges the same way so they track the mesh at scale.
    {
        let n = set.edge_verts.len();
        if set.edge_verts_low.len() != n {
            set.edge_verts_low = vec![[0.0; 3]; n];
        }
        for (v, vl) in set.edge_verts.iter_mut().zip(set.edge_verts_low.iter_mut()) {
            let ax = v[0] as f64 + vl[0] as f64;
            let ay = v[1] as f64 + vl[1] as f64;
            let az = v[2] as f64 + vl[2] as f64;
            let (hx, hy, hz) = (ax as f32, ay as f32, az as f32);
            *v = [hx, hy, hz];
            *vl = [
                (ax - hx as f64) as f32,
                (ay - hy as f64) as f32,
                (az - hz as f64) as f32,
            ];
        }
    }
    if min_x.is_finite() {
        set.world_aabb = [min_x, min_y, max_x, max_y];
    }
    set.recompute_aabb();
    set
}

/// Instance a block-local mesh into the render frame: apply the accumulated
/// INSERT transform (block-local → world/DXF) then subtract world_offset, so a
/// block scaled at the INSERT renders at the right size. Normals are rotated by
/// the transform's linear part and re-normalized. (#123)
fn transform_block_mesh_lod_set(
    set: &MeshLodSet,
    xform: &acadrust::types::Transform,
) -> MeshLodSet {
    use acadrust::types::Vector3;
    let mut out = set.clone();
    out.instance_transform = Some(*xform);
    let transform_direction = |direction: [f32; 3]| {
        let transformed = xform.apply_rotation(Vector3::new(
            direction[0] as f64,
            direction[1] as f64,
            direction[2] as f64,
        ));
        let length = transformed.length();
        if length > 1e-12 {
            [
                (transformed.x / length) as f32,
                (transformed.y / length) as f32,
                (transformed.z / length) as f32,
            ]
        } else {
            direction
        }
    };
    let scale_x = xform.apply_rotation(Vector3::UNIT_X).length();
    let scale_y = xform.apply_rotation(Vector3::UNIT_Y).length();
    let scale_z = xform.apply_rotation(Vector3::UNIT_Z).length();
    let uniform_scale = (scale_x + scale_y + scale_z) / 3.0;
    let is_uniform = (scale_x - uniform_scale).abs() <= uniform_scale.abs().max(1.0) * 1e-8
        && (scale_y - uniform_scale).abs() <= uniform_scale.abs().max(1.0) * 1e-8
        && (scale_z - uniform_scale).abs() <= uniform_scale.abs().max(1.0) * 1e-8;
    if is_uniform {
        let transform_split = |high: &mut [f32; 3], low: &mut [f32; 3]| {
            let transformed = xform.apply(Vector3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ));
            *high = [
                transformed.x as f32,
                transformed.y as f32,
                transformed.z as f32,
            ];
            *low = [
                (transformed.x - high[0] as f64) as f32,
                (transformed.y - high[1] as f64) as f32,
                (transformed.z - high[2] as f64) as f32,
            ];
        };
        for generator in &mut out.curved_gens {
            match generator {
                crate::scene::model::mesh_model::CurvedGen::Cone {
                    base,
                    base_low,
                    axis,
                    u_dir,
                    v_dir,
                    radius,
                    h_max,
                    ..
                } => {
                    transform_split(base, base_low);
                    *axis = transform_direction(*axis);
                    *u_dir = transform_direction(*u_dir);
                    *v_dir = transform_direction(*v_dir);
                    *radius *= uniform_scale as f32;
                    *h_max *= uniform_scale as f32;
                }
                crate::scene::model::mesh_model::CurvedGen::Sphere {
                    center,
                    center_low,
                    pole,
                    u_dir,
                    v_dir,
                    radius,
                    ..
                } => {
                    transform_split(center, center_low);
                    *pole = transform_direction(*pole);
                    *u_dir = transform_direction(*u_dir);
                    *v_dir = transform_direction(*v_dir);
                    *radius *= uniform_scale as f32;
                }
                crate::scene::model::mesh_model::CurvedGen::Torus {
                    center,
                    center_low,
                    axis,
                    u_dir,
                    v_dir,
                    major,
                    minor,
                    ..
                } => {
                    transform_split(center, center_low);
                    *axis = transform_direction(*axis);
                    *u_dir = transform_direction(*u_dir);
                    *v_dir = transform_direction(*v_dir);
                    *major *= uniform_scale as f32;
                    *minor *= uniform_scale as f32;
                }
            }
        }
    } else {
        out.curved_gens.clear();
    }
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for lod in &mut out.lods {
        let has_low = lod.verts_low.len() == lod.verts.len();
        if !has_low {
            lod.verts_low = vec![[0.0; 3]; lod.verts.len()];
        }
        for (v, vl) in lod.verts.iter_mut().zip(lod.verts_low.iter_mut()) {
            // Reconstruct the block-local f64, apply the INSERT transform and
            // subtract world_offset in f64, then re-split into (high, low).
            let w = xform.apply(Vector3::new(
                v[0] as f64 + vl[0] as f64,
                v[1] as f64 + vl[1] as f64,
                v[2] as f64 + vl[2] as f64,
            ));
            let ax = w.x;
            let ay = w.y;
            let az = w.z;
            let hx = ax as f32;
            let hy = ay as f32;
            let hz = az as f32;
            *v = [hx, hy, hz];
            *vl = [
                (ax - hx as f64) as f32,
                (ay - hy as f64) as f32,
                (az - hz as f64) as f32,
            ];
            if hx < min_x {
                min_x = hx;
            }
            if hy < min_y {
                min_y = hy;
            }
            if hx > max_x {
                max_x = hx;
            }
            if hy > max_y {
                max_y = hy;
            }
        }
        for n in &mut lod.normals {
            let d = xform.apply_rotation(Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64));
            let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
            if len > 1e-12 {
                n[0] = (d.x / len) as f32;
                n[1] = (d.y / len) as f32;
                n[2] = (d.z / len) as f32;
            }
        }
    }
    // Apply the same INSERT transform to the feature edges.
    {
        let n = out.edge_verts.len();
        if out.edge_verts_low.len() != n {
            out.edge_verts_low = vec![[0.0; 3]; n];
        }
        for (v, vl) in out.edge_verts.iter_mut().zip(out.edge_verts_low.iter_mut()) {
            let w = xform.apply(Vector3::new(
                v[0] as f64 + vl[0] as f64,
                v[1] as f64 + vl[1] as f64,
                v[2] as f64 + vl[2] as f64,
            ));
            let (hx, hy, hz) = (w.x as f32, w.y as f32, w.z as f32);
            *v = [hx, hy, hz];
            *vl = [
                (w.x - hx as f64) as f32,
                (w.y - hy as f64) as f32,
                (w.z - hz as f64) as f32,
            ];
        }
    }
    for silhouette in &mut out.stored_silhouettes {
        silhouette.view_direction = transform_direction(silhouette.view_direction);
        silhouette.up_vector = transform_direction(silhouette.up_vector);
        let target = xform.apply(Vector3::new(
            silhouette.target[0] as f64,
            silhouette.target[1] as f64,
            silhouette.target[2] as f64,
        ));
        silhouette.target = [target.x as f32, target.y as f32, target.z as f32];
        let count = silhouette.edge_verts.len();
        if silhouette.edge_verts_low.len() != count {
            silhouette.edge_verts_low = vec![[0.0; 3]; count];
        }
        for (high, low) in silhouette
            .edge_verts
            .iter_mut()
            .zip(silhouette.edge_verts_low.iter_mut())
        {
            let transformed = xform.apply(Vector3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ));
            *high = [
                transformed.x as f32,
                transformed.y as f32,
                transformed.z as f32,
            ];
            *low = [
                (transformed.x - high[0] as f64) as f32,
                (transformed.y - high[1] as f64) as f32,
                (transformed.z - high[2] as f64) as f32,
            ];
        }
    }
    if min_x.is_finite() {
        out.world_aabb = [min_x, min_y, max_x, max_y];
    }
    out.recompute_aabb();
    out
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NavPerfOp {
    Pan,
    Zoom,
    Rotate,
}

impl NavPerfOp {
    pub(in crate::scene) fn label(self) -> &'static str {
        match self {
            Self::Pan => "pan",
            Self::Zoom => "zoom",
            Self::Rotate => "rotate",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::scene) struct NavPerfSample {
    pub(in crate::scene) op: NavPerfOp,
    pub(in crate::scene) space: &'static str,
    pub(in crate::scene) mode: &'static str,
    pub(in crate::scene) started: iced::time::Instant,
    pub(in crate::scene) input_ms: f64,
    pub(in crate::scene) build_ms: f64,
}

#[derive(Clone)]
struct BlockMeshInherit {
    insert_color: [f32; 4],
    layer0_color: [f32; 4],
    insert_material: crate::scene::model::material_model::MeshMaterial,
    layer0_material: crate::scene::model::material_model::MeshMaterial,
}

#[derive(Clone)]
struct SceneLight {
    handle: Handle,
    /// Present when `color` must follow the current ByLayer color.
    color_layer: Option<String>,
    light_type: f32,
    position: [f64; 3],
    direction: [f32; 3],
    color: [f32; 3],
    intensity: f32,
    hotspot_cos: f32,
    falloff_cos: f32,
    attenuation_type: f32,
    attenuation_start: f32,
    attenuation_end: f32,
    cast_shadows: bool,
    shadow_softness: f32,
    shadow_map_size: u32,
    web_profile: [f32; 8],
    web_rotation: [f32; 3],
    web_enabled: bool,
}

pub struct Scene {
    pub camera: Rc<RefCell<Camera>>,
    /// Model-space tiled viewport layout. One full-window tile by default;
    /// the split buttons / VPORTS subdivide the active tile.
    pub(crate) model_tiles: RefCell<Vec<ModelTile>>,
    /// Index of the active model tile (camera input + overlays target it).
    pub(crate) active_model_tile: std::cell::Cell<usize>,
    /// Cache of the gathered SDF text vertex list for one viewport, keyed on
    /// the wire-buffer content id. The glyph quads ride on each entity's wire
    /// (built by the tessellator); this caches the per-viewport gather so it is
    /// not re-walked every frame while the wire set is unchanged. Keyed on the
    /// wire content id so it re-gathers exactly when the wires (geometry or
    /// selection) change. See [`Scene::gather_text_verts`].
    /// Small map (not a single slot): a paper frame renders several wire
    /// sources (sheet + content viewports), each with its own stable content
    /// id — one slot would thrash between them every frame.
    sdf_text_cache:
        RefCell<HashMap<u64, std::sync::Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>>>,
    /// Extra scale representations for highlighted annotative entities. These
    /// stay outside the resident wire set so rollover and selection never
    /// invalidate normal geometry, picking, or snapping caches.
    annotation_highlight_cache: RefCell<HashMap<u64, Arc<Vec<WireModel>>>>,
    /// Per wire-source `(geometry_epoch, gathered text)` of the most recent
    /// SDF-text build. When a new content id misses the cache but the journal
    /// shows no text-bearing entity changed since that epoch, the same glyphs
    /// are reused instead of re-walking every wire — an edit on plain geometry
    /// never rebuilds the text. Keyed by the render source (sheet / tile /
    /// content viewport / implicit view): a paper frame gathers several wire
    /// sets in one frame, and a single slot handed the sheet's glyphs to the
    /// content viewports, hiding every model-space text entity (#403).
    last_sdf_text: RefCell<
        HashMap<
            u64,
            (
                u64,
                std::sync::Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>,
            ),
        >,
    >,
    /// pane_grid layout tree for the Model tab — the source of truth for the
    /// tile split layout, resize and focus. `model_tiles` (the renderer's
    /// per-pane data: camera / render-mode / grid) is kept in lock-step with
    /// it, and its rects are derived from the pane regions. Each pane's value
    /// is the index of its backing `ModelTile`. Paper layout is unaffected.
    /// Plain field (not a `RefCell`) so the view can borrow it for the
    /// `PaneGrid` widget's lifetime; mutated through `&mut Scene` in update.
    pub(crate) model_panes: iced::widget::pane_grid::State<usize>,
    /// Natural width of the Model viewport control bar. `DensitySwap` updates
    /// it during layout; every pane-grid geometry calculation reads it on the
    /// next frame so a pane cannot become narrower than its controls.
    model_pane_min_px: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub selection: Rc<RefCell<SelectionState>>,
    /// The CAD document — single source of truth for all entities.
    pub document: CadDocument,
    /// File-open-prepared index for non-graphical semantic object lookups.
    pub(crate) object_data_cache: crate::entities::object_data::ObjectDataCache,
    /// Native AcDbLight/Sun inputs, separated by rendered block and viewport
    /// frozen-layer signature. Invalidated only when a light entity changes;
    /// ordinary geometry edits never rescan the drawing's lights.
    lighting_cache: RefCell<HashMap<(Handle, u64), Vec<SceneLight>>>,
    /// Currently selected entity handles.
    pub selected: HashSet<Handle>,
    /// Session-only ISOLATEOBJECTS / HIDEOBJECTS state. Never written to DWG/DXF.
    pub object_isolation: ObjectIsolationState,
    /// Entity handles temporarily removed from the base render while an
    /// interactive grip preview draws their live replacement.
    /// Separate from object isolation so a grip can never activate the
    /// isolation status or be captured in its undo state.
    pub preview_hidden: HashSet<Handle>,
    /// Source handles replaced by an active command preview. Kept separate
    /// from grip state so either interaction can clean up only its own hides.
    command_preview_hidden: HashSet<Handle>,
    /// During in-place block edit (REFEDIT), the handles of the entities being
    /// edited. Everything else is rendered faded toward the background so the
    /// edited geometry stands out while the surrounding drawing stays visible
    /// for context. `None` = not editing. (#136)
    pub refedit_keep: Option<HashSet<Handle>>,
    /// Entity drawn with the selection-highlight colour without being part
    /// of the real selection — used to preview a row in the cycling list box.
    pub hover_highlight: Option<Handle>,
    /// Whether entity transparency is honoured on screen. When false the
    /// wire shader forces every line opaque (a uniform toggle, no retessellate).
    pub transparency_display: bool,
    /// Selection filter: entity-type names excluded from interactive picking.
    /// Empty = every type is selectable.
    pub selection_filter: HashSet<String>,
    /// In-progress preview wires while a command is active (rubber-band + object ghosts).
    pub preview_wires: Vec<WireModel>,
    /// Live hatch fill used by interactive edits such as dragging the pattern
    /// origin grip. Kept separate from the resident hatch set so moving one
    /// pattern never re-uploads every hatch in a large drawing.
    pub preview_hatches: Arc<Vec<HatchModel>>,
    /// In-progress preview SDF glyph quads (grip drag / command preview). Rides
    /// a per-frame GPU buffer separate from the epoch-cached base text so the
    /// dragged text stays visible while it's hidden from the base set (#316).
    pub preview_text: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
    /// Committed-segment wire drawn during multi-point commands (normal colour).
    pub interim_wire: Option<WireModel>,
    pub camera_generation: u64,
    /// Incremented whenever geometry-affecting state changes (entities, selection,
    /// preview wires, layer visibility, layout). The GPU pipeline uses this to
    /// skip re-uploading unchanged geometry buffers every frame.
    pub geometry_epoch: u64,
    /// Geometry epoch represented by the model bounds cached on the live
    /// camera. Projection switches refresh those bounds through the same
    /// filtered fit path when this falls behind `geometry_epoch`.
    projection_bounds_epoch: std::cell::Cell<u64>,
    /// Separate epoch for the (expensive) block-definition tessellation cache.
    /// Bumped together with `geometry_epoch` by `bump_geometry`, but NOT by
    /// `bump_geometry_no_blocks` — so edits that provably can't change any
    /// block definition (drawing a top-level entity, grip-moving an
    /// entity/insert) re-tessellate only the visible wires (~baseline cost)
    /// instead of rebuilding every block defn (the edit-time spike).
    pub block_epoch: u64,
    /// Incremented when the selection / hover-highlight set changes WITHOUT a
    /// geometry change. The wire tessellation is selection-independent, so a
    /// pick only refreshes the GPU xray overlay (cheap) instead of bumping
    /// `geometry_epoch` and re-tessellating the whole model.
    pub selection_generation: u64,
    /// Cached tessellation of all visible entity wires for the current layout.
    /// Keyed by `(geometry_epoch, camera_generation)` so a camera change
    /// invalidates the cull-dependent wire list as well as a geometry change.
    /// Uses `Arc` so `build_primitive()` avoids a full Vec clone during navigation.
    /// The middle `u64` is the set's [`WIRE_CONTENT_GEN`] content id.
    wire_cache: RefCell<Option<((u64, u64), u64, Arc<Vec<WireModel>>)>>,
    /// Shared wire + segment broad phase for snapping, hover, click/cycling and
    /// area selection. The source pointer is part of the key because a camera
    /// cull can replace the wire Arc without changing `geometry_epoch`.
    interaction_index_cache: RefCell<
        Vec<(
            u64,
            usize,
            std::sync::Weak<Vec<WireModel>>,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Latest large source requested for off-thread preparation. A matching
    /// cache miss returns no candidates temporarily instead of scanning the
    /// entire scene on the UI thread.
    interaction_index_pending_key: std::cell::Cell<Option<(u64, usize)>>,
    /// Large resident interaction index kept as an immutable base across small
    /// entity edits. Geometry-journal handles form a tombstone/delta overlay;
    /// the exact index is rebuilt only after the overlay grows too large.
    interaction_base_index_cache: RefCell<
        Option<(
            u64,
            u64,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Index of only the live handles changed since the immutable interaction
    /// base. Reused across pointer events so a large unchanged block is never
    /// re-indexed merely because one top-level entity was added or removed.
    interaction_overlay_index_cache: RefCell<
        Option<(
            u64,
            u64,
            u64,
            Arc<Vec<WireModel>>,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Auxiliary broad phase for objects that have no wire representation:
    /// hatch-only and solid-only block instances.
    interaction_handle_index_cache: RefCell<
        Option<(
            u64,
            u64,
            Arc<crate::scene::pick::interaction_index::InteractionHandleIndex>,
        )>,
    >,
    /// Index built from every SortEntitiesTable in the document.
    /// Maps block_handle → (entity_handle.value() → sort_handle.value()).
    /// Replaces the O(objects) linear scan inside `wires_for_block()` with an O(1) lookup.
    sort_cache: RefCell<Option<(u64, HashMap<Handle, HashMap<u64, u64>>)>>,
    /// Per-entity stable draw-order depth keyed by entity handle. Initial order
    /// is assigned sparse integer labels; Add/Delete never renormalizes existing
    /// siblings, so a structural edit updates only the changed entity's GPU
    /// constants. `[depth, half]` retains a fixed child sub-range for block
    /// composition. Full sort/layout/block changes rebuild the labels.
    draw_depth_cache: RefCell<Option<DrawDepthCache>>,
    /// Shared empty map for 3-D wireframe, where true depth wins instead of
    /// the entity submission order used by the optimized 2-D style.
    no_draw_depths: Arc<HashMap<u64, [f32; 2]>>,
    /// Cached hatch fill models, keyed by target block and geometry_epoch. View culling
    /// is handled at draw time via `hatch_skip_flags` in the pipeline,
    /// not at build time — that lets the GPU buffer stay stable across
    /// pan/zoom while still skipping out-of-view hatches.
    /// Keyed by `(geometry_epoch, selection_generation)` — selected hatches
    /// are tinted, so a select/deselect must rebuild even when the geometry
    /// is unchanged (issue #71).
    hatch_cache: RefCell<HashMap<(Handle, String), (u64, u64, Arc<Vec<HatchModel>>)>>,
    /// Cached wipeout fill models, keyed by target block and geometry_epoch. Same
    /// reasoning as `hatch_cache`.
    wipeout_cache: RefCell<HashMap<(Handle, String), (u64, Arc<Vec<HatchModel>>)>>,
    /// Cached image models, keyed by target block and geometry_epoch. No camera
    /// key needed here.
    image_cache: RefCell<HashMap<Handle, (u64, Arc<Vec<ImageModel>>)>>,
    /// Cached mesh models, keyed by target block and geometry_epoch.
    mesh_cache: RefCell<HashMap<(Handle, String), (u64, Arc<Vec<MeshLodSet>>)>>,
    /// Picking mesh source for a non-model active space, keyed by geometry epoch
    /// and interaction block. Model/MSPACE reuse `mesh_cache` directly.
    interaction_mesh_cache: RefCell<Option<(u64, u64, Arc<Vec<MeshLodSet>>)>>,
    /// Direct handle → expanded mesh-set indices for the current renderer mesh
    /// source. Block instances may contribute several sets under one Insert
    /// handle. The weak source guard prevents stale pointer reuse.
    #[allow(clippy::type_complexity)]
    mesh_pick_lookup_cache: RefCell<
        Option<(
            usize,
            std::sync::Weak<Vec<MeshLodSet>>,
            Arc<HashMap<Handle, Vec<u32>>>,
        )>,
    >,
    /// Per-viewport (VP-frozen-layer) filtered fill / image / mesh sets, keyed by
    /// the order-independent signature of the viewport's frozen-layer set. A
    /// content viewport that freezes layers must hide their hatches, 2-D solids,
    /// images/OLE and 3-D solids too — not just wires — so these hold the
    /// filtered variants. Viewports sharing a frozen set share one entry (like
    /// the resident wire set). Empty for a viewport with no frozen layers (it
    /// reuses the unfiltered `*_arc` sets directly).
    frozen_hatch_cache:
        RefCell<HashMap<(Handle, String, u64), (u64, u64, Arc<Vec<HatchModel>>)>>,
    frozen_wipeout_cache:
        RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<HatchModel>>)>>,
    frozen_image_cache: RefCell<HashMap<(Handle, u64), (u64, Arc<Vec<ImageModel>>)>>,
    frozen_mesh_cache: RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<MeshLodSet>>)>>,
    /// Cached block-instance hatches for hit-testing, keyed by geometry_epoch.
    /// The set is geometry-derived, so a camera move or hover never invalidates
    /// the shared graph result.
    insert_hatch_cache: RefCell<Option<(u64, u64, Arc<HashMap<Handle, Vec<HatchModel>>>)>>,
    /// Sheet "dressing" cache over the unified resident set: the paper sheet
    /// drops its own border wire and appends the printable-area guide.
    /// Per-layout `(geometry_epoch, content gen, wires)` — the base is
    /// camera-independent, so paper pan/zoom and Model↔Paper tab switches keep
    /// every already-visited sheet warm.
    paper_sheet_cache: RefCell<HashMap<String, (u64, u64, Arc<Vec<WireModel>>)>>,
    /// Layout viewport handles, collected from the owning block record once per
    /// geometry epoch. Avoids walking every document entity on every Paper
    /// pan/zoom frame just to rediscover the same handful of viewports.
    paper_viewport_cache: RefCell<HashMap<String, PaperViewportCache>>,
    /// Stable Paper sheet fill/image sources. Without this cache every camera
    /// movement scanned the whole document for wipeouts and recreated the same
    /// Arcs, making Paper frame construction CPU-bound on large drawings.
    paper_sheet_render_cache: RefCell<HashMap<String, PaperSheetRenderCache>>,
    /// Per-viewport projected wire cache for paper-space content viewports.
    /// Stores projected + clipped wires in paper-space coordinates.
    /// Maps vp_handle → (geometry_epoch, Vec<WireModel>).
    paper_projected_cache: RefCell<HashMap<Handle, (u64, Vec<WireModel>)>>,
    /// Active layout name — "Model" or a paper space layout name.
    pub current_layout: String,
    /// When `Some`, the active space is a BEDIT block editor (issue #261):
    /// rendering, picking and new-entity ownership are scoped to this block
    /// record's handle instead of the layout's, so only the block's own
    /// (block-local) entities are drawn and edited. `current_layout` stays
    /// "Model" so the rest of the Model-vs-paper code keeps model behaviour.
    pub block_edit_block: Option<Handle>,
    /// UCS→world rotation for the ViewCube, kept in sync with the tab's active
    /// UCS by `DocumentTab::sync_ucs_to_scene`. Identity = WCS. Applied only in
    /// model space so the cube's faces follow the user's coordinate system.
    pub viewcube_ucs: glam::Mat4,
    /// GPU render data for hatch fills, keyed by the DXF entity Handle.
    pub hatches: HashMap<Handle, HatchModel>,
    /// GPU render data for solid meshes (truck Shell/Solid tessellation).
    /// Top-level (layout-owned) solids only, stored in the offset-relative
    /// render frame and drawn flat.
    pub meshes: HashMap<Handle, MeshLodSet>,
    /// Directory of the opened drawing, used to resolve relative AcDbMaterial
    /// bitmap maps. `None` for unsaved drawings and browser uploads.
    pub material_base_dir: Option<std::path::PathBuf>,
    /// Meshes of block-definition solids, kept in *block-local* coordinates
    /// (no world_offset). They are not drawn directly; each INSERT of the
    /// owning block emits a transformed instance so a block placed at an
    /// INSERT scale renders at the right size. (#123)
    pub block_meshes: HashMap<Handle, MeshLodSet>,
    /// Live truck B-reps for solids created this session by the Model tab,
    /// keyed by entity handle. Backs the Design-group boolean tools (a solid
    /// must be here to be combined). Not persisted — rebuilt only by creating
    /// or combining primitives in-session.
    pub solid_models: HashMap<Handle, truck_modeling::Solid>,
    /// GPU render data for raster images (RasterImage entities), keyed by handle.
    pub images: HashMap<Handle, ImageModel>,
    /// The viewport that is currently "entered" (MSPACE mode).
    /// `None` = paper space editing (PSPACE).  Only meaningful when
    /// `current_layout != "Model"`.
    pub active_viewport: Option<Handle>,
    /// Custom model-space background fill color for Wipeout entities.
    /// Set from the active tab's `bg_color`; defaults to dark grey.
    pub bg_color: [f32; 4],
    /// Custom paper-space background fill color for Wipeout entities.
    pub paper_bg_color: [f32; 4],
    /// Largest local-space coordinate expected from real geometry, derived from
    /// EXTMIN/EXTMAX (10× safety margin). Used by fit_all() to ignore garbage
    /// entity coordinates (origin-stuck entities, bad Ray/XLine direction vectors).
    pub local_extent_max: f32,
    /// Robust centre (median of entity centroids) of the dense model-space
    /// cluster. Used together with `local_extent_max` to frame a viewport whose
    /// saved view is missing — aiming at the raw extents centre would land in
    /// the empty gap when a drawing has a second, far cluster.
    pub local_center: [f64; 2],
    /// Current annotation scale (CANNOSCALE equivalent).
    /// Multiplier applied to Text/MText/Dimension sizes during tessellation.
    /// 1.0 = no scaling. 50.0 = "1:50" drawing scale.
    pub annotation_scale: f32,
    /// Cached per-epoch: does annotation scale actually change wire geometry?
    /// PSLTSCALE is handled by a per-viewport GPU uniform, so only real
    /// annotative entities require a scale-specific resident set.
    annotation_affects_wires: std::cell::Cell<Option<(u64, bool)>>,
    /// Cached model-space bounding box, keyed by geometry_epoch.
    /// Avoids re-tessellating all entities on every ZOOM E / auto-fit call.
    model_extents_cache: RefCell<Option<(u64, Option<(glam::Vec3, glam::Vec3)>)>>,
    /// Reverse map: entity_handle → block_record_handle, built from entity_handles lists.
    /// Keyed by geometry_epoch. Eliminates the O(B) fallback scan in belongs_to_visible_block.
    entity_block_map_cache: RefCell<Option<(u64, HashMap<Handle, Handle>)>>,
    /// Reverse dependencies from layer/style/block definitions to the top-level
    /// entities whose resident wire runs actually change. Kept independent from
    /// `geometry_epoch`: a layer colour toggle can reuse the index, invalidate
    /// only its dependants, and avoid a whole-document scan on every toggle.
    dependency_index_cache: RefCell<Option<SceneDependencyIndex>>,
    /// Tessellated block definitions in block-local coords, keyed by render
    /// background and block epoch. Model and Paper adapt black/white colours
    /// differently; retaining both variants prevents a full block rebuild on
    /// every layout-tab switch.
    block_defn_cache: RefCell<HashMap<u64, (u64, Arc<cache::block_cache::BlockCache>)>>,
    /// Spatial index + always-emit list for top-level entities
    /// (Phase 2.1). Lazily rebuilt by `entity_index()` on
    /// `geometry_epoch` change. See `EntityIndex` for what each side
    /// holds and why both are needed.
    entity_index_cache: RefCell<Option<(u64, EntityIndex)>>,
    /// Last viewport aspect ratio captured by the render pipeline. Used by
    /// `view_world_aabb` to compute the world-space view rect on demand.
    last_render_aspect: std::cell::Cell<f32>,
    /// World units that map to one screen pixel at the current camera +
    /// viewport size, captured each render. Drives the LOD pixel-size cull
    /// in expand_insert / tessellate_entity. 0 means "not yet set" — culling
    /// falls back to None.
    last_world_per_pixel: std::cell::Cell<f32>,
    /// ViewCube hover region (0..25, face/edge/corner index), driven by the
    /// `CursorMoved` message that the cube hit-area overlay publishes. Lives
    /// here so the unified render path can read it for the active viewport
    /// without depending on the shader widget's internal `Program::State`
    /// (which can miss events under overlapping overlays).
    pub viewcube_hover: std::cell::Cell<Option<usize>>,
    /// Wall time (ms) of the most recent wire re-tessellation — the work done
    /// on a wire-cache miss in `model_tile_wires_arc` / `paper_sheet_wires_arc`.
    /// Stays at the last value while the cache is hit (idle pan/zoom on a warm
    /// cache reads ~0). Surfaced by the frame-budget HUD (Phase 5.3).
    pub(crate) last_tess_ms: std::cell::Cell<f32>,
    /// Wire count produced by that most recent re-tessellation.
    pub(crate) last_tess_wires: std::cell::Cell<usize>,
    /// Content id ([`WIRE_CONTENT_GEN`]) of the Model wire set returned by the
    /// most recent `model_tile_wires_arc` call — stamped when the static set is
    /// (re)built, otherwise the held value. `build_primitive` reads it right
    /// after the call to gate GPU wire re-upload. 0 = none yet.
    pub(crate) last_model_wire_gen: std::cell::Cell<u64>,
    /// Interaction-LOD state: `camera_generation` seen on the previous frame and
    /// the wall time it last changed. Used to detect "the view is actively being
    /// panned / zoomed / orbited" so the expensive per-pixel hatch pass can be
    /// suppressed while moving and rendered once on settle (held by the
    /// scene-render cache). See [`Scene::navigating_lod`].
    nav_last_gen: std::cell::Cell<u64>,
    nav_changed_at: std::cell::Cell<Option<iced::time::Instant>>,
    /// Latest pan / zoom / rotate event waiting for the renderer. Present only
    /// while PERF tracing is enabled; consumed by the active shader primitive.
    nav_perf_pending: std::cell::Cell<Option<NavPerfSample>>,
    /// Unified static-hold wire cache — ONE infrastructure for EVERY space
    /// (Model tiles, BEDIT block editor, the paper sheet block, and paper
    /// content viewports): the FULL, un-culled, LOD-free tessellation per
    /// `(block, ambient bg, anno scale, vp-frozen set)` key, held resident and
    /// reused for every camera. Each entry carries a stable
    /// [`WIRE_CONTENT_GEN`] id so the GPU upload gate and `render_signature`
    /// can tell "same content" from "changed" — no per-frame re-upload and no
    /// camera-keyed re-tessellation anywhere. Rebuilt per key when
    /// `geometry_epoch` changes; stale entries evicted on insert.
    #[allow(clippy::type_complexity)]
    resident_wire_sets: RefCell<HashMap<u64, ResidentWireSet>>,
    /// Memoized `(face3d, other)` split of a resident wire set, keyed by its
    /// [`WIRE_CONTENT_GEN`] id. `split_face3d_wires` is an O(N) per-wire
    /// handle lookup + clone that otherwise re-runs every frame. A map, not a
    /// slot: a paper frame walks several sources (sheet + viewports).
    ///
    /// `None` for the second element means "no Face3D wire in this set — use
    /// the base set itself". The base `Arc` is deliberately NOT stored here:
    /// `try_resident_patch` can only move a resident set out for an
    /// incremental patch while its `Arc` is uniquely held, so pinning it in
    /// this cache would silently force a full rebuild on every edit (#358).
    #[allow(clippy::type_complexity)]
    split_cache: RefCell<HashMap<u64, (Arc<Vec<WireModel>>, Option<Arc<Vec<WireModel>>>)>>,
    /// Cached `selected ∪ hover` handle set for the GPU xray overlay, keyed by
    /// `selection_generation`. Rebuilt only when the selection changes so
    /// `build_primitive` doesn't clone the set every frame.
    /// Per-entity tessellation memo for the culled Model render path (Phase
    /// 2.2). Maps a top-level handle to its already-tessellated wires so a
    /// single-entity edit re-tessellates only the changed entity and reuses the
    /// rest, instead of re-running the whole model. Keyed implicitly by
    /// `tess_memo_guard` (tol / view / anno / offset / bg); a guard mismatch
    /// (zoom, layout, …) clears it. `bump_geometry` clears it (structural
    /// change); `mark_entity_dirty` drops one handle (incremental edit).
    tess_memo: RefCell<HashMap<Handle, Arc<Vec<WireModel>>>>,
    /// Hash of the tessellation parameters `tess_memo` was built under. When
    /// the current call's parameters differ, the memo is stale and cleared.
    tess_memo_guard: std::cell::Cell<u64>,
    /// Per-entity memo for the **resident** model wire set (`model_tile_wires_arc`,
    /// the one the main GPU render holds). Kept separate from `tess_memo` because
    /// the resident set is camera-INDEPENDENT (no view cull, no zoom LOD), so its
    /// guard depends only on anno-scale / background — it survives pan/zoom, and a
    /// single-entity edit re-tessellates just the changed entity instead of the
    /// whole model. Sharing `tess_memo` would let the camera-dependent culled path
    /// thrash it on every zoom. (#perf)
    resident_tess_memo: RefCell<HashMap<Handle, Arc<Vec<WireModel>>>>,
    /// Guard hash for `resident_tess_memo` (anno-scale / bg only).
    resident_tess_guard: std::cell::Cell<u64>,
    /// Per-mutation delta journal: which handles changed on each `geometry_epoch`
    /// bump. Bounded ring ([`GEOMETRY_JOURNAL_CAP`]); a derived cache replays the
    /// entries past its last-synced epoch and patches per-handle instead of
    /// re-walking the whole document. See [`Scene::replay_since`].
    geometry_deltas: RefCell<std::collections::VecDeque<GeometryDelta>>,
    /// Before-category hints staged by erase/history immediately before the
    /// entity disappears, then consumed by the matching geometry delta.
    pending_removed_categories: RefCell<HashMap<Handle, u16>>,
    /// Highest epoch whose delta has been evicted from the ring. A consumer
    /// synced older than this can't be caught up incrementally → full rebuild.
    geometry_journal_floor: std::cell::Cell<u64>,
    /// Count of resident-set rebuilds served by the incremental patch rather than
    /// a full re-tessellation. Diagnostics + a hook for the oracle test to prove
    /// the fast path is actually exercised (not silently always falling back).
    resident_patch_hits: std::cell::Cell<u64>,
    /// Handoff for the GPU wire arena (`OCS_WIRE_GPU_PATCH`): when the MODEL
    /// resident set was brought up to date by an incremental patch, this records
    /// `(prev_gen, new_gen, changed handles)` so the render layer can patch just
    /// those entities' instance slabs instead of re-uploading every wire. The
    /// render layer matches `new_gen` against the viewport's content id and
    /// `prev_gen` against what the GPU currently holds; a mismatch just rebuilds.
    model_wire_gpu_patch: RefCell<Option<(u64, u64, Arc<WireGpuPatch>)>>,
    /// Active delta-undo recording, or `None` when no entity-only command is
    /// capturing. Populated by the five mutation primitives via
    /// [`Scene::record_undo_before`]; consumed by the app (`take_undo_recording`)
    /// to build a cheap history delta instead of a full document clone. See
    /// [`UndoRecording`].
    undo_recording: Option<UndoRecording>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            camera: Rc::new(RefCell::new(Camera::default())),
            model_tiles: RefCell::new(vec![ModelTile {
                rect: iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                camera: Camera::default(),
                render_mode: acadrust::entities::ViewportRenderMode::Wireframe2D,
                grid_on: false,
                snap_on: false,
            }]),
            active_model_tile: std::cell::Cell::new(0),
            sdf_text_cache: RefCell::new(HashMap::default()),
            annotation_highlight_cache: RefCell::new(HashMap::default()),
            last_sdf_text: RefCell::new(HashMap::default()),
            // One pane mapped to tile 0 — matches the single default tile above.
            model_panes: iced::widget::pane_grid::State::new(0).0,
            model_pane_min_px: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            selection: Rc::new(RefCell::new(SelectionState::default())),
            document: CadDocument::new(),
            object_data_cache: crate::entities::object_data::ObjectDataCache::default(),
            lighting_cache: RefCell::new(HashMap::default()),
            selected: HashSet::default(),
            object_isolation: ObjectIsolationState::default(),
            preview_hidden: HashSet::default(),
            command_preview_hidden: HashSet::default(),
            refedit_keep: None,
            hover_highlight: None,
            transparency_display: true,
            selection_filter: HashSet::default(),
            preview_wires: vec![],
            preview_hatches: Arc::new(Vec::new()),
            preview_text: vec![],
            interim_wire: None,
            camera_generation: 0,
            geometry_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            projection_bounds_epoch: std::cell::Cell::new(0),
            block_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            selection_generation: 0,
            wire_cache: RefCell::new(None),
            interaction_index_cache: RefCell::new(Vec::new()),
            interaction_index_pending_key: std::cell::Cell::new(None),
            interaction_base_index_cache: RefCell::new(None),
            interaction_overlay_index_cache: RefCell::new(None),
            interaction_handle_index_cache: RefCell::new(None),
            sort_cache: RefCell::new(None),
            draw_depth_cache: RefCell::new(None),
            no_draw_depths: Arc::new(HashMap::default()),
            hatch_cache: RefCell::new(HashMap::default()),
            wipeout_cache: RefCell::new(HashMap::default()),
            image_cache: RefCell::new(HashMap::default()),
            mesh_cache: RefCell::new(HashMap::default()),
            interaction_mesh_cache: RefCell::new(None),
            mesh_pick_lookup_cache: RefCell::new(None),
            frozen_hatch_cache: RefCell::new(HashMap::default()),
            frozen_wipeout_cache: RefCell::new(HashMap::default()),
            frozen_image_cache: RefCell::new(HashMap::default()),
            frozen_mesh_cache: RefCell::new(HashMap::default()),
            insert_hatch_cache: RefCell::new(None),
            paper_sheet_cache: RefCell::new(HashMap::default()),
            paper_viewport_cache: RefCell::new(HashMap::default()),
            paper_sheet_render_cache: RefCell::new(HashMap::default()),
            paper_projected_cache: RefCell::new(HashMap::default()),
            current_layout: "Model".to_string(),
            block_edit_block: None,
            viewcube_ucs: glam::Mat4::IDENTITY,
            hatches: HashMap::default(),
            meshes: HashMap::default(),
            material_base_dir: None,
            block_meshes: HashMap::default(),
            solid_models: HashMap::default(),
            images: HashMap::default(),
            active_viewport: None,
            bg_color: [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0],
            paper_bg_color: [1.0, 1.0, 1.0, 1.0],
            local_extent_max: 1e9,
            local_center: [0.0, 0.0],
            annotation_scale: 1.0,
            annotation_affects_wires: std::cell::Cell::new(None),
            model_extents_cache: RefCell::new(None),
            entity_block_map_cache: RefCell::new(None),
            dependency_index_cache: RefCell::new(None),
            block_defn_cache: RefCell::new(HashMap::default()),
            entity_index_cache: RefCell::new(None),
            last_render_aspect: std::cell::Cell::new(16.0 / 9.0),
            last_world_per_pixel: std::cell::Cell::new(0.0),
            viewcube_hover: std::cell::Cell::new(None),
            last_tess_ms: std::cell::Cell::new(0.0),
            last_tess_wires: std::cell::Cell::new(0),
            last_model_wire_gen: std::cell::Cell::new(0),
            nav_last_gen: std::cell::Cell::new(0),
            nav_changed_at: std::cell::Cell::new(None),
            nav_perf_pending: std::cell::Cell::new(None),
            resident_wire_sets: RefCell::new(HashMap::default()),
            split_cache: RefCell::new(HashMap::default()),
            tess_memo: RefCell::new(HashMap::default()),
            tess_memo_guard: std::cell::Cell::new(0),
            resident_tess_memo: RefCell::new(HashMap::default()),
            resident_tess_guard: std::cell::Cell::new(0),
            geometry_deltas: RefCell::new(std::collections::VecDeque::new()),
            pending_removed_categories: RefCell::new(HashMap::default()),
            geometry_journal_floor: std::cell::Cell::new(0),
            resident_patch_hits: std::cell::Cell::new(0),
            model_wire_gpu_patch: RefCell::new(None),
            undo_recording: None,
        }
    }

    /// Called by the render pipeline once per frame so camera fit operations
    /// know the active widget's aspect ratio.
    pub fn set_render_aspect(&self, aspect: f32) {
        if aspect.is_finite() && aspect > 0.0 {
            self.last_render_aspect.set(aspect);
        }
    }

    /// World units per screen pixel at the current viewport size. Returns
    /// `None` until the first render captures real bounds.
    ///
    /// Also returns `None` in paper space: `last_world_per_pixel` tracks the
    /// model camera, so a cached value applied to mm-sheet entity AABBs would
    /// be a stale model-world wpp and cull every paper-space annotation.
    /// Paper-space callers use their own scale instead.
    pub(super) fn world_per_pixel(&self) -> Option<f32> {
        if self.current_layout != "Model" {
            return None;
        }
        let v = self.last_world_per_pixel.get();
        if v > 0.0 && v.is_finite() {
            Some(v)
        } else {
            None
        }
    }

    /// Called from the render path with the current widget bounds so the
    /// LOD pixel-size culler knows how big one world unit projects to.
    pub fn set_render_pixel_scale(&self, width_px: f32, height_px: f32) {
        if !width_px.is_finite() || !height_px.is_finite() || height_px <= 0.0 {
            return;
        }
        let cam = self.camera.borrow();
        // Orthographic only. (Perspective varies with depth — we'd want a
        // depth-aware scale per entity. Skipped for now.)
        let h = cam.ortho_size();
        let world_per_px = (2.0 * h) / height_px;
        if world_per_px.is_finite() && world_per_px > 0.0 {
            self.last_world_per_pixel.set(world_per_px);
        }
    }

    /// Get (or build on miss) the block-definition cache for the current epoch.
    /// Built single-threaded — recursive nested expansion makes parallelization
    /// fiddly and the cache only rebuilds when geometry actually changes.
    pub(super) fn block_cache_arc_for(
        &self,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Arc<cache::block_cache::BlockCache> {
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let mut key = annotation_scale_handle.map(|handle| handle.value()).unwrap_or(0);
        for component in bg {
            key = key.rotate_left(13) ^ component.to_bits() as u64;
        }
        key = key.rotate_left(13) ^ all_visible as u64;
        key = key.rotate_left(13) ^ viewport.map(|handle| handle.value()).unwrap_or(0);
        {
            let cache = self.block_defn_cache.borrow();
            if let Some((epoch, arc)) = cache.get(&key) {
                if *epoch == self.block_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        // Block definitions are cached at block-local size (annotation scale
        // 1.0). An annotative block scales as ONE unit at the INSERT level, so
        // its internal geometry / text / attributes must NOT be scaled
        // individually because that would apply the same scale twice.
        let built = cache::block_cache::BlockCache::build(
            &self.document,
            1.0,
            annotation_scale_handle,
            all_visible,
            bg,
            viewport,
            &self.draw_depth_map(),
        );
        let arc = Arc::new(built);
        let mut cache = self.block_defn_cache.borrow_mut();
        cache.retain(|_, (epoch, _)| *epoch == self.block_epoch);
        cache.insert(key, (self.block_epoch, Arc::clone(&arc)));
        arc
    }

    /// Install loader-thread geometry into this scene's Model resident cache.
    ///
    /// The target scene has a different epoch from the temporary loader scene,
    /// so only immutable geometry is transferred and re-stamped here. The
    /// optional interaction index is cached against the exact same `Arc`.
    pub fn install_prepared_open_geometry(&self, prepared: PreparedOpenGeometry) {
        let block = self.model_space_block_handle();
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        let key = Self::resident_wire_key(
            block,
            self.bg_color,
            None,
            scale,
            self.annotation_all_visible(),
            None,
            None,
        );
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        self.resident_wire_sets.borrow_mut().insert(
            key,
            ResidentWireSet {
                epoch: self.geometry_epoch,
                gen,
                wires: Arc::clone(&prepared.wires),
                layout: None,
            },
        );
        if let Some(index) = prepared.interaction_index {
            self.cache_interaction_index(self.geometry_epoch, prepared.wires, index);
        }
    }

    /// Push one delta onto the journal, evicting the oldest past the cap and
    /// raising the floor so a consumer that fell behind falls back to a full
    /// rebuild. Every `geometry_epoch` bump must call this exactly once so the
    /// journal never has a gap (a gap would let a consumer serve stale geometry).
    fn push_geometry_delta(&self, epoch: u64, changes: Vec<(Handle, ChangeKind)>, full: bool) {
        let pending_categories =
            std::mem::take(&mut *self.pending_removed_categories.borrow_mut());
        let removed_categories = if full {
            HashMap::default()
        } else {
            let mut removed = HashMap::default();
            for &(handle, kind) in &changes {
                if let Some(&bits) = pending_categories.get(&handle) {
                    if matches!(kind, ChangeKind::Removed) {
                        removed.insert(handle, bits);
                    }
                }
            }
            removed
        };
        let mut ring = self.geometry_deltas.borrow_mut();
        ring.push_back(GeometryDelta {
            epoch,
            changes,
            removed_categories,
            full,
        });
        while ring.len() > GEOMETRY_JOURNAL_CAP {
            if let Some(ev) = ring.pop_front() {
                self.geometry_journal_floor.set(ev.epoch);
            }
        }
    }

    /// Coalesce the exact set of handles that changed between a consumer's
    /// last-synced epoch and now, or `None` if the range can't be replayed
    /// (a spanned `full` delta, or the consumer fell off the end of the ring) —
    /// in which case the consumer must rebuild from scratch. Added+Removed of the
    /// same handle cancel; otherwise the last change wins.
    pub(crate) fn replay_since(&self, last_epoch: u64) -> Option<Vec<(Handle, ChangeKind)>> {
        // Anything at or below the floor was evicted — can't catch up.
        if last_epoch < self.geometry_journal_floor.get() {
            return None;
        }
        let ring = self.geometry_deltas.borrow();
        let mut state: HashMap<Handle, ChangeKind> = HashMap::default();
        for d in ring.iter() {
            if d.epoch <= last_epoch {
                continue;
            }
            if d.full {
                return None;
            }
            for &(h, k) in &d.changes {
                use ChangeKind::*;
                match (state.get(&h).copied(), k) {
                    (None, k) => {
                        state.insert(h, k);
                    }
                    // Added then removed within the window ⇒ never existed.
                    (Some(Added), Removed) => {
                        state.remove(&h);
                    }
                    // Added stays Added even after later coordinate edits.
                    (Some(Added), _) => {}
                    // Removed then re-added ⇒ treat as a modify (exists, re-tess).
                    (Some(Removed), Added) => {
                        state.insert(h, Modified);
                    }
                    // Removal dominates a prior modify.
                    (Some(Modified), Removed) => {
                        state.insert(h, Removed);
                    }
                    (Some(Removed), _) => {}
                    (Some(Modified), _) => {}
                }
            }
        }
        Some(state.into_iter().collect())
    }

    fn entity_affects_text_cache(&self, entity: &EntityType) -> bool {
        if matches!(
            entity,
            EntityType::Text(_)
                | EntityType::MText(_)
                | EntityType::Dimension(_)
                | EntityType::MultiLeader(_)
                | EntityType::Leader(_)
                | EntityType::Table(_)
                | EntityType::Tolerance(_)
                | EntityType::AttributeEntity(_)
                | EntityType::AttributeDefinition(_)
                | EntityType::Insert(_)
        ) {
            return true;
        }
        // Complex-linetype glyphs ride the host entity's wire.
        let lt = crate::scene::view::render::linetype_name_for(&self.document, entity);
        crate::io::linetypes::resolve_complex_lt(&self.document, &lt).is_some()
    }

    /// Stage the cache categories an entity belongs to before erase removes the
    /// only cheap way to classify it. The next Removed geometry delta consumes
    /// this entry; even a zero mask is meaningful ("known plain geometry").
    fn remember_removed_cache_categories(&self, handle: Handle) {
        let Some(entity) = self.document.get_entity(handle) else {
            return;
        };
        let mut bits = 0u16;
        if crate::scene::annotative::is_annotative(&self.document, entity) {
            bits |= CACHE_CATEGORY_ANNOTATIVE;
        }
        if self.hatches.contains_key(&handle) {
            bits |= CACHE_CATEGORY_HATCH | CACHE_CATEGORY_INTERACTION;
        }
        if matches!(entity, EntityType::Wipeout(_)) {
            bits |= CACHE_CATEGORY_WIPEOUT;
        }
        if self.images.contains_key(&handle) {
            bits |= CACHE_CATEGORY_IMAGE;
        }
        let is_insert = matches!(entity, EntityType::Insert(_));
        if self.meshes.contains_key(&handle) || self.block_meshes.contains_key(&handle) || is_insert
        {
            bits |= CACHE_CATEGORY_MESH | CACHE_CATEGORY_INTERACTION;
        }
        if is_insert {
            bits |= CACHE_CATEGORY_INSERT_HATCH;
        }
        if self.entity_affects_text_cache(entity) {
            bits |= CACHE_CATEGORY_TEXT;
        }
        self.pending_removed_categories
            .borrow_mut()
            .insert(handle, bits);
    }

    /// True when a per-category derived cache synced at `cached_epoch` remains
    /// valid. Adds/edits are classified from live state; removals use the tiny
    /// pre-erase category mask stored in the journal. Thus deleting a LINE no
    /// longer rebuilds every hatch/image/mesh/text cache merely because its type
    /// became unknowable after removal.
    fn category_cache_valid(
        &self,
        cached_epoch: u64,
        category: u16,
        in_category: impl Fn(Handle) -> bool,
    ) -> bool {
        if cached_epoch == self.geometry_epoch {
            return true;
        }
        if cached_epoch < self.geometry_journal_floor.get() {
            return false;
        }
        let ring = self.geometry_deltas.borrow();
        for delta in ring.iter().filter(|delta| delta.epoch > cached_epoch) {
            if delta.full {
                return false;
            }
            for &(handle, kind) in &delta.changes {
                match kind {
                    ChangeKind::Removed => {
                        let Some(bits) = delta.removed_categories.get(&handle) else {
                            return false;
                        };
                        if bits & category != 0 {
                            return false;
                        }
                    }
                    ChangeKind::Added | ChangeKind::Modified if in_category(handle) => {
                        return false;
                    }
                    ChangeKind::Added | ChangeKind::Modified => {}
                }
            }
        }
        true
    }

    /// True when no text-bearing entity changed since `last_epoch`, so cached SDF
    /// glyphs stay valid. Text comes from Text / MText / Dimension / MultiLeader /
    /// Leader / Table / Tolerance / attributes (incl. ATTDEF) and from block
    /// references (their baked text moves with the instance) — an edit to any of
    /// those, or any removal, invalidates it; a plain line / arc / polyline edit
    /// does not.
    /// Whether the per-entity draw-order labels can be replayed since
    /// `last_epoch`. Add/Remove keep every existing label stable; only a full
    /// structural delta (DRAWORDER, file/layout/block rebuild, journal overflow)
    /// requires consumers to regenerate their depth-bearing data.
    pub(super) fn draw_ranks_stable(&self, last_epoch: u64) -> bool {
        self.replay_since(last_epoch).is_some()
    }

    fn text_unchanged(&self, last_epoch: u64) -> bool {
        self.category_cache_valid(last_epoch, CACHE_CATEGORY_TEXT, |handle| {
            self.document
                .get_entity(handle)
                .is_some_and(|entity| self.entity_affects_text_cache(entity))
        })
    }

    /// Report the exact handles a mutation changed. Folds the memo drop and the
    /// epoch bump so a call site can't bump geometry without recording what
    /// changed — the delta lets every derived cache patch per-handle instead of
    /// re-walking the whole drawing. Blocks are untouched (`block_epoch` stays),
    /// so this is for top-level entity add / edit / erase, not block-defn edits.
    /// Begin capturing before-images for a delta-undo entry. Pair with
    /// [`Scene::take_undo_recording`]. Only entity-only commands — whose
    /// mutations flow through the five primitives (add / update / erase /
    /// transform / copy) and that touch no layers / objects / block records —
    /// may use this; anything else must fall back to a full history snapshot.
    pub fn begin_undo_recording(&mut self) {
        self.undo_recording = Some(UndoRecording::default());
    }

    /// Whether a delta-undo recording is currently open.
    pub fn is_recording_undo(&self) -> bool {
        self.undo_recording.is_some()
    }

    /// Finish and return the open recording (if any) for the app to build a
    /// history delta from.
    pub fn take_undo_recording(&mut self) -> Option<UndoRecording> {
        self.undo_recording.take()
    }

    /// Record the pre-mutation image of `handle` (first touch wins). `before`
    /// is `None` for a freshly added entity. No-op when not recording — the
    /// callers guard with [`Scene::is_recording_undo`] so the clone is skipped
    /// entirely on the common (no-recording) path.
    pub(crate) fn record_undo_before(&mut self, handle: Handle, before: Option<Arc<EntityType>>) {
        if let Some(rec) = self.undo_recording.as_mut() {
            if !rec.before.contains_key(&handle) {
                rec.order.push(handle);
                rec.before.insert(handle, before);
            }
        }
    }

    /// Record one document.objects entry before its first mutation. This keeps
    /// Group erase and RasterImage add on targeted history rather than forcing
    /// a clone of all document structure.
    pub(crate) fn record_undo_object_before(
        &mut self,
        handle: Handle,
        before: Option<ObjectType>,
    ) {
        if let Some(rec) = self.undo_recording.as_mut() {
            if !rec.object_before.contains_key(&handle) {
                rec.object_order.push(handle);
                rec.object_before.insert(handle, before);
            }
        }
    }

    /// Flag the open recording as touching non-entity state (a new layer, a
    /// group edit, a `*D` block record) that a pure-entity delta cannot
    /// restore. The app's per-command predicate keeps this from firing.
    pub(crate) fn poison_undo_recording(&mut self) {
        if let Some(rec) = self.undo_recording.as_mut() {
            rec.poisoned = true;
        }
    }

    /// Re-apply one side of an entity delta to the document. For each
    /// `(handle, before, after)`, install the chosen `target` — `before` when
    /// `undo`, `after` on redo: overwrite the entity in place, re-insert it with
    /// its original handle, or remove it. Returns the exact per-handle change
    /// list; derived-cache reseeding and the geometry bump are deferred so a
    /// multi-step undo/redo can process each final handle only once.
    pub(crate) fn apply_entity_delta(
        &mut self,
        entities: &[(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)],
        undo: bool,
    ) -> Vec<(Handle, ChangeKind)> {
        if entities.iter().any(|(_, before, after)| {
            before
                .as_deref()
                .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
                || after
                    .as_deref()
                    .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
        }) {
            self.lighting_cache.borrow_mut().clear();
        }
        let mut changes: Vec<(Handle, ChangeKind)> = Vec::with_capacity(entities.len());
        for (h, before, after) in entities {
            let target = if undo { before } else { after };
            let existed = self.document.get_entity(*h).is_some();
            match target {
                Some(ent) => {
                    if existed {
                        let _ = self.document.replace_entity_arc(*h, Arc::clone(ent));
                        changes.push((*h, ChangeKind::Modified));
                    } else {
                        // Removal keeps the original block membership in place;
                        // restoring only the flat storage avoids an O(all block
                        // members) scan and cannot duplicate the owner link.
                        let _ = self.document.restore_entity_arc(Arc::clone(ent));
                        changes.push((*h, ChangeKind::Added));
                    }
                }
                None => {
                    if existed {
                        self.remember_removed_cache_categories(*h);
                        self.document.remove_entity_arc(*h);
                        changes.push((*h, ChangeKind::Removed));
                    }
                }
            }
        }

        if !changes.is_empty() {
            self.invalidate_dependency_index();
        }

        changes
    }

    pub fn bump_entities(&mut self, changes: &[(Handle, ChangeKind)]) {
        if !changes.is_empty() {
            // A Modified delta can change layer/style/block references as well
            // as coordinates. Rebuild the reverse dependency map lazily on its
            // next use so later targeted global-property updates never follow
            // stale ownership.
            self.invalidate_dependency_index();
        }
        let cached_light_changed = self.lighting_cache.borrow().values().any(|lights| {
            changes
                .iter()
                .any(|(handle, _)| lights.iter().any(|light| light.handle == *handle))
        });
        let live_light_changed = changes.iter().any(|(handle, _)| {
            self.document
                .get_entity(*handle)
                .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
        });
        if cached_light_changed || live_light_changed {
            for (handle, _) in changes {
                let exists = self
                    .document
                    .get_entity(*handle)
                    .is_some_and(|entity| matches!(entity, EntityType::Light(_)));
                crate::entities::object_data::update_light_entity(
                    &mut self.object_data_cache,
                    *handle,
                    exists,
                );
            }
            self.lighting_cache.borrow_mut().clear();
        }
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        {
            let mut tm = self.tess_memo.borrow_mut();
            let mut rm = self.resident_tess_memo.borrow_mut();
            for &(h, k) in changes {
                // A changed or removed entity must re-tessellate; a fresh Add has
                // no memo entry yet, so there is nothing to drop.
                if matches!(k, ChangeKind::Modified | ChangeKind::Removed) {
                    tm.remove(&h);
                    rm.remove(&h);
                }
            }
        }
        self.push_geometry_delta(epoch, changes.to_vec(), false);
    }

    /// True when any changed handle belongs to an ordinary block definition
    /// rather than model/paper space. History replay needs this distinction:
    /// restoring a block-local entity must invalidate the assembled definition
    /// and every INSERT that consumes it, not only that entity's direct wire.
    ///
    /// Removed entities may no longer have a live slot, so fall back to the
    /// BlockRecord membership list, which deliberately retains their handles
    /// for symmetric undo/redo restoration.
    pub(crate) fn changes_touch_block_definition(
        &self,
        changes: &[(Handle, ChangeKind)],
    ) -> bool {
        let layout_blocks: HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        let is_definition = |record: &acadrust::tables::BlockRecord| {
            let name = record.name.to_ascii_uppercase();
            !layout_blocks.contains(&record.handle)
                && !name.starts_with("*MODEL_SPACE")
                && !name.starts_with("*PAPER_SPACE")
        };

        changes.iter().any(|(handle, _)| {
            let owner = self
                .document
                .get_entity(*handle)
                .map(|entity| entity.common().owner_handle);
            self.document.block_records.iter().any(|record| {
                is_definition(record)
                    && (owner == Some(record.handle)
                        || record.block_entity_handle == *handle
                        || record.block_end_handle == *handle
                        || record.entity_handles.contains(handle))
            })
        })
    }

    pub fn bump_geometry(&mut self) {
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        // A full structural change may move lights between blocks or alter
        // layer visibility without naming the affected handles.
        self.lighting_cache.borrow_mut().clear();
        // Default: also invalidate block definitions. Safe for every caller;
        // operations that know blocks are untouched use `bump_geometry_no_blocks`.
        self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        // Structural change — drop both per-entity tessellation memos.
        self.tess_memo.borrow_mut().clear();
        self.resident_tess_memo.borrow_mut().clear();
        // Can't name the changed handles → force every journal consumer to rebuild.
        self.push_geometry_delta(epoch, Vec::new(), true);
    }

    /// Drop a single entity from the tessellation memo so the next render
    /// re-tessellates just that entity while reusing every other. Pair with
    /// [`bump_geometry_no_blocks`] for an incremental single-entity edit.
    pub fn mark_entity_dirty(&mut self, handle: Handle) {
        self.tess_memo.borrow_mut().remove(&handle);
        self.resident_tess_memo.borrow_mut().remove(&handle);
    }

    /// Invalidate the visible-wire tessellation but KEEP the cached block
    /// definitions. Use only when the edit provably can't change any block
    /// defn (top-level entity create/edit, grip-moving an entity or insert) —
    /// it skips the all-blocks re-tessellation that otherwise spikes edit time.
    ///
    /// Prefer [`bump_entities`] when the changed handles are known: this variant
    /// can't name them, so it pushes a `full` journal delta and every derived
    /// cache falls back to a whole-drawing rebuild. Kept for migration and for
    /// callers whose changed set is genuinely unknowable.
    pub fn bump_geometry_no_blocks(&mut self) {
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        self.lighting_cache.borrow_mut().clear();
        self.push_geometry_delta(epoch, Vec::new(), true);
    }

    /// Mark the selection / hover-highlight set dirty without invalidating the
    /// (selection-independent) wire tessellation. Only the GPU xray overlay is
    /// rebuilt — no re-tessellation. Use this for pure select / deselect /
    /// hover changes; use [`bump_geometry`] when the geometry itself changed.
    pub fn bump_selection(&mut self) {
        self.selection_generation = self.selection_generation.wrapping_add(1);
    }

    /// Milliseconds after the last camera change during which the view counts as
    /// "actively navigating" for interaction-LOD purposes.
    const NAV_SETTLE_MS: u128 = 130;

    /// Interaction LOD: true while the view is actively being panned / zoomed /
    /// orbited. Detected purely from `camera_generation` (bumped by every camera
    /// move) plus a short settle timer, so no navigation call site needs to opt
    /// in. Called on the render path; it stamps the change time as a side effect.
    ///
    /// While this is true the per-pixel hatch pass is skipped (it dominates the
    /// GPU frame — a full-screen procedural pattern/boundary test). When it flips
    /// back to false on settle, the frame renders hatches once and the
    /// scene-render cache holds that image, so a still view is full quality.
    pub fn navigating_lod(&self) -> bool {
        let gen = self.camera_generation;
        if gen != self.nav_last_gen.get() {
            self.nav_last_gen.set(gen);
            self.nav_changed_at.set(Some(iced::time::Instant::now()));
        }
        self.nav_changed_at
            .get()
            .map_or(false, |t| t.elapsed().as_millis() < Self::NAV_SETTLE_MS)
    }

    pub(crate) fn record_nav_perf(&self, op: NavPerfOp, started: iced::time::Instant) {
        if !crate::perf::enabled() {
            return;
        }
        let (space, mode) = if self.current_layout == "Model" {
            ("Model", "MODEL")
        } else if self.active_viewport.is_some() {
            ("Paper", "MSPACE")
        } else {
            ("Paper", "PSPACE")
        };
        self.nav_perf_pending.set(Some(NavPerfSample {
            op,
            space,
            mode,
            started,
            input_ms: started.elapsed().as_secs_f64() * 1000.0,
            build_ms: 0.0,
        }));
    }

    pub(in crate::scene) fn take_nav_perf(&self) -> Option<NavPerfSample> {
        self.nav_perf_pending.take()
    }

    /// Whether the interaction-LOD hatch suppression is enabled (env
    /// `OCS_HATCH_LOD`), read once. Default OFF: the tessellated hatch pass is
    /// cheap enough that suppression — and its zoom flicker (#258) — is not
    /// needed. Kept behind the flag as a safety net for pathological drawings.
    pub fn hatch_lod_enabled(&self) -> bool {
        use std::sync::OnceLock;
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("OCS_HATCH_LOD").is_some())
    }

    /// True for a short window around navigation — used by the subscription to
    /// keep requesting frames just past the settle point so the one full-quality
    /// (hatched) frame actually renders after the cursor stops, even when no
    /// input event would otherwise trigger a redraw. Read-only (no side effect).
    pub fn is_settling(&self) -> bool {
        self.nav_changed_at.get().map_or(false, |t| {
            t.elapsed().as_millis() < Self::NAV_SETTLE_MS + 130
        })
    }

    /// Re-evaluate every cached mesh's color through `render_style` so a
    /// Register a Model-tab solid: cache its truck B-rep (for boolean ops) and
    /// tessellate it into the shaded mesh pipeline under `handle`. The solid is
    /// in the same offset-relative frame the mesh pipeline uses, so the mesh is
    /// stored as-is (Model-tab geometry is authored at world_offset 0).
    pub fn register_solid_model(&mut self, handle: Handle, solid: truck_modeling::Solid) {
        let entity = self.document.get_entity(handle);
        let color = entity
            .map(|e| self.render_style(e).0)
            .unwrap_or([0.8, 0.8, 0.85, 1.0]);
        if let Some(mut set) = crate::scene::model::solid_model::mesh_from_solid(&solid, color) {
            if let Some(entity) = entity {
                crate::scene::model::material_model::resolve_material_with_base(
                    &self.document,
                    entity,
                    color,
                    None,
                    self.material_base_dir.as_deref(),
                )
                .apply_to_with_face_overrides(
                    &mut set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    &mut set,
                    &self.document,
                    entity,
                );
            }
            self.meshes.insert(handle, set);
        }
        self.solid_models.insert(handle, solid);
        // Only this solid's mesh changed — report just its handle so the mesh /
        // wire caches patch it in rather than rebuilding the whole drawing (and
        // so a bulk register loop stays O(n), not O(n²)).
        self.bump_entities(&[(handle, ChangeKind::Modified)]);
    }

    /// `BACKGROUND` change picks up the new `adapt_to_bg` result without
    /// re-tessellating ACIS geometry. Caller must bump `geometry_epoch`
    /// afterwards so the GPU re-uploads the now-updated colour data.
    pub fn recolor_meshes(&mut self) {
        // Cache colour lookups by handle to avoid borrowing the document
        // re-entrantly through `render_style` inside a `&mut self` loop.
        // Covers both top-level solid meshes and block-definition meshes
        // (instanced per INSERT), so a solid recolours wherever it lives.
        // During REFEDIT, solids outside the edited set render faded.
        let bg = self.bg_color;
        let materials: HashMap<Handle, crate::scene::model::material_model::MeshMaterial> = self
            .meshes
            .keys()
            .chain(self.block_meshes.keys())
            .filter_map(|&h| {
                self.document.get_entity(h).map(|e| {
                    let color = self.render_style(e).0;
                    let mut material =
                        crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        e,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                    if let Some(keep) = &self.refedit_keep {
                        if !keep.contains(&h) {
                            material.diffuse = crate::scene::cache::block_cache::fade_toward_bg(
                                material.diffuse,
                                bg,
                            );
                        }
                    }
                    (h, material)
                })
            })
            .collect();
        for (h, set) in self.meshes.iter_mut().chain(self.block_meshes.iter_mut()) {
            if let Some(material) = materials.get(h) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
            }
            if let Some(entity) = self.document.get_entity(*h) {
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    set,
                    &self.document,
                    entity,
                );
            }
        }
    }

    /// Recolour only the named cached solids after a property edit.
    pub fn recolor_meshes_for_handles(&mut self, handles: &[Handle]) {
        let bg = self.bg_color;
        let materials: HashMap<Handle, crate::scene::model::material_model::MeshMaterial> = handles
            .iter()
            .filter_map(|&handle| {
                self.document.get_entity(handle).map(|entity| {
                    let color = self.render_style(entity).0;
                    let mut material =
                        crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        entity,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                    if self
                        .refedit_keep
                        .as_ref()
                        .is_some_and(|keep| !keep.contains(&handle))
                    {
                        material.diffuse = crate::scene::cache::block_cache::fade_toward_bg(
                            material.diffuse,
                            bg,
                        );
                    }
                    (handle, material)
                })
            })
            .collect();
        for handle in handles {
            let Some(material) = materials.get(handle) else {
                continue;
            };
            if let Some(set) = self.meshes.get_mut(handle) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                if let Some(entity) = self.document.get_entity(*handle) {
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        set,
                        &self.document,
                        entity,
                    );
                }
            } else if let Some(set) = self.block_meshes.get_mut(handle) {
                material.apply_to_with_face_overrides(
                    set,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                if let Some(entity) = self.document.get_entity(*handle) {
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        set,
                        &self.document,
                        entity,
                    );
                }
            }
        }
    }

    /// Enter / leave the REFEDIT fade. `keep` holds the edited entities (left
    /// bright); everything else renders faded. Re-tessellates wires and
    /// recolours solids so the change shows immediately. (#136)
    pub fn set_refedit_keep(&mut self, keep: Option<HashSet<Handle>>) {
        self.refedit_keep = keep;
        self.recolor_meshes();
        // Fading is applied after the raw per-entity tessellation memo is read;
        // keep those expensive wires and only rebuild the resident assembly.
        self.bump_geometry_no_blocks();
    }

    /// Fade the colours of wires that belong to entities outside the REFEDIT
    /// keep set (no-op when not editing). The geometry is untouched, so
    /// hit-testing still works on faded entities.
    fn apply_refedit_fade(&self, wires: &mut [WireModel], bg: [f32; 4]) {
        let Some(keep) = &self.refedit_keep else {
            return;
        };
        for w in wires.iter_mut() {
            let keep_bright =
                Self::handle_from_wire_name(&w.name).is_some_and(|h| keep.contains(&h));
            if !keep_bright {
                w.color = crate::scene::cache::block_cache::fade_toward_bg(w.color, bg);
            }
        }
    }

    /// Switch the active layout without pretending the document geometry
    /// changed. Resident wire sets already key on block/background, so keeping
    /// `geometry_epoch` stable lets Model and Paper retain their CPU/GPU data
    /// across tab switches. Only caches whose contents genuinely depend on the
    /// active layout or its background are dropped.
    pub fn set_current_layout(&mut self, name: String) {
        if self.current_layout != name {
            self.current_layout = name;
            self.sync_active_space_to_document();
            self.recolor_meshes();
            *self.wire_cache.borrow_mut() = None;
            *self.interaction_mesh_cache.borrow_mut() = None;
            *self.mesh_pick_lookup_cache.borrow_mut() = None;
            *self.insert_hatch_cache.borrow_mut() = None;
        }
    }

    /// Mirror the active space (Model tile-mode vs a paper layout) into the
    /// document's persisted settings so it round-trips on save: the `$TILEMODE`
    /// header (`show_model_space`) and the `CTAB` current-tab variable. The
    /// reader restores it via [`current_layout`] on open. Called whenever the
    /// active layout changes; the file otherwise always reopened in Model.
    pub fn sync_active_space_to_document(&mut self) {
        self.document.header.show_model_space = self.current_layout == "Model";
        crate::io::set_saved_active_layout(&mut self.document, &self.current_layout);
    }

    /// Returns true if this viewport should display model-space content
    /// (i.e. it is a user viewport, not the sheet/overall viewport).
    ///
    /// Rules:
    /// - id=1  → always the sheet viewport → false
    /// - id≥2  → always a user viewport    → true
    /// - id=0 or id<0 (DWG reader omits the id; some DXF exporters write -1):
    ///   use geometry: the sheet viewport is centred at the paper origin (0,0)
    ///   with scale≈1.0 (view_height ≈ paper-space height).
    pub fn is_content_viewport(vp: &acadrust::entities::Viewport) -> bool {
        if vp.id == 1 {
            return false;
        }
        if vp.id > 1 {
            return true;
        }
        // id ≤ 0: DWG files never write group-code 69 (viewport id), so all
        // viewports arrive with id=0.
        //
        // In DWG format the sheet ("overall") viewport always has its center at
        // the paper-space origin (0, 0). Content viewports are placed at their
        // actual position on the paper and therefore have a non-zero center.
        // Using center position is more reliable than a scale heuristic because
        // the sheet viewport's scale is not always exactly 1:1 (observed: 0.8965
        // in real-world files, which the old 0.02 tolerance missed entirely).
        vp.center.x.abs() >= 0.5 || vp.center.y.abs() >= 0.5
    }

    fn current_layout_sheet_viewport_handle(&self) -> Handle {
        self.document
            .objects
            .values()
            .find_map(|obj| {
                let ObjectType::Layout(layout) = obj else {
                    return None;
                };
                if layout.name == self.current_layout {
                    Some(layout.viewport)
                } else {
                    None
                }
            })
            .unwrap_or(Handle::NULL)
    }

    /// Guarantee that a paper layout has its full-screen overall (`id == 1`)
    /// sheet viewport. AutoCAD always writes one, and `add_layout` creates it,
    /// but this is a safety net for layouts that arrive without it. The sheet
    /// viewport is the authoritative paper-space view and the canvas every
    /// floating viewport overlays.
    pub fn ensure_sheet_viewport(&mut self, layout_name: &str) {
        if layout_name == "Model" {
            return;
        }
        // Locate the layout: its object handle, block-record handle, current
        // sheet-viewport link, and paper limits.
        let info = self.document.objects.iter().find_map(|(h, obj)| {
            if let ObjectType::Layout(l) = obj {
                if l.name == layout_name {
                    return Some((*h, l.block_record, l.viewport, l.min_limits, l.max_limits));
                }
            }
            None
        });
        let Some((layout_handle, block_record, cur_vp, min_lim, max_lim)) = info else {
            return;
        };
        if block_record.is_null() {
            return;
        }

        // Normal files carry a valid direct Layout→Viewport link. This O(1)
        // path is hit on every ordinary layout-tab switch.
        if cur_vp.is_valid()
            && matches!(
                self.document.get_entity(cur_vp),
                Some(EntityType::Viewport(vp)) if vp.common.owner_handle == block_record
            )
        {
            return;
        }

        // Already present? Accept either the linked viewport handle or any
        // `id == 1` viewport owned by the layout block.
        let has_sheet = self.document.entities().any(|e| {
            matches!(e, EntityType::Viewport(vp)
                if vp.common.owner_handle == block_record
                    && (vp.id == 1 || vp.common.handle == cur_vp))
        });
        if has_sheet {
            // Keep the layout's link in sync if it was missing.
            if !cur_vp.is_valid() {
                let h = self.document.entities().find_map(|e| match e {
                    EntityType::Viewport(vp)
                        if vp.common.owner_handle == block_record && vp.id == 1 =>
                    {
                        Some(vp.common.handle)
                    }
                    _ => None,
                });
                if let Some(h) = h {
                    if let Some(ObjectType::Layout(l)) =
                        self.document.objects.get_mut(&layout_handle)
                    {
                        l.viewport = h;
                    }
                }
            }
            return;
        }

        // Create the full-screen overall viewport covering the paper limits.
        let pw = (max_lim.0 - min_lim.0).abs().max(1.0);
        let ph = (max_lim.1 - min_lim.1).abs().max(1.0);
        let mut vp = acadrust::entities::Viewport::new();
        vp.id = 1;
        vp.status = acadrust::entities::ViewportStatusFlags::default_on();
        // Paper-space center is a 2D (x, y) point with z = 0 — the same
        // convention MVIEW uses for floating viewports. AutoCAD/TrueView read
        // the viewport center as (x, y); putting the paper-height midpoint in z
        // (with y = 0) left the sheet view centered at y = 0, shifting the whole
        // layout half a page down. See issue #156.
        vp.center = acadrust::types::Vector3::new(
            (min_lim.0 + max_lim.0) / 2.0,
            (min_lim.1 + max_lim.1) / 2.0,
            0.0,
        );
        vp.width = pw;
        vp.height = ph;
        // Frame the new layout on the whole sheet: look straight down at the
        // paper centre with the visible height a touch taller than the page.
        // Without this the viewport keeps `Viewport::new`'s default view
        // (target 0,0 / height 210), so the first time a fresh drawing's
        // layout is opened the camera sits on the paper's bottom-left corner
        // instead of centring the sheet.
        vp.view_target = acadrust::types::Vector3::new(
            (min_lim.0 + max_lim.0) / 2.0,
            (min_lim.1 + max_lim.1) / 2.0,
            0.0,
        );
        vp.view_center = acadrust::types::Vector3::ZERO;
        vp.view_height = ph * 1.1;
        if let Ok(handle) = self
            .document
            .add_entity_to_layout(EntityType::Viewport(vp), layout_name)
        {
            if let Some(ObjectType::Layout(l)) = self.document.objects.get_mut(&layout_handle) {
                l.viewport = handle;
            }
        }
    }

    fn is_content_viewport_in_layout(
        &self,
        vp: &acadrust::entities::Viewport,
        layout_block: Handle,
    ) -> bool {
        if vp.common.owner_handle != layout_block {
            return false;
        }
        let sheet_handle = self.current_layout_sheet_viewport_handle();
        if sheet_handle.is_valid() {
            vp.common.handle != sheet_handle
        } else {
            Self::is_content_viewport(vp)
        }
    }

    /// Public accessor for the block-record handle of the current layout.
    /// Used by external callers (e.g. `commit_entity`) that need the handle
    /// without going through private API.
    pub fn current_layout_block_handle_pub(&self) -> Handle {
        self.current_layout_block_handle()
    }

    /// True when an entity belongs to the active layout, including imported
    /// DXF entities whose owner handle is NULL but whose BlockRecord still
    /// lists the entity. Keep command validation aligned with the same
    /// ownership fallback used by rendering and hit-testing.
    pub(crate) fn entity_belongs_to_current_layout(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        self.belongs_to_visible_block(
            handle,
            entity.common().owner_handle,
            self.current_layout_block_handle(),
        )
    }

    /// True when an entity belongs to the space receiving newly drawn
    /// entities: BEDIT's block, paper space, or model space (including MSPACE).
    pub(crate) fn entity_belongs_to_active_space(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        let block = if let Some(block) = self.block_edit_block {
            block
        } else if self.current_layout != "Model" && self.active_viewport.is_none() {
            self.current_layout_block_handle()
        } else {
            self.model_space_block_handle()
        };
        self.belongs_to_visible_block(handle, entity.common().owner_handle, block)
    }

    /// Returns the block-record handle for `current_layout`.
    ///
    /// Primary path: the Layout object's `block_record` field (set correctly
    /// by the DWG reader).
    ///
    /// Fallback for DXF files: the DXF reader never reads group code 340
    /// (block_record handle), so `block_record` is NULL after loading DXF.
    /// In that case we derive the block-record name from the DXF convention:
    ///   Model            → "*Model_Space"
    ///   first paper tab  → "*Paper_Space"
    ///   second paper tab → "*Paper_Space0"
    ///   Nth paper tab    → "*Paper_Space{N-2}"
    fn current_layout_block_handle(&self) -> Handle {
        // BEDIT block editor: the active space IS a block record — draw / pick /
        // own only its own (block-local) entities (issue #261).
        if let Some(h) = self.block_edit_block {
            if !h.is_null() {
                return h;
            }
        }
        // Locate the Layout object for the active layout name.
        let layout = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == self.current_layout {
                    Some(l)
                } else {
                    None
                }
            } else {
                None
            }
        });

        if let Some(l) = layout {
            // Fast path: block_record already set (DWG reader).
            if !l.block_record.is_null() {
                return l.block_record;
            }

            // Fallback: resolve via conventional DXF block-record name.
            let br_name: String = if self.current_layout == "Model" {
                "*Model_Space".into()
            } else {
                // tab_order 1 → "*Paper_Space",  2 → "*Paper_Space0", etc.
                let tab = l.tab_order;
                if tab <= 1 {
                    "*Paper_Space".into()
                } else {
                    format!("*Paper_Space{}", tab - 2)
                }
            };

            if let Some(br) = self.document.block_records.get(&br_name) {
                return br.handle;
            }

            // Last resort: match by position among paper layouts when tab_order
            // is unreliable (some exporters set it to 0 for all layouts).
            if self.current_layout != "Model" {
                let mut ps_brs: Vec<_> = self
                    .document
                    .block_records
                    .iter()
                    .filter(|br| br.is_paper_space())
                    .collect();
                ps_brs.sort_by(|a, b| a.name.cmp(&b.name));

                let mut paper_layouts: Vec<(i16, &str)> = self
                    .document
                    .objects
                    .values()
                    .filter_map(|obj| {
                        if let ObjectType::Layout(l) = obj {
                            if l.name != "Model" {
                                Some((l.tab_order, l.name.as_str()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                paper_layouts.sort_by_key(|(o, n)| (*o, *n));

                if let Some(pos) = paper_layouts
                    .iter()
                    .position(|(_, n)| *n == self.current_layout)
                {
                    if let Some(br) = ps_brs.get(pos) {
                        return br.handle;
                    }
                }
            } else if let Some(br) = self.document.block_records.get("*Model_Space") {
                return br.handle;
            }
        }

        Handle::NULL
    }

    /// Returns `(min, max)` paper-space limits for the current layout, or `None`
    /// when in Model space.  Falls back to A4 landscape if nothing reliable is found.
    /// A solid white fill covering the paper sheet's printable area, rendered
    /// by the GPU hatch pipeline behind the paper entities. Replaces the 2-D
    /// white-rectangle the old PaperCanvas drew. `None` in model space or when
    /// the layout has no limits.
    pub(super) fn paper_sheet_fill(&self) -> Option<HatchModel> {
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let (x0, y0, x1, y1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        Some(HatchModel {
            world_origin: [0.0, 0.0],
            boundary: Arc::new(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]),
            boundary_wcs: None,
            pattern: crate::scene::model::hatch_model::HatchPattern::Solid,
            name: "SOLID".to_string(),
            color: self.paper_bg_color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            // Draw-order bias is signed: entity fills/wires land in (-1, 1)
            // (0 = neutral). A value below -1 forces the sheet strictly behind
            // EVERY object, in every case, with a tiny z offset (BIAS = 0.001,
            // so no far-plane clipping). The sheet is the canvas, never on top.
            draw_depth: -2.0,
        })
    }

    /// Dashed rectangle marking the printable area — the paper inset by the
    /// layout's plot margins. AutoCAD draws this guide on every layout; with the
    /// margins now preserved we can reflect it too. `None` in model space, when
    /// the layout has no margins, or when the inset would be degenerate.
    pub(super) fn printable_area_wire(&self) -> Option<WireModel> {
        if self.current_layout == "Model" {
            return None;
        }
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let (left, bottom, right, top, rot) = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == self.current_layout {
                    return Some((
                        l.plot_margin_left,
                        l.plot_margin_bottom,
                        l.plot_margin_right,
                        l.plot_margin_top,
                        l.plot_rotation,
                    ));
                }
            }
            None
        })?;
        // `paper_limits()` already swaps the sheet for a 90°/270° rotation, so the
        // margins must rotate to the same edges: a margin on a physical side moves
        // to the displayed side that side rotates onto.
        let (ml, mb, mr, mt) = match rot {
            1 | 3 => (bottom, left, top, right),
            2 => (right, top, left, bottom),
            _ => (left, bottom, right, top),
        };
        // Plot margins are millimetres like the paper size; scale them into the
        // layout's paper-space units so the inset matches the (already scaled)
        // sheet rect. Without this an inch paper space insets an ~8-inch sheet
        // by ~6 "inches" of margin and the printable rect collapses.
        let f = self.paper_space_unit_factor();
        let (ml, mb, mr, mt) = (ml * f, mb * f, mr * f, mt * f);
        // Nothing to show when there are no margins (printable area == sheet).
        if ml <= 0.0 && mb <= 0.0 && mr <= 0.0 && mt <= 0.0 {
            return None;
        }
        let (px0, py0, px1, py1) = (x0 + ml, y0 + mb, x1 - mr, y1 - mt);
        if px1 - px0 < 1e-3 || py1 - py0 < 1e-3 {
            return None;
        }
        let (px0, py0, px1, py1) = (px0 as f32, py0 as f32, px1 as f32, py1 as f32);
        let mut wire = WireModel::solid(
            "paper_printable_area".to_string(),
            vec![
                [px0, py0, 0.0],
                [px1, py0, 0.0],
                [px1, py1, 0.0],
                [px0, py1, 0.0],
                [px0, py0, 0.0],
            ],
            [0.5, 0.5, 0.5, 1.0],
            false,
        );
        // Dashed: 4 mm dash, 3 mm gap. The dash lengths are millimetres, so
        // scale them into the layout's paper-space units too (`f`) — otherwise
        // an inch paper space draws 4-"inch" dashes (25.4× too long).
        let ff = f as f32;
        wire.pattern_length = 7.0 * ff;
        wire.pattern = [4.0 * ff, -3.0 * ff, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        Some(wire)
    }

    /// The effective plot settings for the current layout: a standalone
    /// PlotSettings page setup if one exists, otherwise the settings embedded in
    /// the LAYOUT object (paper size, margins, origin, rotation, scale). Loaded
    /// AutoCAD files keep their settings embedded, so without this fallback the
    /// plot/PDF path would ignore the file's rotation, origin and scale.
    pub fn effective_plot_settings(&self) -> Option<acadrust::objects::PlotSettings> {
        self.plot_settings_for(&self.current_layout)
    }

    /// Plot settings for a specific layout by name: its standalone
    /// `PlotSettings` object if one exists, else synthesized from the `Layout`
    /// object's embedded fields.
    pub fn plot_settings_for(&self, name: &str) -> Option<acadrust::objects::PlotSettings> {
        use acadrust::objects::{
            ObjectType, PaperMargin, PlotPaperUnits, PlotRotation, PlotSettings, PlotType,
            PlotWindow, ScaledType,
        };
        if let Some(ps) = self.document.objects.values().find_map(|o| {
            if let ObjectType::PlotSettings(ps) = o {
                if ps.page_name.as_str() == name {
                    return Some(ps.clone());
                }
            }
            None
        }) {
            return Some(ps);
        }
        self.document.objects.values().find_map(|o| {
            let ObjectType::Layout(l) = o else {
                return None;
            };
            if l.name.as_str() != name {
                return None;
            }
            let mut ps = PlotSettings::new(l.name.clone());
            ps.paper_width = l.paper_width;
            ps.paper_height = l.paper_height;
            ps.paper_size = l.paper_size.clone();
            ps.margins = PaperMargin::new(
                l.plot_margin_left,
                l.plot_margin_bottom,
                l.plot_margin_right,
                l.plot_margin_top,
            );
            ps.origin_x = l.plot_origin_x;
            ps.origin_y = l.plot_origin_y;
            ps.plot_window = PlotWindow::new(
                l.plot_window_min_x,
                l.plot_window_min_y,
                l.plot_window_max_x,
                l.plot_window_max_y,
            );
            ps.paper_units = PlotPaperUnits::from_code(l.plot_paper_units);
            ps.rotation = PlotRotation::from_code(l.plot_rotation);
            ps.plot_type = PlotType::from_code(l.plot_type);
            ps.scale_type = ScaledType::from_code(l.plot_scale_type);
            ps.scale_numerator = l.plot_scale_numerator;
            ps.scale_denominator = l.plot_scale_denominator;
            Some(ps)
        })
    }

    /// PlotSettings store the paper size and plot margins in millimetres, but a
    /// layout's paper space is laid out in its own units — its viewports,
    /// drawing limits and the sheet the user sees can be in inches, millimetres,
    /// or an arbitrary scaled unit. Return the factor that converts those
    /// millimetre plot-settings values into the active layout's paper-space
    /// units (`1.0` for a millimetre paper space, `1/25.4` for an inch one, and
    /// anything else for a scaled layout).
    ///
    /// The unit is derived from the plot scale, which maps paper-space units to
    /// the physical page: `mm_per_unit = (scale_num / scale_den) × page_unit_mm`
    /// where `page_unit_mm` is 25.4 for an inch page (code 72 == 0) or 1 for a
    /// millimetre page. The scale and the page unit are a matched pair, so this
    /// is correct even when the page unit alone looks wrong (an inch page at
    /// scale 1:25.4 is a millimetre paper space). Falls back to `1.0` when the
    /// scale is missing or degenerate.
    pub fn paper_space_unit_factor(&self) -> f64 {
        if self.current_layout == "Model" {
            return 1.0;
        }
        self.document
            .objects
            .values()
            .find_map(|obj| match obj {
                ObjectType::Layout(l) if l.name == self.current_layout => {
                    Some(Self::layout_unit_factor(l))
                }
                _ => None,
            })
            .unwrap_or(1.0)
    }

    /// Millimetre → paper-space-unit factor for a specific layout (see
    /// [`Scene::paper_space_unit_factor`]).
    fn layout_unit_factor(l: &acadrust::objects::Layout) -> f64 {
        // Millimetres represented by one plotted page unit: an inch page
        // (plot_paper_units == 0) is 25.4 mm, a millimetre page is 1 mm.
        let page_unit_mm = if l.plot_paper_units == 0 { 25.4 } else { 1.0 };
        let (num, den) = (l.plot_scale_numerator, l.plot_scale_denominator);
        if num > 1e-12 && den > 1e-12 {
            // One paper-space unit spans this many millimetres on the page.
            let mm_per_unit = (num / den) * page_unit_mm;
            if mm_per_unit > 1e-9 {
                return 1.0 / mm_per_unit;
            }
        }
        // No usable plot scale — assume the paper space is already millimetres.
        1.0
    }

    pub fn paper_limits(&self) -> Option<((f64, f64), (f64, f64))> {
        if self.current_layout == "Model" {
            return None;
        }

        if let Some(cache) = self.paper_viewport_cache.borrow().get(&self.current_layout) {
            if cache.epoch == self.geometry_epoch && cache.layout == self.current_layout {
                return cache.paper_limits;
            }
        }

        self.document
            .objects
            .values()
            .find_map(|obj| {
                if let ObjectType::Layout(l) = obj {
                    if l.name != self.current_layout {
                        return None;
                    }

                    // Use the physical paper dimensions from PlotSettings if available
                    // (populated from DWG embedded plot settings or DXF codes 44/45/73).
                    // Rotation 1=90° or 3=270° → swap width and height.
                    if l.paper_width > 1e-6 && l.paper_height > 1e-6 {
                        let (pw, ph) = if l.plot_rotation == 1 || l.plot_rotation == 3 {
                            (l.paper_height, l.paper_width)
                        } else {
                            (l.paper_width, l.paper_height)
                        };
                        // paper_width/height are millimetres; scale them into
                        // the layout's paper-space units so the sheet lands in
                        // the same coordinate system as the viewports drawn on
                        // it. An inch-based paper space would otherwise get a
                        // 25.4× oversized sheet, shrinking the content into the
                        // corner.
                        let f = Self::layout_unit_factor(l);
                        let (pw, ph) = (pw * f, ph * f);
                        let ox = l.min_limits.0.min(0.0);
                        let oy = l.min_limits.1.min(0.0);
                        return Some(((ox, oy), (ox + pw, oy + ph)));
                    }

                    // Fall back to the Layout's drawing limits.
                    let (min, max) = (l.min_limits, l.max_limits);
                    let w = (max.0 - min.0).abs();
                    let h = (max.1 - min.1).abs();
                    if w < 1e-6 || h < 1e-6 {
                        return Some(((0.0, 0.0), (297.0, 210.0)));
                    }
                    Some((min, max))
                } else {
                    None
                }
            })
            .or(Some(((0.0, 0.0), (297.0, 210.0))))
    }

    /// Printable rectangle of the current paper layout, in paper-space units.
    /// Falls back to the whole sheet when the layout has no usable margins.
    pub fn printable_area_limits(&self) -> Option<((f64, f64), (f64, f64))> {
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let margins = self.document.objects.values().find_map(|object| {
            if let ObjectType::Layout(layout) = object {
                if layout.name == self.current_layout {
                    return Some((
                        layout.plot_margin_left,
                        layout.plot_margin_bottom,
                        layout.plot_margin_right,
                        layout.plot_margin_top,
                        layout.plot_rotation,
                    ));
                }
            }
            None
        });
        let Some((left, bottom, right, top, rotation)) = margins else {
            return Some(((x0, y0), (x1, y1)));
        };
        let (left, bottom, right, top) = match rotation {
            1 | 3 => (bottom, left, top, right),
            2 => (right, top, left, bottom),
            _ => (left, bottom, right, top),
        };
        let factor = self.paper_space_unit_factor();
        let printable = (
            (x0 + left * factor, y0 + bottom * factor),
            (x1 - right * factor, y1 - top * factor),
        );
        if printable.1.0 - printable.0.0 < 1e-6
            || printable.1.1 - printable.0.1 < 1e-6
        {
            Some(((x0, y0), (x1, y1)))
        } else {
            Some(printable)
        }
    }

    /// Scale of the active, selected, or first content viewport in the current
    /// paper layout. Returns `None` in Model space or when no viewport exists.
    pub fn first_viewport_scale(&self) -> Option<f64> {
        let handle = self.target_viewport_handle()?;
        let EntityType::Viewport(vp) = self.document.get_entity(handle)? else {
            return None;
        };
        Some(vp_effective_scale(
            vp.custom_scale,
            vp.view_height,
            vp.height,
        ))
    }

    /// Annotation/viewport scales defined in the drawing's scale list
    /// (the `ACAD_SCALELIST` dictionary), as `(label, annotation_multiplier,
    /// viewport_factor)`. The annotation multiplier sizes model-space
    /// text/dims (50.0 for "1:50"); the viewport factor is the paper/drawing
    /// ratio (0.02 for "1:50"). Sorted smallest ratio first (1:100 … 1:1 …
    /// 10:1). Falls back to a standard ratio set when the drawing carries no
    /// scale list of its own, so the scale picker is always usable. (#154)
    /// Standard fallback sets as (label, paper/drawing factor). The drawing's
    /// INSUNITS selects metric ratios or imperial architectural labels. Used
    /// when no file scales exist and materialised on demand by
    /// [`Scene::ensure_real_scale_list`].
    const DEFAULT_METRIC_SCALES: &'static [(&'static str, f64)] = &[
        ("1:1000", 0.001),
        ("1:500", 0.002),
        ("1:200", 0.005),
        ("1:100", 0.01),
        ("1:50", 0.02),
        ("1:25", 0.04),
        ("1:20", 0.05),
        ("1:10", 0.1),
        ("1:5", 0.2),
        ("1:2", 0.5),
        ("1:1", 1.0),
        ("2:1", 2.0),
        ("5:1", 5.0),
        ("10:1", 10.0),
    ];

    const DEFAULT_IMPERIAL_SCALES: &'static [(&'static str, f64)] = &[
        ("1/128\" = 1'-0\"", 1.0 / 1536.0),
        ("1/64\" = 1'-0\"", 1.0 / 768.0),
        ("1/32\" = 1'-0\"", 1.0 / 384.0),
        ("1/16\" = 1'-0\"", 1.0 / 192.0),
        ("3/32\" = 1'-0\"", 1.0 / 128.0),
        ("1/8\" = 1'-0\"", 1.0 / 96.0),
        ("3/16\" = 1'-0\"", 1.0 / 64.0),
        ("1/4\" = 1'-0\"", 1.0 / 48.0),
        ("3/8\" = 1'-0\"", 1.0 / 32.0),
        ("1/2\" = 1'-0\"", 1.0 / 24.0),
        ("3/4\" = 1'-0\"", 1.0 / 16.0),
        ("1\" = 1'-0\"", 1.0 / 12.0),
        ("1-1/2\" = 1'-0\"", 1.0 / 8.0),
        ("3\" = 1'-0\"", 1.0 / 4.0),
        ("6\" = 1'-0\"", 1.0 / 2.0),
        ("1'-0\" = 1'-0\"", 1.0),
    ];

    fn prefers_imperial_scales(&self) -> Option<bool> {
        match self.document.header.insertion_units {
            1..=3 | 8..=10 | 21..=24 => Some(true),
            4..=7 | 11..=20 => Some(false),
            0 if self.document.header.measurement == 1 => Some(false),
            _ => None,
        }
    }

    fn default_scales(&self) -> &'static [(&'static str, f64)] {
        if self.prefers_imperial_scales() == Some(true) {
            Self::DEFAULT_IMPERIAL_SCALES
        } else {
            Self::DEFAULT_METRIC_SCALES
        }
    }

    fn is_architectural_scale_name(name: &str) -> bool {
        name.contains('=') && name.contains('"') && name.contains('\'')
    }

    fn is_ratio_scale_name(name: &str) -> bool {
        name.split_once(':')
            .and_then(|(paper, drawing)| {
                Some((
                    paper.trim().parse::<f64>().ok()?,
                    drawing.trim().parse::<f64>().ok()?,
                ))
            })
            .is_some_and(|(paper, drawing)| paper > 0.0 && drawing > 0.0)
    }

    /// True when the drawing owns at least one annotation scale of its own
    /// (i.e. `scale_list` returns real objects rather than the fallback set).
    fn has_own_scales(&self) -> bool {
        self.document.objects.values().any(|o| {
            matches!(o, ObjectType::Scale(s)
                if !s.is_temporary
                    && !s.name.contains('|')
                    && !s.name.to_ascii_uppercase().ends_with("_XREF"))
        })
    }

    /// If the drawing has no annotation scales of its own, populate it with the
    /// standard fallback set as real `Scale` objects (and the `ACAD_SCALELIST`
    /// dictionary) so they can be selected, edited and renamed uniformly with a
    /// drawing that ships its own list. Returns `true` if any were created.
    ///
    /// The scale manager calls this on open and stages the result, so the set
    /// is discarded again unless the user applies an edit.
    pub fn ensure_real_scale_list(&mut self) -> bool {
        if self.has_own_scales() {
            return false;
        }
        for &(label, factor) in self.default_scales() {
            // Fallback labels are "paper:drawing"; parse them for tidy whole
            // units, else derive drawing units from the factor (paper = 1).
            let (paper, drawing) = label
                .split_once(':')
                .and_then(|(p, d)| {
                    Some((p.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?))
                })
                .filter(|&(p, d)| p > 0.0 && d > 0.0)
                .unwrap_or((1.0, 1.0 / factor));
            self.add_scale(label, paper, drawing);
        }
        true
    }

    pub fn scale_list(&self) -> Vec<(String, f32, f64)> {
        let mut list: Vec<(String, f32, f64)> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                // Skip xref-derived scales. Scales pulled in from an external
                // reference get an "_XREF" suffix ("1:50_XREF"); unbound
                // dependent symbols carry a "xref|name" prefix. Neither
                // belongs to this drawing's own scale list.
                ObjectType::Scale(s)
                    if !s.is_temporary
                        && !s.name.contains('|')
                        && !s.name.to_ascii_uppercase().ends_with("_XREF") =>
                {
                    Some((s.name.clone(), s.inverse_factor() as f32, s.factor()))
                }
                _ => None,
            })
            .collect();
        list.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        if list.is_empty() {
            // Many drawings (and minimal DXF/DWG exports) carry no
            // ACAD_SCALELIST Scale objects at all. Without a fallback the
            // annotation / viewport scale picker would be empty and so appear
            // broken. Substitute the standard ratio set — file scales still
            // win whenever the drawing actually defines any. (#154)
            list = self
                .default_scales()
                .iter()
                .map(|&(label, vp)| (label.to_string(), (1.0 / vp) as f32, vp))
                .collect();
        }
        list
    }

    /// Scale list for the status-bar picker. Some DWGs carry both the standard
    /// metric and architectural sets in `ACAD_SCALELIST`; show only the family
    /// matching INSUNITS while retaining custom names. The complete file list
    /// remains available to the scale manager and is written back unchanged.
    pub fn scale_picker_list(&self) -> Vec<(String, f32, f64)> {
        let all = self.scale_list();
        let Some(imperial) = self.prefers_imperial_scales() else {
            return all;
        };

        let mut visible: Vec<_> = all
            .iter()
            .filter(|(name, _, _)| {
                let architectural = Self::is_architectural_scale_name(name);
                let ratio = Self::is_ratio_scale_name(name);
                if imperial {
                    architectural || (!architectural && !ratio)
                } else {
                    ratio || (!architectural && !ratio)
                }
            })
            .cloned()
            .collect();

        let has_matching_family = visible.iter().any(|(name, _, _)| {
            if imperial {
                Self::is_architectural_scale_name(name)
            } else {
                Self::is_ratio_scale_name(name)
            }
        });
        if !has_matching_family {
            visible.extend(
                self.default_scales()
                    .iter()
                    .map(|&(label, vp)| (label.to_string(), (1.0 / vp) as f32, vp)),
            );
        }

        // If the current scale belongs to the opposite family and has no
        // equivalent factor in the visible family, keep that one active entry
        // visible instead of silently hiding the drawing's current state.
        if let Some(active) = all.iter().find(|(name, _, _)| {
            name.eq_ignore_ascii_case(&self.document.header.current_annotation_scale)
        }) {
            let has_equivalent = visible
                .iter()
                .any(|(_, _, vp)| (vp - active.2).abs() < 0.001 * active.2.max(0.001));
            if !has_equivalent {
                visible.push(active.clone());
            }
        }

        visible.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        visible
    }

    /// Handle of the drawing's `ACAD_SCALELIST` dictionary, located robustly.
    ///
    /// The canonical path is the root named-objects dict → `"ACAD_SCALELIST"`,
    /// but DWGs written by other programs don't always expose a resolvable root
    /// dict (the header handle points at no loaded object). In that case fall
    /// back to the owner dictionary shared by the drawing's `Scale` objects.
    /// Returns `None` only when the drawing genuinely has no scale list.
    pub(crate) fn scalelist_dict_handle(&self) -> Option<Handle> {
        use acadrust::objects::ObjectType;
        let root_h = self.document.header.named_objects_dict_handle;
        if let Some(h) = crate::scene::annotative::as_dict(&self.document, root_h)
            .and_then(|d| d.get("ACAD_SCALELIST"))
        {
            return Some(h);
        }
        let owner = self.document.objects.values().find_map(|o| match o {
            ObjectType::Scale(s) if !s.is_temporary => Some(s.owner_handle),
            _ => None,
        })?;
        matches!(
            self.document.objects.get(&owner),
            Some(ObjectType::Dictionary(_))
        )
        .then_some(owner)
    }

    /// Handle of the real (non-temporary) `Scale` object with this display name.
    /// Scales are matched by their `name` field, not the dictionary key — some
    /// files key the entries by an internal identifier (`*A1`, `A0`, …) that
    /// bears no relation to the scale name.
    pub(crate) fn scale_object_handle(&self, name: &str) -> Option<Handle> {
        use acadrust::objects::ObjectType;
        self.document.objects.iter().find_map(|(h, o)| match o {
            ObjectType::Scale(s) if !s.is_temporary && s.name.eq_ignore_ascii_case(name) => {
                Some(*h)
            }
            _ => None,
        })
    }

    /// Resolve the current annotation scale (CANNOSCALE) to a real `Scale`
    /// object handle, materializing the object from the scale list when the
    /// drawing names the scale but has no `Scale` object for it. Returns `None`
    /// when there is no current annotation scale. Used when giving an object a
    /// per-object annotation context (the context's `340` must point at a real
    /// `AcDbScale`).
    pub(crate) fn current_annotation_scale_handle(&mut self) -> Option<Handle> {
        let name = self.document.header.current_annotation_scale.clone();
        if name.trim().is_empty() {
            return None;
        }
        self.scale_handle_ensuring(&name)
    }

    pub(crate) fn paper_annotation_scale_handle(&self) -> Option<Handle> {
        self.scale_object_handle("1:1").or_else(|| {
            self.document.objects.iter().find_map(|(handle, object)| match object {
                ObjectType::Scale(scale)
                    if !scale.is_temporary && (scale.factor() - 1.0).abs() <= 1.0e-9 =>
                {
                    Some(*handle)
                }
                _ => None,
            })
        })
    }

    pub(crate) fn creation_annotation_scale_handle(&mut self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return self.current_annotation_scale_handle();
        }
        if let Some(viewport) = self.active_viewport {
            if let Some(scale) = self.viewport_scale_handle(viewport) {
                return Some(scale);
            }
        }
        if let Some(scale) = self.paper_annotation_scale_handle() {
            return Some(scale);
        }
        self.scale_handle_ensuring("1:1")
    }

    pub(crate) fn creation_annotation_multiplier(&self) -> f64 {
        if self.current_layout == "Model" {
            return self.annotation_scale.max(1.0e-9) as f64;
        }
        let Some(handle) = self.active_viewport else {
            return 1.0;
        };
        let Some(EntityType::Viewport(viewport)) = self.document.get_entity(handle) else {
            return 1.0;
        };
        let paper_to_model = vp_effective_scale(
            viewport.custom_scale,
            viewport.view_height,
            viewport.height,
        );
        if paper_to_model.is_finite() && paper_to_model > 1.0e-9 {
            1.0 / paper_to_model
        } else {
            1.0
        }
    }

    pub fn set_annotation_scale_named(&mut self, name: &str) -> Option<Handle> {
        let handle = self.scale_handle_ensuring(name)?;
        let ObjectType::Scale(scale) = self.document.objects.get(&handle)? else {
            return None;
        };
        let multiplier = scale.inverse_factor();
        self.annotation_scale = multiplier as f32;
        self.document.header.current_annotation_scale = scale.name.clone();
        self.document.header.annotation_scale_value = scale.factor();
        self.invalidate_annotation_dependencies();
        Some(handle)
    }

    fn current_layout_object_handle(&self) -> Option<Handle> {
        self.document.objects.iter().find_map(|(handle, object)| match object {
            ObjectType::Layout(layout)
                if layout.name.eq_ignore_ascii_case(&self.current_layout) =>
            {
                Some(*handle)
            }
            _ => None,
        })
    }

    pub fn annotation_all_visible(&self) -> bool {
        use acadrust::objects::XRecordValue;
        let Some(layout) = self.current_layout_object_handle() else {
            return true;
        };
        self.document
            .xrecord(layout, "OPEN_CAD_ANNOTATION_STATE")
            .and_then(|record| {
                record.entries.iter().find_map(|entry| match entry.value {
                    XRecordValue::Bool(value) if entry.code == 290 => Some(value),
                    _ => None,
                })
            })
            .unwrap_or(true)
    }

    pub fn set_annotation_all_visible(&mut self, value: bool) {
        use acadrust::objects::{XRecordEntry, XRecordValue};
        let Some(layout) = self.current_layout_object_handle() else {
            return;
        };
        self.document
            .ensure_xrecord(layout, "OPEN_CAD_ANNOTATION_STATE");
        if let Some(record) = self
            .document
            .xrecord_mut(layout, "OPEN_CAD_ANNOTATION_STATE")
        {
            if let Some(entry) = record.entries.iter_mut().find(|entry| entry.code == 290) {
                entry.value = XRecordValue::Bool(value);
            } else {
                record.entries.push(XRecordEntry::bool(290, value));
            }
        }
        self.invalidate_annotation_dependencies();
    }

    pub fn add_annotation_scale_to_objects(
        &mut self,
        scale: Handle,
        previous_scale: Option<Handle>,
        mode: u8,
    ) -> usize {
        if previous_scale == Some(scale) || !(1..=4).contains(&mode) {
            return 0;
        }
        let viewport_frozen: HashSet<Handle> = self
            .target_viewport_handle()
            .and_then(|handle| match self.document.get_entity(handle) {
                Some(EntityType::Viewport(viewport)) => {
                    Some(viewport.frozen_layers.iter().copied().collect())
                }
                _ => None,
            })
            .unwrap_or_default();
        let handles: Vec<_> = self
            .document
            .entities()
            .filter(|entity| {
                if !crate::scene::annotative::is_annotative(&self.document, entity) {
                    return false;
                }
                let memberships = crate::scene::annotative::object_scale_memberships(
                    &self.document,
                    entity.common().handle,
                );
                if !previous_scale
                    .is_some_and(|current| memberships.iter().any(|(_, member)| *member == current))
                {
                    return false;
                }
                let layer = self.document.layers.get(&entity.common().layer);
                let (off, frozen, locked, viewport_frozen) = layer
                    .map(|layer| {
                        (
                            layer.flags.off,
                            layer.flags.frozen,
                            layer.flags.locked,
                            viewport_frozen.contains(&layer.handle),
                        )
                    })
                    .unwrap_or((false, false, false, false));
                match mode {
                    1 => !(off || frozen || locked || viewport_frozen),
                    2 => !(off || frozen || viewport_frozen),
                    3 => !locked,
                    4 => true,
                    _ => false,
                }
            })
            .map(|entity| entity.common().handle)
            .collect();
        let mut added = 0;
        for handle in handles {
            let existed = crate::scene::annotative::object_scale_memberships(
                &self.document,
                handle,
            )
            .iter()
            .any(|(_, member)| *member == scale);
            if !existed
                && crate::scene::annotative::create_annotation_context(
                    &mut self.document,
                    handle,
                    scale,
                )
            {
                added += 1;
            }
        }
        if added > 0 {
            self.bump_geometry();
        }
        added
    }

    pub(crate) fn displayed_annotation_scale_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return self.scale_object_handle(&self.document.header.current_annotation_scale);
        }
        self.explicit_viewport_handle()
            .and_then(|viewport| self.viewport_scale_handle(viewport))
            .or_else(|| self.paper_annotation_scale_handle())
    }

    /// Resolve a named annotation scale to a real `Scale` object handle,
    /// materializing the object from the scale list when the drawing names the
    /// scale (e.g. a virtual fallback scale) but has no `Scale` object for it.
    pub(crate) fn scale_handle_ensuring(&mut self, name: &str) -> Option<Handle> {
        if let Some(h) = self.scale_object_handle(name) {
            return Some(h);
        }
        let fallback = self
            .default_scales()
            .iter()
            .find(|(label, _)| label.eq_ignore_ascii_case(name))
            .map(|(_, factor)| (1.0, 1.0 / factor))
            .or_else(|| {
                let (paper, drawing) = name.split_once(':')?;
                let paper = paper.trim().parse::<f64>().ok()?;
                let drawing = drawing.trim().parse::<f64>().ok()?;
                (paper > 0.0 && drawing > 0.0).then_some((paper, drawing))
            })
            .or_else(|| {
                let drawing = name.trim().parse::<f64>().ok()?;
                (drawing > 0.0).then_some((1.0, drawing))
            })?;
        let (paper, drawing) = self.scale_paper_drawing(name).unwrap_or(fallback);
        self.add_scale(name, paper, drawing);
        self.scale_object_handle(name)
    }

    /// Add a named annotation scale to the drawing's `ACAD_SCALELIST`. Returns
    /// `false` if a scale with that name already exists. Creates the SCALELIST
    /// dictionary (and registers it in the root dictionary) when the drawing
    /// has none of its own — many minimal files carry no scale list at all.
    pub fn add_scale(&mut self, name: &str, paper: f64, drawing: f64) -> bool {
        use acadrust::objects::{Dictionary, ObjectType, Scale};
        // Reject a duplicate by the scale's display name (the dictionary key may
        // be an unrelated internal identifier, so check the objects directly).
        if self.scale_object_handle(name).is_some() {
            return false;
        }
        let scalelist_h = match self.scalelist_dict_handle() {
            Some(h) => h,
            None => {
                // Resolve (or synthesise) the root robustly — a drawing whose
                // header root pointer is unresolvable would otherwise register
                // the new dictionary against a missing root and silently orphan
                // it. See `annotative::root_named_dict_handle`.
                let root_h = crate::scene::annotative::root_named_dict_handle(&mut self.document);
                let h = self.document.allocate_handle();
                let mut d = Dictionary::new();
                d.handle = h;
                d.owner = root_h;
                self.document.objects.insert(h, ObjectType::Dictionary(d));
                if let Some(ObjectType::Dictionary(root)) = self.document.objects.get_mut(&root_h) {
                    root.add_entry("ACAD_SCALELIST", h);
                }
                h
            }
        };
        let sh = self.document.allocate_handle();
        let mut s = Scale::new(name, paper, drawing);
        s.handle = sh;
        s.owner_handle = scalelist_h;
        self.document.objects.insert(sh, ObjectType::Scale(s));
        if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&scalelist_h) {
            sl.add_entry(name, sh);
        }
        true
    }

    /// Remove a named annotation scale from the drawing's `ACAD_SCALELIST`
    /// (both the `Scale` object and its dictionary entry). Returns `false` if
    /// no such scale exists. The current annotation scale should not be removed
    /// — the caller guards that.
    pub fn remove_scale(&mut self, name: &str) -> bool {
        use acadrust::objects::ObjectType;
        let Some(sh) = self.scale_object_handle(name) else {
            return false;
        };
        if self
            .document
            .header
            .current_annotation_scale
            .eq_ignore_ascii_case(name)
        {
            return false;
        }
        let entity_handles: Vec<_> = self
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        for entity in entity_handles {
            crate::scene::annotative::remove_annotation_context_for_scale(
                &mut self.document,
                entity,
                sh,
            );
        }
        let replacement = self.document.objects.iter().find_map(|(handle, object)| match object {
            ObjectType::Scale(scale) if *handle != sh && !scale.is_temporary => Some(*handle),
            _ => None,
        });
        let viewports: Vec<_> = self
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Viewport(viewport)
                    if self.document.viewport_annotation_scale(viewport.common.handle) == Some(sh) =>
                {
                    Some(viewport.common.handle)
                }
                _ => None,
            })
            .collect();
        for viewport in viewports {
            if let Some(replacement) = replacement {
                self.document
                    .set_viewport_annotation_scale(viewport, replacement);
            } else if let Some(record) = self
                .document
                .xrecord_mut(viewport, "ASDK_XREC_ANNOTATION_SCALE_INFO")
            {
                record.entries.retain(|entry| entry.code != 340);
            }
        }
        // Resolve the dictionary before removing the object (the fallback lookup
        // reads a surviving scale's owner).
        let dict_h = self.scalelist_dict_handle();
        self.document.objects.remove(&sh);
        if let Some(dict_h) = dict_h {
            if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&dict_h) {
                // Match by handle — the entry key is not necessarily the name.
                sl.entries.retain(|(_, h)| *h != sh);
            }
        }
        self.invalidate_annotation_dependencies();
        true
    }

    /// Rename `old_name` and/or change its paper:drawing ratio. Returns `false`
    /// if the scale is missing or the new name collides with another scale.
    pub fn edit_scale(&mut self, old_name: &str, new_name: &str, paper: f64, drawing: f64) -> bool {
        use acadrust::objects::ObjectType;
        let Some(sh) = self.scale_object_handle(old_name) else {
            return false;
        };
        // A new name that collides with a *different* scale object is rejected.
        if !new_name.eq_ignore_ascii_case(old_name) {
            if let Some(other) = self.scale_object_handle(new_name) {
                if other != sh {
                    return false;
                }
            }
        }
        if let Some(ObjectType::Scale(s)) = self.document.objects.get_mut(&sh) {
            s.name = new_name.to_string();
            s.paper_units = paper;
            s.drawing_units = drawing;
            s.is_unit_scale = (paper - drawing).abs() < 1e-10;
        }
        // Keep the owning dictionary's entry key in sync, matched by handle.
        if let Some(dict_h) = self.scalelist_dict_handle() {
            if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&dict_h) {
                if let Some(e) = sl.entries.iter_mut().find(|(_, h)| *h == sh) {
                    e.0 = new_name.to_string();
                }
            }
        }
        true
    }

    /// The (paper_units, drawing_units) of a named scale, for the editor.
    pub fn scale_paper_drawing(&self, name: &str) -> Option<(f64, f64)> {
        self.document.objects.values().find_map(|o| match o {
            ObjectType::Scale(s) if !s.is_temporary && s.name.eq_ignore_ascii_case(name) => {
                Some((s.paper_units, s.drawing_units))
            }
            _ => None,
        })
    }

    /// List of user viewports in the current layout: (handle, label, frozen_layer_handles).
    pub fn viewport_list(&self) -> Vec<(acadrust::Handle, String, Vec<acadrust::Handle>)> {
        if self.current_layout == "Model" {
            return vec![];
        }
        let (layout_block, _, content) = self.paper_viewport_handles();
        if layout_block.is_null() {
            return vec![];
        }
        let mut result: Vec<(acadrust::Handle, String, Vec<acadrust::Handle>)> = content
            .iter()
            .filter_map(|handle| {
                let Some(EntityType::Viewport(vp)) = self.document.get_entity(*handle) else {
                    return None;
                };
                Some((vp.common.handle, vp.id, vp.frozen_layers.clone()))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(i, (h, id, frozen))| {
                let label = if id > 1 {
                    format!("VP {}", id - 1)
                } else {
                    format!("VP {}", i + 1)
                };
                (h, label, frozen)
            })
            .collect();
        result.sort_by_key(|(_, label, _)| label.clone());
        result
    }

    /// Count of user viewports (id > 1) in the current layout.
    pub fn viewport_count(&self) -> usize {
        if self.current_layout == "Model" {
            return 0;
        }
        let (layout_block, _, content) = self.paper_viewport_handles();
        if layout_block.is_null() {
            return 0;
        }
        content.len()
    }

    /// True when ISOLATEOBJECTS or HIDEOBJECTS has an active session filter.
    pub fn is_isolation_active(&self) -> bool {
        self.object_isolation.is_active()
    }

    /// True when a top-level entity must be omitted for a session-only object
    /// visibility command or an interactive replacement preview.
    fn entity_temporarily_hidden(&self, handle: Handle) -> bool {
        self.object_isolation.hides(handle)
            || self.preview_hidden.contains(&handle)
            || self.command_preview_hidden.contains(&handle)
    }

    /// Replace the command-owned source hide set and refresh only handles whose
    /// visibility changed. An empty slice restores every source on completion.
    pub fn set_command_preview_hidden(&mut self, handles: &[Handle]) {
        let desired: HashSet<_> = handles.iter().copied().collect();
        if desired == self.command_preview_hidden {
            return;
        }
        let changes: Vec<_> = self
            .command_preview_hidden
            .symmetric_difference(&desired)
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.command_preview_hidden = desired;
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Set (or clear) the previewed entity that renders with the selection
    /// highlight without joining the real selection. Only refreshes the GPU
    /// xray overlay (no re-tessellation).
    pub fn set_hover_highlight(&mut self, handle: Option<Handle>) {
        if self.hover_highlight == handle {
            return;
        }
        // Selectable groups roll over as one unit, matching click selection.
        // Exclude already-selected members because they already contribute to
        // the overlay and remain in the selected colour.
        let contribution = |scene: &Scene, hovered: Option<Handle>| {
            hovered
                .map(|handle| scene.handles_expanded_for_selectable_groups(&[handle]))
                .unwrap_or_default()
                .into_iter()
                .filter(|handle| !scene.selected.contains(handle))
                .collect::<HashSet<_>>()
        };
        let changed = contribution(self, self.hover_highlight) != contribution(self, handle);
        self.hover_highlight = handle;
        if changed {
            self.bump_selection();
        }
    }

    /// Handles that currently contribute the rollover colour. The stored hover
    /// remains the picked member for hit-test/UI bookkeeping; rendering expands
    /// it to the same selectable group that a click would select.
    pub fn hover_highlight_handles(&self) -> HashSet<Handle> {
        self.hover_highlight
            .map(|handle| self.handles_expanded_for_selectable_groups(&[handle]))
            .unwrap_or_default()
    }

    /// Keep the current selection visible and temporarily filter every other
    /// top-level object. Block-definition children are intentionally not added
    /// here: an isolated INSERT remains a complete visible block instance.
    pub fn isolate_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let keep = self.selected.clone();
        let active_block = self.interaction_block_handle();
        let hide: Vec<Handle> = self
            .document
            .entities()
            .filter(|entity| {
                let common = entity.common();
                !common.handle.is_null()
                    && !keep.contains(&common.handle)
                    && self.belongs_to_visible_block(
                        common.handle,
                        common.owner_handle,
                        active_block,
                    )
            })
            .map(|entity| entity.common().handle)
            .collect();
        let changes: Vec<_> = hide
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation.hidden.extend(hide);
        self.object_isolation.keep = Some(keep);
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Temporarily hide the current selection without changing any entity
    /// property in the document.
    pub fn hide_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let changes: Vec<_> = self
            .selected
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation
            .hidden
            .extend(self.selected.iter().copied());
        self.selected.clear();
        self.bump_entities(&changes);
    }

    /// Clear every session-only object visibility filter.
    pub fn end_isolation(&mut self) {
        if !self.object_isolation.is_active() {
            return;
        }
        let changes: Vec<_> = self
            .object_isolation
            .hidden
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation = ObjectIsolationState::default();
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// A newly opened/replaced document starts with no session visibility.
    /// Persisted `EntityCommon::invisible` values stay untouched and continue
    /// to serve file/dynamic-block visibility.
    pub fn reset_transient_visibility(&mut self) {
        self.object_isolation = ObjectIsolationState::default();
        self.preview_hidden.clear();
        self.command_preview_hidden.clear();
    }

    /// True if any currently selected entity is a Viewport.
    /// Used to enable the scale picker when a viewport is selected in paper space.
    pub fn has_selected_viewport(&self) -> bool {
        self.selected
            .iter()
            .any(|&h| matches!(self.document.get_entity(h), Some(EntityType::Viewport(_))))
    }

    /// First content viewport handle in the current layout, used as fallback target
    /// when no viewport is active or explicitly selected.
    fn first_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return None;
        }
        self.document.entities().find_map(|e| {
            if let EntityType::Viewport(vp) = e {
                if self.is_content_viewport_in_layout(vp, layout_block) {
                    return Some(vp.common.handle);
                }
            }
            None
        })
    }

    fn target_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        self.active_viewport
            .filter(|handle| {
                matches!(self.document.get_entity(*handle), Some(EntityType::Viewport(_)))
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(self.document.get_entity(*handle), Some(EntityType::Viewport(_)))
                })
            })
            .or_else(|| self.first_viewport_handle())
    }

    fn explicit_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        self.active_viewport
            .filter(|handle| {
                matches!(self.document.get_entity(*handle), Some(EntityType::Viewport(_)))
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(self.document.get_entity(*handle), Some(EntityType::Viewport(_)))
                })
            })
    }

    fn viewport_scale_handle(&self, viewport: Handle) -> Option<Handle> {
        if let Some(handle) = self.document.viewport_annotation_scale(viewport) {
            if matches!(self.document.objects.get(&handle), Some(ObjectType::Scale(_))) {
                return Some(handle);
            }
        }
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };
        let factor = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        self.document
            .objects
            .iter()
            .filter_map(|(handle, object)| match object {
                ObjectType::Scale(scale) if !scale.is_temporary => {
                    Some((*handle, (scale.factor() - factor).abs()))
                }
                _ => None,
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(handle, _)| handle)
    }

    fn viewport_annotation_multiplier(&self, viewport: Handle) -> f32 {
        self.viewport_scale_handle(viewport)
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.inverse_factor() as f32),
                _ => None,
            })
            .unwrap_or(self.annotation_scale)
    }

    pub fn displayed_annotation_scale_name(&self) -> String {
        if self.current_layout == "Model" {
            return self.document.header.current_annotation_scale.clone();
        }
        self.explicit_viewport_handle()
            .and_then(|viewport| self.viewport_scale_handle(viewport))
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                _ => None,
            })
            .or_else(|| {
                self.paper_annotation_scale_handle()
                    .and_then(|handle| match self.document.objects.get(&handle) {
                        Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_else(|| "1:1".to_string())
    }

    pub fn set_viewport_scale_named(&mut self, name: &str) -> Option<Handle> {
        let scale_handle = self.scale_handle_ensuring(name)?;
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => scale.factor(),
            _ => return None,
        };
        let viewport = self.explicit_viewport_handle()?;
        let locked = matches!(
            self.document.get_entity(viewport),
            Some(EntityType::Viewport(vp)) if vp.status.locked
        );
        if locked || factor <= 1.0e-9 {
            return None;
        }
        if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(viewport) {
            vp.custom_scale = factor;
            vp.view_height = vp.height / factor;
        }
        self.document
            .set_viewport_annotation_scale(viewport, scale_handle);
        self.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        Some(scale_handle)
    }

    pub fn viewport_annotation_scale_synced(&self) -> Option<bool> {
        let viewport = self.explicit_viewport_handle()?;
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };
        let scale = self.viewport_scale_handle(viewport)?;
        let ObjectType::Scale(scale) = self.document.objects.get(&scale)? else {
            return None;
        };
        let effective = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        Some((effective - scale.factor()).abs() <= 1.0e-6 * scale.factor().max(1.0))
    }

    pub fn sync_viewport_annotation_scale(&mut self) -> bool {
        let Some(viewport) = self.explicit_viewport_handle() else {
            return false;
        };
        let Some(scale_handle) = self.viewport_scale_handle(viewport) else {
            return false;
        };
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => scale.factor(),
            _ => return false,
        };
        let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(viewport) else {
            return false;
        };
        if vp.status.locked || factor <= 1.0e-9 {
            return false;
        }
        vp.custom_scale = factor;
        vp.view_height = vp.height / factor;
        self.document
            .set_viewport_annotation_scale(viewport, scale_handle);
        self.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        true
    }

    /// Sorted list of layout names: "Model" first, then paper layouts by tab order.
    pub fn layout_names(&self) -> Vec<String> {
        let mut names = vec!["Model".to_string()];
        // Deduplicate by name: prefer the entry with a non-null block_record (the
        // real layout from the file) over the default placeholder created by
        // CadDocument::new().
        let mut by_name: rustc_hash::FxHashMap<String, (i16, Handle)> = Default::default();
        for obj in self.document.objects.values() {
            if let ObjectType::Layout(l) = obj {
                if l.name == "Model" || l.name.is_empty() {
                    continue;
                }
                let entry = by_name
                    .entry(l.name.clone())
                    .or_insert((l.tab_order, l.block_record));
                if entry.1.is_null() && !l.block_record.is_null() {
                    *entry = (l.tab_order, l.block_record);
                }
            }
        }
        let mut paper: Vec<(i16, String)> = by_name
            .into_iter()
            .map(|(name, (order, _))| (order, name))
            .collect();
        paper.sort_by_key(|(order, _)| *order);
        names.extend(paper.into_iter().map(|(_, n)| n));
        names
    }

    /// Collect closed polygon outlines (world XY) from the current layout.
    pub fn closed_outlines(&self) -> Vec<Vec<[f32; 2]>> {
        self.entity_wires()
            .iter()
            .filter_map(|wire| {
                let pts = &wire.points;
                if pts.len() < 4 {
                    return None;
                }
                let f = pts.first()?;
                let l = pts.last()?;
                let dx = f[0] - l[0];
                let dy = f[1] - l[1];
                if (dx * dx + dy * dy).sqrt() > 1e-2 {
                    return None;
                }
                // Segment-list wires (e.g. LwPolyline) store each segment as an
                // independent NaN-separated pair, so every shared corner repeats
                // (`A B | B C | C D | D A`). Collapse that back into a clean ring:
                // skip the NaN separators and any vertex coincident with the
                // previous one, so consumers (point-in-polygon, the hatch /
                // boundary commands) see one vertex per corner — not the doubled
                // ring that otherwise shows two grips at every corner.
                let mut ring: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
                for p in pts {
                    if !p[0].is_finite() || !p[1].is_finite() {
                        continue;
                    }
                    let q = [p[0], p[1]];
                    if let Some(&last) = ring.last() {
                        if (last[0] - q[0]).abs() < 1e-4 && (last[1] - q[1]).abs() < 1e-4 {
                            continue;
                        }
                    }
                    ring.push(q);
                }
                // Drop a trailing vertex equal to the first — the ring is closed
                // implicitly, so keeping it would be a duplicate corner.
                if ring.len() > 1 {
                    let first = ring[0];
                    let last = *ring.last().unwrap();
                    if (first[0] - last[0]).abs() < 1e-4 && (first[1] - last[1]).abs() < 1e-4 {
                        ring.pop();
                    }
                }
                if ring.len() < 3 {
                    return None;
                }
                Some(ring)
            })
            .collect()
    }

    /// Wire set for the Model layout, shared by every tile.
    ///
    /// The model wire geometry is **camera-independent**, so it is tessellated
    /// in full (un-culled, fixed detail) once per geometry epoch, held resident
    /// (`model_static_wires`), and returned for any camera/tile. A pan/zoom only
    /// changes the view uniform — the GPU re-draws the same buffer, with no
    /// frustum cull, no zoom LOD, and no re-tessellation/re-upload on camera
    /// moves. The per-tile camera args are unused (kept for call-site symmetry
    /// with the paper-space wire sources).
    pub(super) fn model_tile_wires_arc(
        &self,
        _tile_idx: usize,
        _cam: &Camera,
        _cam_aspect: f32,
        _tile_pixel_height: f32,
    ) -> Arc<Vec<WireModel>> {
        // In a BEDIT block editor the resident set is the edited block's own
        // (block-local) entities; otherwise model space. (#261)
        let block = self
            .block_edit_block
            .unwrap_or_else(|| self.model_space_block_handle());
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        self.resident_wires_for(block, None, scale, None, None)
    }

    /// Unified static-hold wire builder — the ONE tessellation path every
    /// space renders through (Model tiles, BEDIT, the paper sheet block, and
    /// paper content viewports): the FULL, un-culled, LOD-free set of `block`,
    /// held resident per `(block, ambient bg, anno, frozen)` key and rebuilt
    /// only when `geometry_epoch` changes. Mints a stable
    /// [`WIRE_CONTENT_GEN`] id per build (relayed via `last_model_wire_gen`)
    /// so the GPU upload gate and `render_signature` skip unchanged content.
    /// Whether the per-viewport annotation scale changes wire output at all
    /// Cached per geometry epoch; the scan short-circuits on the first
    /// annotative entity. PSLTSCALE no longer participates because dash scaling
    /// is applied in the wire shader from the viewport uniform.
    fn annotation_affects_wires(&self) -> bool {
        if let Some((epoch, v)) = self.annotation_affects_wires.get() {
            if epoch == self.geometry_epoch {
                return v;
            }
            // Skip the whole-document annotative scan when nothing annotative
            // changed since (PSLTSCALE toggles route through a full delta, which
            // fails this check and forces the recompute below).
            if self.category_cache_valid(epoch, CACHE_CATEGORY_ANNOTATIVE, |h| {
                self.document
                    .get_entity(h)
                    .map(|e| crate::scene::annotative::is_annotative(&self.document, e))
                    .unwrap_or(false)
            }) {
                self.annotation_affects_wires
                    .set(Some((self.geometry_epoch, v)));
                return v;
            }
        }
        let v = self
            .document
            .entities()
            .any(|e| crate::scene::annotative::is_annotative(&self.document, e));
        self.annotation_affects_wires
            .set(Some((self.geometry_epoch, v)));
        v
    }

    fn resident_wires_for(
        &self,
        block: Handle,
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> Arc<Vec<WireModel>> {
        // Normalize an inert anno override away so distinct viewport scales
        // share one resident set when annotation can't change the wires.
        let anno_scale_override = if self.annotation_affects_wires() {
            anno_scale_override
        } else {
            None
        };
        // Ambient bg is part of the key: the same block tessellated against
        // the model vs paper background adapts wire colors differently.
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let all_visible = self.annotation_all_visible();
        let key = Self::resident_wire_key(
            block,
            bg,
            anno_scale_override,
            annotation_scale_handle,
            all_visible,
            frozen_layers,
            style_viewport,
        );
        {
            let sets = self.resident_wire_sets.borrow();
            if let Some(set) = sets.get(&key) {
                if set.epoch == self.geometry_epoch {
                    self.last_model_wire_gen.set(set.gen);
                    return Arc::clone(&set.wires);
                }
            }
        }
        // Incremental: bring the cached set up to date by replaying just the
        // changed entities into it, instead of re-cloning + re-sorting the whole
        // set. Falls back to the full build below when it can't (no cache entry,
        // journal un-replayable, or a structural assumption violated).
        if let Some(arc) =
            self.try_resident_patch(
                key,
                block,
                bg,
                anno_scale_override,
                annotation_scale_handle,
                all_visible,
                frozen_layers,
                style_viewport,
            )
        {
            return arc;
        }
        // Build once: full tessellation, no cull (region = None), no zoom LOD
        // (wpp = None) — for every space, exactly like the Model static-hold.
        let t_tess = iced::time::Instant::now();
        let mut wires =
            self.wires_for_block_culled(
                block,
                None,
                None,
                frozen_layers,
                anno_scale_override,
                annotation_scale_handle,
                all_visible,
                style_viewport,
            );
        // Synthesized nonprint markers (geo-location daisy) live in model space
        // only and are derived from document objects, not entities — append them
        // to the freshly built resident set (incremental patches preserve them).
        if block == self.model_space_block_handle() {
            self.append_scene_markers(&mut wires, bg);
        }
        self.apply_refedit_fade(&mut wires, bg);
        let layout = Self::resident_wire_layout(&wires);
        self.last_tess_ms
            .set(t_tess.elapsed().as_secs_f32() * 1000.0);
        self.last_tess_wires.set(wires.len());
        let arc = Arc::new(wires);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        // A full rebuild of the Model set — the GPU arena must rebuild too, not
        // apply a stale patch, so clear any pending handoff.
        if block == self.model_space_block_handle() {
            *self.model_wire_gpu_patch.borrow_mut() = None;
        }
        let mut sets = self.resident_wire_sets.borrow_mut();
        // Evict stale entries (older epochs / abandoned keys) so switching
        // spaces or re-scaling a viewport can't accumulate dead full sets.
        let cur_epoch = self.geometry_epoch;
        sets.retain(|_, set| set.epoch == cur_epoch);
        if sets.len() > 8 {
            sets.clear();
        }
        sets.insert(
            key,
            ResidentWireSet {
                epoch: cur_epoch,
                gen,
                wires: Arc::clone(&arc),
                layout,
            },
        );
        arc
    }

    fn resident_wire_key(
        block: Handle,
        bg: [f32; 4],
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> u64 {
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix =
            |value: u64| key = key.rotate_left(17) ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        mix(block.value());
        for component in bg {
            mix(component.to_bits() as u64);
        }
        mix(anno_scale_override
            .map(|scale| scale.to_bits() as u64)
            .unwrap_or(u64::MAX));
        mix(annotation_scale_handle.map(|handle| handle.value()).unwrap_or(0));
        mix(all_visible as u64);
        mix(style_viewport.map(|handle| handle.value()).unwrap_or(0));
        match frozen_layers {
            Some(frozen) => {
                let mut signature = 0u64;
                for handle in frozen {
                    signature ^= handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
                }
                mix(signature);
                mix(frozen.len() as u64);
            }
            None => mix(u64::MAX - 1),
        }
        key
    }

    /// The GPU wire-arena handoff for a viewport whose content id is `gen`:
    /// `(prev_gen, changed handles)` when the Model set reached `gen` via an
    /// incremental resident patch, else `None`. Read (not consumed) so every
    /// Model tile applies the same patch; gen-matching keeps it from applying to
    /// a viewport that rebuilt independently.
    pub(crate) fn model_wire_patch_for(
        &self,
        gen: u64,
    ) -> Option<(u64, Arc<WireGpuPatch>)> {
        match &*self.model_wire_gpu_patch.borrow() {
            Some((prev, new, patch)) if *new == gen => Some((*prev, Arc::clone(patch))),
            _ => None,
        }
    }

    fn resident_wire_layout(wires: &[WireModel]) -> Option<ResidentWireLayout> {
        let mut order = Vec::new();
        let mut ranges = HashMap::default();
        let mut current: Option<(Handle, usize)> = None;
        let mut marker_start = wires.len();

        for (index, wire) in wires.iter().enumerate() {
            let Some(handle) = Self::handle_from_wire_name(&wire.name) else {
                if let Some((handle, start)) = current.take() {
                    if ranges.insert(handle, (start, index - start)).is_some() {
                        return None;
                    }
                }
                marker_start = index;
                if wires[index..]
                    .iter()
                    .any(|tail| Self::handle_from_wire_name(&tail.name).is_some())
                {
                    return None;
                }
                break;
            };
            if current.map(|(h, _)| h) != Some(handle) {
                if let Some((previous, start)) = current.take() {
                    if ranges.insert(previous, (start, index - start)).is_some() {
                        return None;
                    }
                }
                if ranges.contains_key(&handle) {
                    return None;
                }
                order.push(handle);
                current = Some((handle, index));
            }
        }
        if let Some((handle, start)) = current {
            if ranges
                .insert(handle, (start, marker_start - start))
                .is_some()
            {
                return None;
            }
        }
        Some(ResidentWireLayout {
            order,
            ranges,
            vacant: HashMap::default(),
            tombstoned_wires: 0,
            marker_start,
        })
    }

    /// Bring the resident set for `key` up to the current epoch by replaying only
    /// the changed entities into the cached assembly. Returns the patched `Arc`,
    /// or `None` to fall back to a full rebuild — the safe default whenever the
    /// fast path can't apply: no cache entry, the journal can't be replayed
    /// (a `full` delta or ring overflow), the cached `Arc` is still shared (so
    /// its wires can't be moved out), or a structural assumption is violated
    /// (a wire not named with its entity handle, or an entity's wires not
    /// contiguous). Because every failure falls back to the identical full
    /// build, correctness never depends on the fast path being taken.
    fn try_resident_patch(
        &self,
        key: u64,
        block: Handle,
        bg: [f32; 4],
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> Option<Arc<Vec<WireModel>>> {
        if style_viewport.is_some() {
            return None;
        }
        let perf = crate::perf::enabled();
        let t_patch = iced::time::Instant::now();
        // The entry must exist, be stale, and be uniquely held so we can move
        // its wires out rather than deep-clone them.
        let cached_epoch = {
            let sets = self.resident_wire_sets.borrow();
            let entry = sets.get(&key)?;
            if entry.epoch == self.geometry_epoch || entry.layout.is_none() {
                return None;
            }
            let strong = Arc::strong_count(&entry.wires);
            if strong != 1 {
                if perf {
                    crate::perf_record!("[perf] resident-shared strong={strong}");
                }
                return None;
            }
            entry.epoch
        };
        let deltas = self.replay_since(cached_epoch)?;

        // Take ownership of the cached assembly (guaranteed unique above).
        // `prev_gen` is the content id the GPU currently holds for this set — the
        // base the wire-arena patch replays from.
        let (prev_gen, mut owned, mut layout) = {
            let removed = self.resident_wire_sets.borrow_mut().remove(&key)?;
            (
                removed.gen,
                Arc::try_unwrap(removed.wires).ok()?,
                removed.layout?,
            )
        };
        if layout.marker_start > owned.len() {
            return None;
        }

        // Re-tessellate the changed entities, replicating the full build's exact
        // context (anno derivation, empty selection, no cull / no LOD). Faded
        // copy goes into the assembly; the raw copy refreshes the memo, matching
        // wires_for_block_culled (memo stores pre-fade wires).
        let anno = if let Some(a) = anno_scale_override {
            a
        } else if self.current_layout == "Model" {
            self.annotation_scale
        } else {
            1.0
        };
        let blk = self.block_cache_arc_for(annotation_scale_handle, all_visible, style_viewport);
        let empty_sel: HashSet<Handle> = HashSet::default();
        let mut new_runs: HashMap<Handle, Vec<WireModel>> = HashMap::default();
        let mut memo_updates: Vec<(Handle, Arc<Vec<WireModel>>)> = Vec::new();
        let mut visible_changed: HashSet<Handle> = HashSet::default();
        for (h, kind) in &deltas {
            if matches!(kind, ChangeKind::Removed) {
                continue;
            }
            let Some(e) = self.document.get_entity(*h) else {
                continue;
            };
            if !self.resident_entity_visible(
                e,
                block,
                frozen_layers,
                annotation_scale_handle,
                all_visible,
            ) {
                continue;
            }
            visible_changed.insert(*h);
            let raw = tessellate_entity(
                &self.document,
                &empty_sel,
                style_viewport.or(self.active_viewport),
                bg,
                anno,
                annotation_scale_handle,
                e,
                Some(&blk),
                None,
                None,
                anno_scale_override.is_some(),
            );
            memo_updates.push((*h, Arc::new(raw.clone())));
            let mut faded = raw;
            self.apply_refedit_fade(&mut faded, bg);
            new_runs.insert(*h, faded);
        }
        {
            let mut memo = self.resident_tess_memo.borrow_mut();
            for (h, a) in memo_updates {
                memo.insert(h, a);
            }
        }
        // Capture whether any replaced/removed OLD run fed the Face3D/fill
        // pass before blanking its resident slots. Treating every removal as a
        // face change made deleting an ordinary line rescan/re-upload the whole
        // fill pass on large drawings.
        let old_face_pass_changed = deltas.iter().any(|(handle, _)| {
            layout
                .ranges
                .get(handle)
                .is_some_and(|&(start, len)| {
                    start + len <= layout.marker_start
                        && owned[start..start + len]
                            .iter()
                            .any(|wire| !wire.fill_tris.is_empty())
                })
        });

        // Keep the submission-order directory current. Adds are inserted by
        // the same effective SortEntitiesTable key used by the full builder;
        // hidden Modified entities retain a zero-length placeholder so grip
        // commit restores them at their original position.
        {
            let cache = self.sort_cache.borrow();
            let sort_map = cache
                .as_ref()
                .and_then(|(_, index)| index.get(&block));
            let effective = |handle: Handle| {
                sort_map
                    .and_then(|map| map.get(&handle.value()))
                    .copied()
                    .unwrap_or(handle.value())
            };
            for &(handle, kind) in &deltas {
                if matches!(kind, ChangeKind::Removed) {
                    // Keep the order entry as a tombstone. Undo/Redo can then
                    // restore the exact physical slot and submission position.
                    continue;
                }
                if visible_changed.contains(&handle) && !layout.order.contains(&handle) {
                    let key = effective(handle);
                    let position = layout
                        .order
                        .partition_point(|&existing| effective(existing) <= key);
                    layout.order.insert(position, handle);
                }
            }
        }

        // Patch changed runs without shifting the resident Vec. Delete/hide
        // blanks the old slots; same-shaped Undo/Redo restores them in place.
        // Only a true tail add/resize touches Vec length (and shifts the handful
        // of synthesized marker wires, never the drawing's entity runs).
        let mut gpu_runs: HashMap<Handle, Arc<Vec<WireModel>>> = HashMap::default();
        let mut index_edits = Vec::new();
        for &(handle, kind) in &deltas {
            let new_run = if matches!(kind, ChangeKind::Removed) {
                Vec::new()
            } else {
                new_runs.remove(&handle).unwrap_or_default()
            };
            gpu_runs.insert(handle, Arc::new(new_run.clone()));
            let new_len = new_run.len();
            let old_range = layout.ranges.remove(&handle);

            if new_len == 0 {
                if let Some((start, old_len)) = old_range {
                    if start + old_len > layout.marker_start {
                        return None;
                    }
                    for slot in &mut owned[start..start + old_len] {
                        *slot = WireModel::solid(
                            String::new(),
                            Vec::new(),
                            [0.0; 4],
                            false,
                        );
                        slot.aabb = [
                            f32::INFINITY,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            f32::NEG_INFINITY,
                        ];
                    }
                    layout.vacant.insert(handle, (start, old_len));
                    layout.tombstoned_wires =
                        layout.tombstoned_wires.saturating_add(old_len);
                    index_edits.push(WireIndexEdit {
                        handle,
                        start,
                        old_len,
                        new_len: old_len,
                        visible: false,
                    });
                }
                continue;
            }

            if let Some((start, vacant_len)) = layout.vacant.remove(&handle) {
                if vacant_len != new_len || start + vacant_len > layout.marker_start {
                    return None;
                }
                for (slot, wire) in owned[start..start + vacant_len]
                    .iter_mut()
                    .zip(new_run)
                {
                    *slot = wire;
                }
                layout.ranges.insert(handle, (start, new_len));
                layout.tombstoned_wires =
                    layout.tombstoned_wires.saturating_sub(vacant_len);
                index_edits.push(WireIndexEdit {
                    handle,
                    start,
                    old_len: vacant_len,
                    new_len,
                    visible: true,
                });
                continue;
            }

            if let Some((start, old_len)) = old_range {
                if start + old_len > layout.marker_start || layout.marker_start > owned.len() {
                    return None;
                }
                if old_len == new_len {
                    for (slot, wire) in owned[start..start + old_len]
                        .iter_mut()
                        .zip(new_run)
                    {
                        *slot = wire;
                    }
                    layout.ranges.insert(handle, (start, new_len));
                    continue;
                }
                // Shape-changing edits are relocation-free only for the
                // terminal entity (the live-polyline case). A non-terminal
                // resize takes the clean full-build fallback instead of
                // shifting every following range.
                if start + old_len != layout.marker_start {
                    return None;
                }
                index_edits.push(WireIndexEdit {
                    handle,
                    start,
                    old_len,
                    new_len,
                    visible: true,
                });
                owned.splice(start..start + old_len, new_run);
                let delta = new_len as isize - old_len as isize;
                layout.marker_start = (layout.marker_start as isize + delta) as usize;
                layout.ranges.insert(handle, (start, new_len));
                continue;
            }

            // A genuinely new handle must append after all live entity runs.
            // Monotonic document handles make this the ordinary Add path. A
            // middle insertion (custom sort key) falls back to a full rebuild.
            let order_index = layout.order.iter().position(|&h| h == handle)?;
            let insertion = layout.order[order_index + 1..]
                .iter()
                .find_map(|next| layout.ranges.get(next).map(|range| range.0))
                .unwrap_or(layout.marker_start);
            if insertion != layout.marker_start {
                return None;
            }
            index_edits.push(WireIndexEdit {
                handle,
                start: insertion,
                old_len: 0,
                new_len,
                visible: true,
            });
            owned.splice(insertion..insertion, new_run);
            layout.marker_start += new_len;
            layout.ranges.insert(handle, (insertion, new_len));
        }
        if !new_runs.is_empty() {
            return None;
        }
        if layout.tombstoned_wires > layout.marker_start / 2 {
            return None;
        }
        let added: HashSet<Handle> = deltas
            .iter()
            .filter_map(|&(handle, kind)| {
                matches!(kind, ChangeKind::Added).then_some(handle)
            })
            .collect();
        let mut saw_added = false;
        let mut new_handles_are_suffix = true;
        for handle in &layout.order {
            if added.contains(handle) {
                saw_added = true;
            } else if saw_added && layout.ranges.contains_key(handle) {
                new_handles_are_suffix = false;
                break;
            }
        }
        let face_pass_changed = old_face_pass_changed
            || deltas.iter().any(|&(handle, _)| {
                matches!(self.document.get_entity(handle), Some(EntityType::Face3D(_)))
                || gpu_runs.get(&handle).is_some_and(|run| {
                    run.iter().any(|wire| !wire.fill_tris.is_empty())
                })
            });

        let arc = Arc::new(owned);
        self.last_tess_wires.set(arc.len());
        self.resident_patch_hits
            .set(self.resident_patch_hits.get() + 1);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        // Hand the exact changed handles to the GPU wire arena so it patches just
        // those entities' slabs. Only for the Model set (the arena is model-only);
        // the render layer verifies prev_gen against what the GPU actually holds.
        if wire_gpu_patch_enabled() && block == self.model_space_block_handle() {
            *self.model_wire_gpu_patch.borrow_mut() = Some((
                prev_gen,
                gen,
                Arc::new(WireGpuPatch {
                    changes: Arc::new(deltas.clone()),
                    runs: Arc::new(gpu_runs),
                    index_edits: Arc::new(index_edits),
                    new_handles_are_suffix,
                    face_pass_changed,
                }),
            ));
        }
        let cur_epoch = self.geometry_epoch;
        let mut sets = self.resident_wire_sets.borrow_mut();
        sets.retain(|_, set| set.epoch == cur_epoch);
        sets.insert(
            key,
            ResidentWireSet {
                epoch: cur_epoch,
                gen,
                wires: Arc::clone(&arc),
                layout: Some(layout),
            },
        );
        if perf {
            crate::perf_record!(
                "[perf] resident-patch {:>7.1}ms wires={} changes={}",
                t_patch.elapsed().as_secs_f64() * 1000.0,
                arc.len(),
                deltas.len(),
            );
        }
        Some(arc)
    }

    /// Cached tessellation of the current layout block's paper-space entities,
    /// shared by `entity_wires_arc()` and the GPU sheet viewport. A thin
    /// "dressing" layer over the unified resident set (border drop +
    /// printable-area guide) — camera-independent, epoch-keyed, so paper
    /// pan/zoom never re-tessellates the sheet.
    fn paper_sheet_wires_arc(&self) -> Arc<Vec<WireModel>> {
        {
            let cache = self.paper_sheet_cache.borrow();
            if let Some((epoch, gen, arc)) = cache.get(&self.current_layout) {
                if *epoch == self.geometry_epoch {
                    self.last_model_wire_gen.set(*gen);
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        let scale = self.paper_annotation_scale_handle();
        let base = self.resident_wires_for(layout_block, None, scale, None, None);
        let mut wires = (*base).clone();
        // The overall "sheet" viewport now IS the paper view itself, so its own
        // border rectangle must not be drawn as an entity on the sheet.
        let sheet = self.current_layout_sheet_viewport_handle();
        if sheet.is_valid() {
            let sheet_name = sheet.value().to_string();
            wires.retain(|w| w.name != sheet_name);
        }
        // Printable-area guide (paper inset by plot margins), paper space only.
        if let Some(pa) = self.printable_area_wire() {
            wires.push(pa);
        }
        let arc = Arc::new(wires);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        let mut cache = self.paper_sheet_cache.borrow_mut();
        cache.retain(|_, (epoch, _, _)| *epoch == self.geometry_epoch);
        cache.insert(
            self.current_layout.clone(),
            (self.geometry_epoch, gen, Arc::clone(&arc)),
        );
        arc
    }

    /// Build WireModels from all document entities for the current layout.
    /// Returns a shared `Arc` so `build_primitive()` can skip the clone during
    /// navigation frames where no preview wires are active.
    pub(super) fn entity_wires_arc(&self) -> Arc<Vec<WireModel>> {
        let key = (self.geometry_epoch, self.camera_generation);
        {
            let cache = self.wire_cache.borrow();
            if let Some((cached_key, gen, ref arc)) = *cache {
                if cached_key == key {
                    self.last_model_wire_gen.set(gen);
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        // Model space: reuse the resident, camera-independent static wire set the
        // render already holds (keyed on geometry_epoch only). This is the FULL
        // entity set — pick / snap want every entity, not a view-culled subset —
        // and it does NOT re-tessellate on a camera move. The old path went
        // through the camera_generation-keyed, view-culled paper_sheet set, so
        // every pan/rotate cold-missed it and paid a full O(visible) re-tess
        // (~300 ms on large drawings) the first time hit-testing ran after the
        // move — the "jump" at the start of each gesture. The tile args are
        // unused by `model_tile_wires_arc`.
        if self.current_layout == "Model" {
            let cam = self.camera.borrow().clone();
            return self.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        }
        // Paper space: extend sheet wires with projected viewport content.
        // This composite is the CPU pick/snap set; the projection follows the
        // paper camera, so the key keeps `camera_generation`.
        let mut wires = (*self.paper_sheet_wires_arc()).clone();
        wires.extend(self.viewport_content_wires(layout_block, None, None));
        let arc = Arc::new(wires);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        *self.wire_cache.borrow_mut() = Some((key, gen, Arc::clone(&arc)));
        arc
    }

    /// Build WireModels from all document entities + optional preview wire.
    ///
    /// Returns the memoized set by `Arc` — callers only ever iterate it, and
    /// the previous per-call deep clone was a full copy of every wire buffer
    /// (hundreds of MB on large mesh imports, #358).
    pub fn entity_wires(&self) -> Arc<Vec<WireModel>> {
        self.entity_wires_arc()
    }

    /// Return paper entities and projected model-viewport entities separately.
    /// A plot-only render override is applied to cloned wires; viewport entities
    /// and their saved display modes remain unchanged.
    pub fn plot_wire_groups(
        &self,
        render_mode_override: Option<acadrust::entities::ViewportRenderMode>,
    ) -> (Vec<WireModel>, Vec<WireModel>) {
        let apply_mode = |wires: &mut Vec<WireModel>, mode| {
            let flags = view::render::render_mode_flags(mode);
            for wire in wires.iter_mut().filter(|wire| wire.fill_is_3d) {
                if !flags.face3d_fill && !flags.mesh_fill {
                    wire.fill_tris.clear();
                    wire.fill_tris_low.clear();
                }
                if !flags.show_3d_edges {
                    wire.points.clear();
                    wire.points_low.clear();
                }
            }
        };
        if self.current_layout == "Model" {
            let mut wires = self.entity_wires_arc().as_ref().clone();
            apply_mode(
                &mut wires,
                render_mode_override.unwrap_or_else(|| self.active_model_tile_render_mode()),
            );
            return (wires, Vec::new());
        }
        let paper_block = self.current_layout_block_handle();
        let (_, _, viewport_handles) = self.paper_viewport_handles();
        let mut model_wires = Vec::new();
        for handle in viewport_handles.iter().copied() {
            let Some(EntityType::Viewport(viewport)) = self.document.get_entity(handle) else {
                continue;
            };
            if viewport.common.owner_handle != paper_block || !viewport.status.is_on {
                continue;
            }
            let mode = render_mode_override.unwrap_or(viewport.render_mode);
            let mut wires = self.viewport_content_wires(paper_block, Some(handle), None);
            apply_mode(&mut wires, mode);
            model_wires.extend(wires);
        }
        (self.paper_sheet_wires_arc().as_ref().clone(), model_wires)
    }

    /// Per-entity stable draw-order depth, keyed by entity handle value.
    /// A full build assigns sparse labels in effective draw order. Incremental
    /// Add/Remove then changes only the named handle: existing siblings retain
    /// their depth, avoiding an O(all entities) map rewrite and GPU const upload.
    pub(super) fn draw_depth_map(&self) -> Arc<HashMap<u64, [f32; 2]>> {
        {
            let cache = self.draw_depth_cache.borrow();
            if let Some(cache) = cache.as_ref() {
                if cache.epoch == self.geometry_epoch {
                    return Arc::clone(&cache.depths);
                }
            }
        }
        let perf = crate::perf::enabled();
        let t_depth = iced::time::Instant::now();

        // Replay Add/Remove into the retained sparse order directory. Removed
        // entries stay in `blocks`/`owners` as tombstones, so Undo/Redo restores
        // their exact prior label without shifting a sibling.
        let stale_cache = self.draw_depth_cache.borrow_mut().take();
        if let Some(mut cache) = stale_cache {
            if let Some(deltas) = self.replay_since(cache.epoch) {
                if deltas
                    .iter()
                    .all(|(_, kind)| matches!(kind, ChangeKind::Modified))
                {
                    cache.epoch = self.geometry_epoch;
                    let arc = Arc::clone(&cache.depths);
                    *self.draw_depth_cache.borrow_mut() = Some(cache);
                    return arc;
                }

                let depths = Arc::make_mut(&mut cache.depths);
                let ms = self.model_space_block_handle();
                let mut incremental_ok = true;
                for &(handle, kind) in &deltas {
                    match kind {
                        ChangeKind::Modified => {}
                        ChangeKind::Removed => {
                            depths.remove(&handle.value());
                        }
                        ChangeKind::Added => {
                            let Some(entity) = self.document.get_entity(handle) else {
                                continue;
                            };
                            if matches!(
                                entity,
                                EntityType::Solid3D(_)
                                    | EntityType::Region(_)
                                    | EntityType::Body(_)
                                    | EntityType::Surface(_)
                                    | EntityType::Mesh(_)
                                    | EntityType::PolygonMesh(_)
                                    | EntityType::PolyfaceMesh(_)
                            ) {
                                continue;
                            }
                            let common = entity.common();
                            let block = if common.owner_handle.is_null() {
                                ms
                            } else {
                                common.owner_handle
                            };
                            let value = handle.value();
                            if let Some(&known_block) = cache.owners.get(&value) {
                                if known_block != block {
                                    incremental_ok = false;
                                    break;
                                }
                                let restored = cache
                                    .blocks
                                    .get(&block)
                                    .and_then(|order| {
                                        order.iter().find(|entry| entry.handle == value)
                                    });
                                let Some(entry) = restored else {
                                    incremental_ok = false;
                                    break;
                                };
                                depths.insert(
                                    value,
                                    draw_depth_value(entry.label, entry.half_label),
                                );
                                continue;
                            }
                            // A newly allocated handle cannot already have a
                            // SortEntitiesTable override; its effective key is
                            // therefore its own handle.
                            let order = cache.blocks.entry(block).or_default();
                            let position = order.partition_point(|entry| {
                                (entry.effective, entry.handle) <= (value, value)
                            });
                            let Some((label, half_label)) =
                                inserted_draw_depth_label(order, position)
                            else {
                                // Pathological middle insertion exhausted the
                                // reserved label gap. A full rebuild is the safe
                                // rare fallback (ordinary monotonic adds use the
                                // two-million-slot tail reserve).
                                incremental_ok = false;
                                break;
                            };
                            order.insert(
                                position,
                                DrawDepthEntry {
                                    handle: value,
                                    effective: value,
                                    label,
                                    half_label,
                                },
                            );
                            cache.owners.insert(value, block);
                            depths.insert(value, draw_depth_value(label, half_label));
                        }
                    }
                }
                if incremental_ok {
                    cache.epoch = self.geometry_epoch;
                    let arc = Arc::clone(&cache.depths);
                    *self.draw_depth_cache.borrow_mut() = Some(cache);
                    if perf {
                        crate::perf_record!(
                            "[perf] draw-depth-patch {:>7.1}ms entries={} changes={}",
                            t_depth.elapsed().as_secs_f64() * 1000.0,
                            arc.len(),
                            deltas.len(),
                        );
                    }
                    return arc;
                }
            }
        }

        use acadrust::objects::ObjectType;
        // Per-block SortEntitiesTable overrides: block -> (entity_val -> sort_val).
        let mut overrides: HashMap<Handle, HashMap<u64, u64>> = HashMap::default();
        for obj in self.document.objects.values() {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if !t.is_empty() {
                    overrides.insert(
                        t.block_owner_handle,
                        t.entries()
                            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                            .collect(),
                    );
                }
            }
        }
        let ms = self.model_space_block_handle();
        // Group entities by owning block, carrying each entity's effective key.
        let mut by_block: HashMap<Handle, Vec<(u64, u64)>> = HashMap::default();
        for e in self.document.entities() {
            let c = e.common();
            // 3D meshes keep real geometric depth — exclude them from
            // draw-order biasing so 3D occlusion is never flattened.
            if matches!(
                e,
                EntityType::Solid3D(_)
                    | EntityType::Region(_)
                    | EntityType::Body(_)
                    | EntityType::Surface(_)
                    | EntityType::Mesh(_)
                    | EntityType::PolygonMesh(_)
                    | EntityType::PolyfaceMesh(_)
            ) {
                continue;
            }
            let block = if c.owner_handle.is_null() {
                ms
            } else {
                c.owner_handle
            };
            let hv = c.handle.value();
            let eff = overrides
                .get(&block)
                .and_then(|m| m.get(&hv))
                .copied()
                .unwrap_or(hv);
            by_block.entry(block).or_default().push((hv, eff));
        }
        let mut depth_map: HashMap<u64, [f32; 2]> = HashMap::default();
        let mut owners: HashMap<u64, Handle> = HashMap::default();
        let mut blocks: HashMap<Handle, Vec<DrawDepthEntry>> = HashMap::default();
        for (block, mut order) in by_block {
            order.sort_by_key(|(handle, effective)| (*effective, *handle));
            let entries = seed_draw_depth_entries(order);
            for entry in &entries {
                depth_map.insert(
                    entry.handle,
                    draw_depth_value(entry.label, entry.half_label),
                );
                owners.insert(entry.handle, block);
            }
            blocks.insert(block, entries);
        }
        let arc = Arc::new(depth_map);
        *self.draw_depth_cache.borrow_mut() = Some(DrawDepthCache {
            epoch: self.geometry_epoch,
            depths: Arc::clone(&arc),
            blocks,
            owners,
        });
        if perf {
            crate::perf_record!(
                "[perf] draw-depth     {:>7.1}ms entries={}",
                t_depth.elapsed().as_secs_f64() * 1000.0,
                arc.len(),
            );
        }
        arc
    }

    pub(super) fn hatch_models_arc(&self) -> Arc<Vec<HatchModel>> {
        // Hatch models bake the selection tint (issue #71), so they depend on
        // the *selected set* — but NOT on hover. Keying on `selection_generation`
        // (which also bumps on every hover) made each hover-over a new entity
        // rebuild every hatch model: an O(N-hatch) stutter on hatch-heavy
        // drawings. Key on a signature of `selected` instead, so hover (which
        // never changes `selected`) keeps the cache warm.
        let sel_sig = self.selected_hatch_sig();
        let target_block = self.content_render_block_handle();
        let key = (target_block, self.current_layout.clone());
        {
            let reuse = {
                let cache = self.hatch_cache.borrow();
                match cache.get(&key) {
                    // Selection tint is baked in, so the selected set must also
                    // match; category = a changed direct fill or an INSERT
                    // whose expanded block tree can contain hatch fills.
                    Some((cached_epoch, cached_sel, arc))
                        if *cached_sel == sel_sig
                            && self.category_cache_valid(
                                *cached_epoch,
                                CACHE_CATEGORY_HATCH | CACHE_CATEGORY_INSERT_HATCH,
                                |h| {
                                    self.hatches.contains_key(&h)
                                        || matches!(
                                            self.document.get_entity(h),
                                            Some(EntityType::Insert(_))
                                        )
                                },
                            ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _, _)) = self.hatch_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let arc = Arc::new(self.synced_hatch_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        self.hatch_cache.borrow_mut().insert(
            key,
            (self.geometry_epoch, sel_sig, Arc::clone(&arc)),
        );
        arc
    }

    /// Order-independent signature of the selected set. Cheap (the set is
    /// normally a handful of entities) and unchanged by hover, so caches that
    /// only depend on what's *selected* don't thrash on rollover.
    fn selected_set_sig(&self) -> u64 {
        let mut sig: u64 = self.selected.len() as u64;
        for h in self.selected.iter() {
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig
    }

    fn selected_hatch_sig(&self) -> u64 {
        let mut sig = 0u64;
        let mut count = 0u64;
        for &h in &self.selected {
            if !self.hatches.contains_key(&h)
                && !matches!(self.document.get_entity(h), Some(EntityType::Insert(_)))
            {
                continue;
            }
            count += 1;
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig ^ count
    }

    pub(super) fn wipeout_models_arc(&self) -> Arc<Vec<HatchModel>> {
        let target_block = self.content_render_block_handle();
        let key = (target_block, self.current_layout.clone());
        {
            let reuse = {
                let cache = self.wipeout_cache.borrow();
                match cache.get(&key) {
                    Some((cached_epoch, arc))
                        // wipeout_models scans the whole document for Wipeout
                        // entities; relevance = the changed handle is a Wipeout.
                        if self.category_cache_valid(
                            *cached_epoch,
                            CACHE_CATEGORY_WIPEOUT,
                            |h| {
                                matches!(
                                    self.document.get_entity(h),
                                    Some(EntityType::Wipeout(_))
                                )
                            },
                        ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _)) = self.wipeout_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let arc = Arc::new(self.wipeout_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
        ));
        self.wipeout_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    pub(super) fn images_arc(&self) -> Arc<Vec<ImageModel>> {
        let target_block = self.content_render_block_handle();
        {
            let reuse = {
                let cache = self.image_cache.borrow();
                match cache.get(&target_block) {
                    Some((cached_epoch, arc))
                        if self.category_cache_valid(
                            *cached_epoch,
                            CACHE_CATEGORY_IMAGE,
                            |h| self.images.contains_key(&h),
                        ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                // No image changed since — keep it warm, just advance the sync
                // epoch so the next replay window stays short.
                if let Some((e, _)) = self.image_cache.borrow_mut().get_mut(&target_block) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let arc = Arc::new(self.image_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        self.image_cache
            .borrow_mut()
            .insert(target_block, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Image / OLE-frame models owned by `target_block`, optionally dropping
    /// those whose layer is frozen in a content viewport (`frozen`).
    fn image_models(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<ImageModel> {
        let depth_map = self.draw_depth_map();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_viewport(viewport);
        let mut models = Vec::new();
        let root = if self.block_edit_block.is_none() {
            viewport.map(|viewport| render_graph::SceneRoot::Viewport {
                paper_block: self.current_layout_block_handle(),
                viewport,
                model_block: target_block,
            })
        } else {
            None
        }
        .unwrap_or_else(|| self.render_scene_root(target_block));
        graph.walk_root(
            root,
            |entity, context| {
                context.is_instanced()
                    || !self.entity_temporarily_hidden(entity.common().handle)
            },
            |entity, context| {
                let handle = entity.common().handle;
                let Some(model) = self.images.get(&handle) else {
                    return;
                };
                let mut placed = model.clone();
                if context.is_instanced() {
                    for (corner, low) in
                        placed.corners.iter_mut().zip(&mut placed.corners_low)
                    {
                        let point = acadrust::types::Vector3::new(
                            corner[0] as f64 + low[0] as f64,
                            corner[1] as f64 + low[1] as f64,
                            corner[2] as f64 + low[2] as f64,
                        );
                        let point = context.transform.apply(point);
                        *corner = [point.x as f32, point.y as f32, point.z as f32];
                        *low = [
                            (point.x - corner[0] as f64) as f32,
                            (point.y - corner[1] as f64) as f32,
                            (point.z - corner[2] as f64) as f32,
                        ];
                    }
                    for vertex in &mut placed.verts {
                        let point = acadrust::types::Vector3::new(
                            vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                            vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                            vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
                        );
                        let point = context.transform.apply(point);
                        vertex.pos = [
                            point.x as f32,
                            point.y as f32,
                            point.z as f32,
                        ];
                        vertex.pos_low = [
                            (point.x - vertex.pos[0] as f64) as f32,
                            (point.y - vertex.pos[1] as f64) as f32,
                            (point.z - vertex.pos[2] as f64) as f32,
                        ];
                    }
                }
                placed.draw_depth = context.draw_depth(handle, depth_map.as_ref());
                models.push(placed);
            },
        );
        models
    }

    /// Images owned by the active paper layout block only. The full-canvas
    /// sheet viewport uses this so model-block images don't bleed onto the
    /// paper sheet (mirrors `paper_canvas_hatches`).
    pub(super) fn paper_sheet_images(&self) -> Arc<Vec<ImageModel>> {
        let layout_block = self.current_layout_block_handle();
        Arc::new(self.image_models(
            layout_block,
            None,
            self.paper_annotation_scale_handle(),
            self.annotation_all_visible(),
            None,
        ))
    }

    pub(super) fn meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        let target_block = self.content_render_block_handle();
        let key = (target_block, self.current_layout.clone());
        {
            let reuse = {
                let cache = self.mesh_cache.borrow();
                match cache.get(&key) {
                    // Top-level solids seed self.meshes; the instanced_block part
                    // is driven by INSERTs, so an INSERT edit (e.g. a move) must
                    // also invalidate. Block-definition edits route through
                    // bump_geometry (a full delta) and invalidate regardless.
                    Some((cached_epoch, arc))
                        if self.category_cache_valid(
                            *cached_epoch,
                            CACHE_CATEGORY_MESH,
                            |h| {
                                self.meshes.contains_key(&h)
                                    || matches!(
                                        self.document.get_entity(h),
                                        Some(EntityType::Insert(_))
                                    )
                            },
                        ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _)) = self.mesh_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let arc = Arc::new(self.mesh_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        self.mesh_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Block rendered by model-content viewports. Paper layout entities belong
    /// only to the sheet viewport; floating paper viewports always show model
    /// space. BEDIT is the sole exception and renders the edited block itself.
    fn content_render_block_handle(&self) -> Handle {
        self.block_edit_block
            .filter(|block| !block.is_null())
            .unwrap_or_else(|| self.model_space_block_handle())
    }

    fn render_scene_root(&self, block: Handle) -> render_graph::SceneRoot {
        let role = if self.block_edit_block == Some(block) {
            render_graph::BlockRootRole::DefinitionEdit
        } else if block == self.model_space_block_handle() {
            render_graph::BlockRootRole::ModelSpace
        } else {
            render_graph::BlockRootRole::PaperSpace
        };
        render_graph::SceneRoot::Block(render_graph::BlockRoot {
            record: block,
            role,
        })
    }

    fn interaction_block_handle(&self) -> Handle {
        if self.block_edit_block.is_none() && self.active_viewport.is_some() {
            self.model_space_block_handle()
        } else {
            self.current_layout_block_handle()
        }
    }

    fn interaction_viewport_frozen_layers(&self) -> Option<&[Handle]> {
        if self.block_edit_block.is_some() {
            return None;
        }
        let handle = self.active_viewport?;
        match self.document.get_entity(handle) {
            Some(EntityType::Viewport(viewport)) => Some(&viewport.frozen_layers),
            _ => None,
        }
    }

    fn interaction_space_key(&self) -> u64 {
        let block = self.interaction_block_handle().value();
        let viewport = self
            .active_viewport
            .filter(|_| self.block_edit_block.is_none())
            .map_or(0, |handle| handle.value());
        let frozen = self
            .interaction_viewport_frozen_layers()
            .map_or(0, |layers| {
                layers
                    .iter()
                    .fold(layers.len() as u64, |signature, handle| {
                        signature ^ handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    })
            });
        block ^ viewport.rotate_left(29) ^ frozen.rotate_left(47)
    }

    fn interaction_layer_frozen(&self, layer: &str) -> bool {
        let Some(frozen) = self.interaction_viewport_frozen_layers() else {
            return false;
        };
        self.document
            .layers
            .get(layer)
            .is_some_and(|record| frozen.contains(&record.handle))
    }

    fn interaction_meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        let block = self.interaction_block_handle();
        if self.block_edit_block.is_none() && block == self.model_space_block_handle() {
            let Some(frozen) = self.interaction_viewport_frozen_layers() else {
                return self.meshes_arc();
            };
            if frozen.is_empty() {
                return self.meshes_arc();
            }
            let key = self.interaction_space_key();
            if let Some((epoch, cached_key, meshes)) = self.interaction_mesh_cache.borrow().as_ref()
            {
                if *epoch == self.geometry_epoch && *cached_key == key {
                    return Arc::clone(meshes);
                }
            }
            let frozen: HashSet<Handle> = frozen.iter().copied().collect();
            let viewport = self.active_viewport.unwrap_or(Handle::NULL);
            let meshes = self.meshes_for_viewport(viewport, &frozen);
            *self.interaction_mesh_cache.borrow_mut() =
                Some((self.geometry_epoch, key, Arc::clone(&meshes)));
            return meshes;
        }
        let key = self.interaction_space_key();
        if let Some((epoch, cached_key, meshes)) = self.interaction_mesh_cache.borrow().as_ref() {
            if *epoch == self.geometry_epoch && *cached_key == key {
                return Arc::clone(meshes);
            }
        }
        let meshes = Arc::new(self.mesh_models(
            block,
            None,
            self.displayed_annotation_scale_handle(),
            self.annotation_all_visible(),
            self.active_viewport,
        ));
        *self.interaction_mesh_cache.borrow_mut() =
            Some((self.geometry_epoch, key, Arc::clone(&meshes)));
        meshes
    }

    fn mesh_pick_lookup(&self, meshes: &Arc<Vec<MeshLodSet>>) -> Arc<HashMap<Handle, Vec<u32>>> {
        let source = Arc::as_ptr(meshes) as usize;
        {
            let cache = self.mesh_pick_lookup_cache.borrow();
            if let Some((ptr, weak, lookup)) = cache.as_ref() {
                if *ptr == source
                    && weak
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, meshes))
                {
                    return Arc::clone(lookup);
                }
            }
        }
        let mut lookup: HashMap<Handle, Vec<u32>> = HashMap::default();
        for (index, set) in meshes.iter().enumerate() {
            let Some(value) = set
                .lods
                .first()
                .and_then(|mesh| mesh.name.parse::<u64>().ok())
            else {
                continue;
            };
            lookup
                .entry(Handle::new(value))
                .or_default()
                .push(index as u32);
        }
        let lookup = Arc::new(lookup);
        *self.mesh_pick_lookup_cache.borrow_mut() =
            Some((source, Arc::downgrade(meshes), Arc::clone(&lookup)));
        lookup
    }

    /// Solid-mesh set (top-level + block-instanced), optionally dropping those
    /// whose layer is frozen in a content viewport (`frozen`). `None` reproduces
    /// the full set.
    fn mesh_models(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<MeshLodSet> {
        // Top-level solids: drop those whose layer is off/frozen or that are
        // flagged invisible / isolated-hidden, mirroring the 2D wire path, plus
        // any whose layer is frozen in the requesting viewport.
        let mut all: Vec<MeshLodSet> = self
            .meshes
            .iter()
            .filter(|(&h, _)| {
                self.mesh_entity_visible(h)
                    && self
                        .document
                        .get_entity(h)
                        .map(|e| {
                            !self.layer_frozen_in(&e.common().layer, frozen)
                                && self.belongs_to_visible_block(
                                    h,
                                    e.common().owner_handle,
                                    target_block,
                                )
                        })
                        .unwrap_or(false)
            })
            .map(|(&handle, set)| {
                let mut set = set.clone();
                if let Some(entity) = self.document.get_entity(handle) {
                    let style = crate::scene::view::render::render_style_for_viewport(
                        &self.document,
                        entity,
                        viewport,
                    );
                    let attached_material = set
                        .material
                        .as_ref()
                        .is_some_and(|material| material.handle.is_some());
                    for mesh in &mut set.lods {
                        if !attached_material {
                            mesh.color[0..3].copy_from_slice(&style.0[0..3]);
                        }
                        mesh.color[3] = style.0[3];
                    }
                    if let Some(material) = set.material.as_mut() {
                        if !attached_material {
                            material.diffuse[0..3].copy_from_slice(&style.0[0..3]);
                        }
                        material.diffuse[3] = style.0[3];
                    }
                }
                set
            })
            .collect();
        // Block-definition solids are instanced per INSERT of the ACTIVE space's
        // block so a block placed at an INSERT scale renders at the right size
        // (#123) — model space normally, the edited block in a BEDIT editor so
        // model-space solids don't leak into it (#261).
        all.extend(self.instanced_block_meshes(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
        ));
        all
    }

    /// True when `layer` is turned off or frozen — entities on it never render.
    fn layer_hidden(&self, layer: &str) -> bool {
        self.document
            .layers
            .get(layer)
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
    }

    /// True when `layer`'s handle is in a content viewport's per-viewport
    /// frozen-layer set (VP freeze). Mirrors the wire path's test in
    /// [`Scene::resident_entity_visible`] so fills / images / meshes hide on the
    /// same layers wires do. `None` / empty set → never frozen.
    fn layer_frozen_in(&self, layer: &str, frozen: Option<&HashSet<Handle>>) -> bool {
        match frozen {
            Some(fz) if !fz.is_empty() => self
                .document
                .layers
                .get(layer)
                .map(|l| fz.contains(&l.handle))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Order-independent signature of a per-viewport frozen-layer set, so
    /// viewports that freeze the same layers share one cached filtered fill /
    /// image / mesh set. Matches the fold the resident wire cache uses.
    fn frozen_layers_sig(frozen: &HashSet<Handle>) -> u64 {
        let mut sig: u64 = frozen.len() as u64;
        for h in frozen.iter() {
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig
    }

    /// Hatch / 2-D-solid fills for a content viewport, with its frozen layers
    /// removed. Cached per frozen-set signature (viewports sharing a frozen set
    /// share the build). No frozen layers → the shared unfiltered set.
    pub(super) fn hatch_models_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<HatchModel>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.hatch_models_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ viewport.value().rotate_left(23);
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig;
        let key = (target_block, self.current_layout.clone(), sig);
        let sel = self.selected_hatch_sig();
        if let Some((e, s, arc)) = self.frozen_hatch_cache.borrow().get(&key) {
            if *e == self.geometry_epoch && *s == sel {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.synced_hatch_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
        ));
        self.frozen_hatch_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, sel, Arc::clone(&arc)));
        arc
    }

    /// Wipeout fills for a content viewport, with its frozen layers removed.
    pub(super) fn wipeout_models_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<HatchModel>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.wipeout_models_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ viewport.value().rotate_left(23);
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig;
        let key = (target_block, self.current_layout.clone(), sig);
        if let Some((e, arc)) = self.frozen_wipeout_cache.borrow().get(&key) {
            if *e == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.wipeout_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
        ));
        self.frozen_wipeout_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Image / OLE models for a content viewport, with its frozen layers removed.
    pub(super) fn images_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<ImageModel>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.images_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ viewport.value().rotate_left(23);
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig;
        let key = (target_block, sig);
        if let Some((e, arc)) = self.frozen_image_cache.borrow().get(&key) {
            if *e == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.image_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
        ));
        self.frozen_image_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Solid meshes for a content viewport, with its frozen layers removed.
    pub(super) fn meshes_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
    ) -> Arc<Vec<MeshLodSet>> {
        if viewport.is_null() && frozen.is_empty() {
            return self.meshes_arc();
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ viewport.value().rotate_left(23);
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig;
        let key = (target_block, self.current_layout.clone(), sig);
        if let Some((e, arc)) = self.frozen_mesh_cache.borrow().get(&key) {
            if *e == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = Arc::new(self.mesh_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            Some(viewport),
        ));
        self.frozen_mesh_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// True when `handle`'s entity sits on a locked layer. Locked objects stay
    /// visible and snappable but cannot be selected or modified — callers in
    /// the pick / modify paths consult this to skip them.
    pub fn is_layer_locked(&self, handle: Handle) -> bool {
        self.document
            .get_entity(handle)
            .map(|e| e.common().layer.clone())
            .and_then(|name| self.document.layers.get(&name).map(|l| l.is_locked()))
            .unwrap_or(false)
    }

    /// The name of the locked layer `handle` sits on, if any (for messages).
    pub fn locked_layer_name(&self, handle: Handle) -> Option<String> {
        let name = self
            .document
            .get_entity(handle)
            .map(|e| e.common().layer.clone())?;
        let locked = self
            .document
            .layers
            .get(&name)
            .map(|l| l.is_locked())
            .unwrap_or(false);
        locked.then_some(name)
    }

    /// File-backed visibility shared by top-level and block-definition meshes.
    /// Object isolation is intentionally absent: a retained INSERT must retain
    /// every visible child of its block definition.
    fn mesh_definition_entity_visible(&self, handle: Handle) -> bool {
        let Some(c) = self.document.get_entity(handle).map(|e| e.common()) else {
            return false;
        };
        if c.invisible {
            return false;
        }
        !self.layer_hidden(&c.layer)
    }

    /// Visibility test for a top-level solid mesh entity, mirroring the direct
    /// 2D wire path.
    fn mesh_entity_visible(&self, handle: Handle) -> bool {
        if !self.mesh_definition_entity_visible(handle) {
            return false;
        }
        if self.entity_temporarily_hidden(handle) {
            return false;
        }
        true
    }

    fn mesh_visible_for_interaction(&self, handle: Handle) -> bool {
        self.mesh_entity_visible(handle)
            && self.document.get_entity(handle).is_some_and(|entity| {
                !self.interaction_layer_frozen(&entity.common().layer)
                    && self.belongs_to_visible_block(
                        handle,
                        entity.common().owner_handle,
                        self.interaction_block_handle(),
                    )
            })
    }

    /// One transformed mesh per block-definition solid instance reached from an
    /// INSERT owned by `layout_block`. Nested INSERTs accumulate their
    /// transform. Empty when no block solids exist. (#123)
    fn instanced_block_meshes(
        &self,
        layout_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<MeshLodSet> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let depth_map = self.draw_depth_map();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_viewport(viewport);
        let mut out = Vec::new();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                context.is_instanced()
                    || !self.entity_temporarily_hidden(entity.common().handle)
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let handle = entity.common().handle;
                let Some(set) = self.block_meshes.get(&handle) else {
                    return;
                };
                let inherit = self.mesh_inherit_for_path(&context.insert_path);
                let own_alpha =
                    set.lods.first().map(|mesh| mesh.color[3]).unwrap_or(1.0);
                let mut transformed =
                    transform_block_mesh_lod_set(set, &context.transform);
                if let Some(color) = self.block_mesh_override_color(
                    entity,
                    handle,
                    inherit.as_ref(),
                    own_alpha,
                ) {
                    for mesh in &mut transformed.lods {
                        mesh.color = color;
                    }
                    if let Some(material) = transformed.material.as_mut() {
                        if material.handle.is_none() {
                            material.diffuse = color;
                        }
                    }
                }
                if let Some(material) =
                    self.block_mesh_override_material(entity, inherit.as_ref())
                {
                    material.apply_to_with_face_overrides(
                        &mut transformed,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                }
                let root_name = context.root_handle.value().to_string();
                for mesh in &mut transformed.lods {
                    mesh.name = root_name.clone();
                }
                out.push(transformed);
            },
        );
        out
    }

    fn mesh_inherit_for_path(
        &self,
        path: &[DxfInsert],
    ) -> Option<BlockMeshInherit> {
        let first = path.first()?;
        let entity = EntityType::Insert(first.clone());
        let bg = self.current_bg();
        let insert_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::render_style_for(&self.document, &entity).0,
            bg,
        );
        let layer0_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::layer_render_style(
                &self.document,
                &first.common.layer,
            )
            .color,
            bg,
        );
        let mut inherit = BlockMeshInherit {
            insert_color,
            layer0_color,
            insert_material:
                crate::scene::model::material_model::resolve_material_with_base(
                    &self.document,
                    &entity,
                    insert_color,
                    None,
                    self.material_base_dir.as_deref(),
                ),
            layer0_material:
                crate::scene::model::material_model::resolve_layer_material_with_base(
                    &self.document,
                    &first.common.layer,
                    layer0_color,
                    self.material_base_dir.as_deref(),
                ),
        };
        for insert in &path[1..] {
            inherit = self.chain_mesh_inherit(insert, &inherit);
        }
        Some(inherit)
    }

    /// Background colour for the current layout (model vs paper).
    fn current_bg(&self) -> [f32; 4] {
        if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        }
    }

    /// Chain block-child colour inheritance into a nested INSERT, mirroring
    /// `expand_insert`: ByBlock keeps the parent source; a nested insert on
    /// layer "0" with ByLayer colour adopts the parent layer-0 target; else it
    /// uses its own resolved colour / layer. Returned colours are bg-adapted.
    fn chain_mesh_inherit(
        &self,
        ins: &acadrust::entities::Insert,
        parent: &BlockMeshInherit,
    ) -> BlockMeshInherit {
        use acadrust::types::Color;
        let bg = self.current_bg();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&ins.common.layer);
        let insert_entity = EntityType::Insert(ins.clone());
        let has_book_color =
            crate::scene::view::render::has_resolved_book_color(
                &self.document,
                &insert_entity,
            );
        let child_ins_color = if !has_book_color && ins.common.color == Color::ByBlock {
            parent.insert_color
        } else if !has_book_color && on_l0 && ins.common.color == Color::ByLayer {
            parent.layer0_color
        } else {
            crate::scene::view::render::adapt_to_bg(
                crate::scene::view::render::render_style_for(
                    &self.document,
                    &EntityType::Insert(ins.clone()),
                )
                .0,
                bg,
            )
        };
        let child_l0 = if on_l0 {
            parent.layer0_color
        } else {
            crate::scene::view::render::adapt_to_bg(
                crate::scene::view::render::layer_render_style(&self.document, &ins.common.layer)
                    .color,
                bg,
            )
        };
        let child_insert_material = if ins.common.material_flags == 1 {
            parent.insert_material.clone()
        } else if on_l0 && ins.common.material_flags == 0 {
            parent.layer0_material.clone()
        } else {
            crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                &insert_entity,
                child_ins_color,
                Some(&parent.insert_material),
                self.material_base_dir.as_deref(),
            )
        };
        let child_l0_material = if on_l0 {
            parent.layer0_material.clone()
        } else {
            crate::scene::model::material_model::resolve_layer_material_with_base(
                &self.document,
                &ins.common.layer,
                child_l0,
                self.material_base_dir.as_deref(),
            )
        };
        BlockMeshInherit {
            insert_color: child_ins_color,
            layer0_color: child_l0,
            insert_material: child_insert_material,
            layer0_material: child_l0_material,
        }
    }

    /// Per-instance colour override for a block-internal solid mesh: ByBlock →
    /// insert colour, layer-0 + ByLayer → insert-layer colour, else `None`
    /// (keep the cached own-layer colour). Applies the same REFEDIT fade as
    /// `recolor_meshes`, keyed on the inner solid handle. (#221)
    fn block_mesh_override_color(
        &self,
        e: &EntityType,
        h: Handle,
        inherit: Option<&BlockMeshInherit>,
        own_alpha: f32,
    ) -> Option<[f32; 4]> {
        let inherit = inherit?;
        use acadrust::types::Color;
        let common = e.common();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&common.layer);
        let has_book_color =
            crate::scene::view::render::has_resolved_book_color(&self.document, e);
        let mut c = if !has_book_color && common.color == Color::ByBlock {
            inherit.insert_color
        } else if !has_book_color && on_l0 && common.color == Color::ByLayer {
            // Inherit the insert layer's RGB but keep the solid's own alpha,
            // matching the wire/hatch path (render_style_for_block_sub).
            [
                inherit.layer0_color[0],
                inherit.layer0_color[1],
                inherit.layer0_color[2],
                own_alpha,
            ]
        } else {
            return None;
        };
        if let Some(keep) = &self.refedit_keep {
            if !keep.contains(&h) {
                c = crate::scene::cache::block_cache::fade_toward_bg(c, self.current_bg());
            }
        }
        Some(c)
    }

    fn block_mesh_override_material(
        &self,
        entity: &EntityType,
        inherit: Option<&BlockMeshInherit>,
    ) -> Option<crate::scene::model::material_model::MeshMaterial> {
        let inherit = inherit?;
        let common = entity.common();
        if common.material_flags == 1 {
            Some(inherit.insert_material.clone())
        } else if common.material_flags == 0
            && crate::scene::view::render::is_effective_layer_zero(&common.layer)
        {
            Some(inherit.layer0_material.clone())
        } else {
            None
        }
    }

    /// Hatches eligible for click / box / lasso hit-testing in the current
    /// layout. Filters out block-internal source hatches (stored in
    /// `self.hatches` at block-local coords for the block-defn position,
    /// which doesn't project correctly through the offset-rel view_proj
    /// and was causing the wrong hatch to be selected on click).
    pub fn visible_hatches_for_click(
        &self,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> HashMap<Handle, HatchModel> {
        let visible = |h: Handle, m: &HatchModel| {
            self.hatch_visible_for_interaction(h)
                .then(|| (h, m.clone()))
        };
        if let Some(handles) = candidate_handles {
            handles
                .iter()
                .filter_map(|&handle| {
                    self.hatches
                        .get(&handle)
                        .and_then(|hatch| visible(handle, hatch))
                })
                .collect()
        } else {
            self.hatches
                .iter()
                .filter_map(|(&handle, hatch)| visible(handle, hatch))
                .collect()
        }
    }

    fn hatch_visible_for_interaction(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        // Use the SOLID's WCS-aware wire fill for interaction as well as
        // drawing. Its cached HatchModel is flattened to XY for plotting and
        // must not leave an invisible selection target at z=0 (#617).
        if matches!(entity, EntityType::Solid(_)) {
            return false;
        }
        let common = entity.common();
        if common.invisible
            || self.entity_temporarily_hidden(handle)
            || self.layer_hidden(&common.layer)
            || self.interaction_layer_frozen(&common.layer)
            || crate::scene::annotative::annotative_offscale_for(
                &self.document,
                common,
                self.displayed_annotation_scale_handle(),
                self.annotation_all_visible(),
            )
        {
            return false;
        }
        self.belongs_to_visible_block(handle, common.owner_handle, self.interaction_block_handle())
    }

    /// Per-Insert hatch models in the current layout, keyed by the Insert
    /// handle so a click on a block-internal hatch can select the parent
    /// Insert; block-owned leaves resolve to their owning instance.

    pub fn insert_hatches_for_click(&self) -> Arc<HashMap<Handle, Vec<HatchModel>>> {
        let interaction_block = self.interaction_block_handle();
        let space_key = self.interaction_space_key();
        {
            let c = self.insert_hatch_cache.borrow();
            if let Some((epoch, cached_space, ref arc)) = *c {
                if cached_space == space_key
                    && self.category_cache_valid(
                        epoch,
                        CACHE_CATEGORY_INSERT_HATCH,
                        |handle| {
                            matches!(
                                self.document.get_entity(handle),
                                Some(EntityType::Insert(_))
                            )
                        },
                    )
                {
                    let arc = Arc::clone(arc);
                    drop(c);
                    if let Some((cached_epoch, _, _)) =
                        self.insert_hatch_cache.borrow_mut().as_mut()
                    {
                        *cached_epoch = self.geometry_epoch;
                    }
                    return arc;
                }
            }
        }
        let layer_hidden =
            |layer: &str| self.layer_hidden(layer) || self.interaction_layer_frozen(layer);
        let mut out: HashMap<Handle, Vec<HatchModel>> = HashMap::default();
        // Skip block subtrees that contain no hatch at all. The presence test
        // is memoised across instances.
        let mut hatch_memo: std::collections::HashMap<String, bool> =
            std::collections::HashMap::new();
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        let depth_map = self.draw_depth_map();
        let frozen: HashSet<Handle> = self
            .interaction_viewport_frozen_layers()
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            (!frozen.is_empty()).then_some(&frozen),
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        );
        for entity in self.document.entities() {
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                entity,
                annotation_scale_handle,
            );
            let EntityType::Insert(ins) = contextual.as_ref() else {
                continue;
            };
            if ins.common.invisible
                || self.entity_temporarily_hidden(ins.common.handle)
                || layer_hidden(&ins.common.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    &ins.common,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                continue;
            }
            if !self.belongs_to_visible_block(
                ins.common.handle,
                ins.common.owner_handle,
                interaction_block,
            ) {
                continue;
            }
            if !render_graph::block_contains_hatch(
                &self.document,
                &ins.block_name,
                &mut hatch_memo,
            ) {
                continue;
            }
            graph.walk_insert(
                ins,
                ins.common.handle,
                |_, _| true,
                |entity, context| {
                    let EntityType::Hatch(source) = entity else {
                        return;
                    };
                    let mut placed = EntityType::Hatch(source.clone());
                    placed.apply_transform(&context.transform);
                    let EntityType::Hatch(hatch) = placed else {
                        return;
                    };
                    let color = crate::scene::view::render::adapt_to_bg(
                        context.style_for(&self.document, entity).0,
                        self.current_bg(),
                    );
                    let Some(mut model) = Self::hatch_model_from_dxf(&hatch, color)
                    else {
                        return;
                    };
                    for clip in &context.clips {
                        let clip: Vec<[f32; 2]> = clip
                            .iter()
                            .map(|point| [point[0] as f32, point[1] as f32])
                            .collect();
                        let clipped = pick::xclip::clip_hatch_boundary(
                            &model.boundary,
                            model.world_origin,
                            &clip,
                        );
                        if clipped.is_empty() {
                            return;
                        }
                        model.boundary = Arc::new(clipped);
                    }
                    out.entry(ins.common.handle).or_default().push(model);
                },
            );
        }
        let arc = Arc::new(out);
        *self.insert_hatch_cache.borrow_mut() =
            Some((self.geometry_epoch, space_key, Arc::clone(&arc)));
        arc
    }

    /// Wires that should participate in hit-testing, snapping, and selection.
    ///
    /// - Model layout: all entity wires (same as entity_wires).
    /// - PSPACE (paper layout, no active viewport): paper-space entities only —
    ///   viewport content is NOT interactive.
    /// - MSPACE (active viewport set): model-space content of the active viewport
    ///   only — paper-space entities are NOT interactive.
    pub fn hit_test_wires(&self) -> Arc<Vec<WireModel>> {
        if self.current_layout == "Model" {
            // Model uses the resident, camera-independent full wire set, so the
            // interaction index survives pan/zoom and reaches every entity.
            return self.entity_wires_arc();
        }
        match self.active_viewport {
            None => self.paper_sheet_wires_arc(),
            // MSPACE editing already uses the viewport's model camera, so its
            // interaction source must stay in model coordinates too. The old
            // projected-Paper copy rebuilt a large Vec/Arc and interaction
            // index on entry even though the resident model set already exists.
            Some(vp_handle) => self.model_wires_for_viewport_arc(vp_handle, 0.0),
        }
    }

    /// Small simple sets are cheaper to scan. Either many wires or substantial
    /// sub-geometry (one giant polyline/text batch/mesh) enables the index.
    const INTERACTION_INDEX_MIN_WIRES: usize = 4_000;
    const INTERACTION_INDEX_MIN_WORK: usize = 20_000;

    fn interaction_index_worthwhile(&self, wires: &[WireModel]) -> bool {
        wires.len() >= Self::INTERACTION_INDEX_MIN_WIRES
            || crate::scene::pick::interaction_index::InteractionIndex::estimated_work(wires)
                >= Self::INTERACTION_INDEX_MIN_WORK
            || self.hatches.len() >= Self::INTERACTION_INDEX_MIN_WIRES
            || self.meshes.len().saturating_add(self.block_meshes.len())
                >= Self::INTERACTION_INDEX_MIN_WIRES
            || self
                .hatches
                .values()
                .any(|hatch| hatch.boundary.len() >= Self::INTERACTION_INDEX_MIN_WORK)
            || self
                .meshes
                .values()
                .chain(self.block_meshes.values())
                .any(|set| {
                    set.lods
                        .first()
                        .is_some_and(|mesh| mesh.verts.len() >= Self::INTERACTION_INDEX_MIN_WORK)
                })
    }

    fn cached_interaction_index(
        &self,
        wires: &Arc<Vec<WireModel>>,
    ) -> Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>> {
        let source = Arc::as_ptr(wires) as usize;
        let mut cache = self.interaction_index_cache.borrow_mut();
        cache.retain(|(epoch, _, weak, _)| {
            *epoch == self.geometry_epoch && weak.strong_count() > 0
        });
        if let Some(position) = cache.iter().position(|(_, ptr, weak, _)| {
            *ptr == source
                && weak
                    .upgrade()
                    .is_some_and(|cached| Arc::ptr_eq(&cached, wires))
        }) {
            let entry = cache.remove(position);
            let index = Arc::clone(&entry.3);
            cache.push(entry);
            if self.current_layout == "Model" {
                *self.interaction_base_index_cache.borrow_mut() = Some((
                    self.geometry_epoch,
                    self.interaction_space_key(),
                    Arc::clone(&index),
                ));
            }
            return Some(index);
        }
        None
    }

    fn cache_interaction_index(
        &self,
        epoch: u64,
        wires: Arc<Vec<WireModel>>,
        index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) {
        const MAX_CACHED_SOURCES: usize = 4;
        let source = Arc::as_ptr(&wires) as usize;
        let mut cache = self.interaction_index_cache.borrow_mut();
        cache.retain(|(cached_epoch, ptr, weak, _)| {
            *cached_epoch == self.geometry_epoch
                && weak.strong_count() > 0
                && !(*cached_epoch == epoch && *ptr == source)
        });
        cache.push((
            epoch,
            source,
            Arc::downgrade(&wires),
            Arc::clone(&index),
        ));
        if cache.len() > MAX_CACHED_SOURCES {
            cache.remove(0);
        }
        if epoch == self.geometry_epoch && self.current_layout == "Model" {
            *self.interaction_base_index_cache.borrow_mut() = Some((
                epoch,
                self.interaction_space_key(),
                index,
            ));
        }
    }

    /// Return a stable build key when this source needs its large
    /// interaction index. The app uses it to deduplicate background builds.
    pub fn interaction_index_build_key(
        &self,
        wires: &Arc<Vec<WireModel>>,
        screen_height_px: f32,
    ) -> Option<(u64, usize)> {
        let key = (self.geometry_epoch, Arc::as_ptr(wires) as usize);
        if !self.interaction_source_is_resident(wires, screen_height_px.max(1.0))
            || !self.interaction_index_worthwhile(wires)
            || self.interaction_overlay_base().is_some()
        {
            return None;
        }
        if self.cached_interaction_index(wires).is_some() {
            if self.interaction_index_pending_key.get() == Some(key) {
                self.interaction_index_pending_key.set(None);
            }
            return None;
        }
        Some(key)
    }

    pub fn mark_interaction_index_pending(&self, epoch: u64, source: usize) {
        self.interaction_index_pending_key
            .set(Some((epoch, source)));
    }

    /// Install a fully prepared background index only if its geometry/source
    /// still belongs to this scene.
    pub fn install_prepared_interaction_index(
        &self,
        epoch: u64,
        source: usize,
        wires: std::sync::Weak<Vec<WireModel>>,
        index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) -> bool {
        if self.interaction_index_pending_key.get() == Some((epoch, source)) {
            self.interaction_index_pending_key.set(None);
        }
        let Some(wires) = wires.upgrade() else {
            return false;
        };
        if epoch != self.geometry_epoch || Arc::as_ptr(&wires) as usize != source {
            return false;
        }
        self.cache_interaction_index(epoch, wires, index);
        true
    }

    const INTERACTION_OVERLAY_MAX_HANDLES: usize = 2_048;

    fn interaction_overlay_base(
        &self,
    ) -> Option<(
        u64,
        Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        Vec<(Handle, ChangeKind)>,
    )> {
        let space_key = self.interaction_space_key();
        let (epoch, index) = {
            let cache = self.interaction_base_index_cache.borrow();
            let (epoch, cached_space, index) = cache.as_ref()?;
            if *epoch == self.geometry_epoch || *cached_space != space_key {
                return None;
            }
            (*epoch, Arc::clone(index))
        };
        let changes = self.replay_since(epoch)?;
        (changes.len() <= Self::INTERACTION_OVERLAY_MAX_HANDLES)
            .then_some((epoch, index, changes))
    }

    fn interaction_overlay_changed_index(
        &self,
        base_epoch: u64,
        changes: &[(Handle, ChangeKind)],
    ) -> (
        Arc<Vec<WireModel>>,
        Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) {
        let space_key = self.interaction_space_key();
        {
            let cache = self.interaction_overlay_index_cache.borrow();
            if let Some((epoch, cached_base, cached_space, wires, index)) = cache.as_ref() {
                if *epoch == self.geometry_epoch
                    && *cached_base == base_epoch
                    && *cached_space == space_key
                {
                    return (Arc::clone(wires), Arc::clone(index));
                }
            }
        }
        let changed_live: Vec<Handle> = changes
            .iter()
            .filter_map(|(handle, kind)| {
                (!matches!(kind, ChangeKind::Removed)).then_some(*handle)
            })
            .collect();
        let wires = Arc::new(self.wire_models_for(&changed_live));
        let index = Arc::new(
            crate::scene::pick::interaction_index::InteractionIndex::build(&wires),
        );
        *self.interaction_overlay_index_cache.borrow_mut() = Some((
            self.geometry_epoch,
            base_epoch,
            space_key,
            Arc::clone(&wires),
            Arc::clone(&index),
        ));
        (wires, index)
    }

    fn interaction_overlay_wires(
        &self,
        base_keys: impl IntoIterator<Item = (u64, u32)>,
        changes: &[(Handle, ChangeKind)],
        changed_wires: &[WireModel],
    ) -> (
        Arc<Vec<WireModel>>,
        HashMap<(u64, u32), u32>,
        HashMap<(u64, u32), u32>,
    ) {
        let changed: HashSet<Handle> = changes.iter().map(|(handle, _)| *handle).collect();
        let memo = self.resident_tess_memo.borrow();
        let mut wires = Vec::new();
        let mut base_slots = HashMap::default();
        let mut misses: HashMap<Handle, Vec<u32>> = HashMap::default();
        for (handle_value, ordinal) in base_keys {
            let handle = Handle::new(handle_value);
            if changed.contains(&handle) || self.document.get_entity(handle).is_none() {
                continue;
            }
            if let Some(wire) = memo
                .get(&handle)
                .and_then(|entity_wires| entity_wires.get(ordinal as usize))
            {
                base_slots.insert((handle_value, ordinal), wires.len() as u32);
                wires.push(wire.clone());
            } else {
                misses.entry(handle).or_default().push(ordinal);
            }
        }
        drop(memo);
        // A memo miss is uncommon (guard change / legacy source). Preserve
        // correctness without expanding every nearby entity: regenerate that
        // handle, then retain only the exact wire ordinals the base index
        // reported.
        for (handle, ordinals) in misses {
            let entity_wires = self.wire_models_for(&[handle]);
            for ordinal in ordinals {
                if let Some(wire) = entity_wires.get(ordinal as usize) {
                    base_slots.insert((handle.value(), ordinal), wires.len() as u32);
                    wires.push(wire.clone());
                }
            }
        }
        let mut changed_slots = HashMap::default();
        let mut next_ordinal: HashMap<u64, u32> = HashMap::default();
        for wire in changed_wires {
            if let Ok(handle) = wire.name.parse::<u64>() {
                let ordinal = next_ordinal.entry(handle).or_default();
                changed_slots.insert((handle, *ordinal), wires.len() as u32);
                *ordinal += 1;
            }
            wires.push(wire.clone());
        }
        (Arc::new(wires), base_slots, changed_slots)
    }

    fn indexed_interaction_candidates_xy(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        allow_pending_empty: bool,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let keys = base.query_wire_keys_xy(aabb);
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            let mut result =
                base.query_remapped_xy(Arc::clone(&local), &base_slots, aabb);
            result.extend_indexed(
                changed_index.query_remapped_xy(local, &changed_slots, aabb),
            );
            let remap_ms = t_remap.elapsed().as_secs_f64() * 1000.0;
            let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if perf && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] interaction-overlay {:>7.1}ms keys={} wires={} query={:.1} changed={:.1} gather={:.1} remap={:.1}",
                    total_ms,
                    keys.len(),
                    result.len(),
                    key_ms,
                    changed_ms,
                    local_ms,
                    remap_ms,
                );
            }
            return result;
        }
        if let Some(index) = self.cached_interaction_index(&wires) {
            index.query_xy(wires, aabb)
        } else if allow_pending_empty
            && self.interaction_index_pending_key.get()
            == Some((self.geometry_epoch, Arc::as_ptr(&wires) as usize))
        {
            crate::scene::pick::interaction_index::InteractionCandidates::pending(wires)
        } else {
            crate::scene::pick::interaction_index::InteractionCandidates::all(wires)
        }
    }

    fn indexed_interaction_candidates_screen(
        &self,
        wires: Arc<Vec<WireModel>>,
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        allow_pending_empty: bool,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let keys = base.query_wire_keys_screen(screen_rect, view_rot, eye, bounds);
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            let mut result = base.query_remapped_screen(
                Arc::clone(&local),
                &base_slots,
                screen_rect,
                view_rot,
                eye,
                bounds,
            );
            result.extend_indexed(changed_index.query_remapped_screen(
                local,
                &changed_slots,
                screen_rect,
                view_rot,
                eye,
                bounds,
            ));
            let remap_ms = t_remap.elapsed().as_secs_f64() * 1000.0;
            let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if perf && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] interaction-overlay {:>7.1}ms keys={} wires={} query={:.1} changed={:.1} gather={:.1} remap={:.1}",
                    total_ms,
                    keys.len(),
                    result.len(),
                    key_ms,
                    changed_ms,
                    local_ms,
                    remap_ms,
                );
            }
            return result;
        }
        if let Some(index) = self.cached_interaction_index(&wires) {
            index.query_screen(wires, screen_rect, view_rot, eye, bounds)
        } else if allow_pending_empty
            && self.interaction_index_pending_key.get()
            == Some((self.geometry_epoch, Arc::as_ptr(&wires) as usize))
        {
            crate::scene::pick::interaction_index::InteractionCandidates::pending(wires)
        } else {
            crate::scene::pick::interaction_index::InteractionCandidates::all(wires)
        }
    }

    fn indexed_interaction_pick_radius(
        &self,
        wires: &Arc<Vec<WireModel>>,
        base_radius_px: f32,
    ) -> f32 {
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            let (_, changed) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            base.pick_radius_px(changed.pick_radius_px(base_radius_px))
        } else {
            self.cached_interaction_index(wires)
                .map_or(base_radius_px, |index| index.pick_radius_px(base_radius_px))
        }
    }

    fn interaction_source_is_resident(
        &self,
        wires: &Arc<Vec<WireModel>>,
        screen_height_px: f32,
    ) -> bool {
        if self.current_layout == "Model" {
            return Arc::ptr_eq(&self.entity_wires_arc(), wires);
        }
        match self.active_viewport {
            Some(viewport) => {
                let resident = self.model_wires_for_viewport_arc(viewport, screen_height_px);
                Arc::ptr_eq(&resident, wires)
            }
            None => Arc::ptr_eq(&self.paper_sheet_wires_arc(), wires),
        }
    }

    fn interaction_handle_index(
        &self,
    ) -> Arc<crate::scene::pick::interaction_index::InteractionHandleIndex> {
        let space_key = self.interaction_space_key();
        {
            let reuse = {
                let cache = self.interaction_handle_index_cache.borrow();
                match cache.as_ref() {
                    Some((epoch, cached_space, index))
                        if *cached_space == space_key
                            && self.category_cache_valid(
                                *epoch,
                                CACHE_CATEGORY_INTERACTION,
                                |handle| {
                                self.hatches.contains_key(&handle)
                                    || self.meshes.contains_key(&handle)
                                    || self.block_meshes.contains_key(&handle)
                                    || matches!(
                                        self.document.get_entity(handle),
                                        Some(EntityType::Insert(_))
                                    )
                                },
                            ) =>
                    {
                        Some(Arc::clone(index))
                    }
                    _ => None,
                }
            };
            if let Some(index) = reuse {
                if let Some((epoch, _, _)) =
                    self.interaction_handle_index_cache.borrow_mut().as_mut()
                {
                    *epoch = self.geometry_epoch;
                }
                return index;
            }
        }
        let mut entries: Vec<(u64, [f64; 6])> = Vec::new();
        for (&handle, hatch) in &self.hatches {
            if !self.hatch_visible_for_interaction(handle) {
                continue;
            }
            if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
            }
        }
        for (&handle, hatches) in self.insert_hatches_for_click().iter() {
            for hatch in hatches {
                if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                    entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
                }
            }
        }
        for set in self.interaction_meshes_arc().iter() {
            let Some(mesh) = set.lods.first() else {
                continue;
            };
            let Ok(handle) = mesh.name.parse::<u64>() else {
                continue;
            };
            if !self.mesh_visible_for_interaction(Handle::new(handle)) {
                continue;
            }
            if let Some(aabb) = mesh_interaction_aabb(set) {
                entries.push((handle, aabb));
            }
        }
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionHandleIndex::build(entries));
        *self.interaction_handle_index_cache.borrow_mut() =
            Some((self.geometry_epoch, space_key, Arc::clone(&index)));
        index
    }

    /// Cursor-local interaction candidates. `radius_px` is supplied by the
    /// consumer (OSNAP aperture or click tolerance), avoiding the old fixed
    /// 64-pixel neighbourhood. Returned wires stay borrowed from `wires`;
    /// no heavyweight `WireModel` clone occurs.
    pub fn interaction_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires, cursor, view_rot, eye, bounds, radius_px, false, false,
        )
    }

    pub fn interaction_pick_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            self.document.header.lineweight_display,
            false,
        )
    }

    pub fn interaction_hover_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            self.document.header.lineweight_display,
            true,
        )
    }

    fn interaction_candidates_near_impl(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
        include_line_weight: bool,
        allow_pending_empty: bool,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        if !self.interaction_source_is_resident(&wires, bounds.height)
            || !self.interaction_index_worthwhile(&wires)
        {
            return crate::scene::pick::interaction_index::InteractionCandidates::all(wires);
        }
        let radius_px = if include_line_weight {
            self.indexed_interaction_pick_radius(&wires, radius_px)
        } else {
            radius_px
        };
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        if !flat_ortho {
            let clip = view_rot * (cursor - eye).as_vec3().extend(1.0);
            if !clip.is_finite() || clip.w <= 1e-6 {
                return crate::scene::pick::interaction_index::InteractionCandidates::all(wires);
            }
            let ndc = clip.truncate() / clip.w;
            let screen = iced::Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            );
            let radius = radius_px.max(0.0);
            return self.indexed_interaction_candidates_screen(
                wires,
                [
                    screen.x - radius,
                    screen.y - radius,
                    screen.x + radius,
                    screen.y + radius,
                ],
                view_rot,
                eye,
                bounds,
                allow_pending_empty,
            );
        }
        let world_x_px = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
            + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
        .sqrt();
        let world_y_px = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
            + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
        .sqrt();
        let s = world_x_px.min(world_y_px);
        if s <= 1e-6 {
            return crate::scene::pick::interaction_index::InteractionCandidates::all(wires);
        }
        let radius = radius_px.max(0.0) as f64 / s as f64;
        let query = [
            cursor.x - radius,
            cursor.y - radius,
            cursor.x + radius,
            cursor.y + radius,
        ];
        self.indexed_interaction_candidates_xy(wires, query, allow_pending_empty)
    }

    /// Shared rectangular broad phase for box/lasso/fence and command windows.
    /// Flat orthographic views query world XY; tilted/perspective views query
    /// projected 3D bounds using the supplied screen rectangle.
    pub fn interaction_candidates_in_aabb(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        if !self.interaction_source_is_resident(&wires, bounds.height)
            || !self.interaction_index_worthwhile(&wires)
        {
            return crate::scene::pick::interaction_index::InteractionCandidates::all(wires);
        }
        if flat_ortho {
            self.indexed_interaction_candidates_xy(wires, aabb, false)
        } else {
            self.indexed_interaction_candidates_screen(
                wires,
                screen_rect,
                view_rot,
                eye,
                bounds,
                false,
            )
        }
    }

    pub fn interaction_candidate_handles(
        &self,
        candidates: &crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> Option<HashSet<Handle>> {
        let mut handles: HashSet<Handle> = candidates
            .iter()
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .collect();
        if candidates.is_indexed()
            && candidates.query_aabb().is_none()
            && candidates.screen_query().is_none()
        {
            return Some(handles);
        }
        if let Some(aabb) = candidates.query_aabb() {
            let index = self.entity_index();
            handles.extend(index.tree.query_rect(aabb));
            handles.extend(
                self.interaction_handle_index()
                    .query_xy(aabb)
                    .into_iter()
                    .map(Handle::new),
            );
            return Some(handles);
        }
        let (screen_rect, view_rot, eye, bounds) = candidates.screen_query()?;
        handles.extend(
            self.interaction_handle_index()
                .query_screen(screen_rect, view_rot, eye, bounds)
                .into_iter()
                .map(Handle::new),
        );
        Some(handles)
    }

    pub fn interaction_handles_in_world_aabb(&self, aabb: [f64; 4]) -> HashSet<Handle> {
        let wires = self.hit_test_wires();
        let candidates = self.indexed_interaction_candidates_xy(wires, aabb, false);
        let mut handles: HashSet<Handle> = candidates
            .iter()
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .collect();
        let insert_hatches = self.insert_hatches_for_click();
        let meshes = self.interaction_meshes_arc();
        let mesh_lookup = self.mesh_pick_lookup(&meshes);
        handles.extend(
            self.interaction_handle_index()
                .query_xy(aabb)
                .into_iter()
                .filter_map(|value| {
                    let handle = Handle::new(value);
                    (self.hatch_visible_for_interaction(handle)
                        || insert_hatches.contains_key(&handle)
                        || (mesh_lookup.contains_key(&handle)
                            && self.mesh_visible_for_interaction(handle)))
                    .then_some(handle)
                }),
        );
        handles
    }

    /// True when any handle resolves to an ACIS volume entity (3D solid /
    /// region / body / surface) — i.e. one whose render geometry is a cached
    /// mesh that must be re-tessellated after an edit.
    pub fn any_solid(&self, handles: &[Handle]) -> bool {
        handles.iter().any(|&h| {
            matches!(
                self.document.get_entity(h),
                Some(EntityType::Solid3D(_))
                    | Some(EntityType::Region(_))
                    | Some(EntityType::Body(_))
                    | Some(EntityType::Surface(_))
                    | Some(EntityType::Mesh(_))
                    | Some(EntityType::PolygonMesh(_))
                    | Some(EntityType::PolyfaceMesh(_))
            )
        })
    }

    /// Top-level solid handles caught by a rectangular selection box.
    pub fn mesh_box_hit(
        &self,
        a: iced::Point,
        b: iced::Point,
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let handles: Vec<Handle> = candidate_handles.map_or_else(
            || lookup.keys().copied().collect(),
            |candidates| candidates.iter().copied().collect(),
        );
        let mut out = Vec::new();
        for handle in handles {
            if !self.mesh_visible_for_interaction(handle)
                || matches!(
                    self.document.get_entity(handle),
                    Some(EntityType::Insert(_))
                )
            {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            if indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize))
                .any(|set| {
                    set.lods.first().is_some_and(|mesh| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, mesh)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            {
                out.push(handle);
            }
        }
        out
    }

    /// Top-level solid handles caught by a lasso polygon.
    pub fn mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let handles: Vec<Handle> = candidate_handles.map_or_else(
            || lookup.keys().copied().collect(),
            |candidates| candidates.iter().copied().collect(),
        );
        let mut out = Vec::new();
        for handle in handles {
            if !self.mesh_visible_for_interaction(handle)
                || matches!(
                    self.document.get_entity(handle),
                    Some(EntityType::Insert(_))
                )
            {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            if indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize))
                .any(|set| {
                    set.lods.first().is_some_and(|mesh| {
                        !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, mesh)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            {
                out.push(handle);
            }
        }
        out
    }

    /// Front-most solid under the cursor across BOTH top-level solid meshes
    /// (keyed by their own handle) and block-internal solid instances (keyed
    /// by the parent INSERT). Combining them in one depth-sorted test means a
    /// block in front of a stray solid wins, instead of the solid always
    /// taking priority by virtue of being tried first.
    pub fn solid_click_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Option<Handle> {
        self.solid_hit(
            cursor,
            view_rot,
            eye,
            bounds,
            candidate_handles,
            false,
        )
    }

    /// Hover may use the coarsest cached solid LOD. Click selection keeps the
    /// full-resolution mesh, while rollover only needs a stable parent handle
    /// and must stay within an interactive frame budget.
    pub fn solid_hover_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Option<Handle> {
        self.solid_hit(
            cursor,
            view_rot,
            eye,
            bounds,
            candidate_handles,
            true,
        )
    }

    fn solid_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
        coarse_lod: bool,
    ) -> Option<Handle> {
        // Reuse the renderer's expanded mesh set (top-level solids + per-INSERT
        // block instances), cached per geometry epoch — so hover no longer
        // re-expands every block instance on each move. Every `MeshLodSet`
        // carries its handle (in `mesh.name`) and a 3D AABB.
        let meshes = self.interaction_meshes_arc();
        let lookup = candidate_handles.map(|_| self.mesh_pick_lookup(&meshes));
        // Candidate handles already came from the precise f64 interaction BVH.
        // When no index is active the source is small/non-resident, so exact
        // triangles are safer than reintroducing the old f32 AABB precision loss.
        let mut sets: Vec<(Handle, &MeshLodSet)> = Vec::new();
        if let (Some(handles), Some(lookup)) = (candidate_handles, lookup.as_ref()) {
            for handle in handles {
                if !self.mesh_visible_for_interaction(*handle) {
                    continue;
                }
                let Some(indices) = lookup.get(handle) else {
                    continue;
                };
                sets.extend(
                    indices
                        .iter()
                        .filter_map(|&index| meshes.get(index as usize).map(|set| (*handle, set))),
                );
            }
        } else {
            sets.extend(meshes.iter().filter_map(|set| {
                let handle = set
                    .lods
                    .first()?
                    .name
                    .parse::<u64>()
                    .ok()
                    .map(Handle::new)?;
                self.mesh_visible_for_interaction(handle)
                    .then_some((handle, set))
            }));
        }
        if coarse_lod {
            return pick::hit_test::mesh_click_hit(
                cursor,
                sets.iter()
                    .filter_map(|(handle, set)| set.lods.last().map(|mesh| (*handle, mesh))),
                view_rot,
                eye,
                bounds,
            );
        }
        pick::hit_test::mesh_click_hit(
            cursor,
            sets.iter()
                .filter_map(|(handle, set)| set.lods.first().map(|mesh| (*handle, mesh))),
            view_rot,
            eye,
            bounds,
        )
    }

    /// Parent INSERT handles whose block-internal solid meshes fall in a
    /// rectangular selection box. A block whose visible body is a solid has
    /// no wires to catch, so box/lasso selection must test its instanced
    /// meshes too.
    pub fn block_mesh_box_hit(
        &self,
        a: iced::Point,
        b: iced::Point,
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let handles: Vec<Handle> = candidate_handles.map_or_else(
            || lookup.keys().copied().collect(),
            |candidates| candidates.iter().copied().collect(),
        );
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) || !self.mesh_visible_for_interaction(handle)
            {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let hit = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize))
                .any(|set| {
                    set.lods.first().map_or(false, |m| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, m)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                });
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Parent INSERT handles whose block-internal solid meshes fall in a lasso.
    pub fn block_mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let handles: Vec<Handle> = candidate_handles.map_or_else(
            || lookup.keys().copied().collect(),
            |candidates| candidates.iter().copied().collect(),
        );
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) || !self.mesh_visible_for_interaction(handle)
            {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let hit = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize))
                .any(|set| {
                    set.lods.first().map_or(false, |m| {
                        !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, m)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                });
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Whether entity `e` renders as direct content of `block_handle` right now:
    /// visible (not invisible / hidden / on an off-or-frozen layer / frozen
    /// through the viewport) and owned by the block. Shared by the full resident
    /// build and the incremental patch so a changed entity is classified
    /// identically either way.
    fn resident_entity_visible(
        &self,
        e: &EntityType,
        block_handle: Handle,
        frozen_layers: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
    ) -> bool {
        let c = e.common();
        if c.invisible {
            return false;
        }
        // Session-only Isolate / Hide and interactive replacement previews.
        if self.entity_temporarily_hidden(c.handle) {
            return false;
        }
        // Block/BlockEnd are block-defn sentinels, not drawable geometry.
        if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            return false;
        }
        let layer = self.document.layers.get(&c.layer);
        if layer
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
        {
            return false;
        }
        if let Some(frozen) = frozen_layers {
            if !frozen.is_empty() {
                if let Some(lh) = layer.map(|l| l.handle) {
                    if frozen.contains(&lh) {
                        return false;
                    }
                }
            }
        }
        if crate::scene::annotative::annotative_offscale_for(
            &self.document,
            c,
            annotation_scale_handle,
            all_visible,
        ) {
            return false;
        }
        self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
    }

    fn wires_for_block_culled(
        &self,
        block_handle: Handle,
        view_aabb: Option<[f32; 4]>,
        wpp: Option<f32>,
        // Layers frozen specifically through the requesting viewport.
        // Hidden in addition to the document-level off / frozen flags.
        // `None` skips the per-viewport check (Model-space callers).
        frozen_layers: Option<&HashSet<Handle>>,
        // Paper-space content viewports compute their own annotation
        // scale from `vp_effective_scale`; the Model-space and paper-
        // sheet paths use `self.annotation_scale` / 1.0 respectively.
        // `None` selects the default branch on `current_layout`.
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        style_viewport: Option<Handle>,
    ) -> Vec<WireModel> {
        use acadrust::objects::ObjectType;

        // ── Ensure sort-order index is current ────────────────────────────
        // Replaces the old O(objects) find_map with one rebuild per epoch,
        // after which every wires_for_block call is an O(1) HashMap lookup.
        {
            let needs_rebuild = self
                .sort_cache
                .borrow()
                .as_ref()
                .map(|(e, _)| *e != self.geometry_epoch)
                .unwrap_or(true);

            if needs_rebuild {
                let mut idx: HashMap<Handle, HashMap<u64, u64>> = HashMap::default();
                for obj in self.document.objects.values() {
                    if let ObjectType::SortEntitiesTable(t) = obj {
                        if !t.is_empty() {
                            let map = t
                                .entries()
                                .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                                .collect();
                            idx.insert(t.block_owner_handle, map);
                        }
                    }
                }
                *self.sort_cache.borrow_mut() = Some((self.geometry_epoch, idx));
            }
        }

        // Visibility test reused by both paths below — and by the resident
        // incremental patch, so a changed entity is included/excluded exactly as
        // a from-scratch build would (no divergence).
        let visibility_ok = |e: &EntityType| {
            self.resident_entity_visible(
                e,
                block_handle,
                frozen_layers,
                annotation_scale_handle,
                all_visible,
            )
        };

        // Phase 2.1 — quadtree-driven candidate selection. When a view
        // AABB exists (Model layout with a settled camera), only iterate
        // entities whose stored WCS bbox intersects the view; unindexable
        // entities (Insert/Viewport) are appended via a small linear scan.
        // Paper space and the first-frame "settle" path fall back to the
        // full doc scan — preserving prior behaviour.
        let visible: Vec<&EntityType> = if let Some(local_view) = view_aabb {
            let view_wcs: [f64; 4] = [
                local_view[0] as f64,
                local_view[1] as f64,
                local_view[2] as f64,
                local_view[3] as f64,
            ];
            let (candidates, unbounded): (Vec<Handle>, Vec<Handle>) = {
                let idx = self.entity_index();
                (idx.tree.query_rect(view_wcs), idx.unbounded_handles.clone())
            };
            let mut out: Vec<&EntityType> =
                Vec::with_capacity(candidates.len() + unbounded.len() + 16);
            for h in candidates {
                if let Some(e) = self.document.get_entity(h) {
                    if visibility_ok(e) {
                        out.push(e);
                    }
                }
            }
            // Unbounded entities — always emit regardless of view, mirroring
            // legacy `entity_aabb`'s UNBOUNDED_AABB sentinel.
            for h in unbounded {
                if let Some(e) = self.document.get_entity(h) {
                    if visibility_ok(e) {
                        out.push(e);
                    }
                }
            }
            // Inserts/Viewports/Block/BlockEnd — handled by their own paths
            // (block expansion, viewport rendering); always candidates.
            for e in self.document.entities() {
                if is_unindexable_entity(e) && visibility_ok(e) {
                    out.push(e);
                }
            }
            out
        } else {
            self.document
                .entities()
                .filter(|e| visibility_ok(e))
                .collect()
        };

        // Tessellate in parallel across all available CPU cores.
        use crate::par::prelude::*;
        let doc = &self.document;
        // Selection / hover highlight is NOT baked into tessellation. It is
        // applied per frame in the GPU xray overlay pass from the live
        // selection set (`Scene::selected` ∪ hover). Keeping `sel` empty here
        // makes the wire cache selection-independent, so picking an entity
        // bumps only `selection_generation` (cheap overlay refresh) instead of
        // `geometry_epoch` (a full model re-tessellation).
        let empty_sel: HashSet<Handle> = HashSet::default();
        let sel: &HashSet<Handle> = &empty_sel;
        let avp = style_viewport.or(self.active_viewport);
        // A paper-space content viewport renders MODEL block entities while
        // the user is sitting in a paper layout — that path expects
        // `world_offset` subtraction even though `current_layout != "Model"`.
        // Decide based on the block being tessellated, not the layout.
        let is_model_block = block_handle == self.model_space_block_handle();
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if let Some(a) = anno_scale_override {
            a
        } else if self.current_layout == "Model" {
            self.annotation_scale
        } else {
            1.0
        };
        // Content shown inside a paper-space viewport carries a scale override;
        // only there does PSLTSCALE resize linetypes by the viewport scale.
        let paper = anno_scale_override.is_some();
        let blk_cache = self.block_cache_arc_for(annotation_scale_handle, all_visible, style_viewport);
        let blk_ref: &cache::block_cache::BlockCache = &blk_cache;
        // Zoom-adaptive curve sampling for top-level Edge tessellation. Target
        // ~0.5 px chord height — far-out arcs that used to emit hundreds of
        // segments now collapse to a handful. The guard clears the override
        // when this scope exits so off-render tessellation (snap previews,
        // hit-test, block_cache rebuild) sees the default.
        struct CurveTolGuard;
        impl Drop for CurveTolGuard {
            fn drop(&mut self) {
                crate::scene::convert::truck_tess::set_curve_tol_override(None);
            }
        }
        let _tol_guard = wpp.map(|w| {
            crate::scene::convert::truck_tess::set_curve_tol_override(Some((w * 0.5) as f64));
            CurveTolGuard
        });
        // Per-entity tessellation memo. Same classify/tessellate logic, two
        // SEPARATE stores so they can't thrash each other:
        //   * culled path (`view_aabb == Some`) → `tess_memo`, guard keyed on the
        //     per-view cull params (zoom/tol, frustum, entered viewport);
        //   * resident path (`view_aabb == None`, no per-view cull, Model block) →
        //     the camera-INDEPENDENT `resident_tess_memo`, guard keyed only on
        //     anno / bg. This is the set the main GPU render holds
        //     (`model_tile_wires_arc`), so memoizing it makes a single-entity edit
        //     re-tessellate just the changed entity instead of the whole model.
        // Frozen-layer / anno-override viewport paths bypass (their params would
        // thrash a shared memo). Hit-test also passes `view_aabb == None` but is
        // not the Model block, so it never lands on the resident branch.
        // An empty vp-frozen set and an anno override are memo-compatible: the
        // guard hash below mixes anno/bg, so a param change invalidates rather
        // than corrupts. Only a NON-empty frozen set (entity subset changes)
        // bypasses the memo.
        let frozen_empty = frozen_layers.map(|f| f.is_empty()).unwrap_or(true);
        let base_ok = is_model_block && frozen_empty;
        // Kill-switch: `OCS_NO_RESIDENT_MEMO` reverts the resident set to the old
        // full re-tessellation on every edit, in case a mutation site is ever
        // found that edits geometry without dropping its handle from the memo.
        fn resident_memo_enabled() -> bool {
            static EN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            *EN.get_or_init(|| std::env::var("OCS_NO_RESIDENT_MEMO").is_err())
        }
        let resident = base_ok && view_aabb.is_none() && wpp.is_none() && resident_memo_enabled();
        let memo_active = base_ok && (view_aabb.is_some() || resident);
        let mut wires: Vec<WireModel> = if memo_active {
            // Guard hash of everything tessellate_entity output depends on
            // besides the entity itself. A mismatch (zoom/tol, view, anno,
            // offset, bg, entered viewport) means the memo is stale. For the
            // resident path wpp/view_aabb are None, so this collapses to anno/bg.
            let guard = {
                let mut g: u64 = 0xcbf2_9ce4_8422_2325;
                let mut mix = |x: u64| g = g.rotate_left(13) ^ x;
                mix(wpp.map(|w| w.to_bits() as u64).unwrap_or(u64::MAX));
                if let Some(v) = view_aabb {
                    for c in v {
                        mix(c.to_bits() as u64);
                    }
                }
                mix(anno.to_bits() as u64);
                mix(annotation_scale_handle.map(|handle| handle.value()).unwrap_or(0));
                mix(all_visible as u64);
                for c in bg {
                    mix(c.to_bits() as u64);
                }
                mix(avp.map(|h| h.value()).unwrap_or(0));
                // SDF glyph quads bake the atlas UV of each tile, so a growth or
                // a re-bake (which rescale / rewind every UV) makes memoized text
                // address the wrong tile — garbage on screen, and a silent miss in
                // the PDF export's glyph lookup (#385 under #347's conditions).
                mix(crate::scene::text::sdf_atlas::generation());
                g
            };
            let (memo_cell, guard_cell) = if resident {
                (&self.resident_tess_memo, &self.resident_tess_guard)
            } else {
                (&self.tess_memo, &self.tess_memo_guard)
            };
            if guard_cell.get() != guard {
                memo_cell.borrow_mut().clear();
                guard_cell.set(guard);
            }
            // Classify (serial, cheap): reuse memoized Arcs, collect misses.
            let mut hit_arcs: Vec<Arc<Vec<WireModel>>> = Vec::new();
            let mut misses: Vec<&EntityType> = Vec::new();
            {
                let memo = memo_cell.borrow();
                for e in &visible {
                    let h = e.common().handle;
                    match memo.get(&h) {
                        Some(a) => hit_arcs.push(Arc::clone(a)),
                        None => misses.push(*e),
                    }
                }
            }
            // Materialize hits + tessellate misses, both in parallel.
            let hit_wires: Vec<WireModel> = hit_arcs
                .par_iter()
                .flat_map_iter(|a| a.iter().cloned())
                .collect();
            let miss_pairs: Vec<(Handle, Arc<Vec<WireModel>>)> = misses
                .par_iter()
                .map(|e| {
                    let e: &EntityType = e;
                    let w = tessellate_entity(
                        doc,
                        sel,
                        avp,
                        bg,
                        anno,
                        annotation_scale_handle,
                        e,
                        Some(blk_ref),
                        view_aabb,
                        wpp,
                        paper,
                    );
                    (e.common().handle, Arc::new(w))
                })
                .collect();
            let mut out = hit_wires;
            {
                let mut memo = memo_cell.borrow_mut();
                for (h, a) in &miss_pairs {
                    out.extend(a.iter().cloned());
                    memo.insert(*h, Arc::clone(a));
                }
            }
            out
        } else {
            visible
                .into_par_iter()
                .flat_map(|e| {
                    tessellate_entity(
                        doc,
                        sel,
                        avp,
                        bg,
                        anno,
                        annotation_scale_handle,
                        e,
                        Some(blk_ref),
                        view_aabb,
                        wpp,
                        paper,
                    )
                })
                .collect()
        };

        // Apply draw order via the cached index (O(1) block lookup).
        {
            let cache = self.sort_cache.borrow();
            if let Some((_, ref idx)) = *cache {
                if let Some(sort_map) = idx.get(&block_handle) {
                    wires.sort_by_key(|w| {
                        let key = Self::handle_from_wire_name(&w.name)
                            .map(|h| h.value())
                            .unwrap_or(u64::MAX);
                        // Entities absent from the table sort by their own
                        // handle — the same key space the table's sort handles
                        // live in — so reordered and untouched entities interleave
                        // correctly instead of all collapsing to one constant.
                        sort_map.get(&key).copied().unwrap_or(key)
                    });
                }
            }
        }
        wires
    }

    /// Decide whether an entity should be drawn as direct content of `block_handle`.
    fn belongs_to_visible_block(
        &self,
        entity_handle: Handle,
        owner_handle: Handle,
        block_handle: Handle,
    ) -> bool {
        if block_handle.is_null() {
            return true;
        }
        if owner_handle == block_handle {
            return true;
        }
        if !owner_handle.is_null() {
            return false;
        }

        // owner_handle is null (common in DXF files that omit group code 330).
        // Use the current layout's entity_handles as the authoritative list when
        // available — this prevents block-definition geometry from leaking into
        // the viewport even when owner handles are missing.
        if let Some(br) = self
            .document
            .block_records
            .iter()
            .find(|br| br.handle == block_handle)
        {
            if !br.entity_handles.is_empty() {
                return br.entity_handles.contains(&entity_handle);
            }
        }

        // P: epoch-cached reverse map replaces O(B) block_records scan.
        let map = self.entity_block_map();
        if let Some(&owner) = map.get(&entity_handle) {
            return owner == block_handle;
        }
        // Map miss. Permissive only when NO BlockRecord enumerated its
        // entity_handles — that's a legacy DXF that omits 330 group codes
        // everywhere, where dropping unknown-owner entities would empty
        // model space. When at least one block did enumerate, the file is
        // capable of declaring ownership, so an unknown-owner entity is
        // an orphan (typically a block-defn entity whose owner was lost on
        // round-trip) and must not leak into the queried block.
        if map.is_empty() {
            return true;
        }
        false
    }

    /// Build (and epoch-cache) a reverse map: entity_handle → block_record_handle,
    /// covering every entity explicitly listed in a block_record's entity_handles.
    fn entity_block_map(&self) -> std::cell::Ref<'_, HashMap<Handle, Handle>> {
        {
            let cache = self.entity_block_map_cache.borrow();
            if let Some((epoch, _)) = *cache {
                if epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.entity_block_map_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }
        let mut map: HashMap<Handle, Handle> = HashMap::default();
        for br in self.document.block_records.iter() {
            for &eh in &br.entity_handles {
                map.insert(eh, br.handle);
            }
        }
        *self.entity_block_map_cache.borrow_mut() = Some((self.geometry_epoch, map));
        std::cell::Ref::map(self.entity_block_map_cache.borrow(), |c| {
            &c.as_ref().unwrap().1
        })
    }

    fn rebuild_dependency_index(&self) -> SceneDependencyIndex {
        let layout_blocks: HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                acadrust::objects::ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        let block_names: HashMap<Handle, String> = self
            .document
            .block_records
            .iter()
            .map(|record| (record.handle, record.name.to_ascii_uppercase()))
            .collect();
        let membership: HashMap<Handle, Handle> = self
            .document
            .block_records
            .iter()
            .flat_map(|record| {
                record
                    .entity_handles
                    .iter()
                    .copied()
                    .map(move |handle| (handle, record.handle))
            })
            .collect();
        let text_style_names: HashMap<Handle, String> = self
            .document
            .text_styles
            .iter()
            .map(|style| (style.handle, style.name.clone()))
            .collect();

        let mut roots: HashMap<String, HashSet<Handle>> = HashMap::default();
        let mut parents: HashMap<String, HashSet<String>> = HashMap::default();
        for entity in self.document.entities() {
            let EntityType::Insert(insert) = entity else {
                continue;
            };
            let target = insert.block_name.to_ascii_uppercase();
            let common = &insert.common;
            let owner = if common.owner_handle.is_null() {
                membership
                    .get(&common.handle)
                    .copied()
                    .unwrap_or(Handle::NULL)
            } else {
                common.owner_handle
            };
            if owner.is_null() || layout_blocks.contains(&owner) {
                roots.entry(target).or_default().insert(common.handle);
            } else if let Some(parent) = block_names.get(&owner) {
                parents.entry(target).or_default().insert(parent.clone());
            }
        }
        // Propagate top-level INSERT users through nested block references.
        // Fixed-point form is cycle-safe and block graphs are normally shallow.
        let mut changed = true;
        while changed {
            changed = false;
            for (child, parent_names) in &parents {
                let inherited: Vec<Handle> = parent_names
                    .iter()
                    .flat_map(|parent| roots.get(parent).into_iter().flatten().copied())
                    .collect();
                let entry = roots.entry(child.clone()).or_default();
                let before = entry.len();
                entry.extend(inherited);
                changed |= entry.len() != before;
            }
        }

        let mut index = SceneDependencyIndex::default();
        for entity in self.document.entities() {
            let common = entity.common();
            let owner = if common.owner_handle.is_null() {
                membership
                    .get(&common.handle)
                    .copied()
                    .unwrap_or(Handle::NULL)
            } else {
                common.owner_handle
            };
            let inside_block = !owner.is_null() && !layout_blocks.contains(&owner);
            let render_handles: HashSet<Handle> = if inside_block {
                block_names
                    .get(&owner)
                    .and_then(|name| roots.get(name))
                    .cloned()
                    .unwrap_or_default()
            } else {
                std::iter::once(common.handle).collect()
            };
            let extend_category = |target: &mut DependencyTargets| {
                target.render_handles.extend(render_handles.iter().copied());
                target.source_handles.insert(common.handle);
                target.touches_block_definition |= inside_block;
            };
            if matches!(entity, EntityType::Point(_)) {
                extend_category(&mut index.points);
            }
            if matches!(
                entity,
                EntityType::Text(_)
                    | EntityType::MText(_)
                    | EntityType::AttributeDefinition(_)
                    | EntityType::AttributeEntity(_)
                    | EntityType::Dimension(_)
                    | EntityType::Leader(_)
                    | EntityType::Tolerance(_)
                    | EntityType::MultiLeader(_)
                    | EntityType::Table(_)
                    | EntityType::Shape(_)
            ) || matches!(entity, EntityType::Insert(insert) if !insert.attributes.is_empty())
            {
                extend_category(&mut index.text_geometry);
            }
            if matches!(entity, EntityType::Hatch(_))
                || crate::scene::annotative::is_annotative(&self.document, entity)
            {
                extend_category(&mut index.annotation_geometry);
            }
            let add = |map: &mut HashMap<String, DependencyTargets>, name: &str| {
                let target = map.entry(name.to_ascii_uppercase()).or_default();
                target.render_handles.extend(render_handles.iter().copied());
                target.source_handles.insert(common.handle);
                target.touches_block_definition |= inside_block;
            };
            let add_handle = |map: &mut HashMap<Handle, DependencyTargets>,
                              handle: Option<Handle>| {
                let Some(handle) = handle.filter(|handle| !handle.is_null()) else {
                    return;
                };
                let target = map.entry(handle).or_default();
                target.render_handles.extend(render_handles.iter().copied());
                target.source_handles.insert(common.handle);
                target.touches_block_definition |= inside_block;
            };
            add(&mut index.layers, &common.layer);
            match entity {
                EntityType::Text(text) => add(&mut index.text_styles, &text.style),
                EntityType::MText(text) => add(&mut index.text_styles, &text.style),
                EntityType::AttributeDefinition(attribute) => {
                    add(&mut index.text_styles, &attribute.text_style)
                }
                EntityType::AttributeEntity(attribute) => {
                    add(&mut index.text_styles, &attribute.text_style)
                }
                EntityType::Insert(insert) => {
                    for attribute in &insert.attributes {
                        add(&mut index.text_styles, &attribute.text_style);
                    }
                }
                EntityType::Dimension(dimension) => {
                    add(&mut index.dim_styles, &dimension.base().style_name);
                    if let Some(style) = self
                        .document
                        .dim_styles
                        .get(&dimension.base().style_name)
                    {
                        add(&mut index.text_styles, &style.dimtxsty);
                    }
                }
                EntityType::Leader(leader) => {
                    add(&mut index.dim_styles, &leader.dimension_style);
                    if let Some(style) = self.document.dim_styles.get(&leader.dimension_style) {
                        add(&mut index.text_styles, &style.dimtxsty);
                    }
                }
                EntityType::Tolerance(tolerance) => {
                    add(&mut index.dim_styles, &tolerance.dimension_style_name);
                    if let Some(style) = self
                        .document
                        .dim_styles
                        .get(&tolerance.dimension_style_name)
                    {
                        add(&mut index.text_styles, &style.dimtxsty);
                    }
                }
                EntityType::Table(table) => {
                    add_handle(&mut index.object_styles, table.table_style_handle);
                    if let Some(ObjectType::TableStyle(style)) = table
                        .table_style_handle
                        .and_then(|handle| self.document.objects.get(&handle))
                    {
                        for row in [
                            &style.data_row_style,
                            &style.header_row_style,
                            &style.title_row_style,
                        ] {
                            add(&mut index.text_styles, &row.text_style_name);
                        }
                    }
                    for row in &table.rows {
                        if let Some(style) = &row.style {
                            if let Some(name) = style
                                .text_style_handle
                                .and_then(|handle| text_style_names.get(&handle))
                            {
                                add(&mut index.text_styles, name);
                            }
                        }
                        for cell in &row.cells {
                            if let Some(style) = &cell.style {
                                if let Some(name) = style
                                    .text_style_handle
                                    .and_then(|handle| text_style_names.get(&handle))
                                {
                                    add(&mut index.text_styles, name);
                                }
                            }
                            for content in &cell.contents {
                                if let Some(name) = content
                                    .text_style_handle
                                    .and_then(|handle| text_style_names.get(&handle))
                                {
                                    add(&mut index.text_styles, name);
                                }
                            }
                        }
                    }
                }
                EntityType::MultiLeader(leader) => {
                    add_handle(&mut index.object_styles, leader.style_handle);
                    for handle in [leader.text_style_handle, leader.context.text_style_handle] {
                        if let Some(name) = handle.and_then(|handle| text_style_names.get(&handle)) {
                            add(&mut index.text_styles, name);
                        }
                    }
                    if let Some(ObjectType::MultiLeaderStyle(style)) = leader
                        .style_handle
                        .and_then(|handle| self.document.objects.get(&handle))
                    {
                        if let Some(name) = style
                            .text_style_handle
                            .and_then(|handle| text_style_names.get(&handle))
                        {
                            add(&mut index.text_styles, name);
                        }
                    }
                }
                EntityType::MLine(line) => add_handle(&mut index.object_styles, line.style_handle),
                _ => {}
            }
        }
        index
    }

    fn dependency_targets(&self, kind: &str, names: &[String]) -> DependencyTargets {
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() = Some(self.rebuild_dependency_index());
        }
        let cache = self.dependency_index_cache.borrow();
        let index = cache.as_ref().unwrap();
        let map = match kind {
            "layer" => &index.layers,
            "text" => &index.text_styles,
            "dim" => &index.dim_styles,
            _ => unreachable!(),
        };
        let mut combined = DependencyTargets::default();
        for name in names {
            let Some(target) = map.get(&name.to_ascii_uppercase()) else {
                continue;
            };
            combined
                .render_handles
                .extend(target.render_handles.iter().copied());
            combined
                .source_handles
                .extend(target.source_handles.iter().copied());
            combined.touches_block_definition |= target.touches_block_definition;
        }
        combined
    }

    fn invalidate_dependency_targets(&mut self, targets: DependencyTargets) {
        if targets.render_handles.is_empty() {
            return;
        }
        if targets.touches_block_definition {
            self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        }
        let sources: Vec<Handle> = targets.source_handles.iter().copied().collect();
        self.recolor_meshes_for_handles(&sources);
        let changes: Vec<(Handle, ChangeKind)> = targets
            .render_handles
            .into_iter()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.bump_entities(&changes);
    }

    pub fn invalidate_layer_dependencies(&mut self, names: &[String]) {
        let targets = self.dependency_targets("layer", names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_text_style_dependencies(&mut self, name: &str) {
        self.invalidate_text_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_text_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets("text", names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_dim_style_dependencies(&mut self, name: &str) {
        self.invalidate_dim_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_dim_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets("dim", names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_object_style_dependencies(&mut self, handles: &[Handle]) {
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() = Some(self.rebuild_dependency_index());
        }
        let mut combined = DependencyTargets::default();
        if let Some(index) = self.dependency_index_cache.borrow().as_ref() {
            for handle in handles {
                let Some(target) = index.object_styles.get(handle) else {
                    continue;
                };
                combined
                    .render_handles
                    .extend(target.render_handles.iter().copied());
                combined
                    .source_handles
                    .extend(target.source_handles.iter().copied());
                combined.touches_block_definition |= target.touches_block_definition;
            }
        }
        self.invalidate_dependency_targets(combined);
    }

    fn invalidate_dependency_category(
        &mut self,
        select: impl FnOnce(&SceneDependencyIndex) -> &DependencyTargets,
    ) {
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() = Some(self.rebuild_dependency_index());
        }
        let targets = self
            .dependency_index_cache
            .borrow()
            .as_ref()
            .map(select)
            .cloned()
            .unwrap_or_default();
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_point_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.points);
    }

    pub fn invalidate_text_geometry_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.text_geometry);
    }

    pub fn invalidate_annotation_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.annotation_geometry);
    }

    pub(crate) fn invalidate_dependency_index(&self) {
        self.dependency_index_cache.borrow_mut().take();
    }

    /// Spatial index + always-emit list for top-level entities. Lazily
    /// rebuilt on `geometry_epoch` change.
    ///
    /// `tree` holds entities whose `bounding_box()` is finite and
    /// non-degenerate. `unbounded_handles` holds entities whose bbox
    /// is degenerate or non-finite — the legacy `entity_aabb` treated
    /// those as `UNBOUNDED_AABB` (never culled), so the wire path must
    /// always emit them regardless of view. Inserts/Viewports/Blocks
    /// /BlockEnds are filtered out at build time and re-added by the
    /// wire path via a separate scan (their WCS bbox depends on
    /// transforms handled elsewhere).
    pub(super) fn entity_index(&self) -> std::cell::Ref<'_, EntityIndex> {
        {
            let cache = self.entity_index_cache.borrow();
            if let Some((epoch, _)) = *cache {
                if epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.entity_index_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }

        // Incremental path: if the cache is only a few edits behind, patch just
        // the changed handles into the existing quadtree (which supports
        // insert/remove/update, out-of-root items falling into overflow) instead
        // of re-walking every entity. Resolve each change against the document
        // first, then apply under one borrow.
        enum IdxOp {
            Remove(Handle),
            SetBounded(Handle, [f64; 4]),
            SetUnbounded(Handle),
        }
        let cached_epoch = self.entity_index_cache.borrow().as_ref().map(|c| c.0);
        if let Some(ce) = cached_epoch {
            if let Some(deltas) = self.replay_since(ce) {
                let mut ops: Vec<IdxOp> = Vec::with_capacity(deltas.len());
                for (h, kind) in &deltas {
                    match kind {
                        ChangeKind::Removed => ops.push(IdxOp::Remove(*h)),
                        ChangeKind::Added | ChangeKind::Modified => {
                            match self.document.get_entity(*h) {
                                None => ops.push(IdxOp::Remove(*h)),
                                Some(e) if is_unindexable_entity(e) => ops.push(IdxOp::Remove(*h)),
                                Some(e) => match entity_world_aabb_f64(e) {
                                    Some(ab) => ops.push(IdxOp::SetBounded(*h, ab)),
                                    None => ops.push(IdxOp::SetUnbounded(*h)),
                                },
                            }
                        }
                    }
                }
                let mut cache = self.entity_index_cache.borrow_mut();
                if let Some((epoch, idx)) = cache.as_mut() {
                    for op in ops {
                        match op {
                            IdxOp::Remove(h) => {
                                idx.tree.remove(h);
                                idx.unbounded_handles.retain(|x| *x != h);
                            }
                            IdxOp::SetBounded(h, ab) => {
                                idx.tree.update(h, ab);
                                idx.unbounded_handles.retain(|x| *x != h);
                            }
                            IdxOp::SetUnbounded(h) => {
                                idx.tree.remove(h);
                                idx.unbounded_handles.retain(|x| *x != h);
                                idx.unbounded_handles.push(h);
                            }
                        }
                    }
                    *epoch = self.geometry_epoch;
                    drop(cache);
                    return std::cell::Ref::map(self.entity_index_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }

        let mut items: Vec<(Handle, [f64; 4])> = Vec::new();
        let mut unbounded: Vec<Handle> = Vec::new();
        let mut union: Option<[f64; 4]> = None;
        for e in self.document.entities() {
            if is_unindexable_entity(e) {
                continue;
            }
            match entity_world_aabb_f64(e) {
                Some(ab) => {
                    union = Some(match union {
                        None => ab,
                        Some(u) => [
                            u[0].min(ab[0]),
                            u[1].min(ab[1]),
                            u[2].max(ab[2]),
                            u[3].max(ab[3]),
                        ],
                    });
                    items.push((e.common().handle, ab));
                }
                None => unbounded.push(e.common().handle),
            }
        }
        let root = match union {
            Some(u) => {
                let w = (u[2] - u[0]).max(1.0);
                let h = (u[3] - u[1]).max(1.0);
                let mx = w * 0.01;
                let my = h * 0.01;
                [u[0] - mx, u[1] - my, u[2] + mx, u[3] + my]
            }
            None => [-1.0, -1.0, 1.0, 1.0],
        };
        let mut tree = pick::quadtree::QuadTree::new(root);
        for (h, ab) in items {
            tree.insert(h, ab);
        }

        *self.entity_index_cache.borrow_mut() = Some((
            self.geometry_epoch,
            EntityIndex {
                tree,
                unbounded_handles: unbounded,
            },
        ));
        std::cell::Ref::map(self.entity_index_cache.borrow(), |c| &c.as_ref().unwrap().1)
    }

    /// Full tessellation pipeline for one entity.
    fn tessellate_one(&self, e: &EntityType) -> Vec<WireModel> {
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if self.current_layout == "Model" {
            self.annotation_scale
        } else if let Some(viewport) = self.active_viewport {
            self.viewport_annotation_multiplier(viewport)
        } else {
            1.0
        };
        let annotation_scale_handle = if self.current_layout == "Model" {
            crate::scene::annotative::scale_handle_by_name(
                &self.document,
                &self.document.header.current_annotation_scale,
            )
        } else if let Some(viewport) = self.active_viewport {
            self.viewport_scale_handle(viewport)
        } else {
            self.paper_annotation_scale_handle()
        };
        let blk_cache = self.block_cache_arc_for(
            annotation_scale_handle,
            self.annotation_all_visible(),
            self.active_viewport,
        );
        // tessellate_one is used for one-off lookups (hit test, properties).
        // Skip culling here so the caller always gets the full geometry.
        tessellate_entity(
            &self.document,
            &self.selected,
            self.active_viewport,
            bg,
            anno,
            annotation_scale_handle,
            e,
            Some(&blk_cache),
            None,
            None,
            self.current_layout != "Model",
        )
    }

    fn model_space_block_handle(&self) -> Handle {
        // Primary: Layout object's block_record (DWG reader sets this).
        if let Some(h) = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == "Model" && !l.block_record.is_null() {
                    Some(l.block_record)
                } else {
                    None
                }
            } else {
                None
            }
        }) {
            return h;
        }
        // Fallback for DXF files: conventional block-record name.
        self.document
            .block_records
            .get("*Model_Space")
            .map(|br| br.handle)
            .unwrap_or(Handle::NULL)
    }

    /// Compute the axis-aligned bounding box of all model-space entities.
    /// Result is epoch-cached so repeated ZOOM E / auto-fit calls are O(1).
    pub fn model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        {
            let cache = self.model_extents_cache.borrow();
            if let Some((epoch, ext)) = *cache {
                if epoch == self.geometry_epoch {
                    return ext;
                }
            }
        }
        let result = self.compute_model_space_extents();
        *self.model_extents_cache.borrow_mut() = Some((self.geometry_epoch, result));
        result
    }

    /// The AABB centre of the current selection, in absolute world coordinates
    /// (same space as `Camera::target`) — the point the 3D view orbits around
    /// when something is selected. `None` when nothing is selected; the caller
    /// then orbits about the point under the cursor. (#229)
    pub fn orbit_pivot(&self) -> Option<glam::DVec3> {
        if self.selected.is_empty() {
            return None;
        }
        let block = self.current_layout_block_handle();
        let scale = if self.current_layout == "Model" {
            crate::scene::annotative::scale_handle_by_name(
                &self.document,
                &self.document.header.current_annotation_scale,
            )
        } else {
            self.paper_annotation_scale_handle()
        };
        let wires = self.wires_for_block_culled(
            block,
            None,
            None,
            None,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        );
        let mut min = glam::DVec2::splat(f64::INFINITY);
        let mut max = glam::DVec2::splat(f64::NEG_INFINITY);
        let mut any = false;
        for wire in &wires {
            let Some(h) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            if !self.selected.contains(&h) {
                continue;
            }
            for &[x, y, _] in &wire.points {
                if x.is_finite() && y.is_finite() {
                    min = min.min(glam::DVec2::new(x as f64, y as f64));
                    max = max.max(glam::DVec2::new(x as f64, y as f64));
                    any = true;
                }
            }
        }
        if any {
            let c = (min + max) * 0.5;
            Some(glam::DVec3::new(c.x, c.y, 0.0))
        } else {
            None
        }
    }

    fn compute_model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let model_block = self.model_space_block_handle();
        if model_block.is_null() {
            return None;
        }
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;

        // Prefer the already-computed wire AABB cache when available — avoids re-tessellating.
        if self.current_layout == "Model" {
            let cache = self.wire_cache.borrow();
            if let Some(((epoch, _cam_gen), _gen, ref arc)) = *cache {
                if epoch == self.geometry_epoch {
                    for wire in arc.iter() {
                        let [ax, ay, bx, by] = wire.aabb;
                        let lo = glam::Vec3::new(ax, ay, 0.0);
                        let hi = glam::Vec3::new(bx, by, 0.0);
                        // Reject the whole AABB unless every component is finite:
                        // rays/xlines carry an unbounded AABB, and checking only
                        // x let a vertical ray's infinite y poison the extents.
                        if lo.is_finite() && hi.is_finite() {
                            min = min.min(lo);
                            max = max.max(hi);
                            any = true;
                        }
                    }
                    // 3D solids render as meshes, not wires, so fold their
                    // complete 3D boxes in too — otherwise view fitting loses
                    // both solid-only drawings and their depth.
                    for (&handle, set) in &self.meshes {
                        if !self.mesh_entity_visible(handle)
                            || !self.document.get_entity(handle).is_some_and(|entity| {
                                self.belongs_to_visible_block(
                                    handle,
                                    entity.common().owner_handle,
                                    model_block,
                                )
                            })
                        {
                            continue;
                        }
                        let [ax, ay, bx, by] = set.world_aabb;
                        let [az, bz] = set.z_aabb;
                        let lo = glam::Vec3::new(ax, ay, az);
                        let hi = glam::Vec3::new(bx, by, bz);
                        if lo.is_finite() && hi.is_finite() {
                            min = min.min(lo);
                            max = max.max(hi);
                            any = true;
                        }
                    }
                    return if any { Some((min, max)) } else { None };
                }
            }
        }

        // Fallback: tessellate (first call or paper-space context).
        // wire.key_vertices live in offset-rel coords (world_offset
        // already subtracted at tessellation time). Add it back so the
        // result matches Path 1 above and the caller's expectation —
        // callers (auto_fit_viewport) write the centroid directly to
        // `Viewport.view_target`, which is a WCS field; storing
        // offset-rel coords there silently double-subtracts world_offset
        // inside `camera_for_viewport` and points the viewport at the
        // wrong location on UTM-scale drawings.
        for entity in self.document.entities() {
            let c = entity.common();
            if c.owner_handle != model_block
                || c.invisible
                || self.entity_temporarily_hidden(c.handle)
            {
                continue;
            }
            for wire in self.tessellate_one(entity) {
                for &[x, y, z] in &wire.key_vertices {
                    let v = glam::Vec3::new(x as f32, y as f32, z as f32);
                    // Check finiteness *after* the f32 cast: a ray/xline endpoint
                    // is a huge-but-finite f64 that overflows to inf in f32, which
                    // the f64 `is_finite` test would have let through.
                    if v.is_finite() {
                        min = min.min(v);
                        max = max.max(v);
                        any = true;
                    }
                }
            }
        }
        // Same mesh inclusion for the tessellate fallback path.
        for (&handle, set) in &self.meshes {
            if !self.mesh_entity_visible(handle)
                || !self.document.get_entity(handle).is_some_and(|entity| {
                    self.belongs_to_visible_block(
                        handle,
                        entity.common().owner_handle,
                        model_block,
                    )
                })
            {
                continue;
            }
            let [ax, ay, bx, by] = set.world_aabb;
            let [az, bz] = set.z_aabb;
            let lo = glam::Vec3::new(ax, ay, az);
            let hi = glam::Vec3::new(bx, by, bz);
            if lo.is_finite() && hi.is_finite() {
                min = min.min(lo);
                max = max.max(hi);
                any = true;
            }
        }
        if any {
            return Some((min, max));
        }
        // Last-resort: the header's saved EXTMIN/EXTMAX. AutoCAD writes these
        // on save so opening a file gives ZOOM EXTENTS a useful answer before
        // the wire cache is built.
        const SANE_EXTENT: f64 = 1.0e16;
        let h = &self.document.header;
        let hmin = h.model_space_extents_min;
        let hmax = h.model_space_extents_max;
        if hmin.x < hmax.x
            && hmin.y < hmax.y
            && hmin.x.abs() < SANE_EXTENT
            && hmax.x.abs() < SANE_EXTENT
            && hmin.y.abs() < SANE_EXTENT
            && hmax.y.abs() < SANE_EXTENT
        {
            return Some((
                glam::Vec3::new(hmin.x as f32, hmin.y as f32, hmin.z as f32),
                glam::Vec3::new(hmax.x as f32, hmax.y as f32, hmax.z as f32),
            ));
        }
        None
    }

    /// Set a newly created viewport's `view_target` and `view_height` so that
    /// all model-space content is visible at a reasonable scale.
    pub fn auto_fit_viewport(&mut self, vp_handle: Handle) {
        let extents = self.model_space_extents();
        let (min, max) = match extents {
            Some(e) => e,
            None => return,
        };
        let center = (min + max) * 0.5;
        let content_w = (max.x - min.x).max(1e-3);
        let content_h = (max.y - min.y).max(1e-3);

        let vp = match self.document.get_entity_mut(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return,
        };
        // Set the view target to the model-space centroid (XY plane, z=0).
        vp.view_target.x = center.x as f64;
        vp.view_target.y = center.y as f64;
        vp.view_target.z = 0.0;

        // Choose the scale that fits both dimensions with a small margin.
        let margin = 1.1_f64;
        let scale_w = vp.width / (content_w as f64 * margin);
        let scale_h = vp.height / (content_h as f64 * margin);
        let fit_scale = scale_w.min(scale_h).min(1000.0).max(1e-6);

        vp.custom_scale = fit_scale;
        vp.view_height = vp.height / fit_scale;
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;

    fn h(v: u64) -> Handle {
        Handle::new(v)
    }

    // Collect replay output into a handle->kind map for order-independent asserts.
    fn as_map(v: Option<Vec<(Handle, ChangeKind)>>) -> Option<HashMap<Handle, ChangeKind>> {
        v.map(|list| list.into_iter().collect())
    }

    #[test]
    fn replay_reports_changes_since_a_synced_epoch() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        s.bump_entities(&[(h(10), ChangeKind::Added)]);
        let e1 = s.geometry_epoch;
        s.bump_entities(&[(h(11), ChangeKind::Modified)]);

        // From e0 we see both; from e1 only the second.
        let all = as_map(s.replay_since(e0)).unwrap();
        assert_eq!(all.get(&h(10)), Some(&ChangeKind::Added));
        assert_eq!(all.get(&h(11)), Some(&ChangeKind::Modified));
        let since_e1 = as_map(s.replay_since(e1)).unwrap();
        assert_eq!(since_e1.len(), 1);
        assert_eq!(since_e1.get(&h(11)), Some(&ChangeKind::Modified));
    }

    #[test]
    fn added_then_removed_cancels() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        s.bump_entities(&[(h(20), ChangeKind::Added)]);
        s.bump_entities(&[(h(20), ChangeKind::Removed)]);
        let m = as_map(s.replay_since(e0)).unwrap();
        assert!(
            !m.contains_key(&h(20)),
            "add+remove in one window must cancel"
        );
    }

    #[test]
    fn modified_then_removed_is_removed() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        s.bump_entities(&[(h(21), ChangeKind::Modified)]);
        s.bump_entities(&[(h(21), ChangeKind::Removed)]);
        assert_eq!(
            as_map(s.replay_since(e0)).unwrap().get(&h(21)),
            Some(&ChangeKind::Removed)
        );
    }

    #[test]
    fn full_bump_forces_rebuild() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        s.bump_entities(&[(h(30), ChangeKind::Added)]);
        s.bump_geometry(); // full delta
        assert!(s.replay_since(e0).is_none(), "a spanned full delta ⇒ None");
    }

    #[test]
    fn insert_add_and_update_publish_targeted_deltas() {
        use acadrust::entities::Insert;
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let before_add = s.geometry_epoch;
        let handle = s.add_entity(EntityType::Insert(Insert::new(
            "EXISTING_BLOCK",
            Vector3::new(0.0, 0.0, 0.0),
        )));
        assert_eq!(
            as_map(s.replay_since(before_add)).unwrap().get(&handle),
            Some(&ChangeKind::Added),
            "adding an INSERT must not force a whole-drawing rebuild"
        );

        let before_update = s.geometry_epoch;
        let mut edited = s.document.get_entity(handle).cloned().unwrap();
        if let EntityType::Insert(insert) = &mut edited {
            insert.insert_point.x = 25.0;
        }
        assert!(s.update_entity(edited));
        assert_eq!(
            as_map(s.replay_since(before_update)).unwrap().get(&handle),
            Some(&ChangeKind::Modified),
            "retargeting or moving an INSERT changes only its render handle"
        );
    }

    #[test]
    fn dependency_categories_invalidate_only_affected_render_handles() {
        use acadrust::entities::{Line, MText, Point, Text};
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let line = s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let point = s.add_entity(EntityType::Point(Point::new()));
        let text = s.add_entity(EntityType::Text(Text::with_value(
            "plain",
            Vector3::new(0.0, 2.0, 0.0),
        )));
        let mut annotative = MText::new();
        annotative.value = "annotative".to_string();
        annotative.is_annotative = true;
        let annotative = s.add_entity(EntityType::MText(annotative));

        let before_points = s.geometry_epoch;
        s.invalidate_point_dependencies();
        let points = as_map(s.replay_since(before_points)).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points.get(&point), Some(&ChangeKind::Modified));

        let before_text = s.geometry_epoch;
        s.invalidate_text_geometry_dependencies();
        let texts = as_map(s.replay_since(before_text)).unwrap();
        assert_eq!(texts.get(&text), Some(&ChangeKind::Modified));
        assert_eq!(texts.get(&annotative), Some(&ChangeKind::Modified));
        assert!(!texts.contains_key(&line));
        assert!(!texts.contains_key(&point));

        let before_annotation = s.geometry_epoch;
        s.invalidate_annotation_dependencies();
        let annotation = as_map(s.replay_since(before_annotation)).unwrap();
        assert_eq!(annotation.len(), 1);
        assert_eq!(
            annotation.get(&annotative),
            Some(&ChangeKind::Modified)
        );
    }

    #[test]
    fn object_visibility_uses_entity_deltas() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let hidden = s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let untouched = s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
        )));

        s.selected.insert(hidden);
        let before_hide = s.geometry_epoch;
        s.hide_selected();
        let hide = as_map(s.replay_since(before_hide)).unwrap();
        assert_eq!(hide.len(), 1);
        assert_eq!(hide.get(&hidden), Some(&ChangeKind::Modified));
        assert!(!hide.contains_key(&untouched));

        let before_show = s.geometry_epoch;
        s.end_isolation();
        let show = as_map(s.replay_since(before_show)).unwrap();
        assert_eq!(show.len(), 1);
        assert_eq!(show.get(&hidden), Some(&ChangeKind::Modified));
    }

    #[test]
    fn ring_overflow_forces_rebuild() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        for i in 0..(GEOMETRY_JOURNAL_CAP as u64 + 5) {
            s.bump_entities(&[(h(1000 + i), ChangeKind::Added)]);
        }
        assert!(
            s.replay_since(e0).is_none(),
            "a consumer older than the ring floor ⇒ None"
        );
        // A recent consumer is still replayable.
        let recent = s.geometry_epoch;
        s.bump_entities(&[(h(9999), ChangeKind::Added)]);
        assert!(s.replay_since(recent).is_some());
    }

    // Differential oracle: the incrementally-patched entity index must always
    // equal a from-scratch rebuild after any add / move / erase.
    #[test]
    fn entity_index_incremental_matches_full_rebuild() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
            EntityType::Line(Line::from_points(
                Vector3::new(a.0, a.1, 0.0),
                Vector3::new(b.0, b.1, 0.0),
            ))
        }
        // Set of handles the index reports over the whole plane (bounded via the
        // quadtree + always-surfaced unbounded).
        fn indexed(s: &Scene) -> HashSet<Handle> {
            let idx = s.entity_index();
            let mut set: HashSet<Handle> = idx
                .tree
                .query_rect([-1e9, -1e9, 1e9, 1e9])
                .into_iter()
                .collect();
            set.extend(idx.unbounded_handles.iter().copied());
            set
        }
        fn full_rebuild(s: &Scene) -> HashSet<Handle> {
            *s.entity_index_cache.borrow_mut() = None;
            indexed(s)
        }

        let mut s = Scene::new();
        let h1 = s.add_entity(line((0.0, 0.0), (10.0, 10.0)));
        let h2 = s.add_entity(line((5.0, 5.0), (20.0, 3.0)));
        // Prime the index (full build), then each edit must keep it == rebuild.
        let _ = indexed(&s);

        // Add
        let h3 = s.add_entity(line((100.0, 100.0), (140.0, 90.0)));
        assert_eq!(indexed(&s), full_rebuild(&s), "after add");

        // Modify (move h1 far away)
        {
            let e = line((900.0, 900.0), (950.0, 970.0));
            let mut e = e;
            e.common_mut().handle = h1;
            s.update_entity(e);
        }
        assert_eq!(indexed(&s), full_rebuild(&s), "after modify");

        // Erase h2
        s.erase_entities(&[h2]);
        assert_eq!(indexed(&s), full_rebuild(&s), "after erase");
        assert!(indexed(&s).contains(&h3) && indexed(&s).contains(&h1));
        assert!(!indexed(&s).contains(&h2));
    }

    // Differential oracle for the resident-wire splice: the incrementally
    // patched resident set must equal a from-scratch rebuild (same wires in the
    // same draw order) after add / move / erase — and the patch path must run.
    #[test]
    fn resident_set_incremental_matches_full_rebuild() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
            EntityType::Line(Line::from_points(
                Vector3::new(a.0, a.1, 0.0),
                Vector3::new(b.0, b.1, 0.0),
            ))
        }
        // Visible wire names, in order. Resident erase deliberately retains
        // blank tombstone slots, which are not part of a from-scratch build.
        fn resident_names(s: &Scene) -> Vec<String> {
            let cam = Camera::default();
            let arc = s.model_tile_wires_arc(0, &cam, 1.0, 1.0);
            let names: Vec<String> = arc
                .iter()
                .filter(|wire| Scene::handle_from_wire_name(&wire.name).is_some())
                .map(|wire| wire.name.clone())
                .collect();
            drop(arc); // release so the next patch can move the wires out
            names
        }
        fn from_scratch(s: &Scene) -> Vec<String> {
            s.resident_wire_sets.borrow_mut().clear();
            resident_names(s)
        }

        let mut s = Scene::new();
        let _h1 = s.add_entity(line((0.0, 0.0), (10.0, 10.0)));
        let h2 = s.add_entity(line((5.0, 5.0), (20.0, 3.0)));
        let h2_image = s.document.get_entity_arc(h2).unwrap();
        let _ = resident_names(&s); // prime the resident cache

        let hits0 = s.resident_patch_hits.get();

        let h3 = s.add_entity(line((1.0, 1.0), (2.0, 2.0)));
        assert_eq!(resident_names(&s), from_scratch(&s), "resident after add");

        {
            let mut e = line((100.0, 100.0), (140.0, 90.0));
            e.common_mut().handle = h3;
            s.update_entity(e);
        }
        assert_eq!(
            resident_names(&s),
            from_scratch(&s),
            "resident after modify"
        );

        let original_h2_start = s
            .resident_wire_sets
            .borrow()
            .values()
            .find_map(|set| {
                set.layout
                    .as_ref()
                    .and_then(|layout| layout.ranges.get(&h2).map(|range| range.0))
            })
            .unwrap();
        let before_erase_names = resident_names(&s);
        s.erase_entities(&[h2]);
        let patched = resident_names(&s);
        let erase_gen = s.last_model_wire_gen.get();
        assert!(
            s.model_wire_patch_for(erase_gen)
                .is_some_and(|(_, patch)| !patch.face_pass_changed),
            "removing a plain line must keep the unrelated Face3D/fill pass warm"
        );
        assert!(
            s.resident_wire_sets.borrow().values().any(|set| {
                set.layout
                    .as_ref()
                    .is_some_and(|layout| layout.vacant.contains_key(&h2))
            }),
            "erase should retain h2's physical range as a tombstone"
        );
        let expected_erased: Vec<_> = before_erase_names
            .iter()
            .filter(|name| *name != &h2.value().to_string())
            .cloned()
            .collect();
        assert_eq!(patched, expected_erased, "resident after erase");

        // Replay the same Added delta history Undo emits. The entity must return
        // to its original physical range instead of appending/shifting the set.
        let restored = s.apply_entity_delta(
            &[(h2, Some(Arc::clone(&h2_image)), None)],
            true,
        );
        for &(handle, _) in &restored {
            s.reseed_derived_caches(handle);
        }
        s.bump_entities(&restored);
        let restored_names = resident_names(&s);
        assert_eq!(restored_names, before_erase_names);
        assert!(s.resident_wire_sets.borrow().values().any(|set| {
            set.layout.as_ref().is_some_and(|layout| {
                layout.ranges.get(&h2).map(|range| range.0)
                    == Some(original_h2_start)
                    && !layout.vacant.contains_key(&h2)
            })
        }));

        assert_eq!(
            s.resident_patch_hits.get(),
            hits0 + 4,
            "every add / modify / erase / restore edit must use the incremental \
             patch, not silently fall back to a full rebuild"
        );
    }

    #[test]
    fn draw_depth_add_and_erase_keep_existing_labels_stable() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let line = |y: f64| {
            EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(10.0, y, 0.0),
            ))
        };
        let mut s = Scene::new();
        let h1 = s.add_entity(line(1.0));
        let h2 = s.add_entity(line(2.0));
        let initial = s.draw_depth_map();
        let d1 = initial[&h1.value()];
        let d2 = initial[&h2.value()];
        drop(initial);

        let h3 = s.add_entity(line(3.0));
        let after_add = s.draw_depth_map();
        assert_eq!(after_add[&h1.value()], d1);
        assert_eq!(after_add[&h2.value()], d2);
        let d3 = after_add[&h3.value()];
        drop(after_add);

        s.erase_entities(&[h2]);
        let after_erase = s.draw_depth_map();
        assert_eq!(after_erase[&h1.value()], d1);
        assert_eq!(after_erase[&h3.value()], d3);
        assert!(!after_erase.contains_key(&h2.value()));
    }

    #[test]
    fn category_gate_keeps_unrelated_caches_warm() {
        let mut s = Scene::new();
        let e0 = s.geometry_epoch;
        // Editing handles NOT in the (empty) hatch category leaves it valid.
        s.bump_entities(&[(h(40), ChangeKind::Modified)]);
        assert!(
            s.category_cache_valid(e0, CACHE_CATEGORY_HATCH, |hh| {
                s.hatches.contains_key(&hh)
            }),
            "an edit outside the category must keep it warm"
        );
        // A caller that bypasses the tracked erase primitive supplies no
        // before-category hint, so removal remains conservatively invalid.
        s.bump_entities(&[(h(41), ChangeKind::Removed)]);
        assert!(
            !s.category_cache_valid(e0, CACHE_CATEGORY_HATCH, |hh| {
                s.hatches.contains_key(&hh)
            }),
            "an unclassified removal must invalidate"
        );
    }

    #[test]
    fn plain_geometry_keeps_insert_hatch_cache_warm() {
        use acadrust::entities::{Insert, Line};
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let initial = s.insert_hatches_for_click();
        s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let after_line = s.insert_hatches_for_click();
        assert!(
            Arc::ptr_eq(&initial, &after_line),
            "a line edit must not rescan and explode every block insert"
        );

        let insert = s.add_entity(EntityType::Insert(Insert::new(
            "EXISTING_BLOCK",
            Vector3::new(0.0, 0.0, 0.0),
        )));
        let after_insert = s.insert_hatches_for_click();
        assert!(!Arc::ptr_eq(&after_line, &after_insert));

        s.erase_entities(&[insert]);
        let after_erase = s.insert_hatches_for_click();
        assert!(!Arc::ptr_eq(&after_insert, &after_erase));
    }

    #[test]
    fn selecting_plain_geometry_keeps_hatch_models_warm() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let line = s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let before = s.hatch_models_arc();
        s.select_entity(line, false);
        let after = s.hatch_models_arc();
        assert!(
            Arc::ptr_eq(&before, &after),
            "selecting a non-hatch must not rebuild every hatch model"
        );
    }

    #[test]
    fn plain_line_erase_keeps_unrelated_category_and_text_caches_warm() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let mut s = Scene::new();
        let line = s.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let cached_epoch = s.geometry_epoch;
        s.erase_entities(&[line]);

        assert!(s.category_cache_valid(cached_epoch, CACHE_CATEGORY_HATCH, |h| {
            s.hatches.contains_key(&h)
        }));
        assert!(s.category_cache_valid(cached_epoch, CACHE_CATEGORY_IMAGE, |h| {
            s.images.contains_key(&h)
        }));
        assert!(s.category_cache_valid(cached_epoch, CACHE_CATEGORY_MESH, |h| {
            s.meshes.contains_key(&h)
        }));
        assert!(s.text_unchanged(cached_epoch));
    }
}

/// Delta-undo round-trips: a recorded entity-only edit must be exactly
/// invertible (undo restores the pre-edit images) and re-appliable (redo
/// restores the post-edit images), including handle preservation and no
/// duplication of a re-inserted handle in its owning block record.
#[cfg(test)]
mod delta_undo_tests {
    use super::*;
    use crate::command::EntityTransform;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;
    use glam::DVec3;

    fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(x1, y1, 0.0),
            Vector3::new(x2, y2, 0.0),
        ))
    }

    /// Build the delta the way `commit_undo_delta` does: pair each recorded
    /// before-image with the entity's current (after) state.
    fn build_delta(
        scene: &Scene,
        rec: UndoRecording,
    ) -> Vec<(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)> {
        rec.into_before_images()
            .into_iter()
            .map(|(h, before)| {
                let after = scene.document.get_entity_arc(h);
                (h, before, after)
            })
            .collect()
    }

    /// Occurrences of a handle in the *Model_Space block record's entity list.
    fn ms_occurrences(scene: &Scene, handle: Handle) -> usize {
        scene
            .document
            .block_records
            .get("*Model_Space")
            .map(|br| br.entity_handles.iter().filter(|&&x| x == handle).count())
            .unwrap_or(0)
    }

    #[test]
    fn transform_delta_round_trips() {
        let mut scene = Scene::new();
        let h = scene.add_entity(line(0.0, 0.0, 10.0, 0.0));
        let orig = scene.document.get_entity(h).cloned().unwrap();

        scene.begin_undo_recording();
        scene.transform_entities(&[h], &EntityTransform::Translate(DVec3::new(5.0, 3.0, 0.0)));
        let rec = scene.take_undo_recording().unwrap();
        assert!(!rec.is_poisoned(), "a plain move must not poison");
        let delta = build_delta(&scene, rec);
        let moved = scene.document.get_entity(h).cloned().unwrap();
        assert_ne!(orig, moved, "the move must actually change the entity");

        // Undo restores the pre-move image exactly, in place (same handle).
        scene.apply_entity_delta(&delta, true);
        assert_eq!(scene.document.get_entity(h).cloned().unwrap(), orig);
        assert_eq!(ms_occurrences(&scene, h), 1);

        // Redo re-applies the post-move image.
        scene.apply_entity_delta(&delta, false);
        assert_eq!(scene.document.get_entity(h).cloned().unwrap(), moved);
        assert_eq!(ms_occurrences(&scene, h), 1);
    }

    #[test]
    fn erase_delta_round_trips_with_handle_and_no_dup() {
        let mut scene = Scene::new();
        let h1 = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));
        let h2 = scene.add_entity(line(2.0, 2.0, 3.0, 3.0));
        let orig1 = scene.document.get_entity(h1).cloned().unwrap();
        let orig2 = scene.document.get_entity(h2).cloned().unwrap();
        let count = scene.document.entities().count();
        scene.select_entity(h2, false);
        scene.set_hover_highlight(Some(h2));
        let selection_generation = scene.selection_generation;

        scene.begin_undo_recording();
        scene.erase_entities(&[h2]);
        let rec = scene.take_undo_recording().unwrap();
        assert!(
            !rec.is_poisoned(),
            "erasing an ungrouped entity must not poison"
        );
        let delta = build_delta(&scene, rec);
        assert!(scene.document.get_entity(h2).is_none(), "h2 must be erased");
        assert!(!scene.selected.contains(&h2));
        assert_eq!(scene.hover_highlight, None);
        assert_ne!(
            scene.selection_generation, selection_generation,
            "erase must invalidate the stale selection overlay"
        );

        // Undo re-inserts h2 with its ORIGINAL handle and image, exactly once in
        // the block record (the strip prevents a duplicated dangling entry),
        // leaving h1 untouched.
        scene.apply_entity_delta(&delta, true);
        assert_eq!(scene.document.get_entity(h2).cloned().unwrap(), orig2);
        assert_eq!(scene.document.get_entity(h1).cloned().unwrap(), orig1);
        assert_eq!(scene.document.entities().count(), count);
        assert_eq!(
            ms_occurrences(&scene, h2),
            1,
            "h2 must appear once, not duplicated"
        );

        // Redo erases h2 again. (remove_entity leaves the handle dangling in
        // the block record — harmless, a lookup miss is skipped on render/save
        // — so we assert absence from the document, not from entity_handles.)
        scene.apply_entity_delta(&delta, false);
        assert!(scene.document.get_entity(h2).is_none());
    }

    #[test]
    fn add_delta_round_trips() {
        let mut scene = Scene::new();
        scene.begin_undo_recording();
        let h = scene.add_entity(line(0.0, 0.0, 4.0, 4.0));
        let rec = scene.take_undo_recording().unwrap();
        assert!(
            !rec.is_poisoned(),
            "a plain add on an existing layer must not poison"
        );
        let added = scene.document.get_entity(h).cloned().unwrap();
        let delta = build_delta(&scene, rec);

        // Undo removes the freshly added entity from the document (a dangling
        // block-record entry may remain — see erase test — that's fine).
        scene.apply_entity_delta(&delta, true);
        assert!(scene.document.get_entity(h).is_none());

        // Redo re-adds it with the same handle, exactly once (the strip clears
        // any dangling entry so add_entity doesn't duplicate it).
        scene.apply_entity_delta(&delta, false);
        assert_eq!(scene.document.get_entity(h).cloned().unwrap(), added);
        assert_eq!(ms_occurrences(&scene, h), 1);
    }

    #[test]
    fn raster_add_records_only_its_image_definition_object() {
        use acadrust::entities::RasterImage;
        use acadrust::objects::ObjectType;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let image = RasterImage::with_size(
            "test.png",
            Vector3::new(0.0, 0.0, 0.0),
            16.0,
            16.0,
            10.0,
            10.0,
        );
        scene.begin_undo_recording();
        let handle = scene.add_entity(EntityType::RasterImage(image));
        let rec = scene.take_undo_recording().unwrap();
        assert!(!rec.is_poisoned());
        let (entities, objects) = rec.into_recorded_images();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0, handle);
        assert_eq!(objects.len(), 1);
        assert!(objects[0].1.is_none());
        assert!(matches!(
            scene.document.objects.get(&objects[0].0),
            Some(ObjectType::ImageDefinition(_))
        ));
    }

    #[test]
    fn grouped_erase_records_group_object_without_poisoning() {
        use acadrust::objects::ObjectType;

        let mut scene = Scene::new();
        let h1 = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));
        let h2 = scene.add_entity(line(2.0, 0.0, 3.0, 0.0));
        let group = scene.create_group("G".to_string(), vec![h1, h2]);

        scene.begin_undo_recording();
        scene.erase_entities(&[h1]);
        let rec = scene.take_undo_recording().unwrap();
        assert!(!rec.is_poisoned());
        let (entities, objects) = rec.into_recorded_images();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0, h1);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].0, group);
        assert!(matches!(objects[0].1, Some(ObjectType::Group(_))));
        assert!(matches!(
            scene.document.objects.get(&group),
            Some(ObjectType::Group(current)) if current.entities == vec![h2]
        ));
    }

    #[test]
    fn copy_delta_round_trips() {
        let mut scene = Scene::new();
        let src = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));

        scene.begin_undo_recording();
        let new_handles = scene.copy_entities(
            &[src],
            &EntityTransform::Translate(DVec3::new(10.0, 0.0, 0.0)),
        );
        let rec = scene.take_undo_recording().unwrap();
        assert!(!rec.is_poisoned(), "copying a plain entity must not poison");
        assert_eq!(new_handles.len(), 1);
        let copy_h = new_handles[0];
        let copy_img = scene.document.get_entity(copy_h).cloned().unwrap();
        let delta = build_delta(&scene, rec);

        // Undo erases the copy from the document, leaving the source intact.
        scene.apply_entity_delta(&delta, true);
        assert!(scene.document.get_entity(copy_h).is_none());
        assert!(scene.document.get_entity(src).is_some());

        // Redo restores the copy with its handle, once.
        scene.apply_entity_delta(&delta, false);
        assert_eq!(
            scene.document.get_entity(copy_h).cloned().unwrap(),
            copy_img
        );
        assert_eq!(ms_occurrences(&scene, copy_h), 1);
    }
}
