// Scene modules grouped by role:
//   convert — DXF/ACIS entities → truck solids & tessellated geometry
//   text    — LFF stroke + TrueType font engines and shaping
//   model   — per-entity GPU render models (wire, hatch, mesh, image, object)
//   pick    — hit-testing, selection, grips, spatial index, xclip
//   view    — camera, transforms, viewport, render pipeline driver
//   cache   — block-definition and property caches
pub mod cache;
pub mod convert;
pub mod model;
pub mod pick;
pub mod pipeline;
pub mod text;
pub mod view;

/// Result of `Scene::entity_index()`. The wire path queries `tree` for
/// view-rect candidates and also always emits `unbounded_handles`
/// (entities with no usable bbox — legacy `UNBOUNDED_AABB` sentinel).
pub(super) struct EntityIndex {
    pub tree: pick::quadtree::QuadTree,
    pub unbounded_handles: Vec<Handle>,
}

use view::camera::Camera;
pub use view::camera::Projection;
pub use model::hatch_model::HatchModel;
pub use model::image_model::ImageModel;
pub use model::mesh_model::MeshLodSet;
pub use model::object::{GripApply, GripDef};
pub use pipeline::uniforms::Uniforms;
pub use pipeline::viewcube::{
    hit_test, hit_test_cardinal, hover_id, CubeRegion, NudgeDir, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD,
    VIEWCUBE_PX, VIEWCUBE_REGION_PX,
};
pub use pick::selection::SelectionState;
pub use model::wire_model::WireModel;

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
use std::cell::RefCell;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Global counter so every Scene and every geometry mutation gets a
/// process-wide unique epoch. This prevents two different tabs (Scenes)
/// from ever sharing the same epoch value, which would cause the shared
/// GPU Pipeline to skip re-uploading geometry when switching tabs.
static GEOMETRY_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Process-wide monotonic id stamped on each Model-tile wire re-tessellation.
/// Reused (not re-stamped) when a pan reuses the cached tessellation, so it
/// uniquely identifies a wire-buffer's *content* across frames. The GPU
/// pipeline gates wire re-upload on it (Phase 3.2): a pan that reuses the
/// tessellation keeps the same id, so the world-space wire buffer is not
/// re-sent. Monotonic (never reused) → free of the ABA hazard a raw `Arc`
/// pointer would carry when an address is freed and reallocated.
static WIRE_CONTENT_GEN: AtomicU64 = AtomicU64::new(1);

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

/// Pre-built entity caches returned by [`build_derived_caches`].
/// Produced in the file-load background task so the UI thread only assigns.
#[derive(Debug, Clone)]
pub struct DerivedCaches {
    pub local_extent_max: f32,
    pub local_center: [f64; 2],
    pub hatches: HashMap<Handle, HatchModel>,
    pub images: HashMap<Handle, ImageModel>,
    pub meshes: HashMap<Handle, MeshLodSet>,
    /// Block-definition solid meshes, block-local frame (instanced per INSERT). (#123)
    pub block_meshes: HashMap<Handle, MeshLodSet>,
    /// Number of entities removed by the corrupt-entity guard during load.
    /// Reported back to the UI so the user knows when a file had parser-junk
    /// entities silently dropped.
    pub corrupt_dropped: usize,
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
}

/// Build hatch / image / mesh caches from a document without needing `&mut Scene`.
/// Intended to run on a background thread during file load.
pub fn build_derived_caches(doc: &CadDocument) -> DerivedCaches {
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
    for e in doc.entities() {
        let h = e.common().handle;
        match e {
            EntityType::Hatch(_) | EntityType::Solid(_) => hatch_handles.push(h),
            EntityType::RasterImage(_) => image_handles.push(h),
            EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) | EntityType::Surface(_) => {
                mesh_handles.push(h)
            }
            _ => {}
        }
        if let Some(c) = offset_centroid(e, model_block, &prep) {
            centers.push(c);
        }
    }
    let (local_center, local_extent_max) = cluster_extent_from_centers(centers, &doc.header);

    // Default bg adaptation target at load: the model background (paper
    // bg is only relevant after the user enters a paper layout, and
    // `synced_hatch_models` re-runs `render_style` per-frame anyway so
    // the per-layout adaptation kicks in later regardless).
    const LOAD_BG: [f32; 4] = [0.11, 0.11, 0.11, 1.0];

    // hatches
    let hatches: HashMap<Handle, HatchModel> = hatch_handles
        .par_iter()
        .filter_map(|&handle| {
            let e = doc.get_entity(handle)?;
            let (raw, ..) = view::render::render_style_for(doc, e);
            let color = view::render::adapt_to_bg(raw, LOAD_BG);
            let model = match e {
                EntityType::Hatch(dxf) => Scene::hatch_model_from_dxf(dxf, color),
                EntityType::Solid(solid) => Some(Scene::solid_hatch_model(solid, color)),
                _ => None,
            };
            model.map(|m| (handle, m))
        })
        .collect();

    // images
    let images: HashMap<Handle, ImageModel> = image_handles
        .par_iter()
        .filter_map(|&handle| {
            if let EntityType::RasterImage(img) = doc.get_entity(handle)? {
                ImageModel::from_raster_image(img).map(|m| (handle, m))
            } else {
                None
            }
        })
        .collect();

    // meshes (parallel tessellation). FACETRES (header.facet_resolution)
    // scales the per-LOD segment counts so users with finer drawings get
    // smoother solids; clamped to AutoCAD's [0.01, 10.0] range inside.
    // Top-level (layout-owned) solids are offset into the render frame; block
    // definition solids keep block-local coords for per-INSERT instancing. (#123)
    let facet_res = doc.header.facet_resolution;
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
            let top_level = layout_blocks.contains(&e.common().owner_handle);
            crate::entities::solid3d::tessellate_volume(e, color, facet_res).map(|m| {
                let m = if top_level { offset_mesh_lod_set(m) } else { m };
                (handle, m, top_level)
            })
        })
        .collect();
    let mut meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    let mut block_meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    for (handle, m, top_level) in built {
        if top_level {
            meshes.insert(handle, m);
        } else {
            block_meshes.insert(handle, m);
        }
    }

    DerivedCaches {
        local_extent_max,
        local_center,
        hatches,
        images,
        meshes,
        block_meshes,
        corrupt_dropped: 0,
        timings: OpenTimings::default(),
    }
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
    let model_br = doc
        .block_records
        .iter()
        .find(|br| br.handle == model_block);
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
    OffsetPrep { mspace_set, any_enumerated, owned_by_other_block }
}

/// Per-entity centroid for the world-offset scan, or `None` if the entity is
/// not MSPACE geometry / has no usable bbox. Single-outlier-robust because
/// the caller takes the median of these per-entity centroids rather than a
/// global min/max midpoint.
fn offset_centroid(
    e: &EntityType,
    model_block: Handle,
    prep: &OffsetPrep,
) -> Option<[f64; 3]> {
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

/// Tolerance for matching two normalized tile coordinates as "the same"
/// edge — drag math leaves small floating-point residue.
const TILE_EPS: f32 = 1e-4;

#[derive(Copy, Clone, Debug)]
pub enum TileEdgeOrient {
    Vertical,
    Horizontal,
}

/// One inner divider between Model tiles, exposed by [`Scene::model_tile_edges`].
/// `coord` is the fixed axis (x for vertical, y for horizontal) and `span`
/// is the perpendicular extent over which the divider actually separates
/// tiles. All values are normalized to the 0..1 canvas.
#[derive(Clone, Debug)]
pub struct TileEdge {
    pub orient: TileEdgeOrient,
    pub coord: f32,
    pub span: (f32, f32),
}

#[derive(Copy, Clone, Debug)]
enum ContactSide {
    Left,
    Right,
    Top,
    Bottom,
}

fn overlap_len(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0.0)
}

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
            *vl = [(ax - hx as f64) as f32, (ay - hy as f64) as f32, (az - hz as f64) as f32];
            if hx < min_x { min_x = hx; }
            if hy < min_y { min_y = hy; }
            if hx > max_x { max_x = hx; }
            if hy > max_y { max_y = hy; }
        }
    }
    if min_x.is_finite() {
        set.world_aabb = [min_x, min_y, max_x, max_y];
    }
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
            *vl = [(ax - hx as f64) as f32, (ay - hy as f64) as f32, (az - hz as f64) as f32];
            if hx < min_x { min_x = hx; }
            if hy < min_y { min_y = hy; }
            if hx > max_x { max_x = hx; }
            if hy > max_y { max_y = hy; }
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
    if min_x.is_finite() {
        out.world_aabb = [min_x, min_y, max_x, max_y];
    }
    out
}

/// Stable hash of a `Camera`'s pose for use as a per-tile cache key.
/// Two cameras with bit-identical target / rotation / distance hash the
/// same; any orbit / pan / zoom on a tile bumps it.
/// Hash of the camera state that affects *tessellation output* on the Model
/// tile, deliberately EXCLUDING the pan target. Rotation governs the
/// projection; `wpp` (world-per-pixel) governs the zoom-adaptive curve tol
/// and the sub-pixel LOD cull. Two cameras with the same value differ only by
/// pan, so the same tessellation is valid for both as long as the new view
/// still fits inside the region the wires were culled to (see
/// `model_tile_wires_arc`). Target is the one varying input on a pure pan.
fn camera_pan_invariant_hash(c: &Camera, wpp: Option<f32>) -> u64 {
    fn h(state: u64, x: f32) -> u64 {
        state.rotate_left(13) ^ x.to_bits() as u64
    }
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    s = h(s, c.rotation.x);
    s = h(s, c.rotation.y);
    s = h(s, c.rotation.z);
    s = h(s, c.rotation.w);
    s = h(s, wpp.unwrap_or(-1.0));
    s
}

/// `outer` fully contains `inner` (both `[min_x, min_y, max_x, max_y]`).
#[inline]
fn aabb_contains(outer: [f32; 4], inner: [f32; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

/// World-XY rectangle the model camera currently sees, expanded by `margin`
/// (1.0 = tight), as `[min_x, min_y, max_x, max_y]` for the entity R-tree cull.
///
/// The screen is a rectangle in the camera's right/up basis, not the world
/// axes — under a view twist or yaw that rectangle is rotated in world XY, so
/// projecting its four corners and taking their bounds gives the correct
/// enclosing box. A naive `target ± (w, h)` box (world-axis aligned) under-
/// covers a rotated view and culls the geometry that lands in the rotated
/// corners.
///
/// Returns `None` for a tilted (non-plan) view, where the view direction is
/// not vertical and a flat XY box cannot bound the visible region (depth
/// collapses onto the plane); callers then skip the frustum cull rather than
/// wrongly hide geometry.
fn view_cull_aabb(cam: &Camera, aspect: f32, margin: f32) -> Option<[f32; 4]> {
    // Plan view ⇔ line of sight is (near) vertical. Anything else can't be
    // bounded by a single world-XY rectangle.
    let fwd = cam.rotation * glam::Vec3::Z;
    if fwd.z.abs() < 0.999 {
        return None;
    }
    let h = cam.ortho_size();
    let w = h * aspect.max(0.01);
    let right = cam.rotation * glam::Vec3::X;
    let up = cam.rotation * glam::Vec3::Y;
    let c = cam.target.as_vec3();
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (sw, sh) in [(-w, -h), (w, -h), (-w, h), (w, h)] {
        let p = c + right * sw + up * sh;
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    let (cx, cy) = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    let (hw, hh) = ((max_x - min_x) * 0.5 * margin, (max_y - min_y) * 0.5 * margin);
    Some([cx - hw, cy - hh, cx + hw, cy + hh])
}

pub struct Scene {
    pub camera: Rc<RefCell<Camera>>,
    /// Model-space tiled viewport layout. One full-window tile by default;
    /// the split buttons / VPORTS subdivide the active tile.
    pub(crate) model_tiles: RefCell<Vec<ModelTile>>,
    /// Index of the active model tile (camera input + overlays target it).
    pub(crate) active_model_tile: std::cell::Cell<usize>,
    pub selection: Rc<RefCell<SelectionState>>,
    /// The CAD document — single source of truth for all entities.
    pub document: CadDocument,
    /// Currently selected entity handles.
    pub selected: HashSet<Handle>,
    /// Entity handles hidden by Isolate / Hide. Empty = nothing hidden.
    /// `tessellate_block`'s visibility test skips these, so they neither
    /// render nor hit-test until isolation ends.
    pub hidden: HashSet<Handle>,
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
    /// Committed-segment wire drawn during multi-point commands (normal colour).
    pub interim_wire: Option<WireModel>,
    pub camera_generation: u64,
    /// Incremented whenever geometry-affecting state changes (entities, selection,
    /// preview wires, layer visibility, layout). The GPU pipeline uses this to
    /// skip re-uploading unchanged geometry buffers every frame.
    pub geometry_epoch: u64,
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
    wire_cache: RefCell<Option<((u64, u64), Arc<Vec<WireModel>>)>>,
    /// Per-Model-tile cached tessellation. Each tile has its own camera
    /// (live for the active tile, stored snapshot for the others), so
    /// LOD / frustum culling has to run independently — the shared
    /// `wire_cache` would cull every tile against whichever camera was
    /// current when it was last built. Keyed by tile index; the value
    /// Carries `(geometry_epoch, pan_invariant_hash, tessellated_region)`.
    /// A hit needs the epoch + the rotation/tol signature to match AND the
    /// tile's current visible view to still fit inside the region the wires
    /// were culled to — so a pure pan within that 1.25×-margin region reuses
    /// the tessellation instead of rebuilding it. Zoom / orbit / edits change
    /// the epoch or signature and rebuild as before.
    model_tile_wire_cache:
        RefCell<HashMap<usize, ((u64, u64, [f32; 4]), u64, Arc<Vec<WireModel>>)>>,
    /// Index built from every SortEntitiesTable in the document.
    /// Maps block_handle → (entity_handle.value() → sort_handle.value()).
    /// Replaces the O(objects) linear scan inside `wires_for_block()` with an O(1) lookup.
    sort_cache: RefCell<Option<(u64, HashMap<Handle, HashMap<u64, u64>>)>>,
    /// Per-entity normalized draw-order depth in (0,1), keyed by
    /// entity_handle.value(). Higher = drawn on top. Built once per
    /// geometry epoch by ranking every entity within its owning block by
    /// effective sort key (SortEntitiesTable override or own handle), then
    /// fed to the 2D pipelines as a small clip-z bias so entities of
    /// *different* types order correctly against each other. 3D meshes are
    /// excluded (they keep real geometric depth).
    draw_depth_cache: RefCell<Option<(u64, Arc<HashMap<u64, f32>>)>>,
    /// Cached hatch fill models, keyed by geometry_epoch. View culling
    /// is handled at draw time via `hatch_skip_flags` in the pipeline,
    /// not at build time — that lets the GPU buffer stay stable across
    /// pan/zoom while still skipping out-of-view hatches.
    /// Keyed by `(geometry_epoch, selection_generation)` — selected hatches
    /// are tinted, so a select/deselect must rebuild even when the geometry
    /// is unchanged (issue #71).
    hatch_cache: RefCell<Option<(u64, u64, Arc<Vec<HatchModel>>)>>,
    /// Cached wipeout fill models, keyed by geometry_epoch. Same
    /// reasoning as `hatch_cache`.
    wipeout_cache: RefCell<Option<(u64, Arc<Vec<HatchModel>>)>>,
    /// Cached image models, keyed by geometry_epoch. Images do their own
    /// per-frame culling in the GPU pipeline (vp_scissor); no camera key
    /// needed here.
    image_cache: RefCell<Option<(u64, Arc<Vec<ImageModel>>)>>,
    /// Cached mesh models, keyed by geometry_epoch.
    mesh_cache: RefCell<Option<(u64, Arc<Vec<MeshLodSet>>)>>,
    /// Per-viewport wire cache for paper-space rendering.
    /// Maps vp_handle → (geometry_epoch, Arc<Vec<WireModel>>).
    viewport_wire_cache: RefCell<HashMap<Handle, ((u64, u32, u64), Arc<Vec<WireModel>>)>>,
    /// Cached tessellation of paper-space layout block entities (title block, annotations, etc.).
    /// Separate from `wire_cache` so the GPU sheet viewport doesn't re-tessellate every frame.
    /// Keyed by `(geometry_epoch, camera_generation)` — paper view changes
    /// on zoom too, so culled wire output depends on camera.
    paper_sheet_cache: RefCell<Option<((u64, u64), Arc<Vec<WireModel>>)>>,
    /// Per-viewport projected wire cache for paper-space content viewports.
    /// Stores projected + clipped wires in paper-space coordinates.
    /// Maps vp_handle → (geometry_epoch, Vec<WireModel>).
    paper_projected_cache: RefCell<HashMap<Handle, (u64, Vec<WireModel>)>>,
    /// Active layout name — "Model" or a paper space layout name.
    pub current_layout: String,
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
    /// Cached model-space bounding box, keyed by geometry_epoch.
    /// Avoids re-tessellating all entities on every ZOOM E / auto-fit call.
    model_extents_cache: RefCell<Option<(u64, Option<(glam::Vec3, glam::Vec3)>)>>,
    /// Reverse map: entity_handle → block_record_handle, built from entity_handles lists.
    /// Keyed by geometry_epoch. Eliminates the O(B) fallback scan in belongs_to_visible_block.
    entity_block_map_cache: RefCell<Option<(u64, HashMap<Handle, Handle>)>>,
    /// Tessellated block definitions in block-local coords, keyed by geometry_epoch.
    /// Lets Insert tessellation transform-copy cached wires instead of
    /// clone+explode+re-tessellate per reference.
    block_defn_cache: RefCell<Option<(u64, Arc<cache::block_cache::BlockCache>)>>,
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
    /// Content id ([`WIRE_CONTENT_GEN`]) of the Model-tile wire set returned by
    /// the most recent `model_tile_wires_arc` call — stamped on a miss, reused
    /// on a pan-hit. `build_primitive` reads it right after the call to gate
    /// GPU wire re-upload (Phase 3.2). 0 = none yet.
    pub(crate) last_model_wire_gen: std::cell::Cell<u64>,
    /// Monotonic per-build nonce for wire sources that must NOT be skipped by
    /// the Phase 3.2 upload gate — the paper / per-viewport wire paths and any
    /// frame carrying live preview / interim wires. High bit set so it can
    /// never collide with a real [`WIRE_CONTENT_GEN`] id; incremented every
    /// use so the GPU always sees a fresh id and re-uploads.
    pub(crate) wire_force_nonce: std::cell::Cell<u64>,
    /// Memoized `(face3d, other)` split of the Model-tile wire set, keyed by
    /// its [`WIRE_CONTENT_GEN`] id. `split_face3d_wires` is an O(N) per-wire
    /// handle lookup + clone that otherwise re-runs every frame; a pan that
    /// reuses the tessellation (same id) reuses this split too.
    #[allow(clippy::type_complexity)]
    split_cache:
        RefCell<Option<(u64, Arc<Vec<WireModel>>, Arc<Vec<WireModel>>)>>,
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
            selection: Rc::new(RefCell::new(SelectionState::default())),
            document: CadDocument::new(),
            selected: HashSet::default(),
            hidden: HashSet::default(),
            refedit_keep: None,
            hover_highlight: None,
            transparency_display: true,
            selection_filter: HashSet::default(),
            preview_wires: vec![],
            interim_wire: None,
            camera_generation: 0,
            geometry_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            block_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            selection_generation: 0,
            wire_cache: RefCell::new(None),
            model_tile_wire_cache: RefCell::new(HashMap::default()),
            sort_cache: RefCell::new(None),
            draw_depth_cache: RefCell::new(None),
            hatch_cache: RefCell::new(None),
            wipeout_cache: RefCell::new(None),
            image_cache: RefCell::new(None),
            mesh_cache: RefCell::new(None),
            viewport_wire_cache: RefCell::new(HashMap::default()),
            paper_sheet_cache: RefCell::new(None),
            paper_projected_cache: RefCell::new(HashMap::default()),
            current_layout: "Model".to_string(),
            viewcube_ucs: glam::Mat4::IDENTITY,
            hatches: HashMap::default(),
            meshes: HashMap::default(),
            block_meshes: HashMap::default(),
            solid_models: HashMap::default(),
            images: HashMap::default(),
            active_viewport: None,
            bg_color: [0.11, 0.11, 0.11, 1.0],
            paper_bg_color: [1.0, 1.0, 1.0, 1.0],
            local_extent_max: 1e9,
            local_center: [0.0, 0.0],
            annotation_scale: 1.0,
            model_extents_cache: RefCell::new(None),
            entity_block_map_cache: RefCell::new(None),
            block_defn_cache: RefCell::new(None),
            entity_index_cache: RefCell::new(None),
            last_render_aspect: std::cell::Cell::new(16.0 / 9.0),
            last_world_per_pixel: std::cell::Cell::new(0.0),
            viewcube_hover: std::cell::Cell::new(None),
            last_tess_ms: std::cell::Cell::new(0.0),
            last_tess_wires: std::cell::Cell::new(0),
            last_model_wire_gen: std::cell::Cell::new(0),
            wire_force_nonce: std::cell::Cell::new(0),
            split_cache: RefCell::new(None),
            tess_memo: RefCell::new(HashMap::default()),
            tess_memo_guard: std::cell::Cell::new(0),
        }
    }

    /// Compute the current camera's world-space XY view AABB with
    /// `world_offset` already subtracted (so the result is in the same f32
    /// space as emitted wire points). Adds a 25% margin around the
    /// frustum to absorb pan inertia and avoid clipped-edge popping.
    pub(super) fn view_world_aabb(&self) -> Option<[f32; 4]> {
        if self.current_layout != "Model" {
            // Paper-space viewport composition handles its own culling; the
            // top-level paper view is small enough not to need it.
            return None;
        }
        // Until the first explicit camera move (typically `fit_all()` after
        // file open), the camera sits at the default origin while geometry
        // lives at large local offsets — culling against the default rect
        // would discard everything and starve fit_all of points to fit to.
        if self.camera_generation == 0 {
            return None;
        }
        let cam = self.camera.borrow();
        let aspect = self.last_render_aspect.get().max(0.01);
        let h = cam.ortho_size();
        let w = h * aspect;
        let margin = 1.25_f32;
        // `cam.target` is in the same local f32 space as emitted wire points
        // (fit_to_bounds populates it from local wire coords). No further
        // `world_offset` subtraction is needed.
        let cx = cam.target.x as f32;
        let cy = cam.target.y as f32;
        Some([
            cx - w * margin,
            cy - h * margin,
            cx + w * margin,
            cy + h * margin,
        ])
    }

    /// Called by the render pipeline once per frame so `view_world_aabb` knows
    /// the active widget's aspect ratio.
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
    /// Matches the same skip already in `view_world_aabb`.
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
    pub(super) fn block_cache_arc(&self) -> Arc<cache::block_cache::BlockCache> {
        {
            let cache = self.block_defn_cache.borrow();
            if let Some((epoch, ref arc)) = *cache {
                if epoch == self.block_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if self.current_layout == "Model" {
            self.annotation_scale
        } else {
            1.0
        };
        let built = cache::block_cache::BlockCache::build(&self.document, anno, bg);
        let arc = Arc::new(built);
        *self.block_defn_cache.borrow_mut() = Some((self.block_epoch, Arc::clone(&arc)));
        arc
    }

    pub fn bump_geometry(&mut self) {
        self.geometry_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        // Default: also invalidate block definitions. Safe for every caller;
        // operations that know blocks are untouched use `bump_geometry_no_blocks`.
        self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        // Structural change — drop the whole per-entity tessellation memo.
        self.tess_memo.borrow_mut().clear();
    }

    /// Drop a single entity from the tessellation memo so the next render
    /// re-tessellates just that entity while reusing every other. Pair with
    /// [`bump_geometry_no_blocks`] for an incremental single-entity edit.
    pub fn mark_entity_dirty(&mut self, handle: Handle) {
        self.tess_memo.borrow_mut().remove(&handle);
    }

    /// Invalidate the visible-wire tessellation but KEEP the cached block
    /// definitions. Use only when the edit provably can't change any block
    /// defn (top-level entity create/edit, grip-moving an entity or insert) —
    /// it skips the all-blocks re-tessellation that otherwise spikes edit time.
    pub fn bump_geometry_no_blocks(&mut self) {
        self.geometry_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark the selection / hover-highlight set dirty without invalidating the
    /// (selection-independent) wire tessellation. Only the GPU xray overlay is
    /// rebuilt — no re-tessellation. Use this for pure select / deselect /
    /// hover changes; use [`bump_geometry`] when the geometry itself changed.
    pub fn bump_selection(&mut self) {
        self.selection_generation = self.selection_generation.wrapping_add(1);
    }


    /// Re-evaluate every cached mesh's color through `render_style` so a
    /// Register a Model-tab solid: cache its truck B-rep (for boolean ops) and
    /// tessellate it into the shaded mesh pipeline under `handle`. The solid is
    /// in the same offset-relative frame the mesh pipeline uses, so the mesh is
    /// stored as-is (Model-tab geometry is authored at world_offset 0).
    pub fn register_solid_model(&mut self, handle: Handle, solid: truck_modeling::Solid) {
        let color = self
            .document
            .get_entity(handle)
            .map(|e| self.render_style(e).0)
            .unwrap_or([0.8, 0.8, 0.85, 1.0]);
        if let Some(set) = crate::scene::model::solid_model::mesh_from_solid(&solid, color) {
            self.meshes.insert(handle, set);
        }
        self.solid_models.insert(handle, solid);
        self.bump_geometry();
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
        let colors: HashMap<Handle, [f32; 4]> = self
            .meshes
            .keys()
            .chain(self.block_meshes.keys())
            .filter_map(|&h| {
                self.document.get_entity(h).map(|e| {
                    let mut c = self.render_style(e).0;
                    if let Some(keep) = &self.refedit_keep {
                        if !keep.contains(&h) {
                            c = crate::scene::cache::block_cache::fade_toward_bg(c, bg);
                        }
                    }
                    (h, c)
                })
            })
            .collect();
        for (h, set) in self.meshes.iter_mut().chain(self.block_meshes.iter_mut()) {
            if let Some(&c) = colors.get(h) {
                for lod in &mut set.lods {
                    lod.color = c;
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
        self.bump_geometry();
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

    /// Switch the active layout. Bumps `geometry_epoch` so the wire cache
    /// re-tessellates — `render_style`'s `adapt_to_bg` picks the model or
    /// paper background depending on `current_layout`, so cached wires
    /// from the previous layout would be coloured against the wrong bg.
    /// Also runs `recolor_meshes` so ACIS mesh colour tracks the new bg.
    pub fn set_current_layout(&mut self, name: String) {
        if self.current_layout != name {
            self.current_layout = name;
            self.recolor_meshes();
            self.bump_geometry();
        }
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
        self.document.objects.values().find_map(|obj| {
            let ObjectType::Layout(layout) = obj else {
                return None;
            };
            if layout.name == self.current_layout {
                Some(layout.viewport)
            } else {
                None
            }
        }).unwrap_or(Handle::NULL)
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
        if let Ok(handle) =
            self.document
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
            pattern: crate::scene::model::hatch_model::HatchPattern::Solid,
            name: "SOLID".to_string(),
            color: self.paper_bg_color,
            angle_offset: 0.0,
            scale: 1.0,
            vp_scissor: None,
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
        let (left, bottom, right, top, rot) =
            self.document.objects.values().find_map(|obj| {
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
        // Dashed: 4 mm dash, 3 mm gap.
        wire.pattern_length = 7.0;
        wire.pattern = [4.0, -3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        Some(wire)
    }

    /// The effective plot settings for the current layout: a standalone
    /// PlotSettings page setup if one exists, otherwise the settings embedded in
    /// the LAYOUT object (paper size, margins, origin, rotation, scale). Loaded
    /// AutoCAD files keep their settings embedded, so without this fallback the
    /// plot/PDF path would ignore the file's rotation, origin and scale.
    pub fn effective_plot_settings(&self) -> Option<acadrust::objects::PlotSettings> {
        use acadrust::objects::{
            ObjectType, PaperMargin, PlotPaperUnits, PlotRotation, PlotSettings, PlotType,
            PlotWindow, ScaledType,
        };
        let name = &self.current_layout;
        if let Some(ps) = self.document.objects.values().find_map(|o| {
            if let ObjectType::PlotSettings(ps) = o {
                if &ps.page_name == name {
                    return Some(ps.clone());
                }
            }
            None
        }) {
            return Some(ps);
        }
        self.document.objects.values().find_map(|o| {
            let ObjectType::Layout(l) = o else { return None };
            if &l.name != name {
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

    pub fn paper_limits(&self) -> Option<((f64, f64), (f64, f64))> {
        if self.current_layout == "Model" {
            return None;
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

    /// Scale of the first user viewport (id > 1) in the current paper layout,
    /// used for the status-bar display.  Returns `None` in Model space or if
    /// no user viewport exists.
    pub fn first_viewport_scale(&self) -> Option<f64> {
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
                    return Some(vp_effective_scale(
                        vp.custom_scale,
                        vp.view_height,
                        vp.height,
                    ));
                }
            }
            None
        })
    }

    /// Annotation/viewport scales defined in the drawing's scale list
    /// (the `ACAD_SCALELIST` dictionary), as `(label, annotation_multiplier,
    /// viewport_factor)`. The annotation multiplier sizes model-space
    /// text/dims (50.0 for "1:50"); the viewport factor is the paper/drawing
    /// ratio (0.02 for "1:50"). Sorted smallest ratio first (1:100 … 1:1 …
    /// 10:1). Falls back to a standard ratio set when the drawing carries no
    /// scale list of its own, so the scale picker is always usable. (#154)
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
            const DEFAULT_SCALES: &[(&str, f64)] = &[
                ("1:500", 0.002),
                ("1:200", 0.005),
                ("1:100", 0.01),
                ("1:50", 0.02),
                ("1:20", 0.05),
                ("1:10", 0.1),
                ("1:5", 0.2),
                ("1:2", 0.5),
                ("1:1", 1.0),
                ("2:1", 2.0),
                ("5:1", 5.0),
                ("10:1", 10.0),
            ];
            list = DEFAULT_SCALES
                .iter()
                .map(|&(label, vp)| (label.to_string(), (1.0 / vp) as f32, vp))
                .collect();
        }
        list
    }

    /// List of user viewports in the current layout: (handle, label, frozen_layer_handles).
    pub fn viewport_list(&self) -> Vec<(acadrust::Handle, String, Vec<acadrust::Handle>)> {
        if self.current_layout == "Model" {
            return vec![];
        }
        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return vec![];
        }
        let mut result: Vec<(acadrust::Handle, String, Vec<acadrust::Handle>)> = self
            .document
            .entities()
            .filter_map(|e| {
                if let EntityType::Viewport(vp) = e {
                    if self.is_content_viewport_in_layout(vp, layout_block) {
                        Some((vp.common.handle, vp.id, vp.frozen_layers.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
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
        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return 0;
        }
        self.document
            .entities()
            .filter(|e| {
                if let EntityType::Viewport(vp) = e {
                    self.is_content_viewport_in_layout(vp, layout_block)
                } else {
                    false
                }
            })
            .count()
    }

    /// True when any entities are hidden by Isolate / Hide.
    pub fn is_isolation_active(&self) -> bool {
        !self.hidden.is_empty()
    }

    /// Set (or clear) the previewed entity that renders with the selection
    /// highlight without joining the real selection. Only refreshes the GPU
    /// xray overlay (no re-tessellation).
    pub fn set_hover_highlight(&mut self, handle: Option<Handle>) {
        if self.hover_highlight == handle {
            return;
        }
        // Hover is folded into the highlight set (selected ∪ {hover}) that
        // drives the xray overlay. A hover handle that's already selected
        // contributes nothing, so the effective set is unchanged — skip the
        // overlay refresh then. The field is still updated for hit-test / UI.
        let contribution = |h: Option<Handle>| h.filter(|h| !self.selected.contains(h));
        let changed = contribution(self.hover_highlight) != contribution(handle);
        self.hover_highlight = handle;
        if changed {
            self.bump_selection();
        }
    }

    /// Hide every drawable entity except the current selection (Isolate).
    pub fn isolate_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let keep = self.selected.clone();
        self.hidden = self
            .document
            .entities()
            .map(|e| e.common().handle)
            .filter(|h| !h.is_null() && !keep.contains(h))
            .collect();
        self.selected.clear();
        self.bump_geometry();
    }

    /// Hide the current selection (Hide Objects).
    pub fn hide_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        for h in self.selected.iter().copied() {
            self.hidden.insert(h);
        }
        self.selected.clear();
        self.bump_geometry();
    }

    /// Clear isolation — bring every hidden entity back (End Isolation).
    pub fn end_isolation(&mut self) {
        if self.hidden.is_empty() {
            return;
        }
        self.hidden.clear();
        self.bump_geometry();
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

    /// Set the scale of the active/selected viewport.
    /// Priority: active_viewport → first selected viewport → first viewport in layout.
    pub fn set_viewport_scale(&mut self, scale: f64) {
        let target =
            self.active_viewport
                .or_else(|| {
                    self.selected.iter().copied().find(|&h| {
                        matches!(self.document.get_entity(h), Some(EntityType::Viewport(_)))
                    })
                })
                .or_else(|| self.first_viewport_handle());

        if let Some(handle) = target {
            if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(handle) {
                if !vp.status.locked && scale > 1e-9 {
                    vp.custom_scale = scale;
                    vp.view_height = vp.height / scale;
                }
            }
            self.viewport_wire_cache.borrow_mut().remove(&handle);
            self.bump_geometry();
        }
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
            .into_iter()
            .filter_map(|wire| {
                let pts = wire.points;
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
                Some(pts.iter().map(|p| [p[0], p[1]]).collect())
            })
            .collect()
    }

    /// Per-tile cached tessellation for the Model layout. Each tile has
    /// its own camera (live for the active tile, stored snapshot for the
    /// others), so LOD / frustum culling has to run independently — the
    /// shared `entity_wires_arc` cache would cull every tile against
    /// whichever camera was current when it was last built.
    ///
    /// `cam_aspect` is the tile's pixel `width / height`; together with the
    /// camera's `ortho_size` it determines the world-XY rectangle culled
    /// against. Returns a clone of the cached `Arc` on a key match.
    pub(super) fn model_tile_wires_arc(
        &self,
        tile_idx: usize,
        cam: &Camera,
        cam_aspect: f32,
        tile_pixel_height: f32,
    ) -> Arc<Vec<WireModel>> {
        let wpp = if tile_pixel_height > 0.0 {
            Some((2.0 * cam.ortho_size()) / tile_pixel_height)
        } else {
            None
        };
        // Cache reuse is split from the exact-camera key: a pure pan keeps the
        // geometry epoch and the pan-invariant signature (rotation + tol) but
        // shifts the view. The cached wires were culled to a 1.25×-margin
        // region; if the new *visible* view still sits inside that region they
        // remain complete, so we can skip re-tessellation entirely — only the
        // GPU re-uploads (camera_generation still bumps). No margin widening,
        // so zoom (which changes the signature) costs exactly as before.
        let pan_sig = camera_pan_invariant_hash(cam, wpp);
        // Exact visible rect (margin 1.0) the reused wires must still cover.
        // A tilted view returns no XY box (cull disabled) → require the cached
        // region to be the full plane, so we only reuse a full tessellation.
        let full = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY];
        let need_region = if self.camera_generation == 0 {
            full
        } else {
            view_cull_aabb(cam, cam_aspect, 1.0).unwrap_or(full)
        };
        {
            let cache = self.model_tile_wire_cache.borrow();
            if let Some(((epoch, sig, region), gen, arc)) = cache.get(&tile_idx) {
                if *epoch == self.geometry_epoch
                    && *sig == pan_sig
                    && aabb_contains(*region, need_region)
                {
                    // Pan-hit: reuse the tessellation AND its content id, so
                    // the GPU wire upload is skipped too (Phase 3.2).
                    self.last_model_wire_gen.set(*gen);
                    return Arc::clone(arc);
                }
            }
        }
        // Miss → cull/tessellate to the 1.25×-margin region (unchanged cost)
        // and remember that region for the pan-reuse test above.
        // A tilted view yields no XY box → `None` = cull nothing (full tess),
        // which is correct, just heavier, for the less common 3D/elevation case.
        let tess_region = if self.camera_generation == 0 {
            None
        } else {
            view_cull_aabb(cam, cam_aspect, 1.25)
        };
        let block = self.model_space_block_handle();
        let t_tess = iced::time::Instant::now();
        let mut wires = self.wires_for_block_culled(block, tess_region, wpp, None, None);
        self.apply_refedit_fade(&mut wires, self.bg_color);
        let arc = Arc::new(wires);
        self.last_tess_ms.set(t_tess.elapsed().as_secs_f32() * 1000.0);
        self.last_tess_wires.set(arc.len());
        let stored_region = tess_region
            .unwrap_or([f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY]);
        // New tessellation → fresh content id; the GPU will re-upload.
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        self.model_tile_wire_cache.borrow_mut().insert(
            tile_idx,
            ((self.geometry_epoch, pan_sig, stored_region), gen, Arc::clone(&arc)),
        );
        arc
    }

    /// Cached tessellation of the current layout block's paper-space entities,
    /// shared by `entity_wires_arc()` and the GPU sheet viewport.
    fn paper_sheet_wires_arc(&self) -> Arc<Vec<WireModel>> {
        let key = (self.geometry_epoch, self.camera_generation);
        {
            let cache = self.paper_sheet_cache.borrow();
            if let Some((cached_key, ref arc)) = *cache {
                if cached_key == key {
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        let t_tess = iced::time::Instant::now();
        let mut wires = self.wires_for_block(layout_block);
        self.last_tess_ms.set(t_tess.elapsed().as_secs_f32() * 1000.0);
        self.last_tess_wires.set(wires.len());
        // The overall "sheet" viewport now IS the paper view itself, so its own
        // border rectangle must not be drawn as an entity on the sheet.
        let sheet = self.current_layout_sheet_viewport_handle();
        if sheet.is_valid() {
            let sheet_name = sheet.value().to_string();
            wires.retain(|w| w.name != sheet_name);
        }
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        self.apply_refedit_fade(&mut wires, bg);
        // Printable-area guide (paper inset by plot margins), paper space only.
        if let Some(pa) = self.printable_area_wire() {
            wires.push(pa);
        }
        let arc = Arc::new(wires);
        *self.paper_sheet_cache.borrow_mut() = Some((key, Arc::clone(&arc)));
        arc
    }

    /// Build WireModels from all document entities for the current layout.
    /// Returns a shared `Arc` so `build_primitive()` can skip the clone during
    /// navigation frames where no preview wires are active.
    pub(super) fn entity_wires_arc(&self) -> Arc<Vec<WireModel>> {
        let key = (self.geometry_epoch, self.camera_generation);
        {
            let cache = self.wire_cache.borrow();
            if let Some((cached_key, ref arc)) = *cache {
                if cached_key == key {
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        // Model space: paper_sheet_wires_arc IS the full entity wire set — share the Arc,
        // no Vec clone needed.
        if self.current_layout == "Model" {
            let arc = self.paper_sheet_wires_arc();
            *self.wire_cache.borrow_mut() = Some((key, Arc::clone(&arc)));
            return arc;
        }
        // Paper space: extend sheet wires with projected viewport content.
        let mut wires = (*self.paper_sheet_wires_arc()).clone();
        wires.extend(self.viewport_content_wires(layout_block, None, None));
        let arc = Arc::new(wires);
        *self.wire_cache.borrow_mut() = Some((key, Arc::clone(&arc)));
        arc
    }

    /// Build WireModels from all document entities + optional preview wire.
    pub fn entity_wires(&self) -> Vec<WireModel> {
        (*self.entity_wires_arc()).clone()
    }

    /// Per-entity normalized draw-order depth, keyed by entity handle value.
    /// Built (and cached per geometry epoch) by ranking every entity within
    /// its owning block by effective sort key (SortEntitiesTable override or
    /// own handle). The result feeds the 2D pipelines as a clip-z bias so
    /// entities of different types order correctly against each other.
    pub(super) fn draw_depth_map(&self) -> Arc<HashMap<u64, f32>> {
        {
            let cache = self.draw_depth_cache.borrow();
            if let Some((epoch, ref arc)) = *cache {
                if epoch == self.geometry_epoch {
                    return Arc::clone(arc);
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
                EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) | EntityType::Surface(_)
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
        let mut depth_map: HashMap<u64, f32> = HashMap::default();
        for (_block, mut v) in by_block {
            v.sort_by_key(|(_, eff)| *eff);
            let denom = (v.len() as f32) + 1.0;
            for (rank, (hv, _)) in v.into_iter().enumerate() {
                // Signed (-1,1): back ranks → negative, front → positive,
                // mid → ~0. The shader applies `z -= draw_depth * BIAS`, so a
                // default/unranked 0.0 means "no bias" (neutral) — which keeps
                // 3D mesh faces and transient wires at their real depth.
                let norm = (rank as f32 + 1.0) / denom; // (0,1)
                depth_map.insert(hv, (norm - 0.5) * 2.0);
            }
        }
        let arc = Arc::new(depth_map);
        *self.draw_depth_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    pub(super) fn hatch_models_arc(&self) -> Arc<Vec<HatchModel>> {
        // Hatch models bake the selection tint (issue #71), so they depend on
        // the *selected set* — but NOT on hover. Keying on `selection_generation`
        // (which also bumps on every hover) made each hover-over a new entity
        // rebuild every hatch model: an O(N-hatch) stutter on hatch-heavy
        // drawings. Key on a signature of `selected` instead, so hover (which
        // never changes `selected`) keeps the cache warm.
        let sel_sig = self.selected_set_sig();
        {
            let cache = self.hatch_cache.borrow();
            if let Some((cached_epoch, cached_sel, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch && cached_sel == sel_sig {
                    return Arc::clone(arc);
                }
            }
        }
        let arc = Arc::new(self.synced_hatch_models());
        *self.hatch_cache.borrow_mut() = Some((self.geometry_epoch, sel_sig, Arc::clone(&arc)));
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

    pub(super) fn wipeout_models_arc(&self) -> Arc<Vec<HatchModel>> {
        {
            let cache = self.wipeout_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let arc = Arc::new(self.wipeout_models());
        *self.wipeout_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    pub(super) fn images_arc(&self) -> Arc<Vec<ImageModel>> {
        {
            let cache = self.image_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let depth_map = self.draw_depth_map();
        let arc = Arc::new(
            self.images
                .iter()
                .map(|(handle, model)| {
                    let mut m = model.clone();
                    m.draw_depth = depth_map.get(&handle.value()).copied().unwrap_or(0.0);
                    m
                })
                .collect(),
        );
        *self.image_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Images owned by the active paper layout block only. The full-canvas
    /// sheet viewport uses this so model-block images don't bleed onto the
    /// paper sheet (mirrors `paper_canvas_hatches`).
    pub(super) fn paper_sheet_images(&self) -> Arc<Vec<ImageModel>> {
        let layout_block = self.current_layout_block_handle();
        let depth_map = self.draw_depth_map();
        Arc::new(
            self.images
                .iter()
                .filter_map(|(&handle, model)| {
                    let entity = self.document.get_entity(handle)?;
                    let c = entity.common();
                    if c.invisible
                        || !self.belongs_to_visible_block(handle, c.owner_handle, layout_block)
                    {
                        return None;
                    }
                    let mut m = model.clone();
                    m.draw_depth = depth_map.get(&handle.value()).copied().unwrap_or(0.0);
                    Some(m)
                })
                .collect(),
        )
    }

    pub(super) fn meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        {
            let cache = self.mesh_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        // Top-level solids: drop those whose layer is off/frozen or that are
        // flagged invisible / isolated-hidden, mirroring the 2D wire path.
        let mut all: Vec<MeshLodSet> = self
            .meshes
            .iter()
            .filter(|(&h, _)| self.mesh_entity_visible(h))
            .map(|(_, set)| set.clone())
            .collect();
        // Block-definition solids are instanced per model-space INSERT so a
        // block placed at an INSERT scale renders at the right size. (#123)
        all.extend(self.instanced_block_meshes(self.model_space_block_handle()));
        let arc = Arc::new(all);
        *self.mesh_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// True when `layer` is turned off or frozen — entities on it never render.
    fn layer_hidden(&self, layer: &str) -> bool {
        self.document
            .layers
            .get(layer)
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
    }

    /// Visibility test for a solid mesh entity, mirroring the 2D wire path:
    /// honour the invisible flag, the isolate/hide set, and the layer's
    /// off/frozen state.
    fn mesh_entity_visible(&self, handle: Handle) -> bool {
        let Some(c) = self.document.get_entity(handle).map(|e| e.common()) else {
            return false;
        };
        if c.invisible {
            return false;
        }
        if !self.hidden.is_empty() && self.hidden.contains(&handle) {
            return false;
        }
        !self.layer_hidden(&c.layer)
    }

    /// One transformed mesh per block-definition solid instance reached from an
    /// INSERT owned by `layout_block`. Nested INSERTs accumulate their
    /// transform. Empty when no block solids exist. (#123)
    fn instanced_block_meshes(&self, layout_block: Handle) -> Vec<MeshLodSet> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in self.document.entities() {
            if e.common().owner_handle != layout_block {
                continue;
            }
            if let EntityType::Insert(ins) = e {
                // INSERT on an off/frozen (or invisible) layer hides the whole
                // instance, block-internal solids included.
                if !self.mesh_entity_visible(ins.common.handle) {
                    continue;
                }
                let start = out.len();
                self.expand_block_meshes(&ins.block_name, &ins.get_transform(), 0, &mut out);
                // Tag the instanced meshes with the parent INSERT handle so the
                // hover / selection highlight (keyed on the mesh name) tints the
                // block, not the inner solid's own handle which nothing selects.
                let name = ins.common.handle.value().to_string();
                for set in &mut out[start..] {
                    for m in &mut set.lods {
                        m.name = name.clone();
                    }
                }
            }
        }
        out
    }

    /// Recursively emit transformed instances of a block's solid meshes,
    /// composing nested-INSERT transforms. (#123)
    fn expand_block_meshes(
        &self,
        block_name: &str,
        accum: &acadrust::types::Transform,
        depth: usize,
        out: &mut Vec<MeshLodSet>,
    ) {
        if depth > 16 {
            return;
        }
        let Some(br) = self.document.block_records.get(block_name) else {
            return;
        };
        let handles: Vec<Handle> = br.entity_handles.clone();
        for h in handles {
            let Some(e) = self.document.get_entity(h) else {
                continue;
            };
            // A block-internal solid / nested INSERT on an off/frozen layer
            // (or flagged invisible) must not render, same as a top-level one.
            if !self.mesh_entity_visible(h) {
                continue;
            }
            if let EntityType::Insert(ins) = e {
                let composed = ins.get_transform().then(accum);
                self.expand_block_meshes(&ins.block_name, &composed, depth + 1, out);
            } else if let Some(set) = self.block_meshes.get(&h) {
                out.push(transform_block_mesh_lod_set(set, accum));
            }
        }
    }

    /// Hatches eligible for click / box / lasso hit-testing in the current
    /// layout. Filters out block-internal source hatches (stored in
    /// `self.hatches` at block-local coords for the block-defn position,
    /// which doesn't project correctly through the offset-rel view_proj
    /// and was causing the wrong hatch to be selected on click).
    pub fn visible_hatches_for_click(&self) -> HashMap<Handle, HatchModel> {
        let layout_block = self.current_layout_block_handle();
        let model_block = self.model_space_block_handle();
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };
        self.hatches
            .iter()
            .filter_map(|(&h, m)| {
                let c = self.document.get_entity(h)?.common();
                if c.invisible || layer_hidden(&c.layer) {
                    return None;
                }
                // Mirror `synced_hatch_models`' visibility test (which drives
                // the fill render) so anything drawn is also clickable on its
                // fill, not just its boundary wire. The model-space fallback
                // matters when the layout block handle differs from the
                // entity's owner (issue: hatch fill not selectable).
                if self.belongs_to_visible_block(h, c.owner_handle, layout_block)
                    || self.belongs_to_visible_block(h, c.owner_handle, model_block)
                {
                    Some((h, m.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Per-Insert hatch models in the current layout, keyed by the Insert
    /// handle so a click on a block-internal hatch can select the parent
    /// Insert (AutoCAD behaviour: sub-entities of a block aren't directly
    /// selectable; the click resolves to the Insert).
    pub fn insert_hatches_for_click(&self) -> Vec<(Handle, HatchModel)> {
        let layout_block = self.current_layout_block_handle();
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };
        let mut out: Vec<(Handle, HatchModel)> = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Insert(ins) = entity else {
                continue;
            };
            if ins.common.invisible || layer_hidden(&ins.common.layer) {
                continue;
            }
            if !self.belongs_to_visible_block(
                ins.common.handle,
                ins.common.owner_handle,
                layout_block,
            ) {
                continue;
            }
            for sub in ins
                .explode_from_document(&self.document)
                .into_iter()
                .map(crate::modules::draw::modify::explode::normalize_insert_entity)
            {
                let EntityType::Hatch(dxf) = sub else {
                    continue;
                };
                if dxf.common.invisible || layer_hidden(&dxf.common.layer) {
                    continue;
                }
                let color = self.render_style(&EntityType::Hatch(dxf.clone())).0;
                if let Some(model) = Self::hatch_model_from_dxf(&dxf, color) {
                    out.push((ins.common.handle, model));
                }
            }
        }
        out
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
            // entity_wires_arc is culled to the current view and keyed on the
            // camera, so it re-culls when the view changes — picking must reach
            // entities that scroll into view after a pan/zoom.
            return self.entity_wires_arc();
        }
        let layout_block = self.current_layout_block_handle();
        match self.active_viewport {
            None => Arc::new(self.wires_for_block(layout_block)),
            Some(vp_handle) => {
                Arc::new(self.viewport_content_wires(layout_block, Some(vp_handle), None))
            }
        }
    }

    /// Pick a meshed 3D solid by clicking on its shaded body (face), not just
    /// its thin projected edges. Returns the front-most mesh under `cursor`.
    #[allow(dead_code)]
    pub fn mesh_click_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<Handle> {
        let iter = self
            .meshes
            .iter()
            .filter_map(|(h, set)| set.lods.first().map(|m| (*h, m)));
        pick::hit_test::mesh_click_hit(cursor, iter, view_rot, eye, bounds)
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
    ) -> Vec<Handle> {
        let iter = self
            .meshes
            .iter()
            .filter_map(|(h, set)| set.lods.first().map(|m| (*h, m)));
        pick::hit_test::mesh_box_hit(a, b, crossing, iter, view_rot, eye, bounds)
    }

    /// Top-level solid handles caught by a lasso polygon.
    pub fn mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Vec<Handle> {
        let iter = self
            .meshes
            .iter()
            .filter_map(|(h, set)| set.lods.first().map(|m| (*h, m)));
        pick::hit_test::mesh_poly_hit(poly, crossing, iter, view_rot, eye, bounds)
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
    ) -> Option<Handle> {
        // Block-internal instances must be owned (transformed copies); keep
        // them in a Vec and chain references alongside the top-level meshes so
        // neither set is cloned wholesale.
        let mut block_owned: Vec<(Handle, crate::scene::model::mesh_model::MeshModel)> = Vec::new();
        if !self.block_meshes.is_empty() {
            let layout_block = self.current_layout_block_handle();
            for e in self.document.entities() {
                if e.common().owner_handle != layout_block {
                    continue;
                }
                let EntityType::Insert(ins) = e else { continue };
                if !self.mesh_entity_visible(ins.common.handle) {
                    continue;
                }
                let mut sets = Vec::new();
                self.expand_block_meshes(&ins.block_name, &ins.get_transform(), 0, &mut sets);
                for set in sets {
                    if let Some(m) = set.lods.into_iter().next() {
                        block_owned.push((ins.common.handle, m));
                    }
                }
            }
        }
        let top = self
            .meshes
            .iter()
            .filter_map(|(h, set)| set.lods.first().map(|m| (*h, m)));
        let blk = block_owned.iter().map(|(h, m)| (*h, m));
        pick::hit_test::mesh_click_hit(cursor, top.chain(blk), view_rot, eye, bounds)
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
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let layout_block = self.current_layout_block_handle();
        let mut out = Vec::new();
        for e in self.document.entities() {
            if e.common().owner_handle != layout_block {
                continue;
            }
            let EntityType::Insert(ins) = e else { continue };
            if !self.mesh_entity_visible(ins.common.handle) {
                continue;
            }
            let mut sets = Vec::new();
            self.expand_block_meshes(&ins.block_name, &ins.get_transform(), 0, &mut sets);
            let hit = sets.iter().any(|set| {
                set.lods.first().map_or(false, |m| {
                    !pick::hit_test::mesh_box_hit(
                        a,
                        b,
                        crossing,
                        std::iter::once((ins.common.handle, m)),
                        view_rot,
                        eye,
                        bounds,
                    )
                    .is_empty()
                })
            });
            if hit {
                out.push(ins.common.handle);
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
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let layout_block = self.current_layout_block_handle();
        let mut out = Vec::new();
        for e in self.document.entities() {
            if e.common().owner_handle != layout_block {
                continue;
            }
            let EntityType::Insert(ins) = e else { continue };
            if !self.mesh_entity_visible(ins.common.handle) {
                continue;
            }
            let mut sets = Vec::new();
            self.expand_block_meshes(&ins.block_name, &ins.get_transform(), 0, &mut sets);
            let hit = sets.iter().any(|set| {
                set.lods.first().map_or(false, |m| {
                    !pick::hit_test::mesh_poly_hit(
                        poly,
                        crossing,
                        std::iter::once((ins.common.handle, m)),
                        view_rot,
                        eye,
                        bounds,
                    )
                    .is_empty()
                })
            });
            if hit {
                out.push(ins.common.handle);
            }
        }
        out
    }

    /// Tessellate all non-invisible entities owned by `block_handle`.
    fn wires_for_block(&self, block_handle: Handle) -> Vec<WireModel> {
        // Default culling is driven by the live `Scene::camera`. Multi-tile
        // Model layouts and paper-space content viewports call
        // `wires_for_block_culled` directly with their own per-view cull
        // parameters so each pane culls independently.
        self.wires_for_block_culled(
            block_handle,
            self.view_world_aabb(),
            self.world_per_pixel(),
            None,
            None,
        )
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

        // Visibility test reused by both paths below.
        let visibility_ok = |e: &EntityType| -> bool {
            let c = e.common();
            if c.invisible {
                return false;
            }
            // Isolate / Hide: skip entities the user has hidden.
            if !self.hidden.is_empty() && self.hidden.contains(&c.handle) {
                return false;
            }
            // Block/BlockEnd are block-defn sentinels, not drawable geometry.
            // Without this skip they fall through to fallback_geometry's `_`
            // arm and emit a 1-unit phantom segment at world_offset that
            // poisons fit_all and shows up in selection.
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                return false;
            }
            let layer = self.document.layers.get(&c.layer);
            if layer.map(|l| l.flags.off || l.flags.frozen).unwrap_or(false) {
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
            self.belongs_to_visible_block(e.common().handle, c.owner_handle, block_handle)
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
        let avp = self.active_viewport;
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
        let blk_cache = self.block_cache_arc();
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
        // Per-entity tessellation memo (Phase 2.2) — only on the culled Model
        // render path. A single-entity edit re-tessellates just the changed
        // entity (dropped from the memo via `mark_entity_dirty`) and reuses the
        // rest, instead of re-running every visible entity. The hit-test path
        // (`view_aabb == None`) and paper / per-viewport paths bypass it so
        // their different cull parameters don't thrash the memo.
        let memo_active = view_aabb.is_some()
            && is_model_block
            && frozen_layers.is_none()
            && anno_scale_override.is_none();
        let mut wires: Vec<WireModel> = if memo_active {
            // Guard hash of everything tessellate_entity output depends on
            // besides the entity itself. A mismatch (zoom/tol, view, anno,
            // offset, bg, entered viewport) means the memo is stale.
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
                for c in bg {
                    mix(c.to_bits() as u64);
                }
                mix(avp.map(|h| h.value()).unwrap_or(0));
                g
            };
            if self.tess_memo_guard.get() != guard {
                self.tess_memo.borrow_mut().clear();
                self.tess_memo_guard.set(guard);
            }
            // Classify (serial, cheap): reuse memoized Arcs, collect misses.
            let mut hit_arcs: Vec<Arc<Vec<WireModel>>> = Vec::new();
            let mut misses: Vec<&EntityType> = Vec::new();
            {
                let memo = self.tess_memo.borrow();
                for e in &visible {
                    let h = e.common().handle;
                    match memo.get(&h) {
                        Some(a) => hit_arcs.push(Arc::clone(a)),
                        None => misses.push(*e),
                    }
                }
            }
            // Materialize hits + tessellate misses, both in parallel.
            let hit_wires: Vec<WireModel> =
                hit_arcs.par_iter().flat_map_iter(|a| a.iter().cloned()).collect();
            let miss_pairs: Vec<(Handle, Arc<Vec<WireModel>>)> = misses
                .par_iter()
                .map(|e| {
                    let e: &EntityType = e;
                    let w = tessellate_entity(
                        doc, sel, avp, bg, anno, e, Some(blk_ref), view_aabb, wpp,
                    );
                    (e.common().handle, Arc::new(w))
                })
                .collect();
            let mut out = hit_wires;
            {
                let mut memo = self.tess_memo.borrow_mut();
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
                        doc, sel, avp, bg, anno, e, Some(blk_ref), view_aabb, wpp,
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
        std::cell::Ref::map(self.entity_index_cache.borrow(), |c| {
            &c.as_ref().unwrap().1
        })
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
        } else {
            1.0
        };
        let blk_cache = self.block_cache_arc();
        // tessellate_one is used for one-off lookups (hit test, properties).
        // Skip culling here so the caller always gets the full geometry.
        tessellate_entity(
            &self.document,
            &self.selected,
            self.active_viewport,
            bg,
            anno,
            e,
            Some(&blk_cache),
            None,
            None,
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
            if let Some(((epoch, _cam_gen), ref arc)) = *cache {
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
                    // XY AABBs in too — otherwise ZOOM EXTENTS ignores them.
                    for set in self.meshes.values() {
                        let [ax, ay, bx, by] = set.world_aabb;
                        let lo = glam::Vec3::new(ax, ay, 0.0);
                        let hi = glam::Vec3::new(bx, by, 0.0);
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
            if c.owner_handle != model_block || c.invisible {
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
        for set in self.meshes.values() {
            let [ax, ay, bx, by] = set.world_aabb;
            let lo = glam::Vec3::new(ax, ay, 0.0);
            let hi = glam::Vec3::new(bx, by, 0.0);
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

    /// Collect model-space wires projected into paper space for all (or one specific)
    /// user viewports.  `only_vp = Some(h)` restricts output to that viewport.
    fn viewport_content_wires(
        &self,
        paper_block: Handle,
        only_vp: Option<Handle>,
        exclude_vp: Option<Handle>,
    ) -> Vec<WireModel> {
        use acadrust::entities::Viewport;

        let viewports: Vec<&Viewport> = self
            .document
            .entities()
            .filter_map(|e| {
                if let EntityType::Viewport(vp) = e {
                    Some(vp)
                } else {
                    None
                }
            })
            .filter(|vp| {
                self.is_content_viewport_in_layout(vp, paper_block)
                    && vp.status.is_on
                    && only_vp.map_or(true, |h| vp.common.handle == h)
                    && exclude_vp.map_or(true, |h| vp.common.handle != h)
            })
            .collect();

        if viewports.is_empty() {
            return vec![];
        }

        let mut result = Vec::new();

        for vp in viewports {
            let vp_handle = vp.common.handle;

            // ── Fast path: return cached projected wires ──────────────────
            {
                let cache = self.paper_projected_cache.borrow();
                if let Some((cached_epoch, ref wires)) = cache.get(&vp_handle) {
                    if *cached_epoch == self.geometry_epoch {
                        result.extend_from_slice(wires);
                        continue;
                    }
                }
            }

            // ── Cache miss: compute projection ────────────────────────────

            // Use camera_for_viewport so the axes match the GPU renderer exactly.
            let cam_frame = match self.camera_for_viewport(vp_handle) {
                Some(c) => c,
                None => continue,
            };
            let view_right = cam_frame.rotation * glam::Vec3::X;
            let view_up = cam_frame.rotation * glam::Vec3::Y;

            // Scale (paper units per model unit) comes straight from the camera
            // the GPU uses: the model height shown is `2 * ortho_size`, mapped
            // onto `vp.height` of paper. `camera_for_viewport` already made the
            // saved-view-vs-auto-fit decision (with the twist-correct overlap
            // test), so deriving scale from it keeps the CPU projection (used
            // for hit-test / snap / fit) locked to the GPU render — no second,
            // independently-computed scale that could disagree under a twist.
            let view_height_eff = (cam_frame.ortho_size() * 2.0) as f64;
            let scale = if view_height_eff > 1e-9 {
                (vp.height / view_height_eff) as f32
            } else {
                1.0
            };

            let pcx = vp.center.x as f32;
            let pcy = vp.center.y as f32;
            let pcz = vp.center.z as f32;
            let hw = (vp.width / 2.0) as f32;
            let hh = (vp.height / 2.0) as f32;

            // ── Use cached tessellation (model_wires_for_viewport_arc) ────
            // This eliminates the per-frame tessellate_one() loop that was here
            // previously; tessellation is now O(1) on navigation frames.
            // Pass 0.0 for screen height — the CPU-projection / hit-test
            // path wants the full-fidelity (no-LOD-stub) wire list,
            // regardless of paper zoom.
            let model_wires = self.model_wires_for_viewport_arc(vp_handle, 0.0);

            // ── Project and clip wires into viewport ──────────────────────
            let vp_x0 = pcx - hw;
            let vp_x1 = pcx + hw;
            let vp_y0 = pcy - hh;
            let vp_y1 = pcy + hh;

            // camera_dist: how far the camera is from the target plane.
            let use_perspective = vp.status.perspective && vp.lens_length > 1.0;
            let camera_dist = if use_perspective {
                (vp.view_height as f32 * vp.lens_length as f32 / 24.0).max(0.001)
            } else {
                0.0
            };

            let mut projected: Vec<WireModel> = Vec::new();

            // Precompute precision-stable WCS-space projection inputs in
            // f64. The previous f32 inner loop suffered catastrophic
            // cancellation on UTM-scale drawings: `(wire_offset_rel -
            // target_offset_rel).dot(view_right) - view_center` is a
            // small paper offset computed by subtracting two values at
            // ~5e6 magnitude — f32 ULP there is ~0.5 m, so paper output
            // jittered by cm even when the actual model was clean.
            //
            // Do everything WCS-relative in f64; cast to f32 only at the
            // final paper position.
            // Display centre = the camera's target, in WCS. `camera_for_viewport`
            // already folded view_center through the (twisted) view basis and
            // applied the empty-WCS auto-fit, so taking its target keeps the CPU
            // projection identical to the GPU renderer under any twist.
            let display_center_x = cam_frame.target.x as f64 + [0.0_f64; 3][0];
            let display_center_y = cam_frame.target.y as f64 + [0.0_f64; 3][1];
            let display_center_z = cam_frame.target.z as f64 + [0.0_f64; 3][2];
            let view_right_d = (
                view_right.x as f64,
                view_right.y as f64,
                view_right.z as f64,
            );
            let view_up_d = (view_up.x as f64, view_up.y as f64, view_up.z as f64);
            let view_fwd = cam_frame.rotation * glam::Vec3::Z;
            let view_fwd_d = (view_fwd.x as f64, view_fwd.y as f64, view_fwd.z as f64);
            let camera_dist_d = camera_dist as f64;
            let scale_d = scale as f64;
            let pcx_d = pcx as f64;
            let pcy_d = pcy as f64;
            // Project one ABSOLUTE-WCS model point (f64) onto the paper sheet.
            // Shared by the polyline points, snap points and key vertices so the
            // hit-test / snap geometry lands in the same paper frame the wire is
            // drawn in — otherwise snaps and the click-AABB stay in model (UTM)
            // space and the cursor never reaches them.
            let proj_abs = |ax: f64, ay: f64, az: f64| -> [f32; 3] {
                let mp_x = ax - display_center_x;
                let mp_y = ay - display_center_y;
                let mp_z = az - display_center_z;
                let u = mp_x * view_right_d.0 + mp_y * view_right_d.1 + mp_z * view_right_d.2;
                let v = mp_x * view_up_d.0 + mp_y * view_up_d.1 + mp_z * view_up_d.2;
                if use_perspective {
                    let d_vd = mp_x * view_fwd_d.0 + mp_y * view_fwd_d.1 + mp_z * view_fwd_d.2;
                    let fwd = camera_dist_d - d_vd;
                    if fwd <= 0.001 {
                        return [f32::NAN; 3];
                    }
                    let factor = camera_dist_d / fwd;
                    [
                        (pcx_d + u * factor * scale_d) as f32,
                        (pcy_d + v * factor * scale_d) as f32,
                        pcz,
                    ]
                } else {
                    [(pcx_d + u * scale_d) as f32, (pcy_d + v * scale_d) as f32, pcz]
                }
            };
            let in_vp = |x: f32, y: f32| x >= vp_x0 && x <= vp_x1 && y >= vp_y0 && y <= vp_y1;

            for wire in model_wires.iter() {
                let projected_pts: Vec<[f32; 3]> = wire
                    .points
                    .iter()
                    .enumerate()
                    .map(|(pi, &[mx, my, mz])| {
                        if mx.is_nan() || my.is_nan() || mz.is_nan() {
                            return [f32::NAN; 3];
                        }
                        // Reconstruct absolute WCS from the double-single high
                        // (`points`) + low (`points_low`) pair — the high f32
                        // alone is ~0.5 m off at UTM scale.
                        let lo = wire.points_low.get(pi).copied().unwrap_or([0.0; 3]);
                        proj_abs(mx as f64 + lo[0] as f64, my as f64 + lo[1] as f64, mz as f64 + lo[2] as f64)
                    })
                    .collect();

                // Fast AABB pre-reject.
                let any_near = projected_pts.iter().any(|&[x, y, _]| {
                    x.is_finite()
                        && y.is_finite()
                        && x >= vp_x0 - 1.0
                        && x <= vp_x1 + 1.0
                        && y >= vp_y0 - 1.0
                        && y <= vp_y1 + 1.0
                });
                let (min_x, max_x, min_y, max_y) =
                    projected_pts.iter().filter(|p| p[0].is_finite()).fold(
                        (
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                        ),
                        |(mnx, mxx, mny, mxy), &[x, y, _]| {
                            (mnx.min(x), mxx.max(x), mny.min(y), mxy.max(y))
                        },
                    );
                let aabb_hits =
                    max_x >= vp_x0 && min_x <= vp_x1 && max_y >= vp_y0 && min_y <= vp_y1;
                if !any_near && !aabb_hits {
                    continue;
                }

                let clipped =
                    clip_polyline_to_rect(&projected_pts, vp_x0, vp_y0, vp_x1, vp_y1, pcz);
                if clipped.is_empty() {
                    continue;
                }

                // Paper-space AABB of the clipped polyline — the cloned model
                // (UTM) AABB would make click_hit's screen-projected pre-reject
                // discard the wire (box selection has no pre-reject, which is why
                // it kept working while picking didn't).
                let mut pmnx = f32::INFINITY;
                let mut pmny = f32::INFINITY;
                let mut pmxx = f32::NEG_INFINITY;
                let mut pmxy = f32::NEG_INFINITY;
                for &[x, y, _] in clipped.iter().filter(|p| p[0].is_finite()) {
                    pmnx = pmnx.min(x);
                    pmny = pmny.min(y);
                    pmxx = pmxx.max(x);
                    pmxy = pmxy.max(y);
                }

                // Project snap points + key vertices into the same paper frame,
                // keeping only those inside the viewport rect, so endpoint /
                // midpoint / centre snaps land on the visible sheet geometry
                // instead of the model's UTM coordinates.
                let snap_pts: Vec<(glam::DVec3, model::wire_model::SnapHint)> = wire
                    .snap_pts
                    .iter()
                    .filter_map(|(w, h)| {
                        let p = proj_abs(w.x, w.y, w.z);
                        (p[0].is_finite() && in_vp(p[0], p[1]))
                            .then(|| (glam::DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64), *h))
                    })
                    .collect();
                let key_vertices: Vec<[f64; 3]> = wire
                    .key_vertices
                    .iter()
                    .filter_map(|&[kx, ky, kz]| {
                        let p = proj_abs(kx, ky, kz);
                        (p[0].is_finite() && in_vp(p[0], p[1]))
                            .then(|| [p[0] as f64, p[1] as f64, p[2] as f64])
                    })
                    .collect();

                let adapted = view::render::adapt_to_bg(wire.color, self.paper_bg_color);
                let [r, g, b, a] = adapted;
                let mut out = wire.clone();
                out.points = clipped;
                // Paper coordinates are small sheet units — no relative-to-eye
                // residual is needed, and keeping the model wire's points_low
                // here would add a model-scale offset to the paper points.
                out.points_low = Vec::new();
                out.snap_pts = snap_pts;
                out.key_vertices = key_vertices;
                // Tangent geometry is in model space and can't be trivially
                // re-expressed in paper coords — drop it (no tangent snap on
                // projected viewport content) rather than snap to UTM.
                out.tangent_geoms = Vec::new();
                out.aabb = if pmnx.is_finite() {
                    [pmnx, pmny, pmxx, pmxy]
                } else {
                    WireModel::UNBOUNDED_AABB
                };
                out.color = [r * 0.80, g * 0.80, b * 0.80, a * 0.85];
                out.line_weight_px = wire.line_weight_px;
                // Wire's pattern was sized for model-space coords during
                // tessellation; we just projected points into paper coords
                // (× scale), so rescale the dash pattern by the same factor
                // to keep dimensional consistency in the GPU shader.
                out.pattern_length = wire.pattern_length * scale;
                out.pattern = wire.pattern.map(|v| v * scale);
                out.vp_scissor = Some([vp_x0, vp_y0, vp_x1, vp_y1]);
                projected.push(out);
            }

            // Store in cache, then extend result.
            self.paper_projected_cache
                .borrow_mut()
                .insert(vp_handle, (self.geometry_epoch, projected.clone()));
            result.extend(projected);
        }

        result
    }

    // ── MSPACE helpers ───────────────────────────────────────────────────

    /// Convert a **paper-space** world coordinate to **model-space** using the
    /// geometry of the currently active viewport.  Returns the input unchanged
    /// when there is no active viewport.
    /// Convert a paper-space point to model space (precise at UTM scale).
    pub fn paper_to_model(&self, paper_pt: glam::DVec3) -> glam::DVec3 {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return paper_pt,
        };
        let vp = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return paper_pt,
        };
        // Uses the viewport's own `view_target` — kept valid by
        // `normalize_active_viewport_view` on entry, which folds a stale UTM
        // saved view onto the auto-fit centre so the display, pan/zoom and this
        // inverse all agree. Cheap (no per-call camera rebuild).
        let scale = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        if scale.abs() < 1e-9 {
            return paper_pt;
        }
        let tx = vp.view_target.x;
        let ty = vp.view_target.y;
        let pcx = vp.center.x;
        let pcy = vp.center.y;
        glam::DVec3::new(
            (paper_pt.x - pcx) / scale + tx,
            (paper_pt.y - pcy) / scale + ty,
            paper_pt.z,
        )
    }

    /// Inverse of [`paper_to_model`]: map a model-space point to the paper
    /// sheet through the active viewport. Returns the input unchanged when
    /// there is no active viewport. Kept as the inverse companion to
    /// `paper_to_model`; in-viewport overlays now project via the viewport
    /// camera ([`viewport_edit_frame`]) rather than mapping onto the sheet.
    #[allow(dead_code)]
    pub fn model_to_paper(&self, model_pt: glam::DVec3) -> glam::DVec3 {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return model_pt,
        };
        let vp = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return model_pt,
        };
        let scale = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        glam::DVec3::new(
            (model_pt.x - vp.view_target.x) * scale + vp.center.x,
            (model_pt.y - vp.view_target.y) * scale + vp.center.y,
            model_pt.z,
        )
    }

    /// In-viewport (MSPACE) editing frame: the active floating viewport's own
    /// camera — *exactly* the one the GPU renders its content with
    /// ([`camera_for_viewport`]) — together with the viewport's full screen
    /// rectangle in canvas pixels ([`viewport_screen_rect`]).
    ///
    /// This is the unified editing adapter (the "süzgeç"). Inside a viewport,
    /// editing IS model-space: treat the returned camera as *the* camera, the
    /// returned rect as *the* pane, and the cursor relative to that rect — then
    /// the existing model-space snap / hit-test / grip / preview / plane-pick
    /// code runs unchanged and lands on the same pixels the GPU draws. Results
    /// come back as model coordinates directly (no paper round-trip).
    ///
    /// Because the camera is the real GPU camera, this tracks the viewport's
    /// pan / zoom / twist / oblique view correctly — unlike a linear
    /// paper-projection, whose auto-fit / saved-view / crop divergence left the
    /// snap stale after pan/zoom. Returns `None` when not editing inside a
    /// floating viewport, or the camera / rect cannot be derived.
    pub fn viewport_edit_frame(
        &self,
        canvas_px: (f32, f32),
    ) -> Option<(view::camera::Camera, iced::Rectangle)> {
        let vp_handle = self.active_viewport?;
        let cam = self.camera_for_viewport(vp_handle)?;
        let full = self.viewport_screen_rect(vp_handle, canvas_px)?;
        Some((cam, full))
    }

    /// Fold the active viewport's saved view onto the effective camera (the
    /// auto-fit centre for stale UTM views) and persist it into `view_target` /
    /// `view_height`. Called on entering MSPACE so pan/zoom, paper↔model and the
    /// rendered content all share one valid view — otherwise a stale `(0,0,0)`
    /// target left the camera auto-fitting to the model centre while the cursor
    /// math used the origin, and pan toggled the two (jitter).
    pub fn normalize_active_viewport_view(&mut self) {
        let Some(vp_handle) = self.active_viewport else {
            return;
        };
        let Some(cam) = self.camera_for_viewport(vp_handle) else {
            return;
        };
        let eff_h = cam.ortho_size() as f64 * 2.0;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            vp.view_target.x = cam.target.x;
            vp.view_target.y = cam.target.y;
            vp.view_center.x = 0.0;
            vp.view_center.y = 0.0;
            if eff_h > 1e-9 {
                vp.view_height = eff_h;
            }
        }
    }

    /// Pan the active viewport's model-space view by `(screen_dx, screen_dy)` pixels.
    /// The delta is converted to model-space units using the camera and viewport scale.
    /// No-op when there is no active viewport.
    pub fn pan_active_viewport(&mut self, screen_dx: f32, screen_dy: f32, bounds: iced::Rectangle) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        // Use the viewport's own camera for the pan axes (matches 3-D view orientation).
        let vp_cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return,
        };

        // Read viewport dims (immutable borrow ends here).
        let (view_height, vp_height, locked) = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => {
                (vp.view_height as f32, vp.height as f32, vp.status.locked)
            }
            _ => return,
        };
        if locked {
            return;
        }

        // Correct pan speed: how many model units correspond to one screen pixel.
        //
        // The paper camera's ortho_size() gives the visible paper-space half-height
        // (in paper mm). One screen pixel = 2*half_h / canvas_height paper mm.
        // Inside the viewport, one paper mm = view_height / vp_height model units.
        // Together: model_per_pixel = (2*half_h / canvas_height) * (view_height / vp_height)
        let paper_half_h = self.camera.borrow().ortho_size();
        let speed = if bounds.height > 0.0 && paper_half_h > 1e-6 && vp_height > 1e-6 {
            (2.0 * paper_half_h / bounds.height) * (view_height / vp_height)
        } else {
            vp_cam.distance * 0.001
        };

        let cam_right = vp_cam.rotation * glam::Vec3::X;
        let cam_up = vp_cam.rotation * glam::Vec3::Y;
        let model_delta = -(cam_right * screen_dx * speed) + (cam_up * screen_dy * speed);

        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            vp.view_target.x += model_delta.x as f64;
            vp.view_target.y += model_delta.y as f64;
            vp.view_target.z += model_delta.z as f64;
        }
    }

    /// Zoom the active viewport's model-space view by `steps` notches.
    /// Positive = zoom in (increase detail), negative = zoom out.
    /// `cursor_paper`: optional paper-space XY of the cursor; when supplied the
    /// model point under the cursor is kept stationary (AutoCAD-style zoom).
    /// No-op when there is no active viewport.
    pub fn zoom_active_viewport(&mut self, steps: f32, cursor_paper: Option<glam::Vec2>) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            // Zoom in = shrink view_height → higher scale → objects appear larger.
            let factor = (1.0_f64 - 0.15 * steps as f64).clamp(0.1, 10.0);

            if let Some(cp) = cursor_paper {
                // Compute the model-space point under the cursor before zoom.
                let scale_before =
                    vp_effective_scale(vp.custom_scale, vp.view_height, vp.height) as f32;
                let cx = vp.center.x as f32;
                let cy = vp.center.y as f32;
                let tx = vp.view_target.x as f32;
                let ty = vp.view_target.y as f32;
                let mx = (cp.x - cx) / scale_before + tx;
                let my = (cp.y - cy) / scale_before + ty;

                // Apply zoom.
                vp.view_height = (vp.view_height * factor).max(1e-6);
                if vp.view_height.abs() > 1e-9 {
                    vp.custom_scale = vp.height / vp.view_height;
                }
                let scale_after = vp.custom_scale as f32;

                // Adjust view_target so the model point under cursor stays there.
                let mx_after = (cp.x - cx) / scale_after + vp.view_target.x as f32;
                let my_after = (cp.y - cy) / scale_after + vp.view_target.y as f32;
                vp.view_target.x += (mx - mx_after) as f64;
                vp.view_target.y += (my - my_after) as f64;
            } else {
                vp.view_height = (vp.view_height * factor).max(1e-6);
                if vp.view_height.abs() > 1e-9 {
                    vp.custom_scale = vp.height / vp.view_height;
                }
            }
        }
    }

    /// Orbit the active viewport's view direction by the given screen-pixel delta.
    /// No-op when there is no active viewport or it is locked.
    pub fn orbit_active_viewport(&mut self, delta_x: f32, delta_y: f32) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        let mut cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return,
        };
        cam.orbit(delta_x, delta_y);
        // yaw_pitch_to_quat(y,p)*Z = (cos(p)*sin(y), -cos(p)*cos(y), sin(p))
        // `camera_for_viewport` reconstructs the rotation so that
        // `rotation * Z == view_direction` exactly (its `yaw = atan2(x, -y)`
        // cancels the sign). Store `eye` directly so the orbit round-trips —
        // negating Y here made each drag step read back a Y-mirrored camera,
        // flipping the model between a rotation and its opposite every frame.
        let eye = cam.rotation * glam::Vec3::Z;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = eye.y as f64;
            vp.view_direction.z = eye.z as f64;
        }
    }

    /// Snap the active viewport's view direction to `eye_dir` (unit
    /// vector from target toward camera). Twist angle is left at its
    /// current value so the up-sense is preserved across successive
    /// snaps. No-op when there is no active viewport or it is locked.
    pub fn snap_active_viewport_to_direction(&mut self, eye_dir: glam::Vec3, ucs: glam::Mat4) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        // Build the full UCS-aligned orientation exactly as the model snap does
        // (snap_to_direction picks the in-plane roll from the UCS axes), seeded
        // from the viewport's current camera so the "best up" stays stable, then
        // decode it back to the stored (view_direction, twist_angle). Writing
        // only view_direction loses the roll and the rebuilt camera snaps to
        // WCS-up instead of the UCS the clicked cube was drawn in.
        let mut tmp = self.camera_for_viewport(vp_handle).unwrap_or_default();
        tmp.snap_to_direction(eye_dir, ucs);
        let dir = (tmp.rotation * glam::Vec3::Z).normalize_or(glam::Vec3::Z);
        let desired_up = (tmp.rotation * glam::Vec3::Y).normalize_or(glam::Vec3::Y);

        // camera_from_view rebuilds the rotation with its OWN yaw convention
        // (atan2(x, -y)) and applies roll = -twist, which is *not* the camera's
        // internal yaw/roll convention — so `-tmp.roll()` does not round-trip.
        // Instead reproduce the decoder's zero-twist basis here, then measure
        // the signed roll about the view axis that carries its up onto the
        // desired UCS up. Store twist = -roll (the decoder negates it back).
        let pitch = dir.z.clamp(-1.0, 1.0).asin();
        let yaw = if dir.x.abs() < 1e-6 && dir.y.abs() < 1e-6 {
            0.0
        } else {
            dir.x.atan2(-dir.y)
        };
        let up0 = (view::camera::yaw_pitch_to_quat(yaw, pitch, 0.0) * glam::Vec3::Y)
            .normalize_or(glam::Vec3::Y);
        let roll = up0.cross(desired_up).dot(dir).atan2(up0.dot(desired_up));
        let twist = -roll as f64;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = dir.x as f64;
            vp.view_direction.y = dir.y as f64;
            vp.view_direction.z = dir.z as f64;
            vp.twist_angle = twist;
        }
    }

    /// Mutate the active viewport's camera through a closure, then re-encode the
    /// result to the stored `(view_direction, twist_angle)` — the same decode
    /// the ViewCube snap uses. Lets the home / roll / nudge controls drive a
    /// floating viewport just like the model camera. Returns `false` if there is
    /// no active (unlocked) viewport.
    pub fn mutate_active_viewport_camera(
        &mut self,
        f: impl FnOnce(&mut view::camera::Camera),
    ) -> bool {
        let Some(vp_handle) = self.active_viewport else {
            return false;
        };
        let mut tmp = self.camera_for_viewport(vp_handle).unwrap_or_default();
        f(&mut tmp);
        let dir = (tmp.rotation * glam::Vec3::Z).normalize_or(glam::Vec3::Z);
        let desired_up = (tmp.rotation * glam::Vec3::Y).normalize_or(glam::Vec3::Y);
        let pitch = dir.z.clamp(-1.0, 1.0).asin();
        let yaw = if dir.x.abs() < 1e-6 && dir.y.abs() < 1e-6 {
            0.0
        } else {
            dir.x.atan2(-dir.y)
        };
        let up0 = (view::camera::yaw_pitch_to_quat(yaw, pitch, 0.0) * glam::Vec3::Y)
            .normalize_or(glam::Vec3::Y);
        let roll = up0.cross(desired_up).dot(dir).atan2(up0.dot(desired_up));
        let twist = -roll as f64;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return false;
            }
            vp.view_direction.x = dir.x as f64;
            vp.view_direction.y = dir.y as f64;
            vp.view_direction.z = dir.z as f64;
            vp.twist_angle = twist;
            return true;
        }
        false
    }

    /// Render mode of the active paper-space viewport, or `None` when no
    /// viewport is active (PSPACE / model layout).
    pub fn active_viewport_render_mode(
        &self,
    ) -> Option<acadrust::entities::ViewportRenderMode> {
        let h = self.active_viewport?;
        match self.document.get_entity(h) {
            Some(acadrust::EntityType::Viewport(vp)) => Some(vp.render_mode),
            _ => None,
        }
    }

    /// Set the active paper-space viewport's render mode. Returns `true`
    /// when a viewport was active and updated; `false` (no-op) otherwise,
    /// so the caller can fall back to the model-layout render mode.
    pub fn set_active_viewport_render_mode(
        &mut self,
        mode: acadrust::entities::ViewportRenderMode,
    ) -> bool {
        let Some(h) = self.active_viewport else {
            return false;
        };
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(h) {
            vp.render_mode = mode;
            true
        } else {
            false
        }
    }

    /// Visual style of the active Model tile (for the render-mode picker).
    pub fn active_model_tile_render_mode(
        &self,
    ) -> acadrust::entities::ViewportRenderMode {
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        tiles
            .get(active)
            .map(|t| t.render_mode)
            .unwrap_or(acadrust::entities::ViewportRenderMode::Wireframe2D)
    }

    /// Set only the active Model tile's render mode. Other tiles keep theirs.
    pub fn set_active_model_tile_render_mode(
        &self,
        mode: acadrust::entities::ViewportRenderMode,
    ) {
        let mut tiles = self.model_tiles.borrow_mut();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        if let Some(t) = tiles.get_mut(active) {
            t.render_mode = mode;
        }
    }

    /// Current gaze direction (canonical +Z eye dir, world space) of whichever
    /// camera owns the ViewCube — the active floating viewport in MSPACE, else
    /// the main camera. Used by the ViewCube "already there → flip to opposite"
    /// check, which must test the camera the cube actually reflects (the paper
    /// camera always looks straight down, so testing it flipped every snap).
    pub fn active_gaze_dir(&self) -> glam::Vec3 {
        if let Some(h) = self.active_viewport {
            if let Some(cam) = self.camera_for_viewport(h) {
                return cam.rotation * glam::Vec3::Z;
            }
        }
        self.camera.borrow().rotation * glam::Vec3::Z
    }

    /// View-rotation matrix for the active viewport (MSPACE), or the
    /// paper-space camera's matrix when not in MSPACE.
    /// Used by ViewCube hit-testing so clicks map to the correct camera.
    pub fn active_view_rotation_mat(&self) -> glam::Mat4 {
        // Must match exactly what the drawn cube uses (see ViewportData's
        // `cam_rotation`): the active context's camera composed with the
        // ViewCube UCS. Inside a floating viewport that's the viewport's own
        // camera; the UCS factor applies in both cases.
        if let Some(h) = self.active_viewport {
            if let Some(cam) = self.camera_for_viewport(h) {
                return cam.view_rotation_mat() * self.viewcube_ucs_mat();
            }
        }
        self.camera.borrow().view_rotation_mat() * self.viewcube_ucs_mat()
    }

    /// The UCS→world rotation the ViewCube should compose with the camera —
    /// the active UCS in model space, identity everywhere else. Render,
    /// hit-test, and click-snap all go through this so they stay in lock-step.
    pub fn viewcube_ucs_mat(&self) -> glam::Mat4 {
        // UCS applies in model space and inside a floating viewport (MSPACE);
        // plain paper space stays WCS.
        if self.current_layout == "Model" || self.active_viewport.is_some() {
            self.viewcube_ucs
        } else {
            glam::Mat4::IDENTITY
        }
    }

    /// Return the handle of the user viewport whose *visible* on-screen rectangle
    /// (clamped to the canvas) contains the given screen-space point, or `None`
    /// if no viewport matches. When viewports overlap, pick the SMALLEST-area
    /// match — the topmost one the user actually sees and means to click, not
    /// the first in document order.
    ///
    /// Viewport activation goes through this so a click only enters a viewport
    /// when it lands on the part it can actually see — clicking the empty area
    /// beside a viewport that runs off-screen no longer matches its full (partly
    /// off-canvas) paper rect and switches to it by mistake.
    pub fn viewport_at_screen_point(
        &self,
        px: f32,
        py: f32,
        canvas: (f32, f32),
    ) -> Option<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document
            .entities()
            .filter_map(|e| {
                let EntityType::Viewport(vp) = e else {
                    return None;
                };
                if !self.is_content_viewport_in_layout(vp, layout_block) || !vp.status.is_on {
                    return None;
                }
                let rect = self.viewport_screen_rect(vp.common.handle, canvas)?;
                let x0 = rect.x.max(0.0);
                let y0 = rect.y.max(0.0);
                let x1 = (rect.x + rect.width).min(canvas.0);
                let y1 = (rect.y + rect.height).min(canvas.1);
                if x1 <= x0 || y1 <= y0 {
                    return None; // fully off-canvas → nothing to click
                }
                if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
                    Some((vp.common.handle, (x1 - x0) * (y1 - y0)))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(h, _)| h)
    }

    /// Return the handle of the first active user viewport in the current layout,
    /// or `None` if there are none.  Used by the MS command.
    pub fn first_user_viewport(&self) -> Option<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document.entities().find_map(|e| {
            let EntityType::Viewport(vp) = e else {
                return None;
            };
            if self.is_content_viewport_in_layout(vp, layout_block)
                && vp.status.is_on
            {
                Some(vp.common.handle)
            } else {
                None
            }
        })
    }

    // ── Layout management ─────────────────────────────────────────────────

    /// Rename a paper-space layout.  Updates the Layout object name in the document.
    pub fn rename_layout(&mut self, old_name: &str, new_name: &str) {
        for obj in self.document.objects.values_mut() {
            if let ObjectType::Layout(l) = obj {
                if l.name == old_name {
                    l.name = new_name.to_string();
                    return;
                }
            }
        }
    }

    /// Delete a paper-space layout and all entities owned by it.
    /// Returns `false` if the layout was not found or is "Model".
    pub fn delete_layout(&mut self, name: &str) -> bool {
        if name == "Model" {
            return false;
        }

        let layout_info = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == name {
                    return Some((l.handle, l.block_record));
                }
            }
            None
        });

        let (layout_handle, block_handle) = match layout_info {
            Some(info) => info,
            None => return false,
        };

        // Remove all entities that belong to this layout's block record.
        let to_remove: Vec<Handle> = self
            .document
            .entities()
            .filter(|e| e.common().owner_handle == block_handle)
            .map(|e| e.common().handle)
            .collect();
        for h in &to_remove {
            self.hatches.remove(h);
            self.meshes.remove(h);
            self.solid_models.remove(h);
            self.document.remove_entity(*h);
        }

        // Remove the Layout object itself.
        self.document.objects.remove(&layout_handle);

        // Drop the layout's entry from the ACAD_LAYOUT dictionary so it does not
        // dangle (and so AutoCAD doesn't try to recover a now-missing layout).
        let dict_handle = self.document.header.acad_layout_dict_handle;
        if let Some(ObjectType::Dictionary(d)) = self.document.objects.get_mut(&dict_handle) {
            d.entries.retain(|(k, _)| k != name);
        }

        // Remove the now-empty paper-space block record.
        let block_name = self
            .document
            .block_records
            .iter()
            .find(|b| b.handle == block_handle)
            .map(|b| b.name.clone());
        if let Some(bn) = block_name {
            self.document.block_records.remove(&bn);
        }

        // Drop any standalone PlotSettings page setup tied to this layout.
        let ps_handles: Vec<Handle> = self
            .document
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                ObjectType::PlotSettings(ps) if ps.page_name == name => Some(*h),
                _ => None,
            })
            .collect();
        for h in ps_handles {
            self.document.objects.remove(&h);
        }

        // If the deleted layout was active, fall back to Model space.
        if self.current_layout == name {
            self.current_layout = "Model".to_string();
        }

        self.bump_geometry();
        true
    }

    /// Swap the `tab_order` of two paper layouts so they appear in swapped order.
    pub fn swap_layout_order(&mut self, name_a: &str, name_b: &str) {
        let mut order_a: Option<i16> = None;
        let mut order_b: Option<i16> = None;
        for obj in self.document.objects.values() {
            if let ObjectType::Layout(l) = obj {
                if l.name == name_a {
                    order_a = Some(l.tab_order);
                }
                if l.name == name_b {
                    order_b = Some(l.tab_order);
                }
            }
        }
        if let (Some(oa), Some(ob)) = (order_a, order_b) {
            for obj in self.document.objects.values_mut() {
                if let ObjectType::Layout(l) = obj {
                    if l.name == name_a {
                        l.tab_order = ob;
                    } else if l.name == name_b {
                        l.tab_order = oa;
                    }
                }
            }
        }
    }

    // ── Entity management ─────────────────────────────────────────────────

    pub fn add_entity(&mut self, mut entity: EntityType) -> Handle {
        // Only Insert / Block entities can introduce or reference a block
        // definition that the block cache must (re)build. Adding a plain
        // top-level entity (line, arc, text, …) leaves every block defn intact,
        // so it keeps the cache and skips the all-blocks re-tessellation.
        let affects_blocks = matches!(
            &entity,
            EntityType::Insert(_) | EntityType::Block(_) | EntityType::BlockEnd(_)
        );
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color))
        } else {
            None
        };
        let image_seed = if let EntityType::RasterImage(img) = &entity {
            ImageModel::from_raster_image(img)
        } else {
            None
        };
        let facet_res = self.document.header.facet_resolution;
        let mesh_seed = if matches!(
            &entity,
            EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) | EntityType::Surface(_)
        ) {
            let color = self.render_style(&entity).0;
            crate::entities::solid3d::tessellate_volume(&entity, color, facet_res)
                .map(|m| offset_mesh_lod_set(m))
        } else {
            None
        };

        // Auto-create an ImageDefinition object for new RasterImage entities
        // that don't already reference one.
        if let EntityType::RasterImage(ref mut img) = entity {
            if img.definition_handle.is_none() {
                use acadrust::objects::{ImageDefinition, ObjectType};
                let def_handle = Handle::new(self.document.next_handle());
                let mut img_def = ImageDefinition::with_dimensions(
                    &img.file_path,
                    img.size.x as u32,
                    img.size.y as u32,
                );
                img_def.handle = def_handle;
                img_def.is_loaded = true;
                self.document
                    .objects
                    .insert(def_handle, ObjectType::ImageDefinition(img_def));
                img.definition_handle = Some(def_handle);
            }
        }

        // Route to the correct block based on current editing mode:
        //   - PSPACE (paper layout, no active viewport): paper-space layout block.
        //   - MSPACE or model layout: model space (document default).
        let handle = if self.current_layout != "Model" && self.active_viewport.is_none() {
            let layout_name = self.current_layout.clone();
            self.document
                .add_entity_to_layout(entity, &layout_name)
                .unwrap_or(Handle::NULL)
        } else {
            self.document.add_entity(entity).unwrap_or(Handle::NULL)
        };

        if !handle.is_null() {
            if let Some(model) = hatch_seed {
                self.hatches.insert(handle, model);
            }
            if let Some(model) = image_seed {
                self.images.insert(handle, model);
            }
            if let Some(model) = mesh_seed {
                self.meshes.insert(handle, model);
            }
            if affects_blocks {
                self.bump_geometry();
            } else {
                self.bump_geometry_no_blocks();
            }
        }
        handle
    }

    /// Returns the RGBA color for the given layer name.
    pub fn layer_color(&self, layer: &str) -> [f32; 4] {
        let layer_entry = self.document.layers.get(layer);
        let color = layer_entry
            .map(|l| &l.color)
            .unwrap_or(&acadrust::types::Color::WHITE);
        let [r, g, b, _] = crate::scene::convert::tess_util::aci_to_rgba(color);
        [r, g, b, 1.0]
    }

    pub fn custom_block_names(&self) -> Vec<String> {
        self.document
            .block_records
            .iter()
            .filter(|br| !br.is_standard() && !br.is_layout())
            .map(|br| br.name.clone())
            .collect()
    }

    pub fn create_block_from_entities(
        &mut self,
        handles: &[Handle],
        name: &str,
        base: glam::Vec3,
    ) -> Result<Handle, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }
        if self.document.block_records.get(name).is_some() {
            return Err(format!("Block \"{name}\" already exists."));
        }

        let source_entities: Vec<_> = handles
            .iter()
            .filter_map(|&h| self.document.get_entity(h).cloned().map(|e| (h, e)))
            .collect();
        if source_entities.is_empty() {
            return Err("No valid entities selected for block creation.".into());
        }

        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        self.document
            .block_records
            .add(block_record)
            .map_err(|e| e.to_string())?;

        let mut block = Block::new(name, acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::Block(block))
            .map_err(|e| e.to_string())?;

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .map_err(|e| e.to_string())?;

        let local = EntityTransform::Translate((-base).as_dvec3());
        for (old_handle, mut entity) in source_entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
            self.erase_entities(&[old_handle]);
        }

        let insert = DxfInsert::new(
            name,
            acadrust::types::Vector3::new(base.x as f64, base.y as f64, base.z as f64),
        );
        Ok(self.add_entity(EntityType::Insert(insert)))
    }

    /// Define a new block named `name` from `entities` (owned, not yet in the
    /// document), with `base` as its insertion origin. Unlike
    /// [`create_block_from_entities`] this does NOT place an insert — the
    /// caller starts an interactive insert so paste-as-block can prompt for the
    /// drop point. The geometry comes from the clipboard rather than live
    /// entities, so there is nothing to stage or erase. (#129)
    pub fn define_block_from_owned_entities(
        &mut self,
        entities: Vec<EntityType>,
        name: &str,
        base: glam::DVec3,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }
        if self.document.block_records.get(name).is_some() {
            return Err(format!("Block \"{name}\" already exists."));
        }
        if entities.is_empty() {
            return Err("Nothing to make into a block.".into());
        }

        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        self.document
            .block_records
            .add(block_record)
            .map_err(|e| e.to_string())?;

        let mut block = Block::new(name, acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::Block(block))
            .map_err(|e| e.to_string())?;

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .map_err(|e| e.to_string())?;

        let local = EntityTransform::Translate(-base);
        for mut entity in entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
        }
        // Block defns don't render on their own, but the geometry cache must
        // pick up the new definition so the interactive insert can preview it.
        self.bump_geometry();
        Ok(())
    }

    /// Recreate a block definition verbatim — the entities are already in
    /// block-local coordinates (unlike `define_block_from_owned_entities`,
    /// which folds in a base offset). No-op if the block already exists.
    /// Used when pasting an INSERT whose block this drawing lacks. (#135)
    pub fn define_block_raw(
        &mut self,
        name: &str,
        base_point: acadrust::types::Vector3,
        entities: Vec<EntityType>,
    ) {
        if name.is_empty() || self.document.block_records.get(name).is_some() {
            return;
        }
        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = acadrust::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        if self.document.block_records.add(block_record).is_err() {
            return;
        }

        let mut block = Block::new(name, base_point);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::Block(block));

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::BlockEnd(block_end));

        for mut entity in entities {
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            let _ = self.document.add_entity(entity);
        }
        self.bump_geometry();
    }

    fn synced_hatch_models(&self) -> Vec<HatchModel> {
        let layout_block = self.current_layout_block_handle();

        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };

        // synced_hatch_models is cached on geometry_epoch and the GPU
        // upload is keyed on geometry_epoch only (see render.rs — hatch
        // buffers are "static"). Don't view-cull here; the per-frame
        // skip flag in compute_hatch_lod handles frustum + sub-pixel
        // culling at draw time, which keeps the GPU upload set stable
        // across pan/zoom.
        //
        // We INCLUDE hatches from blocks other than `current_layout`'s
        // own block (specifically: paper-layout content viewports want
        // model-block hatches). Every hatch's `world_origin` is already
        // baked into the correct block coord-space at
        // `populate_hatches_from_document` time (offset for model, 0 for
        // paper), so projecting them through a camera built for the
        // wrong block lands them outside the frustum and the per-vp
        // scissor / LOD culls them out — no double-rendering.
        let depth_map = self.draw_depth_map();
        let mut models: Vec<HatchModel> = self
            .hatches
            .iter()
            .filter(|(&handle, _)| {
                let Some(entity) = self.document.get_entity(handle) else {
                    return true;
                };
                let c = entity.common();
                if c.invisible || layer_hidden(&c.layer) {
                    return false;
                }
                // Reject block-defn-only hatches (entities owned by a
                // BLOCK record that's neither model nor a paper layout
                // block) — they're tessellated separately via Insert
                // explosion and only the laid-out copies should appear.
                self.belongs_to_visible_block(handle, c.owner_handle, layout_block)
                    || self.belongs_to_visible_block(
                        handle,
                        c.owner_handle,
                        self.model_space_block_handle(),
                    )
            })
            .map(|(&handle, model)| {
                let entity = self.document.get_entity(handle);
                let mut m = model.clone();
                if let Some(e) = entity {
                    m.color = self.render_style(e).0;
                    if let EntityType::Hatch(dxf) = e {
                        match &mut m.pattern {
                            model::hatch_model::HatchPattern::Pattern(_) => {
                                m.angle_offset = dxf.pattern_angle as f32;
                                let anno = if self.current_layout == "Model" {
                                    self.annotation_scale
                                } else {
                                    1.0
                                };
                                m.scale = dxf.pattern_scale as f32 * anno;
                            }
                            model::hatch_model::HatchPattern::Gradient { angle_deg, .. } => {
                                *angle_deg = dxf.pattern_angle.to_degrees() as f32;
                            }
                            model::hatch_model::HatchPattern::Solid => {}
                        }
                    }
                }
                if self.selected.contains(&handle) {
                    m.color = [0.15, 0.55, 1.00, m.color[3]];
                }
                m.draw_depth = depth_map.get(&handle.value()).copied().unwrap_or(0.0);
                m
            })
            .collect();

        for entity in self.document.entities() {
            let EntityType::Insert(ins) = entity else {
                continue;
            };
            if ins.common.invisible || layer_hidden(&ins.common.layer) {
                continue;
            }
            if !self.belongs_to_visible_block(
                ins.common.handle,
                ins.common.owner_handle,
                layout_block,
            ) {
                continue;
            }
            let selected = self.selected.contains(&ins.common.handle);
            // XCLIP: clip this insert's exploded hatch fills to the boundary,
            // matching how the line geometry is clipped in expand_insert.
            let clip_poly = pick::xclip::insert_spatial_filter(&self.document, ins)
                .map(|sf| pick::xclip::world_clip_polygon(sf, ins));
            // Walk the full block tree: `explode_from_document` only descends
            // one level, so nested INSERTs are re-exploded here. Each level
            // bakes its transform into the children it returns, so nested
            // hatches land in the correct world position. A depth guard keeps
            // a malformed cyclic block reference from looping forever.
            let normalize = crate::modules::draw::modify::explode::normalize_insert_entity;
            let mut stack: Vec<(EntityType, usize)> = ins
                .explode_from_document(&self.document)
                .into_iter()
                .map(|e| (normalize(e), 0usize))
                .collect();
            while let Some((sub, depth)) = stack.pop() {
                match sub {
                    EntityType::Insert(nins) => {
                        if depth >= 32 {
                            continue;
                        }
                        for e in nins.explode_from_document(&self.document) {
                            stack.push((normalize(e), depth + 1));
                        }
                    }
                    EntityType::Hatch(dxf) => {
                        if dxf.common.invisible || layer_hidden(&dxf.common.layer) {
                            continue;
                        }
                        let color = self.render_style(&EntityType::Hatch(dxf.clone())).0;
                        if let Some(mut model) =
                            Self::hatch_model_from_dxf(&dxf, color)
                        {
                            if let Some(poly) = &clip_poly {
                                let clipped = pick::xclip::clip_hatch_boundary(
                                    &model.boundary,
                                    model.world_origin,
                                    poly,
                                );
                                if clipped.is_empty() {
                                    continue;
                                }
                                model.boundary = std::sync::Arc::new(clipped);
                            }
                            if selected {
                                model.color = [0.15, 0.55, 1.00, model.color[3]];
                            }
                            models.push(model);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Wide LWPolyline and Polyline2D fills
        for entity in self.document.entities() {
            let (common, fills) = match entity {
                EntityType::LwPolyline(pl) => (&pl.common, crate::entities::lwpolyline::wide_fills(pl)),
                EntityType::Polyline2D(pl) => (&pl.common, crate::entities::polyline::wide_fills(pl)),
                _ => continue,
            };
            if fills.is_empty() {
                continue;
            }
            if common.invisible || layer_hidden(&common.layer) {
                continue;
            }
            if !self.belongs_to_visible_block(common.handle, common.owner_handle, layout_block) {
                continue;
            }
            let base_color = self.render_style(entity).0;
            let selected = self.selected.contains(&common.handle);
            let color = if selected {
                [0.15, 0.55, 1.00, 1.0]
            } else {
                base_color
            };
            for boundary in fills {
                models.push(HatchModel {
                    boundary: Arc::new(boundary),
                    pattern: model::hatch_model::HatchPattern::Solid,
                    name: "SOLID".into(),
                    color,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin: [0.0; 2],
                    vp_scissor: None,
                    draw_depth: depth_map.get(&common.handle.value()).copied().unwrap_or(0.0),
                });
            }
        }

        models
    }

    /// Wipeout fill models — rendered in a separate pass AFTER wires so that
    /// wipeouts correctly mask everything below them in the draw order.
    pub(super) fn wipeout_models(&self) -> Vec<HatchModel> {
        let is_paper = self.current_layout != "Model";
        let bg_color: [f32; 4] = if is_paper {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        // No per-frame view-cull here: GPU wipeout buffer upload is
        // gated on geometry_epoch only (see render.rs), so any cull at
        // build time would freeze the visible subset at the geometry
        // epoch boundary and never re-evaluate as the user pans. The
        // pipeline's `wipeout_skip_flags` (compute_wipeout_lod) does
        // the per-frame skip at draw time instead.
        let mut models = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Wipeout(wo) = entity else {
                continue;
            };
            if entity.common().invisible {
                continue;
            }
            if self
                .document
                .layers
                .get(&entity.common().layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
            {
                continue;
            }
            // Per-entity world_offset selection so paper-layout content
            // viewports still see model-block wipeouts at the right local
            // coordinates (same rationale as hatches).
            let boundary = Self::wipeout_boundary_2d(wo);
            if boundary.len() >= 3 {
                let mut fill_color = bg_color;
                if self.selected.contains(&wo.common.handle) {
                    fill_color = [0.15, 0.55, 1.00, 0.35];
                }
                models.push(HatchModel {
                    boundary: Arc::new(boundary),
                    pattern: model::hatch_model::HatchPattern::Solid,
                    name: "WIPEOUT_FILL".into(),
                    color: fill_color,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin: [0.0; 2],
                    vp_scissor: None,
                    draw_depth: 0.0,
                });
            }
        }
        models
    }

    /// Compute the 2D (XY) boundary polygon for a Wipeout entity.
    fn wipeout_boundary_2d(
        wo: &acadrust::entities::Wipeout,
    ) -> Vec<[f32; 2]> {
        use acadrust::entities::WipeoutClipType;

        let is_polygon = wo.clipping_enabled
            && wo.clip_boundary_vertices.len() >= 3
            && matches!(wo.clip_type, WipeoutClipType::Polygonal);

        if is_polygon {
            let ox = (wo.insertion_point.x) as f32;
            let oy = (wo.insertion_point.y) as f32;
            // DXF clip vertices live in image-pixel space, centred on the
            // image (range −size/2 … +size/2). Image-bottom-left → insertion,
            // image-y-axis points DOWN (per the DXF "v_vector points down the
            // image" convention), so map:
            //   x_off = (clip.x + size.x/2) × u_vec
            //   y_off = (size.y/2 − clip.y) × v_vec    ← y flipped
            let cx_of = |v: &acadrust::types::Vector2| v.x + wo.size.x * 0.5;
            let cy_of = |v: &acadrust::types::Vector2| wo.size.y * 0.5 - v.y;
            let mut poly: Vec<[f32; 2]> = wo
                .clip_boundary_vertices
                .iter()
                .map(|v| {
                    let cx = cx_of(v);
                    let cy = cy_of(v);
                    let wx = (wo.u_vector.x * cx + wo.v_vector.x * cy) as f32;
                    let wy = (wo.u_vector.y * cx + wo.v_vector.y * cy) as f32;
                    [ox + wx, oy + wy]
                })
                .collect();
            // Close the loop: the GPU `in_polygon` ray-cast walks
            // sequential pairs and doesn't wrap, so without an explicit
            // closing vertex the last edge (vN-1 → v0) is never tested and
            // the fill bleeds far past the boundary.
            if let Some(&first) = poly.first() {
                if poly.last() != Some(&first) {
                    poly.push(first);
                }
            }
            poly
        } else {
            // Rectangular boundary from 4 corners.
            let ox = (wo.insertion_point.x) as f32;
            let oy = (wo.insertion_point.y) as f32;
            let oz = wo.insertion_point.z as f32;
            let ux = (wo.u_vector.x * wo.size.x) as f32;
            let uy = (wo.u_vector.y * wo.size.x) as f32;
            let vx = (wo.v_vector.x * wo.size.y) as f32;
            let vy = (wo.v_vector.y * wo.size.y) as f32;
            let _ = oz;
            // Close the loop (repeat corner 0): the GPU `in_polygon` ray-cast
            // walks sequential vertex pairs and never wraps last→first, so an
            // unclosed quad leaves the v3→v0 edge untested and the solid mask
            // bleeds past the boundary — same reason the polygon branch closes.
            vec![
                [ox, oy],
                [ox + ux, oy + uy],
                [ox + ux + vx, oy + uy + vy],
                [ox + vx, oy + vy],
                [ox, oy],
            ]
        }
    }

    fn hatch_model_from_dxf(
        dxf: &DxfHatch,
        color: [f32; 4],
    ) -> Option<HatchModel> {
        let normal = (dxf.normal.x, dxf.normal.y, dxf.normal.z);
        // Build the boundary in f64 first so the precision-preserving
        // origin computation below sees full WCS precision. We only cast
        // to f32 once at the end, after subtracting the AABB centre, so
        // the stored offsets are small-magnitude with high f32 precision
        // even on large UTM-scale drawings.
        let to_xy = |x: f64, y: f64| -> [f64; 2] {
            let (wx, wy, _) =
                crate::scene::view::transform::ocs_point_to_wcs((x, y, dxf.elevation), normal);
            [wx, wy]
        };
        if dxf.paths.is_empty() {
            return None;
        }

        let mut boundary: Vec<[f64; 2]> = Vec::new();

        for path in &dxf.paths {
            let before_path = boundary.len();
            if !boundary.is_empty() {
                boundary.push([f64::NAN, f64::NAN]);
            }
            let path_start = boundary.len();

            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Polyline(poly) => {
                        let verts = &poly.vertices;
                        let count = verts.len();
                        if count == 0 {
                            continue;
                        }
                        let seg_count = if poly.is_closed {
                            count
                        } else {
                            count.saturating_sub(1)
                        };
                        for i in 0..seg_count {
                            let v0 = &verts[i];
                            let v1 = &verts[(i + 1) % count];
                            let bulge = v0.z;
                            // Tess in f64 to preserve ~1 cm precision at
                            // UTM-scale WCS (the f32 path used to produce
                            // visibly wavy hatch arcs at 1e5+ magnitude).
                            let arc = if bulge.abs() < 1e-9 {
                                None
                            } else {
                                crate::entities::common::BulgeArc::from_bulge(
                                    [v0.x, v0.y],
                                    [v1.x, v1.y],
                                    bulge,
                                )
                            };
                            let Some(arc) = arc else {
                                boundary.push(to_xy(v0.x, v0.y));
                                continue;
                            };
                            let segs = convert::tess_util::arc_segments(
                                arc.radius,
                                arc.sweep.abs(),
                                convert::tess_util::fill_chord_tol(arc.radius),
                            );
                            for j in 0..segs {
                                let s = arc.sample(j as f64 / segs as f64);
                                boundary.push(to_xy(s[0], s[1]));
                            }
                        }
                        if poly.is_closed {
                            if let Some(&first) = boundary.get(path_start) {
                                boundary.push(first);
                            }
                        }
                    }
                    BoundaryEdge::Line(line) => {
                        boundary.push(to_xy(line.start.x, line.start.y));
                        boundary.push(to_xy(line.end.x, line.end.y));
                    }
                    BoundaryEdge::CircularArc(arc) => {
                        let (sa, span) = convert::tess_util::arc_signed_span(
                            arc.start_angle,
                            arc.end_angle,
                            arc.counter_clockwise,
                        );
                        let segs = convert::tess_util::arc_segments(
                            arc.radius,
                            span.abs(),
                            convert::tess_util::fill_chord_tol(arc.radius),
                        );
                        for i in 0..=segs {
                            let t = sa + span * (i as f64 / segs as f64);
                            boundary.push(to_xy(
                                arc.center.x + arc.radius * t.cos(),
                                arc.center.y + arc.radius * t.sin(),
                            ));
                        }
                    }
                    BoundaryEdge::EllipticArc(ell) => {
                        let r_maj = (ell.major_axis_endpoint.x * ell.major_axis_endpoint.x
                            + ell.major_axis_endpoint.y * ell.major_axis_endpoint.y)
                            .sqrt();
                        let r_min = r_maj * ell.minor_axis_ratio;
                        let rot = ell
                            .major_axis_endpoint
                            .y
                            .atan2(ell.major_axis_endpoint.x);
                        let (sa, span) = convert::tess_util::arc_signed_span(
                            ell.start_angle,
                            ell.end_angle,
                            ell.counter_clockwise,
                        );
                        let segs = convert::tess_util::arc_segments(
                            r_maj,
                            span.abs(),
                            convert::tess_util::fill_chord_tol(r_maj),
                        );
                        let (cr, sr) = (rot.cos(), rot.sin());
                        for i in 0..=segs {
                            let t = sa + span * (i as f64 / segs as f64);
                            let lx = r_maj * t.cos();
                            let ly = r_min * t.sin();
                            boundary.push(to_xy(
                                ell.center.x + lx * cr - ly * sr,
                                ell.center.y + lx * sr + ly * cr,
                            ));
                        }
                    }
                    BoundaryEdge::Spline(spline) => {
                        // DXF spline control_points pack (x, y, weight) into
                        // a Vector3 — the z field is the rational weight, NOT
                        // a Z coordinate. The legacy code dropped weight and
                        // sampled with a fixed 16 segments; both bugs
                        // produced visibly wrong fill regions for spline-
                        // bounded hatches (especially block-internal ones,
                        // where boundaries are often spline curves with
                        // rational weights and short cubic segments).
                        //
                        // Build a NurbsCurve when `rational`, otherwise a
                        // plain BSplineCurve, and sample adaptively via
                        // truck's `parameter_division` at the same chord
                        // tolerance the fill polygon uses for arcs.
                        let degree = spline.degree.max(0) as usize;
                        let knot_vec = if !spline.knots.is_empty() {
                            KnotVec::from(spline.knots.clone())
                        } else if spline.control_points.len() >= degree + 1 {
                            KnotVec::uniform_knot(degree, spline.control_points.len() - 1)
                        } else {
                            KnotVec::from(vec![])
                        };
                        let knot_ok = spline.control_points.len() >= 2
                            && degree >= 1
                            && knot_vec.len() == spline.control_points.len() + degree + 1;

                        // Rough chord-tolerance: 0.1% of the control-poly
                        // diagonal so adaptive sampling produces enough
                        // points to follow the curve without exploding on
                        // huge splines.
                        let (mut sp_min_x, mut sp_min_y) = (f64::INFINITY, f64::INFINITY);
                        let (mut sp_max_x, mut sp_max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
                        for cp in &spline.control_points {
                            sp_min_x = sp_min_x.min(cp.x);
                            sp_min_y = sp_min_y.min(cp.y);
                            sp_max_x = sp_max_x.max(cp.x);
                            sp_max_y = sp_max_y.max(cp.y);
                        }
                        let diag = ((sp_max_x - sp_min_x).powi(2)
                            + (sp_max_y - sp_min_y).powi(2))
                        .sqrt();
                        let tol = convert::tess_util::fill_chord_tol(diag.max(1.0));

                        let mut sampled = false;
                        if knot_ok {
                            if spline.rational {
                                // NURBS: pack (x, y, 0, w) into Vector4.
                                let cps: Vec<Vector4> = spline
                                    .control_points
                                    .iter()
                                    .map(|p| {
                                        let w = if p.z.abs() > 1e-12 { p.z } else { 1.0 };
                                        Vector4::new(p.x * w, p.y * w, 0.0, w)
                                    })
                                    .collect();
                                let bspl = TruckBSpline::new(knot_vec.clone(), cps);
                                let curve = NurbsCurve::new(bspl);
                                let (t0, t1) = curve.range_tuple();
                                let (_, pts) = curve.parameter_division((t0, t1), tol);
                                for p in pts {
                                    boundary.push(to_xy(p.x, p.y));
                                }
                                sampled = true;
                            } else {
                                let cps: Vec<Point3> = spline
                                    .control_points
                                    .iter()
                                    .map(|p| Point3::new(p.x, p.y, 0.0))
                                    .collect();
                                let bspl = TruckBSpline::new(knot_vec, cps);
                                let (t0, t1) = bspl.range_tuple();
                                let (_, pts) = bspl.parameter_division((t0, t1), tol);
                                for p in pts {
                                    boundary.push(to_xy(p.x, p.y));
                                }
                                sampled = true;
                            }
                        }
                        if !sampled {
                            // Fallback: prefer fit_points (which lie on the
                            // curve) over control_points (which usually
                            // don't). A control-point polyline would draw
                            // the convex-hull silhouette — visibly wrong.
                            let pts: &[_] = if !spline.fit_points.is_empty() {
                                &spline.fit_points
                            } else {
                                &[]
                            };
                            if !pts.is_empty() {
                                for p in pts {
                                    boundary.push(to_xy(p.x, p.y));
                                }
                            } else {
                                for cp in &spline.control_points {
                                    boundary.push(to_xy(cp.x, cp.y));
                                }
                            }
                        }
                    }
                }
            }

            if boundary.len() == path_start {
                boundary.truncate(before_path);
                continue;
            }
            if boundary.len() >= path_start + 3 {
                let first = boundary[path_start];
                let last = *boundary.last().unwrap();
                if (first[0] - last[0]).abs() > 1e-5 || (first[1] - last[1]).abs() > 1e-5 {
                    boundary.push(first);
                }
            }
        }

        if boundary.is_empty() {
            return None;
        }
        // The batched hatch renderer keeps boundaries in a GPU storage
        // buffer (no fixed length), so a hatch with many island loops must
        // retain *every* loop or even-odd island detection breaks. The old
        // flat `truncate(1024)` cut complex multi-loop hatches mid-boundary:
        // trailing islands were dropped and the final partial loop was left
        // open, flipping the even-odd parity so the fill bled across the
        // rest of the shape. Only guard against pathological vertex counts,
        // and when trimming, cut at a whole-loop (NaN sentinel) boundary so
        // no sub-loop is ever left open. (#148)
        const MAX_HATCH_MODEL_VERTS: usize = 16_384;
        if boundary.len() > MAX_HATCH_MODEL_VERTS {
            // Drop only whole trailing loops: cut at the last NaN sentinel
            // at/before the cap. If the first loop alone exceeds the cap,
            // keep it whole rather than leaving it open.
            let cut = boundary[..MAX_HATCH_MODEL_VERTS]
                .iter()
                .rposition(|&[x, y]| x.is_nan() || y.is_nan())
                .unwrap_or(boundary.len());
            boundary.truncate(cut);
        }

        let pattern = if dxf.gradient_color.is_enabled() {
            let color2 = dxf
                .gradient_color
                .colors
                .get(1)
                .and_then(|e| e.color.rgb())
                .map(|(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
                .unwrap_or(color);
            let angle_deg = dxf.pattern_angle.to_degrees() as f32;
            model::hatch_model::HatchPattern::Gradient { angle_deg, color2 }
        } else if dxf.is_solid {
            model::hatch_model::HatchPattern::Solid
        } else {
            let pat_name = &dxf.pattern.name;
            if let Some(entry) = crate::scene::model::hatch_patterns::find(pat_name) {
                entry.gpu.clone()
            } else {
                model::hatch_model::HatchPattern::Pattern(vec![model::hatch_model::PatFamily {
                    angle_deg: 0.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 5.0 * dxf.pattern_scale as f32,
                    dashes: vec![],
                }])
            }
        };

        let name = if dxf.gradient_color.is_enabled() {
            dxf.gradient_color.name.clone()
        } else if dxf.is_solid {
            "SOLID".into()
        } else {
            dxf.pattern.name.clone()
        };

        // Precision-preserving cast f64 → f32: pick an `world_origin`
        // anchor (boundary AABB centre in f64) and store every vertex
        // as a small f32 offset from it. NaN separators are preserved
        // so the in_polygon ray-cast still sees the path breaks.
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &[x, y] in &boundary {
            if x.is_finite() && y.is_finite() {
                if x < min_x { min_x = x; }
                if y < min_y { min_y = y; }
                if x > max_x { max_x = x; }
                if y > max_y { max_y = y; }
            }
        }
        let world_origin = if min_x.is_finite() && min_y.is_finite() {
            [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5]
        } else {
            [0.0, 0.0]
        };
        let boundary_f32: Vec<[f32; 2]> = boundary
            .iter()
            .map(|&[x, y]| {
                if x.is_finite() && y.is_finite() {
                    [(x - world_origin[0]) as f32, (y - world_origin[1]) as f32]
                } else {
                    [f32::NAN, f32::NAN]
                }
            })
            .collect();

        Some(HatchModel {
            boundary: std::sync::Arc::new(boundary_f32),
            pattern,
            name,
            color,
            angle_offset: dxf.pattern_angle as f32,
            scale: dxf.pattern_scale as f32,
            world_origin,
            vp_scissor: None,
            draw_depth: 0.0,
        })
    }

    /// Decode and cache all RasterImage entities from the current document.
    /// Silently skips images whose files cannot be read.
    pub fn populate_images_from_document(&mut self) {
        self.images.clear();
        let entries: Vec<(Handle, acadrust::entities::RasterImage)> = self
            .document
            .entities()
            .filter_map(|e| {
                if let EntityType::RasterImage(img) = e {
                    Some((img.common.handle, img.clone()))
                } else {
                    None
                }
            })
            .collect();
        for (handle, img) in entries {
            if let Some(model) = ImageModel::from_raster_image(&img) {
                self.images.insert(handle, model);
            }
        }
        self.bump_geometry();
    }

    pub fn populate_hatches_from_document(&mut self) {
        self.hatches.clear();

        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((h.common.handle, e.clone())),
                EntityType::Solid(s) => Some((s.common.handle, e.clone())),
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        self.hatches = entries
            .into_par_iter()
            .filter_map(|(handle, kind)| {
                // Paper-space entities live in sheet coordinates — world_offset must not
                let model = match &kind {
                    EntityType::Hatch(dxf) => {
                        let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color)
                    }
                    EntityType::Solid(solid) => {
                        let color = convert::tess_util::aci_to_rgba(&solid.common.color);
                        Some(Self::solid_hatch_model(solid, color))
                    }
                    _ => None,
                };
                model.map(|m| (handle, m))
            })
            .collect();

        self.bump_geometry();
    }

    /// Tessellate all `Solid3D` entities in the current document into
    /// GPU-ready `MeshModel`s and store them in `self.meshes`.
    ///
    /// Called after loading a document or after undo/redo so that every
    /// `Solid3D` entity is represented in the mesh cache.
    pub fn populate_meshes_from_document(&mut self) {
        self.meshes.clear();
        self.block_meshes.clear();
        // BLOCK-entity handles of the layout (model + paper) blocks. A solid
        // owned by one of these is top-level; anything else lives in a block
        // definition and is instanced per INSERT instead. (#123)
        let layout_blocks: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .collect();
        // Resolve color through `render_style` so the same bg adaptation
        // wires use kicks in (pure black on dark bg → white, pure white
        // on light bg → black). Without this, ACIS meshes ignore
        // `adapt_to_bg` and stay invisible against matching bg colours.
        let entries: Vec<(Handle, EntityType, [f32; 4], bool)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) | EntityType::Surface(_) => {
                    let color = self.render_style(e).0;
                    let top_level = layout_blocks.contains(&e.common().owner_handle);
                    Some((e.common().handle, e.clone(), color, top_level))
                }
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        let facet_res = self.document.header.facet_resolution;
        // Top-level solids: offset into the render frame, drawn flat.
        // Block-definition solids: keep block-local coords for per-INSERT
        // instancing (no offset applied here).
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(&entity, color, facet_res).map(|m| {
                    let m = if top_level { offset_mesh_lod_set(m) } else { m };
                    (handle, m, top_level)
                })
            })
            .collect();
        for (handle, m, top_level) in built {
            if top_level {
                self.meshes.insert(handle, m);
            } else {
                self.block_meshes.insert(handle, m);
            }
        }

        self.bump_geometry();
    }

    /// Rebuild hatch / image / mesh caches after the document is modified
    /// outside the normal `add_entity` path (e.g. REFCLOSE SAVE).
    pub fn rebuild_derived_caches(&mut self) {
        self.populate_hatches_from_document();
        self.populate_images_from_document();
        self.populate_meshes_from_document();
    }

    /// Build a solid-fill HatchModel for a DXF Solid entity.
    /// DXF SOLID corners are in "Z-order": p0-p1 top, p2-p3 bottom.
    /// Visual quad is p0→p1→p3→p2 (closed).
    fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4]) -> HatchModel {
        let boundary = vec![
            [
                (solid.first_corner.x) as f32,
                (solid.first_corner.y) as f32,
            ],
            [
                (solid.second_corner.x) as f32,
                (solid.second_corner.y) as f32,
            ],
            [
                (solid.fourth_corner.x) as f32,
                (solid.fourth_corner.y) as f32,
            ],
            [
                (solid.third_corner.x) as f32,
                (solid.third_corner.y) as f32,
            ],
        ];
        HatchModel {
            boundary: std::sync::Arc::new(boundary),
            pattern: model::hatch_model::HatchPattern::Solid,
            name: "SOLID".into(),
            color,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin: [0.0; 2],
            vp_scissor: None,
            draw_depth: 0.0,
        }
    }

    pub fn add_hatch(&mut self, model: HatchModel) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::model::hatch_model::HatchPattern::Solid
        );
        // The boundary points arrive in local render space (world_offset
        // already subtracted). The stored DXF entity must hold WCS, so add the
        // offset back — otherwise the boundary wire, re-projected through the
        // normal entity path, lands `world_offset` away from the fill.
        let wx = model.world_origin[0];
        let wy = model.world_origin[1];
        let verts: Vec<Vector2> = model
            .boundary
            .iter()
            .filter(|v| v[0].is_finite() && v[1].is_finite())
            .map(|&[x, y]| Vector2::new(x as f64 + wx, y as f64 + wy))
            .collect();
        let edge = PolylineEdge::new(verts, true);
        let mut path = BoundaryPath::external();
        path.add_edge(BoundaryEdge::Polyline(edge));
        dxf.paths.push(path);
        if let Some(entry) = crate::scene::model::hatch_patterns::find(&model.name) {
            dxf.pattern = crate::scene::model::hatch_patterns::build_dxf_pattern(entry);
        }
        dxf.pattern_angle = model.angle_offset as f64;
        dxf.pattern_scale = if model.scale.abs() > 1e-6 {
            model.scale as f64
        } else {
            1.0
        };

        let handle = self.add_entity(EntityType::Hatch(dxf));
        if !handle.is_null() {
            self.hatches.insert(handle, model);
        }
        handle
    }

    pub fn clear(&mut self) {
        self.document = CadDocument::new();
        self.selected = HashSet::default();
        self.preview_wires = vec![];
        self.current_layout = "Model".to_string();
        self.hatches = HashMap::default();
        self.meshes = HashMap::default();
        *self.camera.borrow_mut() = Camera::default();
        self.camera_generation += 1;
        self.bump_geometry();
    }

    // ── Preview wire ──────────────────────────────────────────────────────

    pub fn set_preview_wires(&mut self, wires: Vec<WireModel>) {
        // Preview wires are an overlay appended to the cached base wire set in
        // `build_primitive`; they are NOT part of the tessellation cache. So a
        // preview update must NOT bump `geometry_epoch` — that would re-
        // tessellate the whole model on every rubber-band frame. The overlay
        // forces a GPU wire re-upload on its own (the `has_overlay` content-id
        // path), and iced redraws after the message that set the preview.
        self.preview_wires = wires;
    }

    pub fn clear_preview_wire(&mut self) {
        // No geometry bump — see `set_preview_wires`. Dropping the overlay
        // flips the wire content id back to the base tessellation id, which
        // re-uploads the base wires (without the preview) on the next frame.
        self.preview_wires = vec![];
        self.interim_wire = None;
    }

    pub fn wire_models_for(&self, handles: &[acadrust::Handle]) -> Vec<WireModel> {
        handles
            .iter()
            .flat_map(|h| {
                self.document
                    .entities()
                    .find(|e| e.common().handle == *h)
                    .map(|e| self.tessellate_one(e))
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Build wire models for an arbitrary slice of entities (e.g. clipboard contents).
    /// Entities need not be in the document — they are tessellated directly.
    pub fn wires_for_entities(&self, entities: &[acadrust::EntityType]) -> Vec<WireModel> {
        entities
            .iter()
            .flat_map(|e| self.tessellate_one(e))
            .collect()
    }

    pub fn set_interim_wire(&mut self, w: WireModel) {
        // Overlay wire — same reasoning as `set_preview_wires`: no geometry
        // bump, so the model isn't re-tessellated on every interim update.
        self.interim_wire = Some(w);
    }

    // ── Selection ─────────────────────────────────────────────────────────

    pub fn select_entity(&mut self, handle: Handle, exclusive: bool) {
        if exclusive {
            self.selected.clear();
        }
        self.selected.insert(handle);
        self.bump_selection();
    }

    pub fn deselect_all(&mut self) {
        self.selected.clear();
        self.bump_selection();
    }

    /// Remove a single entity from the selection (Shift+click subtractive pick).
    pub fn deselect_entity(&mut self, handle: Handle) {
        if self.selected.remove(&handle) {
            self.bump_selection();
        }
    }

    pub fn selected_entities(&self) -> Vec<(Handle, &EntityType)> {
        self.selected
            .iter()
            .filter_map(|&h| self.document.get_entity(h).map(|e| (h, e)))
            .collect()
    }

    /// Iterates every entity owned by the current layout's block-record.
    /// Returns an empty vec when the block-record is missing or holds no
    /// entity handles (legacy DXF without group-code 330 — we err on the
    /// side of "no candidates" instead of scanning the whole document, so
    /// model-block entities don't leak into a paper-layout selection).
    fn current_layout_entity_handles(&self) -> Vec<Handle> {
        let block = self.current_layout_block_handle();
        self.document
            .block_records
            .iter()
            .find(|br| br.handle == block)
            .map(|br| br.entity_handles.clone())
            .unwrap_or_default()
    }

    /// Extends the current selection with every entity in the active
    /// layout that matches one of the selected entities by `(variant,
    /// layer)`. The seed selection stays selected. No-op when nothing is
    /// selected. Returns the number of newly-added entities.
    pub fn select_similar(&mut self) -> usize {
        use crate::entities::traits::entity_type_name;
        if self.selected.is_empty() {
            return 0;
        }
        let pairs: rustc_hash::FxHashSet<(&'static str, String)> = self
            .selected
            .iter()
            .filter_map(|h| self.document.get_entity(*h))
            .map(|e| (entity_type_name(e), e.as_entity().layer().to_string()))
            .collect();
        let handles = self.current_layout_entity_handles();
        let mut added = 0;
        for h in handles {
            if self.selected.contains(&h) {
                continue;
            }
            if let Some(e) = self.document.get_entity(h) {
                let key = (entity_type_name(e), e.as_entity().layer().to_string());
                if pairs.contains(&key) {
                    self.selected.insert(h);
                    added += 1;
                }
            }
        }
        if added > 0 {
            self.bump_selection();
        }
        added
    }

    /// Replace the selection with its complement: every selectable object
    /// in the active layout that isn't currently selected. The candidate
    /// set is the visible wire set, so objects on off/frozen layers (which
    /// can't be picked anyway) are excluded. Returns the new count.
    pub fn invert_selection(&mut self) -> usize {
        let prev: rustc_hash::FxHashSet<Handle> = self.selected.iter().copied().collect();
        let all: Vec<Handle> = self
            .entity_wires()
            .iter()
            .filter_map(|w| Self::handle_from_wire_name(&w.name))
            .collect();
        self.selected.clear();
        for h in all {
            if !prev.contains(&h) {
                self.selected.insert(h);
            }
        }
        self.bump_selection();
        self.selected.len()
    }

    /// Replaces (or extends, when `append` is true) the current
    /// selection with every entity in the active layout that matches
    /// the filter. Returns the number of newly-matching entities.
    ///
    /// `type_name` of `None` means "any type". `property_field` of
    /// `None` skips the property test (only the type filter applies).
    /// The operator's `Any` variant also skips the property test.
    /// Numeric operators (`Gt` / `Lt`) parse both sides as `f64` and
    /// reject anything non-numeric.
    pub fn qselect(
        &mut self,
        type_name: Option<&str>,
        property_field: Option<&str>,
        op: crate::app::QSelectOp,
        value: &str,
        append: bool,
    ) -> usize {
        use crate::app::QSelectOp;
        use crate::entities::traits::entity_type_name;
        if !append {
            self.selected.clear();
        }
        let handles = self.current_layout_entity_handles();
        let mut matched = 0;
        for h in handles {
            let Some(e) = self.document.get_entity(h) else {
                continue;
            };
            if let Some(t) = type_name {
                if entity_type_name(e) != t {
                    continue;
                }
            }
            let prop_ok = match (property_field, op) {
                (None, _) | (_, QSelectOp::Any) => true,
                (Some(field), op) => {
                    let Some(actual) = self.entity_property_value(e, field) else {
                        continue;
                    };
                    match op {
                        QSelectOp::Eq => actual.eq_ignore_ascii_case(value),
                        QSelectOp::Neq => !actual.eq_ignore_ascii_case(value),
                        QSelectOp::Gt | QSelectOp::Lt => {
                            let (Ok(a), Ok(b)) =
                                (actual.parse::<f64>(), value.parse::<f64>())
                            else {
                                continue;
                            };
                            if matches!(op, QSelectOp::Gt) {
                                a > b
                            } else {
                                a < b
                            }
                        }
                        QSelectOp::Any => true,
                    }
                }
            };
            if prop_ok {
                self.selected.insert(h);
                matched += 1;
            }
        }
        self.bump_selection();
        matched
    }

    /// Returns the sorted set of entity-type names present in the active
    /// layout. Used to populate the Quick Select "Object type" dropdown
    /// with only the types that actually exist in the drawing.
    pub fn entity_type_names_in_layout(&self) -> Vec<&'static str> {
        use crate::entities::traits::entity_type_name;
        let mut names: std::collections::BTreeSet<&'static str> =
            std::collections::BTreeSet::new();
        for h in self.current_layout_entity_handles() {
            if let Some(e) = self.document.get_entity(h) {
                names.insert(entity_type_name(e));
            }
        }
        names.into_iter().collect()
    }

    /// True when `handle`'s entity type is allowed by the selection filter.
    /// The filter stores excluded type names; empty = everything allowed.
    pub fn passes_selection_filter(&self, handle: Handle) -> bool {
        if self.selection_filter.is_empty() {
            return true;
        }
        match self.document.get_entity(handle) {
            Some(e) => !self
                .selection_filter
                .contains(crate::entities::traits::entity_type_name(e)),
            None => true,
        }
    }

    /// True when the selection filter is excluding at least one type.
    pub fn selection_filter_active(&self) -> bool {
        !self.selection_filter.is_empty()
    }

    /// Returns the list of `(field, label)` pairs the Quick Select
    /// "Properties" dropdown should show given the current type filter:
    ///
    /// * Common properties (Layer, Color, Linetype, Lineweight) are
    ///   always included.
    /// * When `type_name` names a specific entity type present in the
    ///   active layout, the first entity of that type contributes its
    ///   `geometry_properties()` rows (Start X, Length, Radius, …) so
    ///   type-specific filtering works.
    pub fn qselect_properties(
        &self,
        type_name: Option<&str>,
    ) -> Vec<(String, String)> {
        use crate::entities::traits::{entity_type_name, EntityTypeOps};
        let mut out: Vec<(String, String)> = vec![
            ("layer".to_string(), "Layer".to_string()),
            ("color".to_string(), "Color".to_string()),
            ("linetype".to_string(), "Linetype".to_string()),
            ("lineweight".to_string(), "Lineweight".to_string()),
        ];
        if let Some(t) = type_name {
            let text_style_names: Vec<String> = self
                .document
                .text_styles
                .iter()
                .map(|s| s.name.clone())
                .collect();
            let sample = self
                .current_layout_entity_handles()
                .into_iter()
                .filter_map(|h| self.document.get_entity(h))
                .find(|e| entity_type_name(e) == t);
            if let Some(sample) = sample {
                if let Some(section) = sample.geometry_properties(&text_style_names) {
                    for prop in section.props {
                        // Skip rows that don't sensibly compare via
                        // `entity_property_value` (read-only labels are
                        // fine — users can still match against them).
                        out.push((prop.field.to_string(), prop.label.clone()));
                    }
                }
            }
        }
        out
    }

    /// Reads a property value from an entity for QSELECT comparison.
    /// Returns the canonical string used as the left-hand side of the
    /// operator test. Common properties have hand-rolled formatting so
    /// `"ByLayer"` / `"7"` / `"0.30mm"` are stable; everything else
    /// goes through `geometry_properties()` and pulls the matching
    /// row's value out.
    pub fn entity_property_value(
        &self,
        entity: &acadrust::EntityType,
        field: &str,
    ) -> Option<String> {
        use crate::entities::traits::EntityTypeOps;
        use crate::scene::model::object::PropValue;
        match field {
            "layer" => Some(entity.common().layer.clone()),
            "color" => Some(Self::format_color(entity.common().color)),
            "linetype" => Some(entity.common().linetype.clone()),
            "lineweight" => Some(Self::format_lineweight(entity.common().line_weight)),
            _ => {
                let text_style_names: Vec<String> = self
                    .document
                    .text_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let section = entity.geometry_properties(&text_style_names)?;
                let prop = section.props.into_iter().find(|p| p.field == field)?;
                Some(match prop.value {
                    PropValue::ReadOnly(s) | PropValue::EditText(s) => s,
                    PropValue::LayerChoice(s) => s,
                    PropValue::Choice { selected, .. } => selected,
                    PropValue::ColorChoice(c) => Self::format_color(c),
                    PropValue::LwChoice(lw) => Self::format_lineweight(lw),
                    PropValue::LinetypeChoice(s) => s,
                    PropValue::HatchPatternChoice(s) => s,
                    PropValue::BoolToggle { value, .. } => value.to_string(),
                    PropValue::ColorVaries | PropValue::LwVaries => return None,
                })
            }
        }
    }

    fn format_color(c: acadrust::types::Color) -> String {
        use acadrust::types::Color;
        match c {
            Color::ByLayer => "ByLayer".to_string(),
            Color::ByBlock => "ByBlock".to_string(),
            Color::Index(i) => i.to_string(),
            Color::Rgb { r, g, b } => format!("{},{},{}", r, g, b),
        }
    }

    fn format_lineweight(lw: acadrust::types::LineWeight) -> String {
        use acadrust::types::LineWeight;
        match lw {
            LineWeight::ByLayer => "ByLayer".to_string(),
            LineWeight::ByBlock => "ByBlock".to_string(),
            LineWeight::Default => "Default".to_string(),
            LineWeight::Value(v) => format!("{:.2}mm", v as f64 / 100.0),
        }
    }

    // ── Erase ─────────────────────────────────────────────────────────────

    pub fn erase_entities(&mut self, handles: &[Handle]) {
        for &h in handles {
            self.document.remove_entity(h);
            self.selected.remove(&h);
            self.hatches.remove(&h);
            self.meshes.remove(&h);
            self.solid_models.remove(&h);
            self.mark_entity_dirty(h);
        }
        // Remove erased handles from all groups; delete groups that become empty.
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let to_remove: Vec<Handle> = self
            .document
            .objects
            .values_mut()
            .filter_map(|obj| match obj {
                ObjectType::Group(g) => {
                    g.entities.retain(|h| !handles.contains(h));
                    if g.entities.is_empty() {
                        Some(g.handle)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        for gh in &to_remove {
            if let Some(ObjectType::Dictionary(dict)) =
                self.document.objects.get_mut(&group_dict_handle)
            {
                dict.entries.retain(|(_, h)| h != gh);
            }
            self.document.objects.remove(gh);
        }
        // Deleting top-level entities/inserts leaves block definitions intact;
        // the erased handles were already dropped from the memo above.
        self.bump_geometry_no_blocks();
    }

    // ── Group helpers ──────────────────────────────────────────────────────

    pub fn groups(&self) -> impl Iterator<Item = &acadrust::objects::Group> {
        self.document.objects.values().filter_map(|obj| match obj {
            ObjectType::Group(g) => Some(g),
            _ => None,
        })
    }

    /// Returns the names of all groups that contain `handle`.
    pub fn group_names_for_entity(&self, handle: Handle) -> Vec<String> {
        self.groups()
            .filter(|g| g.contains(handle))
            .map(|g| g.name.clone())
            .collect()
    }

    /// Creates a named group from the given handles and registers it in the group dictionary.
    pub fn create_group(&mut self, name: String, handles: Vec<Handle>) -> Handle {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let mut group = acadrust::objects::Group::new(&name);
        group.handle = self.document.allocate_handle();
        group.owner = group_dict_handle;
        group.add_entities(handles);
        let gh = group.handle;
        self.document.objects.insert(gh, ObjectType::Group(group));
        if let Some(ObjectType::Dictionary(dict)) =
            self.document.objects.get_mut(&group_dict_handle)
        {
            dict.add_entry(&name, gh);
        }
        gh
    }

    /// Dissolves all groups that contain any of the given handles.
    /// Returns the number of groups removed.
    pub fn delete_groups_containing(&mut self, handles: &[Handle]) -> usize {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let to_delete: Vec<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|obj| match obj {
                ObjectType::Group(g) if handles.iter().any(|h| g.contains(*h)) => Some(g.handle),
                _ => None,
            })
            .collect();
        let count = to_delete.len();
        for gh in &to_delete {
            if let Some(ObjectType::Dictionary(dict)) =
                self.document.objects.get_mut(&group_dict_handle)
            {
                dict.entries.retain(|(_, h)| h != gh);
            }
            self.document.objects.remove(gh);
        }
        count
    }

    /// If `handle` belongs to any selectable groups, also select all other members of those groups.
    pub fn expand_selection_for_groups(&mut self, handles: &[Handle]) {
        let to_add: Vec<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|obj| match obj {
                ObjectType::Group(g) if g.selectable && handles.iter().any(|h| g.contains(*h)) => {
                    Some(g.entities.clone())
                }
                _ => None,
            })
            .flatten()
            .collect();
        for h in to_add {
            self.selected.insert(h);
        }
        self.bump_selection();
    }

    // ── Layer helpers ──────────────────────────────────────────────────────

    pub fn toggle_layer_visibility(&mut self, name: &str) {
        if let Some(layer) = self.document.layers.get_mut(name) {
            layer.flags.off = !layer.flags.off;
        }
        self.bump_geometry();
    }

    pub fn toggle_layer_lock(&mut self, name: &str) {
        if let Some(layer) = self.document.layers.get_mut(name) {
            layer.flags.locked = !layer.flags.locked;
        }
    }

    // ── Modify (transform / copy) ─────────────────────────────────────────

    pub fn transform_entities(&mut self, handles: &[Handle], t: &EntityTransform) {
        // MIRRTEXT (header.mirror_text): when false AutoCAD positions text /
        // mtext / shape by the mirror but keeps the original rotation +
        // oblique so the text stays right-reading. Capture before the
        // transform and re-apply afterwards.
        let preserve_text_orientation =
            matches!(t, EntityTransform::Mirror { .. }) && !self.document.header.mirror_text;
        let mut text_orient_backup: Vec<(Handle, f64, f64, f64)> = Vec::new();
        if preserve_text_orientation {
            for &h in handles {
                if let Some(entity) = self.document.get_entity(h) {
                    match entity {
                        EntityType::Text(t) => {
                            text_orient_backup.push((h, t.rotation, t.oblique_angle, 0.0))
                        }
                        EntityType::MText(m) => {
                            text_orient_backup.push((h, m.rotation, 0.0, 0.0))
                        }
                        EntityType::Shape(s) => text_orient_backup.push((
                            h,
                            s.rotation,
                            s.oblique_angle,
                            s.relative_x_scale,
                        )),
                        _ => {}
                    }
                }
            }
        }
        // A dimension's final geometry is baked into a per-instance `*D`
        // block, and the render draws those sub-entities directly (not the
        // definition points). Transform them with the dimension, or it would
        // stay drawn in place while only its def points move.
        let dim_block_subs: Vec<Handle> = handles
            .iter()
            .filter_map(|&h| match self.document.get_entity(h) {
                Some(EntityType::Dimension(d)) => {
                    let bn = d.base().block_name.clone();
                    if bn.trim().is_empty() {
                        None
                    } else {
                        Some(bn)
                    }
                }
                _ => None,
            })
            .filter_map(|bn| {
                self.document
                    .block_records
                    .iter()
                    .find(|br| br.name.eq_ignore_ascii_case(&bn))
                    .map(|br| br.entity_handles.clone())
            })
            .flatten()
            .collect();
        for &h in handles {
            if let Some(entity) = self.document.get_entity_mut(h) {
                view::dispatch::apply_transform(entity, t);
            }
            if self.hatches.contains_key(&h) {
                let existing_color = self.hatches[&h].color;
                let new_model = match self.document.get_entity(h) {
                    Some(EntityType::Hatch(dxf)) => {
                        Self::hatch_model_from_dxf(dxf, existing_color)
                    }
                    // A DXF SOLID renders as a solid-fill hatch; rebuild it from
                    // the moved corners so the fill follows the transform.
                    Some(EntityType::Solid(s)) => {
                        Some(Self::solid_hatch_model(s, existing_color))
                    }
                    _ => None,
                };
                if let Some(model) = new_model {
                    self.hatches.insert(h, model);
                }
            }
        }
        if preserve_text_orientation {
            for (h, rot, oblique, x_scale) in text_orient_backup {
                if let Some(entity) = self.document.get_entity_mut(h) {
                    match entity {
                        EntityType::Text(t) => {
                            t.rotation = rot;
                            t.oblique_angle = oblique;
                        }
                        EntityType::MText(m) => {
                            m.rotation = rot;
                        }
                        EntityType::Shape(s) => {
                            s.rotation = rot;
                            s.oblique_angle = oblique;
                            s.relative_x_scale = x_scale;
                        }
                        _ => {}
                    }
                }
            }
        }
        // Move the baked dimension-block sub-entities too (collected above).
        for h in &dim_block_subs {
            if let Some(entity) = self.document.get_entity_mut(*h) {
                view::dispatch::apply_transform(entity, t);
            }
        }
        // Only the transformed entities changed (a top-level move/rotate/scale/
        // mirror never edits a block definition) — re-tessellate just those and
        // keep the block cache + every other entity's memoized wires.
        for &h in handles {
            self.mark_entity_dirty(h);
        }
        self.bump_geometry_no_blocks();
    }

    /// Give a freshly-cloned entity brand-new handles for every *inline*
    /// sub-entity that stores one (INSERT attributes, 3D-polyline vertices).
    /// `document.add_entity` only assigns the top-level handle, so without this
    /// a copy keeps its source's sub-handles — duplicate handles that corrupt
    /// the saved DWG (file won't reopen in other CAD apps). Vertices that don't
    /// store a handle (LwPolyline / heavy 2D polyline) get one from the writer,
    /// so they need no fix-up here. (#129)
    fn reset_clone_subhandles(doc: &mut acadrust::CadDocument, entity: &mut EntityType) {
        match entity {
            EntityType::Insert(ins) => {
                for att in ins.attributes.iter_mut() {
                    att.common.handle = doc.allocate_handle();
                }
            }
            EntityType::Polyline3D(p) => {
                for v in p.vertices.iter_mut() {
                    v.handle = doc.allocate_handle();
                }
            }
            _ => {}
        }
    }

    /// Add a freshly-cloned entity, allocating a new handle for it *and* every
    /// inline sub-entity so the copy never shares a handle with its source.
    /// Use this (not `add_entity`) whenever inserting a duplicate. (#129)
    pub fn add_entity_clone(&mut self, mut entity: EntityType) -> Handle {
        Self::reset_clone_subhandles(&mut self.document, &mut entity);
        entity.common_mut().handle = Handle::NULL;
        self.add_entity(entity)
    }

    /// Duplicate the anonymous block `src_name`, transforming every sub-entity
    /// by `t`, and return the new block's name. A dimension's drawn geometry
    /// lives in such a baked `*D` block, so a copied dimension needs its own
    /// transformed block — otherwise it still references the source block and
    /// renders on top of the original instead of at the copy. Returns None when
    /// the source block is missing or empty. (#161)
    fn clone_transformed_block(&mut self, src_name: &str, t: &EntityTransform) -> Option<String> {
        let sub_handles = self
            .document
            .block_records
            .iter()
            .find(|br| br.name.eq_ignore_ascii_case(src_name))
            .map(|br| br.entity_handles.clone())?;
        if sub_handles.is_empty() {
            return None;
        }
        // Smallest free `*D<n>` anonymous name.
        let mut n = 0u64;
        let new_name = loop {
            let cand = format!("*D{n}");
            if self.document.block_records.get(&cand).is_none() {
                break cand;
            }
            n += 1;
        };
        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);
        let mut br = acadrust::tables::BlockRecord::new(&new_name);
        br.handle = br_handle;
        br.block_entity_handle = block_handle;
        br.block_end_handle = end_handle;
        self.document.block_records.add(br).ok()?;
        let mut block = Block::new(&new_name, acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document.add_entity(EntityType::Block(block)).ok()?;
        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document.add_entity(EntityType::BlockEnd(block_end)).ok()?;
        for sh in sub_handles {
            if let Some(mut sub) = self.document.get_entity(sh).cloned() {
                view::dispatch::apply_transform(&mut sub, t);
                Self::reset_clone_subhandles(&mut self.document, &mut sub);
                sub.common_mut().handle = Handle::NULL;
                sub.common_mut().owner_handle = br_handle;
                let _ = self.document.add_entity(sub);
            }
        }
        Some(new_name)
    }

    pub fn copy_entities(&mut self, handles: &[Handle], t: &EntityTransform) -> Vec<Handle> {
        let clones: Vec<EntityType> = handles
            .iter()
            .filter_map(|&h| self.document.get_entity(h).cloned())
            .collect();
        let mut new_handles = Vec::with_capacity(clones.len());
        for mut entity in clones {
            view::dispatch::apply_transform(&mut entity, t);
            // A dimension draws from its baked `*D` block; give the copy its own
            // transformed block so it lands at the copy position rather than
            // rendering on top of the source. (#161)
            if let EntityType::Dimension(d) = &entity {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    if let Some(new_bn) = self.clone_transformed_block(&bn, t) {
                        if let EntityType::Dimension(d) = &mut entity {
                            d.base_mut().block_name = new_bn;
                        }
                    }
                }
            }
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            let h = self.document.add_entity(entity).unwrap_or(Handle::NULL);
            if !h.is_null() {
                let new_model = match self.document.get_entity(h) {
                    Some(EntityType::Hatch(dxf)) => {
                        let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color)
                    }
                    Some(EntityType::Solid(s)) => {
                        let color = convert::tess_util::aci_to_rgba(&s.common.color);
                        Some(Self::solid_hatch_model(s, color))
                    }
                    _ => None,
                };
                if let Some(model) = new_model {
                    self.hatches.insert(h, model);
                }
            }
            new_handles.push(h);
        }
        // The copies are new handles (natural memo misses, tessellated fresh)
        // and reference only already-cached blocks — no block defn changes.
        self.bump_geometry_no_blocks();
        new_handles
    }

    // ── Grip editing ──────────────────────────────────────────────────────

    pub fn apply_grip(&mut self, handle: Handle, grip_id: usize, apply: GripApply) {
        // For Solid3D / Region / Body, record the old point_of_reference so we
        // can translate the pre-tessellated MeshModel by the same delta after
        // the grip is applied (the ACIS data itself is not modified).
        let old_por: Option<[f64; 3]> = self
            .document
            .get_entity(handle)
            .and_then(crate::entities::solid3d::point_of_reference)
            .map(|p| [p.x, p.y, p.z]);

        if let Some(entity) = self.document.get_entity_mut(handle) {
            view::dispatch::apply_grip(entity, grip_id, apply);
        }

        // Translate MeshModel vertices by the same delta the grip applied.
        if let Some(old) = old_por {
            let new_por: Option<[f64; 3]> = self
                .document
                .get_entity(handle)
                .and_then(crate::entities::solid3d::point_of_reference)
                .map(|p| [p.x, p.y, p.z]);
            if let Some(new) = new_por {
                let dx = (new[0] - old[0]) as f32;
                let dy = (new[1] - old[1]) as f32;
                let dz = (new[2] - old[2]) as f32;
                if let Some(set) = self.meshes.get_mut(&handle) {
                    for lod in &mut set.lods {
                        for v in &mut lod.verts {
                            v[0] += dx;
                            v[1] += dy;
                            v[2] += dz;
                        }
                    }
                    set.world_aabb[0] += dx;
                    set.world_aabb[1] += dy;
                    set.world_aabb[2] += dx;
                    set.world_aabb[3] += dy;
                }
            }
        }

        // Rebuild GPU hatch/solid model when a boundary vertex or corner moves.
        match self.document.get_entity(handle) {
            Some(EntityType::Hatch(dxf)) => {
                let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                if let Some(model) = Self::hatch_model_from_dxf(dxf, color) {
                    self.hatches.insert(handle, model);
                } else {
                    self.hatches.remove(&handle);
                }
            }
            Some(EntityType::Solid(solid)) => {
                let color = convert::tess_util::aci_to_rgba(&solid.common.color);
                self.hatches
                    .insert(handle, Self::solid_hatch_model(solid, color));
            }
            _ => {}
        }
        // NOTE: no `bump_geometry()` here. The grip-drag caller hides the
        // edited entity and previews it as an overlay during the drag (so a
        // move doesn't re-tessellate the whole model), then bumps once on
        // commit. Any other caller must bump geometry itself.
    }

    // ── Hit-test convenience: wire name → Handle ──────────────────────────

    pub fn handle_from_wire_name(name: &str) -> Option<Handle> {
        name.parse::<u64>().ok().map(Handle::new)
    }

    /// Restore camera to a named view from the document view table.
    pub fn restore_named_view(&mut self, view: &acadrust::tables::View) {
        use glam::Vec3;
        let cam = &mut *self.camera.borrow_mut();
        // view.target is the look-at point; view.direction is eye→target direction.
        cam.target = glam::DVec3::new(view.target.x, view.target.y, view.target.z);
        // direction in acadrust = from-target-to-eye (same as AutoCAD convention).
        let eye_dir = Vec3::new(
            view.direction.x as f32,
            view.direction.y as f32,
            view.direction.z as f32,
        );
        let eye_dir = if eye_dir.length_squared() > 1e-10 {
            eye_dir.normalize()
        } else {
            Vec3::Z
        };
        // Build rotation: canonical eye is +Z, rotate to eye_dir.
        cam.rotation = glam::Quat::from_rotation_arc(Vec3::Z, eye_dir);
        // Sync yaw/pitch from new rotation (for ViewCube).
        let pitch = eye_dir.z.clamp(-1.0, 1.0).asin();
        let yaw = eye_dir.x.atan2(eye_dir.y);
        cam.yaw = yaw;
        cam.pitch = pitch;
        // Derive distance from view height and fov.
        let h = view.height as f32;
        cam.distance = if h > 0.0 {
            h / (2.0 * (cam.fov_y * 0.5).tan())
        } else {
            cam.distance
        };
        self.camera_generation += 1;
    }

    /// Save the current camera state into a new named view entry.
    /// Returns the view; caller must push it into document.views.
    pub fn current_as_named_view(&self, name: &str) -> acadrust::tables::View {
        use acadrust::types::Vector3;
        let cam = self.camera.borrow();
        let eye_dir = cam.rotation * glam::Vec3::Z;
        let height = cam.ortho_size() * 2.0;
        let width = height; // caller can adjust; rough square
        acadrust::tables::View {
            handle: acadrust::types::Handle::NULL,
            name: name.to_string(),
            center: Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: 0.0,
            },
            target: Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: cam.target.z as f64,
            },
            direction: Vector3 {
                x: eye_dir.x as f64,
                y: eye_dir.y as f64,
                z: eye_dir.z as f64,
            },
            height: height as f64,
            width: width as f64,
            lens_length: 50.0,
            front_clip: 0.0,
            back_clip: 0.0,
            twist_angle: 0.0,
        }
    }

    /// Zoom the model-space camera in/out by a percentage.
    /// factor > 1 = zoom out, factor < 1 = zoom in.
    pub fn zoom_camera(&mut self, factor: f32) {
        let mut cam = self.camera.borrow_mut();
        cam.distance = (cam.distance * factor).max(0.001);
        drop(cam);
        self.camera_generation += 1;
    }

    /// Fit the camera to a world-space bounding box (corners p1, p2).
    pub fn zoom_to_window(&mut self, p1: glam::Vec3, p2: glam::Vec3) {
        let min = p1.min(p2);
        let max = p1.max(p2);
        if min == max {
            return;
        }
        self.camera.borrow_mut().fit_to_bounds(min, max);
        self.camera_generation += 1;
    }

    /// Apply camera state from an acadrust View table entry, through the shared
    /// `camera_from_view` decoder so the twist round-trips like every other
    /// saved view. `model_space`: if true, subtracts world_offset from target
    /// (wire-space); paper-space entries carry no offset.
    fn apply_camera_from_view_entry(
        &mut self,
        view: &acadrust::tables::View,
        model_space: bool,
    ) -> bool {
        let _ = model_space;
        let Some(cam) = self.camera_from_view(
            view.direction,
            view.target,
            acadrust::types::Vector2 {
                x: view.center.x,
                y: view.center.y,
            },
            view.height,
            view.twist_angle,
        ) else {
            return false;
        };
        *self.camera.borrow_mut() = cam;
        self.camera_generation += 1;
        true
    }

    /// Set the model-space camera from the VPORT table's *Active entry.
    /// Returns true if the entry was found and the camera was set.
    fn apply_active_vport_camera(&mut self) -> bool {
        // Restore the single tile's visual style + grid/snap from the *Active
        // entry, independent of where the camera itself comes from below.
        if let Some(vp) = self.document.vports.iter().find(|v| v.name == "*Active") {
            let mode = vp.render_mode;
            let (grid_on, snap_on) = (vp.grid_on, vp.snap_on);
            let mut tiles = self.model_tiles.borrow_mut();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            if let Some(t) = tiles.get_mut(active) {
                t.render_mode = mode;
                t.grid_on = grid_on;
                t.snap_on = snap_on;
            }
        }
        // Prefer our named View entry — survives DWG save without being overridden.
        let saved_view = self
            .document
            .views
            .iter()
            .find(|v| v.name == "OpenCADStudio_Camera_Model")
            .cloned();
        if let Some(view) = saved_view {
            return self.apply_camera_from_view_entry(&view, true);
        }
        let vp = match self.document.vports.iter().find(|v| v.name == "*Active") {
            Some(v) => v.clone(),
            None => return false,
        };
        let Some(new_cam) = self.camera_from_vport(&vp) else {
            return false;
        };
        *self.camera.borrow_mut() = new_cam;
        self.camera_generation += 1;
        true
    }

    /// Decode a saved view into a `Camera`. This is the single shared decoder
    /// for both a model-space VPORT table entry (tiled) and a paper-space
    /// VIEWPORT entity (floating): tiled vs floating only changes *where* the
    /// fields come from and the floating auto-fit fallback — the projection
    /// math (view direction → yaw/pitch, twist → roll, view_center fold,
    /// view_height → distance) is identical, so it lives here once. Callers pass
    /// their already-effective `view_target` / `view_center` / `view_height`.
    ///
    /// Returns `None` for a zero `view_height` (an uninitialised entry).
    fn camera_from_view(
        &self,
        view_direction: acadrust::types::Vector3,
        view_target: acadrust::types::Vector3,
        view_center: acadrust::types::Vector2,
        view_height: f64,
        twist: f64,
        // Subtracted from `view_target` to reach wire-space. Model views pass
        // `[0.0_f64; 3]`; paper-space views (whose entities carry no
        // offset) pass `[0; 3]`.
    ) -> Option<Camera> {
        if view_height.abs() < 1e-9 {
            return None;
        }
        let vd = glam::Vec3::new(
            view_direction.x as f32,
            view_direction.y as f32,
            view_direction.z as f32,
        )
        .normalize_or(glam::Vec3::Z);
        let pitch = vd.z.clamp(-1.0, 1.0).asin();
        // view_dir = (sin(yaw)*cos(pitch), -cos(yaw)*cos(pitch), sin(pitch))
        // → yaw = atan2(x, -y), but when looking straight up/down cos(pitch)≈0
        //   both x and y are near zero and atan2(0, -0.0) = π due to IEEE 754.
        let yaw = if vd.x.abs() < 1e-6 && vd.y.abs() < 1e-6 {
            0.0_f32 // plan/nadir view: yaw is undefined, default to 0
        } else {
            vd.x.atan2(-vd.y)
        };
        // The saved view can carry a twist (rotation about the view axis), set
        // when the view was aligned to a rotated UCS. The twist is the angle
        // that rotates world-X onto screen-right, so the world direction that
        // ends up horizontal is its negative; feed that in as the camera roll
        // so the drawing opens square, the way it was saved, instead of in raw
        // world orientation (which looks tilted).
        let rotation = view::camera::yaw_pitch_to_quat(yaw, pitch, -twist as f32);
        let view_right = rotation * glam::Vec3::X;
        let view_up = rotation * glam::Vec3::Y;
        // view_target is WCS; wire-space subtracts world_offset. view_center is
        // a DCS (screen-plane) offset, so fold it through the view basis.
        // Keep the target in f64: casting it to f32 first quantizes the camera
        // to ~0.5 m at UTM scale, so panning/zooming inside a floating viewport
        // (which nudges view_target by sub-metre f64 steps) made the content
        // jump on the f32 grid. The axis directions stay f32 (orientation only);
        // only the position must stay precise — matching the model camera.
        let base = glam::DVec3::new(
            view_target.x,
            view_target.y,
            view_target.z,
        );
        let target = base
            + view_right.as_dvec3() * view_center.x
            + view_up.as_dvec3() * view_center.y;
        let fov_y = 45.0_f32.to_radians();
        let distance = ((view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);
        Some(Camera {
            target,
            rotation,
            distance,
            fov_y,
            projection: view::camera::Projection::Orthographic,
            yaw,
            pitch,
        })
    }

    /// Decode a VPort table entry (model-space tiled view) into a `Camera`.
    fn camera_from_vport(&self, vp: &acadrust::tables::VPort) -> Option<Camera> {
        self.camera_from_view(
            vp.view_direction,
            vp.view_target,
            vp.view_center,
            vp.view_height,
            vp.view_twist,
        )
    }

    /// Reverse of `camera_from_vport`: write `cam`'s view target / direction
    /// / height onto a fresh VPort entry with the given `name` and screen
    /// rectangle (0..1 normalized, DXF bottom-left origin convention).
    fn vport_from_camera(
        &self,
        name: &str,
        cam: &Camera,
        lower_left: acadrust::types::Vector2,
        upper_right: acadrust::types::Vector2,
    ) -> acadrust::tables::VPort {
        let view_dir = cam.rotation * glam::Vec3::Z;
        let view_height = cam.ortho_size() * 2.0;
        let target_wcs = acadrust::types::Vector3 {
            x: (cam.target.x as f64) + [0.0_f64; 3][0],
            y: (cam.target.y as f64) + [0.0_f64; 3][1],
            z: (cam.target.z as f64) + [0.0_f64; 3][2],
        };
        let mut entry = acadrust::tables::VPort::new(name);
        entry.lower_left = lower_left;
        entry.upper_right = upper_right;
        entry.view_target = target_wcs;
        entry.view_direction = acadrust::types::Vector3 {
            x: view_dir.x as f64,
            y: view_dir.y as f64,
            z: view_dir.z as f64,
        };
        entry.view_height = view_height as f64;
        entry.view_center = acadrust::types::Vector2::ZERO;
        // Stored twist = -roll, matching the decoder (roll = -twist).
        entry.view_twist = -cam.roll() as f64;
        entry
    }

    /// Convert a `ModelTile`'s normalized iced rectangle (top-left origin) to
    /// the (lower_left, upper_right) pair the VPort table uses (bottom-left
    /// origin).
    fn tile_rect_to_vport(rect: iced::Rectangle) -> (acadrust::types::Vector2, acadrust::types::Vector2) {
        let lower_left = acadrust::types::Vector2 {
            x: rect.x as f64,
            y: (1.0 - rect.y - rect.height) as f64,
        };
        let upper_right = acadrust::types::Vector2 {
            x: (rect.x + rect.width) as f64,
            y: (1.0 - rect.y) as f64,
        };
        (lower_left, upper_right)
    }

    /// Inverse of `tile_rect_to_vport`.
    fn vport_to_tile_rect(lower_left: acadrust::types::Vector2, upper_right: acadrust::types::Vector2) -> iced::Rectangle {
        iced::Rectangle {
            x: lower_left.x as f32,
            y: (1.0 - upper_right.y) as f32,
            width: (upper_right.x - lower_left.x) as f32,
            height: (upper_right.y - lower_left.y) as f32,
        }
    }

    /// Restore `model_tiles` from VPort entries that a previous save left in
    /// the document. Native AutoCAD tiled model-space layouts are represented
    /// by duplicate `*Active` VPort entries.
    /// Returns true on success — the caller skips `apply_active_vport_camera`
    /// in that case because the active tile's camera has already been loaded
    /// into `self.camera`.
    fn restore_model_tiles_from_vports(&mut self) -> bool {
        let active_vports: Vec<acadrust::tables::VPort> = self
            .document
            .vports
            .iter()
            .filter(|v| v.name == "*Active")
            .cloned()
            .collect();

        if active_vports.len() <= 1 {
            return false;
        }

        let tiles: Vec<ModelTile> = active_vports
            .iter()
            .filter_map(|vp| {
                self.camera_from_vport(vp).map(|cam| ModelTile {
                    rect: Self::vport_to_tile_rect(vp.lower_left, vp.upper_right),
                    camera: cam,
                    render_mode: vp.render_mode,
                    grid_on: vp.grid_on,
                    snap_on: vp.snap_on,
                })
            })
            .collect();

        if tiles.len() <= 1 {
            return false;
        }

        let active_cam = tiles[0].camera.clone();
        *self.model_tiles.borrow_mut() = tiles;
        self.active_model_tile.set(0);
        *self.camera.borrow_mut() = active_cam;
        self.camera_generation += 1;
        true
    }

    /// Persist `model_tiles` to the VPort table. Native AutoCAD tiled model
    /// viewports are written as duplicate `*Active` entries.
    fn save_model_tiles_to_vports(&mut self) {
        // Stash the live camera into the active tile so the about-to-write
        // snapshot reflects the user's most recent orbit / pan / zoom.
        {
            let live_cam = self.camera.borrow().clone();
            let mut tiles = self.model_tiles.borrow_mut();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            if let Some(t) = tiles.get_mut(active) {
                t.camera = live_cam;
            }
        }

        let table_handle = self.document.vports.handle();
        let preserved_vps: Vec<acadrust::tables::VPort> = self
            .document
            .vports
            .iter()
            .filter(|v| v.name != "*Active")
            .cloned()
            .collect();
        let mut new_vports = acadrust::tables::Table::with_handle(table_handle);
        for vp in preserved_vps {
            new_vports.add_or_replace(vp);
        }
        self.document.vports = new_vports;

        let tiles = self.model_tiles.borrow().clone();
        if tiles.is_empty() {
            return;
        }

        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        let mut ordered_tiles = Vec::with_capacity(tiles.len());
        ordered_tiles.push(tiles[active].clone());
        for (i, tile) in tiles.iter().enumerate() {
            if i != active {
                ordered_tiles.push(tile.clone());
            }
        }

        for tile in ordered_tiles {
            let (ll, ur) = Self::tile_rect_to_vport(tile.rect);
            let mut entry = self.vport_from_camera("*Active", &tile.camera, ll, ur);
            entry.render_mode = tile.render_mode;
            // Each viewport persists its own grid display + grid-snap (#121).
            entry.grid_on = tile.grid_on;
            entry.snap_on = tile.snap_on;
            entry.handle = self.document.allocate_handle();
            self.document.vports.add_allow_duplicate(entry);
        }
    }

    /// Mirror the live grid/snap toggles onto the active view's own store so the
    /// state stays independent per viewport: a model tile in model space, the
    /// layout's sheet viewport in paper space. (#121)
    pub fn set_active_tile_grid_snap(&mut self, grid_on: bool, snap_on: bool) {
        if self.current_layout != "Model" {
            // Paper space: target the active floating viewport if the user is
            // working inside one, otherwise the layout's sheet viewport. Each
            // viewport keeps its own grid/snap (round-tripped via status flags).
            let h = self
                .active_viewport
                .filter(|h| h.is_valid())
                .unwrap_or_else(|| self.current_layout_sheet_viewport_handle());
            if h.is_valid() {
                if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(h) {
                    vp.status.grid_on = grid_on;
                    vp.status.snap_on = snap_on;
                }
            }
            return;
        }
        let mut tiles = self.model_tiles.borrow_mut();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        if let Some(t) = tiles.get_mut(active) {
            t.grid_on = grid_on;
            t.snap_on = snap_on;
        }
    }

    /// The active view's grid display + grid-snap, adopted into the live toggles
    /// on load and whenever the active viewport / tab / layout changes. Reads the
    /// model tile in model space, the sheet viewport in paper space. (#121)
    pub fn active_tile_grid_snap(&self) -> Option<(bool, bool)> {
        if self.current_layout != "Model" {
            let h = self
                .active_viewport
                .filter(|h| h.is_valid())
                .unwrap_or_else(|| self.current_layout_sheet_viewport_handle());
            if h.is_valid() {
                if let Some(EntityType::Viewport(vp)) = self.document.get_entity(h) {
                    return Some((vp.status.grid_on, vp.status.snap_on));
                }
            }
            return Some((false, false));
        }
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        tiles.get(active).map(|t| (t.grid_on, t.snap_on))
    }

    /// Set the paper-space camera from the sheet viewport's stored view.
    /// Returns true if a valid sheet viewport was found and the camera was set.
    ///
    /// The sheet viewport entity is the authoritative paper-space view (it
    /// round-trips through both the DXF and DWG writers). An older
    /// `OpenCADStudio_Camera_<layout>` named View is honoured only as a
    /// backward-compatible fallback for files saved under the previous scheme.
    fn apply_sheet_viewport_camera(&mut self) -> bool {
        let layout_block = self.current_layout_block_handle();
        let sheet_vp = if layout_block.is_null() {
            None
        } else {
            self.document
                .entities()
                .filter_map(|e| {
                    if let EntityType::Viewport(vp) = e {
                        Some(vp)
                    } else {
                        None
                    }
                })
                .find(|vp| {
                    vp.common.owner_handle == layout_block
                        && !self.is_content_viewport_in_layout(vp, layout_block)
                })
                .cloned()
        };

        let vp = match sheet_vp {
            Some(v) if v.view_height.abs() >= 1e-9 => v,
            _ => {
                // Back-compat: files OCS saved with the named-View side-channel.
                let view_name = format!("OpenCADStudio_Camera_{}", self.current_layout);
                let fallback =
                    self.document.views.iter().find(|v| v.name == view_name).cloned();
                if let Some(view) = fallback {
                    return self.apply_camera_from_view_entry(&view, false);
                }
                return false;
            }
        };

        // Paper-space entities carry no world_offset → decode with a zero
        // offset, through the same shared decoder (twist included).
        let Some(cam) = self.camera_from_view(
            vp.view_direction,
            vp.view_target,
            acadrust::types::Vector2 {
                x: vp.view_center.x,
                y: vp.view_center.y,
            },
            vp.view_height,
            vp.twist_angle,
        ) else {
            return false;
        };
        *self.camera.borrow_mut() = cam;
        self.camera_generation += 1;
        true
    }

    /// Write the current camera back into the document (VPort or sheet viewport)
    /// so it is saved with the file. Returns true if the document was modified.
    pub fn sync_camera_to_document(&mut self) -> bool {
        let cam = self.camera.borrow().clone();
        let view_dir = cam.rotation * glam::Vec3::Z;
        let view_height = cam.ortho_size() * 2.0;
        // Stored twist is the negative of the camera roll (the decoder applies
        // roll = -twist), so the saved view round-trips square.
        let twist = -cam.roll() as f64;
        let vd3 = acadrust::types::Vector3 {
            x: view_dir.x as f64,
            y: view_dir.y as f64,
            z: view_dir.z as f64,
        };

        if self.current_layout == "Model" {
            let target_wcs = acadrust::types::Vector3 {
                x: (cam.target.x as f64) + [0.0_f64; 3][0],
                y: (cam.target.y as f64) + [0.0_f64; 3][1],
                z: (cam.target.z as f64) + [0.0_f64; 3][2],
            };

            // Write back to the *Active VPort entry (may be overridden by DWG writer).
            if let Some(vp) = self
                .document
                .vports
                .iter_mut()
                .find(|v| v.name == "*Active")
            {
                vp.view_target = target_wcs;
                vp.view_center = acadrust::types::Vector2::ZERO;
                vp.view_direction = vd3;
                vp.view_height = view_height as f64;
                vp.view_twist = twist;
            }

            // Persist the tiled layout as duplicate `*Active` VPort entries.
            self.save_model_tiles_to_vports();

            // Also write to View table — survives DWG save without override.
            self.write_camera_view_entry(
                "OpenCADStudio_Camera_Model",
                target_wcs,
                vd3,
                view_height,
                twist,
            );
            true
        } else {
            let target_wcs = acadrust::types::Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: cam.target.z as f64,
            };

            // The sheet viewport entity is the authoritative paper-space view;
            // it round-trips natively, so no named-View side-channel is needed.
            let layout_block = self.current_layout_block_handle();
            if !layout_block.is_null() {
                let sheet_handle = self
                    .document
                    .entities()
                    .filter_map(|e| {
                        if let EntityType::Viewport(vp) = e {
                            Some(vp)
                        } else {
                            None
                        }
                    })
                    .find(|vp| {
                        vp.common.owner_handle == layout_block && !self.is_content_viewport_in_layout(vp, layout_block)
                    })
                    .map(|vp| vp.common.handle);

                if let Some(handle) = sheet_handle {
                    if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(handle) {
                        // AutoCAD stores the paper-space view position in
                        // `view_center` (DCS) with `view_target` at the origin —
                        // writing it the other way round shifts the layout and
                        // crashes nothing but renders the sheet off-place. Paper
                        // space is always a plan view, so DCS == WCS XY here.
                        vp.view_center =
                            acadrust::types::Vector3::new(target_wcs.x, target_wcs.y, 0.0);
                        vp.view_target = acadrust::types::Vector3::ZERO;
                        vp.view_direction = vd3;
                        vp.view_height = view_height as f64;
                        vp.twist_angle = twist;
                    }
                }
            }
            true
        }
    }

    /// Upsert a named View entry with the given camera fields.
    fn write_camera_view_entry(
        &mut self,
        name: &str,
        target: acadrust::types::Vector3,
        direction: acadrust::types::Vector3,
        height: f32,
        twist: f64,
    ) {
        let existing_handle = self
            .document
            .views
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.handle);
        let mut entry = acadrust::tables::View::new(name);
        entry.handle = existing_handle.unwrap_or_else(|| self.document.allocate_handle());
        entry.target = target;
        entry.direction = direction;
        entry.height = height as f64;
        entry.width = height as f64;
        entry.center = acadrust::types::Vector3::ZERO;
        entry.twist_angle = twist;
        self.document.views.add_or_replace(entry);
    }

    /// Restore the camera from the file's saved view (called once on open).
    /// Falls back to fit_all() if no saved view is available.
    pub fn restore_saved_camera(&mut self) {
        let restored = if self.current_layout == "Model" {
            // Tiled-layout restore takes precedence — it sets the camera too.
            // Single-tile files fall through to the *Active branch.
            self.restore_model_tiles_from_vports() || self.apply_active_vport_camera()
        } else {
            // Every paper layout has a full-screen sheet viewport that holds
            // its view; create one if a loaded file lacks it.
            let layout = self.current_layout.clone();
            self.ensure_sheet_viewport(&layout);
            self.apply_sheet_viewport_camera()
        };
        if !restored {
            self.fit_all();
        }
    }

    pub fn fit_all(&mut self) {
        // Use the FULL, un-culled wire set — not `entity_wires()`, which is
        // frustum-culled to the current view. Culled input would fit only the
        // entities already on screen, so each call would zoom out a little and
        // reveal more, converging on the true extent only after several uses
        // (issue #51). `wpp = None` also tessellates at a fixed tolerance so
        // the bounds don't drift with zoom-adaptive curve sampling.
        let layout_block = self.current_layout_block_handle();
        let mut wires = self.wires_for_block_culled(layout_block, None, None, None, None);
        if self.current_layout != "Model" {
            wires.extend(self.viewport_content_wires(layout_block, None, None));
        }
        // 3D solids render as meshes, not wires, so collect their (offset-rel)
        // XY AABBs separately — a drawing of only solids has no wires to fit.
        let mesh_aabbs: Vec<[f32; 4]> = self
            .meshes
            .iter()
            .filter(|(h, _)| {
                self.document
                    .get_entity(**h)
                    .map(|e| e.common().owner_handle == layout_block)
                    .unwrap_or(false)
            })
            .map(|(_, set)| set.world_aabb)
            .filter(|a| a[0].is_finite() && a[2].is_finite())
            .collect();
        if wires.is_empty() && mesh_aabbs.is_empty() {
            return;
        }

        // Per-wire centroid pass — used both for the absolute-magnitude reject
        // (`local_extent_max`) and for the IQR-based outlier reject below.
        // A wire whose centroid sits far outside the drawing's consensus
        // cluster is an orphan (block-defn entity that leaked into MSPACE,
        // bogus hatch boundary, Ray/XLine far point) and must not poison the
        // bounding box.
        struct WireCent {
            idx: usize,
            cx: f32,
            cy: f32,
        }
        let lim = self.local_extent_max;
        let mut cents: Vec<WireCent> = Vec::with_capacity(wires.len());
        for (idx, wire) in wires.iter().enumerate() {
            let mut sx = 0.0_f64;
            let mut sy = 0.0_f64;
            let mut n = 0_usize;
            for &[x, y, _] in &wire.points {
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                sx += x as f64;
                sy += y as f64;
                n += 1;
            }
            if n > 0 {
                cents.push(WireCent {
                    idx,
                    cx: (sx / n as f64) as f32,
                    cy: (sy / n as f64) as f32,
                });
            }
        }
        if cents.is_empty() && mesh_aabbs.is_empty() {
            return;
        }

        // Robust drawing centre (median centroid). `lim` is a span RELATIVE to
        // this centre, so every reject below is distance-from-centre — geometry
        // now reaches fit_all as absolute coordinates (no world_offset), which
        // at UTM scale are ~5.7e6; an absolute `|x| > lim` test would reject the
        // entire drawing and make ZOOM Extents a no-op.
        let (mcx, mcy) = {
            let mut xs: Vec<f32> = cents.iter().map(|c| c.cx).collect();
            let mut ys: Vec<f32> = cents.iter().map(|c| c.cy).collect();
            if xs.is_empty() {
                (0.0_f32, 0.0_f32)
            } else {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                (xs[xs.len() / 2], ys[ys.len() / 2])
            }
        };

        // IQR-based reject only kicks in with enough samples for the quartiles
        // to be meaningful. Below that, the centre-relative `lim` filter is the
        // only gate (legacy behavior).
        let (rx_lo, rx_hi, ry_lo, ry_hi) = if cents.len() >= 8 {
            let mut xs: Vec<f32> = cents.iter().map(|c| c.cx).collect();
            let mut ys: Vec<f32> = cents.iter().map(|c| c.cy).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q = |v: &[f32], frac: f32| v[((v.len() as f32 - 1.0) * frac) as usize];
            let q1x = q(&xs, 0.25);
            let q3x = q(&xs, 0.75);
            let q1y = q(&ys, 0.25);
            let q3y = q(&ys, 0.75);
            // k=10× the inter-quartile span is permissive enough to keep
            // legitimate sparse outlying geometry (annotation labels, scattered
            // dim leaders) but tight enough to drop a single wire stranded at
            // -world_offset. `max(1.0)` guards against a degenerate IQR=0
            // (e.g. all wires at the same centroid).
            const K: f32 = 10.0;
            let dx = (q3x - q1x).max(1.0) * K;
            let dy = (q3y - q1y).max(1.0) * K;
            (q1x - dx, q3x + dx, q1y - dy, q3y + dy)
        } else {
            (mcx - lim, mcx + lim, mcy - lim, mcy + lim)
        };

        let mut min = glam::Vec3::splat(f32::MAX);
        let mut max = glam::Vec3::splat(f32::MIN);
        for c in &cents {
            if c.cx < rx_lo || c.cx > rx_hi || c.cy < ry_lo || c.cy > ry_hi {
                continue;
            }
            let wire = &wires[c.idx];
            for &[x, y, z] in &wire.points {
                if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    continue;
                }
                if (x - mcx).abs() > lim || (y - mcy).abs() > lim {
                    continue;
                }
                min = min.min(glam::Vec3::new(x, y, z));
                max = max.max(glam::Vec3::new(x, y, z));
            }
        }
        // Fold in 3D-solid mesh AABBs (not subject to the wire IQR reject).
        for [ax, ay, bx, by] in &mesh_aabbs {
            min = min.min(glam::Vec3::new(*ax, *ay, 0.0));
            max = max.max(glam::Vec3::new(*bx, *by, 0.0));
        }
        // If no usable points found, leave the camera unchanged.
        if min.x > max.x {
            return;
        }
        if min == max {
            max += glam::Vec3::splat(1.0);
        }
        self.camera.borrow_mut().fit_to_bounds(min, max);
        self.camera_generation += 1;
    }

    pub fn update(&mut self, _dt: Duration) {}

    // ── Paper-space coordinate helpers ───────────────────────────────────

    /// Discover the inner divider edges between Model tiles. Each entry
    /// is one draggable horizontal or vertical edge, with the span along
    /// the perpendicular axis that the edge actually covers (the union
    /// of touching tiles' extents). Coordinates are in normalized 0..1
    /// canvas space. Returns an empty list outside Model or for a
    /// single-tile layout.
    pub fn model_tile_edges(&self) -> Vec<TileEdge> {
        if self.current_layout != "Model" {
            return vec![];
        }
        let tiles = self.model_tiles.borrow();
        if tiles.len() < 2 {
            return vec![];
        }
        let mut out = Vec::new();
        // Collect candidate inner x's: any tile edge that's strictly
        // inside (0, 1). Dedup by epsilon.
        let mut xs: Vec<f32> = Vec::new();
        let mut ys: Vec<f32> = Vec::new();
        for t in tiles.iter() {
            for x in [t.rect.x, t.rect.x + t.rect.width] {
                if x > TILE_EPS && x < 1.0 - TILE_EPS {
                    xs.push(x);
                }
            }
            for y in [t.rect.y, t.rect.y + t.rect.height] {
                if y > TILE_EPS && y < 1.0 - TILE_EPS {
                    ys.push(y);
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        xs.dedup_by(|a, b| (*a - *b).abs() < TILE_EPS);
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ys.dedup_by(|a, b| (*a - *b).abs() < TILE_EPS);
        for x in xs {
            let mut y0 = f32::INFINITY;
            let mut y1 = f32::NEG_INFINITY;
            let mut has_left = false;
            let mut has_right = false;
            for t in tiles.iter() {
                if ((t.rect.x + t.rect.width) - x).abs() < TILE_EPS {
                    has_left = true;
                    y0 = y0.min(t.rect.y);
                    y1 = y1.max(t.rect.y + t.rect.height);
                }
                if (t.rect.x - x).abs() < TILE_EPS {
                    has_right = true;
                    y0 = y0.min(t.rect.y);
                    y1 = y1.max(t.rect.y + t.rect.height);
                }
            }
            if has_left && has_right && y1 > y0 {
                out.push(TileEdge {
                    orient: TileEdgeOrient::Vertical,
                    coord: x,
                    span: (y0, y1),
                });
            }
        }
        for y in ys {
            let mut x0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            let mut has_top = false;
            let mut has_bot = false;
            for t in tiles.iter() {
                if ((t.rect.y + t.rect.height) - y).abs() < TILE_EPS {
                    has_top = true;
                    x0 = x0.min(t.rect.x);
                    x1 = x1.max(t.rect.x + t.rect.width);
                }
                if (t.rect.y - y).abs() < TILE_EPS {
                    has_bot = true;
                    x0 = x0.min(t.rect.x);
                    x1 = x1.max(t.rect.x + t.rect.width);
                }
            }
            if has_top && has_bot && x1 > x0 {
                out.push(TileEdge {
                    orient: TileEdgeOrient::Horizontal,
                    coord: y,
                    span: (x0, x1),
                });
            }
        }
        out
    }

    /// Hit-test the inner Model-tile dividers against a pixel cursor.
    /// `bounds` is the canvas pixel rectangle (origin = canvas top-left).
    /// Returns the closest edge within `tolerance_px` pixels of the cursor
    /// along its perpendicular axis, also requiring the cursor to lie
    /// within the edge's actual span.
    pub fn hit_model_tile_edge(
        &self,
        cursor_px: iced::Point,
        bounds: iced::Rectangle,
        tolerance_px: f32,
    ) -> Option<TileEdge> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let cx = cursor_px.x - bounds.x;
        let cy = cursor_px.y - bounds.y;
        let nx = cx / bounds.width;
        let ny = cy / bounds.height;
        let tol_nx = tolerance_px / bounds.width;
        let tol_ny = tolerance_px / bounds.height;
        let mut best: Option<(f32, TileEdge)> = None;
        for e in self.model_tile_edges() {
            let (dist, in_span) = match e.orient {
                TileEdgeOrient::Vertical => (
                    (e.coord - nx).abs() / tol_nx.max(1e-9),
                    ny >= e.span.0 && ny <= e.span.1,
                ),
                TileEdgeOrient::Horizontal => (
                    (e.coord - ny).abs() / tol_ny.max(1e-9),
                    nx >= e.span.0 && nx <= e.span.1,
                ),
            };
            if in_span && dist <= 1.0 {
                if best.as_ref().map_or(true, |(d, _)| dist < *d) {
                    best = Some((dist, e));
                }
            }
        }
        best.map(|(_, e)| e)
    }

    /// Move the inner divider edge from `old_coord` to `new_coord`, both
    /// in normalized 0..1 space. Adjusts every tile that touches the
    /// edge on either side. `min_size` clamps the new coordinate so no
    /// tile crosses into a non-positive width / height — caller still
    /// runs `collapse_small_model_tiles` afterward to merge any tiles
    /// that fell below the viewcube threshold.
    pub fn move_model_tile_edge(
        &self,
        orient: TileEdgeOrient,
        old_coord: f32,
        new_coord: f32,
        min_size: f32,
    ) {
        let mut tiles = self.model_tiles.borrow_mut();
        // Clamp the new coordinate so no tile becomes ≤ 0 wide / tall.
        // (Sub-`min_size` results are still allowed — the collapse pass
        // handles those.)
        let new_coord = match orient {
            TileEdgeOrient::Vertical => {
                let mut lo = 0.0_f32;
                let mut hi = 1.0_f32;
                for t in tiles.iter() {
                    if ((t.rect.x + t.rect.width) - old_coord).abs() < TILE_EPS {
                        lo = lo.max(t.rect.x + min_size * 0.25);
                    }
                    if (t.rect.x - old_coord).abs() < TILE_EPS {
                        hi = hi.min(t.rect.x + t.rect.width - min_size * 0.25);
                    }
                }
                new_coord.clamp(lo, hi)
            }
            TileEdgeOrient::Horizontal => {
                let mut lo = 0.0_f32;
                let mut hi = 1.0_f32;
                for t in tiles.iter() {
                    if ((t.rect.y + t.rect.height) - old_coord).abs() < TILE_EPS {
                        lo = lo.max(t.rect.y + min_size * 0.25);
                    }
                    if (t.rect.y - old_coord).abs() < TILE_EPS {
                        hi = hi.min(t.rect.y + t.rect.height - min_size * 0.25);
                    }
                }
                new_coord.clamp(lo, hi)
            }
        };
        for t in tiles.iter_mut() {
            match orient {
                TileEdgeOrient::Vertical => {
                    if ((t.rect.x + t.rect.width) - old_coord).abs() < TILE_EPS {
                        t.rect.width = (new_coord - t.rect.x).max(0.0);
                    } else if (t.rect.x - old_coord).abs() < TILE_EPS {
                        let old_right = t.rect.x + t.rect.width;
                        t.rect.x = new_coord;
                        t.rect.width = (old_right - new_coord).max(0.0);
                    }
                }
                TileEdgeOrient::Horizontal => {
                    if ((t.rect.y + t.rect.height) - old_coord).abs() < TILE_EPS {
                        t.rect.height = (new_coord - t.rect.y).max(0.0);
                    } else if (t.rect.y - old_coord).abs() < TILE_EPS {
                        let old_bottom = t.rect.y + t.rect.height;
                        t.rect.y = new_coord;
                        t.rect.height = (old_bottom - new_coord).max(0.0);
                    }
                }
            }
        }
    }

    /// Remove every tile whose width or height has dropped below the
    /// supplied minima, absorbing each one's area into the neighbour
    /// that shares the longest contact edge. Iterates until no tile is
    /// too small (handles chains of collapses). Adjusts
    /// `active_model_tile` so the live camera stays bound to a real
    /// tile (preferring the neighbour that absorbed the active tile).
    pub fn collapse_small_model_tiles(&self, min_w: f32, min_h: f32) {
        let mut tiles = self.model_tiles.borrow_mut();
        loop {
            if tiles.len() < 2 {
                break;
            }
            let small = tiles
                .iter()
                .enumerate()
                .find(|(_, t)| t.rect.width < min_w || t.rect.height < min_h)
                .map(|(i, _)| i);
            let Some(idx) = small else { break };
            let removed = tiles[idx].rect;
            // Find the neighbour with the longest shared contact edge.
            let mut best: Option<(usize, f32, ContactSide)> = None;
            for (j, t) in tiles.iter().enumerate() {
                if j == idx {
                    continue;
                }
                let probes = [
                    (
                        ContactSide::Left,
                        ((t.rect.x + t.rect.width) - removed.x).abs() < TILE_EPS,
                        overlap_len(
                            (t.rect.y, t.rect.y + t.rect.height),
                            (removed.y, removed.y + removed.height),
                        ),
                    ),
                    (
                        ContactSide::Right,
                        (t.rect.x - (removed.x + removed.width)).abs() < TILE_EPS,
                        overlap_len(
                            (t.rect.y, t.rect.y + t.rect.height),
                            (removed.y, removed.y + removed.height),
                        ),
                    ),
                    (
                        ContactSide::Top,
                        ((t.rect.y + t.rect.height) - removed.y).abs() < TILE_EPS,
                        overlap_len(
                            (t.rect.x, t.rect.x + t.rect.width),
                            (removed.x, removed.x + removed.width),
                        ),
                    ),
                    (
                        ContactSide::Bottom,
                        (t.rect.y - (removed.y + removed.height)).abs() < TILE_EPS,
                        overlap_len(
                            (t.rect.x, t.rect.x + t.rect.width),
                            (removed.x, removed.x + removed.width),
                        ),
                    ),
                ];
                for (side, touches, c) in probes {
                    if touches && c > 0.0 {
                        if best.map_or(true, |(_, len, _)| c > len) {
                            best = Some((j, c, side));
                        }
                    }
                }
            }
            if let Some((nbr_idx, _, side)) = best {
                match side {
                    ContactSide::Left => {
                        tiles[nbr_idx].rect.width =
                            (removed.x + removed.width) - tiles[nbr_idx].rect.x;
                    }
                    ContactSide::Right => {
                        let old_right =
                            tiles[nbr_idx].rect.x + tiles[nbr_idx].rect.width;
                        tiles[nbr_idx].rect.x = removed.x;
                        tiles[nbr_idx].rect.width = old_right - removed.x;
                    }
                    ContactSide::Top => {
                        tiles[nbr_idx].rect.height =
                            (removed.y + removed.height) - tiles[nbr_idx].rect.y;
                    }
                    ContactSide::Bottom => {
                        let old_bottom =
                            tiles[nbr_idx].rect.y + tiles[nbr_idx].rect.height;
                        tiles[nbr_idx].rect.y = removed.y;
                        tiles[nbr_idx].rect.height = old_bottom - removed.y;
                    }
                }
                let active = self.active_model_tile.get();
                let new_active = if active == idx {
                    if nbr_idx > idx { nbr_idx - 1 } else { nbr_idx }
                } else if active > idx {
                    active - 1
                } else {
                    active
                };
                tiles.remove(idx);
                self.active_model_tile
                    .set(new_active.min(tiles.len().saturating_sub(1)));
            } else {
                // Isolated tile (shouldn't happen with axis-aligned
                // splits) — drop it and stretch the first remaining
                // tile to fill the canvas so we don't leave a hole.
                tiles.remove(idx);
                let active = self.active_model_tile.get();
                self.active_model_tile
                    .set(active.saturating_sub(if active > idx { 1 } else { 0 }).min(tiles.len().saturating_sub(1)));
                if let Some(first) = tiles.first_mut() {
                    first.rect = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    };
                }
                break;
            }
        }
    }

    /// Split the active Model tile in two. `horizontal` → a horizontal
    /// divider (top / bottom halves); otherwise a vertical divider (left /
    /// right). Both halves inherit the active tile's current camera; the
    /// active tile stays the first half. No-op outside the Model layout.
    pub fn split_active_model_tile(&self, horizontal: bool) {
        if self.current_layout != "Model" {
            return;
        }
        let cam_now = self.camera.borrow().clone();
        let mut tiles = self.model_tiles.borrow_mut();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        let r = tiles[active].rect;
        let (a, b) = if horizontal {
            (
                iced::Rectangle { height: r.height / 2.0, ..r },
                iced::Rectangle {
                    y: r.y + r.height / 2.0,
                    height: r.height / 2.0,
                    ..r
                },
            )
        } else {
            (
                iced::Rectangle { width: r.width / 2.0, ..r },
                iced::Rectangle {
                    x: r.x + r.width / 2.0,
                    width: r.width / 2.0,
                    ..r
                },
            )
        };
        let mode = tiles[active].render_mode;
        let (grid_on, snap_on) = (tiles[active].grid_on, tiles[active].snap_on);
        tiles[active] = ModelTile {
            rect: a,
            camera: cam_now.clone(),
            render_mode: mode,
            grid_on,
            snap_on,
        };
        tiles.insert(
            active + 1,
            ModelTile {
                rect: b,
                camera: cam_now,
                render_mode: mode,
                grid_on,
                snap_on,
            },
        );
    }

    /// Make the Model tile containing normalized point `(nx, ny)` active,
    /// swapping cameras so the live `Scene::camera` follows the new tile.
    /// Returns `true` when the active tile changed. No-op outside Model.
    pub fn set_active_model_tile_at(&self, nx: f32, ny: f32) -> bool {
        if self.current_layout != "Model" {
            return false;
        }
        let new = {
            let tiles = self.model_tiles.borrow();
            tiles.iter().position(|t| {
                nx >= t.rect.x
                    && nx < t.rect.x + t.rect.width
                    && ny >= t.rect.y
                    && ny < t.rect.y + t.rect.height
            })
        };
        let Some(new) = new else { return false };
        let old = self.active_model_tile.get();
        if new == old {
            return false;
        }
        // Stash the live camera into the outgoing tile, load the incoming.
        let incoming = {
            let mut tiles = self.model_tiles.borrow_mut();
            if let Some(t) = tiles.get_mut(old) {
                t.camera = self.camera.borrow().clone();
            }
            tiles.get(new).map(|t| t.camera.clone())
        };
        if let Some(cam) = incoming {
            *self.camera.borrow_mut() = cam;
        }
        self.active_model_tile.set(new);
        // Caller bumps camera_generation (it needs &mut Scene).
        true
    }

    /// Replace the Model tiled layout with the given normalized rectangles
    /// (each in 0..1). Every tile inherits the current camera; the first
    /// tile becomes active. Used by VPORTS presets and `reset_model_tiles`.
    pub fn set_model_tile_layout(&self, rects: Vec<iced::Rectangle>) {
        let cam_now = self.camera.borrow().clone();
        // Every new pane inherits the active tile's current visual style.
        let (mode, grid_on, snap_on) = {
            let tiles = self.model_tiles.borrow();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            tiles
                .get(active)
                .map(|t| (t.render_mode, t.grid_on, t.snap_on))
                .unwrap_or((
                    acadrust::entities::ViewportRenderMode::Wireframe2D,
                    false,
                    false,
                ))
        };
        let tiles: Vec<ModelTile> = rects
            .into_iter()
            .map(|rect| ModelTile {
                rect,
                camera: cam_now.clone(),
                render_mode: mode,
                grid_on,
                snap_on,
            })
            .collect();
        *self.model_tiles.borrow_mut() = if tiles.is_empty() {
            vec![ModelTile {
                rect: iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                camera: cam_now,
                render_mode: mode,
                grid_on,
                snap_on,
            }]
        } else {
            tiles
        };
        self.active_model_tile.set(0);
    }

    /// Screen-pixel rectangle of the active Model tile within a canvas of
    /// `(vw, vh)`. Full canvas outside the Model layout or for a single
    /// tile. Used to map cursor coordinates into the active tile so pick /
    /// pan / ViewCube work per-pane in a tiled layout.
    /// Canvas bounds + camera for every Model tile whose grid display is on.
    /// Each pane renders its own grid independently of which tile is active or
    /// hovered, so the grid never flickers as the cursor crosses panes. The
    /// active tile uses the live camera (mid-orbit/pan); others use their
    /// stored camera. (#121)
    /// Screen rect + camera for every grid-on sub-view in the current layout —
    /// model tiles in model space, the sheet plus each floating viewport
    /// (clipped to its rectangle) in paper space. Derived from the same
    /// `active_viewports` enumeration the renderer uses, so the grid overlay can
    /// never drift from the views actually on screen (issue #121). The grid
    /// ignores render mode, so any value passes through.
    pub fn grid_views(&self, vw: f32, vh: f32) -> Vec<(iced::Rectangle, Camera, Handle)> {
        self.active_viewports(vw, vh, acadrust::entities::ViewportRenderMode::Wireframe2D)
            .into_iter()
            .filter(|inst| inst.grid_on)
            .map(|inst| (inst.screen_rect, inst.camera, inst.handle))
            .collect()
    }

    pub fn active_model_tile_bounds(&self, vw: f32, vh: f32) -> iced::Rectangle {
        if self.current_layout != "Model" {
            return iced::Rectangle { x: 0.0, y: 0.0, width: vw, height: vh };
        }
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        match tiles.get(active) {
            Some(t) => iced::Rectangle {
                x: t.rect.x * vw,
                y: t.rect.y * vh,
                width: (t.rect.width * vw).max(1.0),
                height: (t.rect.height * vh).max(1.0),
            },
            None => iced::Rectangle { x: 0.0, y: 0.0, width: vw, height: vh },
        }
    }

    /// The viewports to render this frame, one entry per scissor pass.
    ///
    /// - **Model layout**: a single full-canvas instance driven by the
    ///   scene camera (tiled splits will append more later). `model_mode`
    ///   supplies its render mode (held on the tab, not the scene).
    /// - **Paper layout**: one instance per content viewport entity
    ///   (`id > 1`, owned by the current layout block, switched on),
    ///   using each viewport's own camera and render mode.
    pub fn active_viewports(
        &self,
        canvas_w: f32,
        canvas_h: f32,
        model_mode: acadrust::entities::ViewportRenderMode,
    ) -> Vec<ViewportInstance> {
        if self.current_layout == "Model" {
            let tiles = self.model_tiles.borrow();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            return tiles
                .iter()
                .enumerate()
                .map(|(i, tile)| {
                    // The active tile renders the live camera (orbit/pan act
                    // on it); inactive tiles use their stored snapshot.
                    let camera = if i == active {
                        self.camera.borrow().clone()
                    } else {
                        tile.camera.clone()
                    };
                    ViewportInstance {
                        handle: Handle::NULL,
                        tile_idx: Some(i),
                        screen_rect: iced::Rectangle {
                            x: tile.rect.x * canvas_w,
                            y: tile.rect.y * canvas_h,
                            width: tile.rect.width * canvas_w,
                            height: tile.rect.height * canvas_h,
                        },
                        camera,
                        // The active tile shows the live mode the picker
                        // drives; every other tile keeps its own stored
                        // style so editing one never disturbs the rest.
                        render_mode: if i == active { model_mode } else { tile.render_mode },
                        active: i == active,
                        grid_on: tile.grid_on,
                        paper_sheet: false,
                    }
                })
                .collect();
        }
        let layout_block = self.current_layout_block_handle();
        let mut out: Vec<ViewportInstance> = Vec::new();
        // The full-canvas sheet viewport renders the paper-space entities
        // themselves — the layout's own view, drawn first so the floating
        // content viewports overlay it. Its camera keeps the paper pan/zoom
        // (target + ortho size) but is LOCKED to the top/plan orientation:
        // paper is 2-D, so the sheet never orbits.
        let mut sheet_cam = self.camera.borrow().clone();
        sheet_cam.yaw = 0.0;
        sheet_cam.pitch = std::f32::consts::FRAC_PI_2;
        sheet_cam.rotation = view::camera::yaw_pitch_to_quat(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        sheet_cam.projection = view::camera::Projection::Orthographic;
        let sheet_grid_on = match self
            .document
            .get_entity(self.current_layout_sheet_viewport_handle())
        {
            Some(EntityType::Viewport(vp)) => vp.status.grid_on,
            _ => false,
        };
        out.push(ViewportInstance {
            handle: Handle::NULL,
            tile_idx: None,
            screen_rect: iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas_w,
                height: canvas_h,
            },
            camera: sheet_cam,
            render_mode: acadrust::entities::ViewportRenderMode::Wireframe2D,
            active: false,
            grid_on: sheet_grid_on,
            paper_sheet: true,
        });
        for e in self.document.entities() {
            let EntityType::Viewport(vp) = e else {
                continue;
            };
            if !self.is_content_viewport_in_layout(vp, layout_block)
                || !vp.status.is_on
            {
                continue;
            }
            let h = vp.common.handle;
            let (Some(screen_rect), Some(camera)) = (
                self.viewport_screen_rect(h, (canvas_w, canvas_h)),
                self.camera_for_viewport(h),
            ) else {
                continue;
            };
            out.push(ViewportInstance {
                handle: h,
                tile_idx: None,
                screen_rect,
                camera,
                render_mode: vp.render_mode,
                active: self.active_viewport == Some(h),
                grid_on: vp.status.grid_on,
                paper_sheet: false,
            });
        }
        out
    }

    /// Convert a paper-space Viewport entity's position/size into a pixel
    /// `Rectangle` relative to the top-left of the canvas.
    ///
    /// Uses the same top-down ortho transform as the GPU sheet viewport so the
    /// overlay lands exactly over the drawn viewport border regardless of zoom
    /// or pan level.
    pub fn viewport_screen_rect(
        &self,
        vp_handle: Handle,
        canvas_px: (f32, f32),
    ) -> Option<iced::Rectangle> {
        let vp = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp,
            _ => return None,
        };

        let (canvas_w, canvas_h) = canvas_px;
        if canvas_w < 1.0 || canvas_h < 1.0 {
            return None;
        }

        let cam = self.camera.borrow();
        let aspect = canvas_w / canvas_h;
        let half_h = cam.ortho_size();
        let half_w = half_h * aspect;
        let tx = cam.target.x as f32;
        let ty = cam.target.y as f32;
        drop(cam);

        // Top-down ortho mapping matching the GPU sheet viewport's camera.
        let to_px = |wx: f32, wy: f32| -> (f32, f32) {
            let x = (wx - tx + half_w) / (2.0 * half_w) * canvas_w;
            let y = (ty + half_h - wy) / (2.0 * half_h) * canvas_h;
            (x, y)
        };

        let cx = vp.center.x as f32;
        let cy = vp.center.y as f32;
        let hw = (vp.width / 2.0) as f32;
        let hh = (vp.height / 2.0) as f32;

        let (x0, y0) = to_px(cx - hw, cy + hh); // top-left in screen
        let (x1, y1) = to_px(cx + hw, cy - hh); // bottom-right in screen

        let w = (x1 - x0).max(1.0);
        let h = (y1 - y0).max(1.0);

        Some(iced::Rectangle {
            x: x0,
            y: y0,
            width: w,
            height: h,
        })
    }

    // ── Paper-space helpers ───────────────────────────────────────────────

    /// Paper-layout hatch fills, restricted to the active layout block (used by
    /// paper-space hatch hit-testing / export). The GPU-rendered
    /// content viewports already draw model-block hatches inside their
    /// own scissor; including those here would also draw them on the
    /// paper sheet through the paper camera (huge / off-position), so
    /// restrict the canvas list to entities owned by the active paper
    /// layout block. Iterates the source `self.hatches` map (keyed by
    /// entity handle) rather than the already-flattened arc — the
    /// flattened arc carries pattern names, not handles, so filtering
    /// there is unreliable.
    pub fn paper_canvas_hatches(&self) -> Arc<Vec<HatchModel>> {
        let layout_block = self.current_layout_block_handle();
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };
        let mut models: Vec<HatchModel> = Vec::new();
        for (&handle, model) in self.hatches.iter() {
            let Some(entity) = self.document.get_entity(handle) else {
                continue;
            };
            let c = entity.common();
            if c.invisible || layer_hidden(&c.layer) {
                continue;
            }
            if !self.belongs_to_visible_block(handle, c.owner_handle, layout_block) {
                continue;
            }
            let mut m = model.clone();
            m.color = self.render_style(entity).0;
            if let EntityType::Hatch(dxf) = entity {
                if let model::hatch_model::HatchPattern::Pattern(_) = &m.pattern {
                    m.angle_offset = dxf.pattern_angle as f32;
                    m.scale = dxf.pattern_scale as f32;
                }
            }
            if self.selected.contains(&handle) {
                m.color = [0.15, 0.55, 1.00, m.color[3]];
            }
            models.push(m);
        }
        Arc::new(models)
    }

    /// Paper-layout wipeout fills (paper hit-testing / export). Same rationale as
    /// `paper_canvas_hatches` — only include wipeouts owned by the
    /// active paper layout block, so model wipeouts (drawn through their
    /// content viewport's GPU pipeline) don't get a second mis-projected
    /// copy on the paper sheet.
    pub fn paper_canvas_wipeouts(&self) -> Arc<Vec<HatchModel>> {
        let layout_block = self.current_layout_block_handle();
        let bg_color = self.paper_bg_color;
        let mut models = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Wipeout(wo) = entity else {
                continue;
            };
            if wo.common.invisible {
                continue;
            }
            if self
                .document
                .layers
                .get(&wo.common.layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
            {
                continue;
            }
            if !self.belongs_to_visible_block(wo.common.handle, wo.common.owner_handle, layout_block)
            {
                continue;
            }
            // Paper-block wipeouts live in paper coords — no `world_offset`.
            let boundary = Self::wipeout_boundary_2d(wo);
            if boundary.len() < 3 {
                continue;
            }
            let mut fill_color = bg_color;
            if self.selected.contains(&wo.common.handle) {
                fill_color = [0.15, 0.55, 1.00, 0.35];
            }
            models.push(HatchModel {
                boundary: Arc::new(boundary),
                pattern: model::hatch_model::HatchPattern::Solid,
                name: "WIPEOUT_FILL".into(),
                color: fill_color,
                angle_offset: 0.0,
                scale: 1.0,
                world_origin: [0.0; 2],
                vp_scissor: None,
                draw_depth: 0.0,
            });
        }
        Arc::new(models)
    }

    /// Build a Camera oriented and scaled to match a paper-space Viewport entity.
    /// Used by `active_viewports` to render model-space content through each
    /// content viewport's own view direction and scale.
    fn camera_for_viewport(&self, vp_handle: Handle) -> Option<view::camera::Camera> {
        let vp = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp,
            _ => return None,
        };

        // Floating-viewport–specific step: decide saved-view vs auto-fit, then
        // hand the effective view to the shared `camera_from_view` decoder so
        // twist / view_center / distance behave identically to a model VPORT.
        //
        // UTM / coordinate-shifted drawings often arrive with
        // `view_target = (0, 0, 0)` and a stale `view_center` from before the
        // file was geo-referenced; the saved view points at empty WCS while the
        // actual model sits ~`world_offset` away. Decode the saved view first
        // and keep it only if its target actually frames the model cluster.
        //
        // The overlap test runs on the *decoded* target (wire-space, so the
        // cluster is `±cluster_half` about the origin), NOT a raw
        // `view_target + view_center` sum: under a view twist `view_center` is a
        // DCS offset, so the raw sum lands far from the real WCS centre and
        // would wrongly trip the auto-fit — replacing the saved view_height with
        // the whole-cluster fit and rendering the content at the wrong zoom.
        let saved_h = vp.view_height.abs();
        let aspect_d = (vp.width / vp.height.max(1.0)).max(1e-9);
        let cluster_half = self.local_extent_max.max(1.0) as f64;
        // Absolute drawing centre. Geometry now reaches the scene at absolute
        // (UTM) coordinates — the old code centred the overlap test and the
        // auto-fit on the origin, which was right only while world_offset
        // re-centred the model there. Without it a UTM drawing sits ~5.7e6 away,
        // so a stale `(0,0,0)` saved view failed the overlap test AND the
        // auto-fit aimed at empty origin → blank viewports.
        // Frame the overlap test / auto-fit on the robust cluster centre (median
        // of entity centroids), NOT the raw extents centre: a drawing with a
        // far second cluster (e.g. a small-coordinate legend beside a UTM survey)
        // has an extents centre in the empty gap, which would reject a valid
        // saved view and then auto-fit onto blank space. Fall back to the extents
        // centre only when no cluster centre was computed.
        let (cx, cy) = if self.local_center != [0.0, 0.0] {
            (self.local_center[0], self.local_center[1])
        } else {
            self.model_space_extents()
                .map(|(mn, mx)| {
                    (((mn.x + mx.x) * 0.5) as f64, ((mn.y + mx.y) * 0.5) as f64)
                })
                .unwrap_or((0.0, 0.0))
        };

        if let Some(cam) = self.camera_from_view(
            vp.view_direction,
            vp.view_target,
            acadrust::types::Vector2 {
                x: vp.view_center.x,
                y: vp.view_center.y,
            },
            saved_h,
            vp.twist_angle,
        ) {
            let half_h = saved_h * 0.5;
            let half_w = half_h * aspect_d;
            let (tx, ty) = (cam.target.x as f64, cam.target.y as f64);
            let overlaps = tx + half_w >= cx - cluster_half
                && tx - half_w <= cx + cluster_half
                && ty + half_h >= cy - cluster_half
                && ty - half_h <= cy + cluster_half;
            if overlaps {
                return Some(cam);
            }
        }

        // Auto-fit: aim at the content cluster centre, drop the stale view_center.
        let fit_h = cluster_half * 2.0 * 1.05;
        let tgt = acadrust::types::Vector3 {
            x: cx,
            y: cy,
            z: vp.view_target.z,
        };
        self.camera_from_view(
            vp.view_direction,
            tgt,
            acadrust::types::Vector2::ZERO,
            fit_h,
            vp.twist_angle,
        )
    }

    /// Collect model-space WireModels visible through `vp_handle`, respecting
    /// global layer visibility, the viewport's per-viewport layer freeze list,
    /// and the per-viewport frustum + LOD cull derived from
    /// `screen_height_px` (the on-paper pixel height of this viewport).
    fn model_wires_for_viewport(
        &self,
        vp_handle: Handle,
        screen_height_px: f32,
    ) -> Vec<WireModel> {
        use rustc_hash::FxHashSet as HSet;

        let (frozen, vp_anno_scale, vp_aspect) = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => {
                let f: HSet<Handle> = vp.frozen_layers.iter().cloned().collect();
                let vp_scale =
                    vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
                let anno = if vp_scale > 1e-9 {
                    (1.0 / vp_scale) as f32
                } else {
                    1.0_f32
                };
                let aspect = if vp.height > 1e-9 {
                    (vp.width / vp.height) as f32
                } else {
                    1.0_f32
                };
                (f, anno, aspect)
            }
            _ => (HSet::default(), 1.0_f32, 1.0_f32),
        };

        // Drive the per-viewport view_aabb / wpp from the *effective* camera
        // `camera_for_viewport` produces — it folds in the auto-fit
        // fallback for UTM-style files whose saved `view_target` sits at
        // empty WCS. Without that, the GPU pass would frustum-cull every
        // entity (saved-view rect doesn't overlap the offset-subtracted
        // model cluster) and the viewport would render blank.
        let Some(cam) = self.camera_for_viewport(vp_handle) else {
            return Vec::new();
        };
        let vp_ortho_h = cam.ortho_size();
        // Rotation-aware cull box: a twisted/rotated viewport sees a rotated
        // rectangle in world XY, so derive the box from the camera basis.
        // `None` (tilted view) disables the cull — render everything.
        let view_aabb = view_cull_aabb(&cam, vp_aspect, 1.25);
        // World units per on-screen pixel for LOD substitution + curve
        // tolerance. Tracks the paper-zoom-driven pixel height the
        // viewport currently occupies.
        let wpp = if screen_height_px > 1.0 {
            Some((2.0 * vp_ortho_h) / screen_height_px)
        } else {
            None
        };

        self.wires_for_block_culled(
            self.model_space_block_handle(),
            view_aabb,
            wpp,
            Some(&frozen),
            Some(vp_anno_scale),
        )
    }

    /// Cached per-paper-viewport tessellation. Each viewport's wpp tracks
    /// the on-paper pixel height (paper-zoom dependent), so the cache key
    /// includes a quantized form of that height in addition to the
    /// geometry epoch — every paper zoom step that actually changes the
    /// LOD bucket invalidates this viewport's entry.
    pub(super) fn model_wires_for_viewport_arc(
        &self,
        vp_handle: Handle,
        screen_height_px: f32,
    ) -> Arc<Vec<WireModel>> {
        // Drop sub-pixel noise so trivial paper-zoom jitter does not
        // re-tessellate a 100k-entity drawing every frame; round to an
        // integer pixel.
        let height_key = screen_height_px.max(1.0).round() as u32;
        // Hash the viewport's own view (pan + zoom + orbit) into the key.
        // Editing inside the viewport (MSPACE) changes its frustum but does NOT
        // bump geometry_epoch, so without this the stale frustum-culled subset
        // is returned and newly-revealed lines stay invisible until the layout
        // re-tessellates. Quantize to ~1 px / fine steps to ignore jitter.
        let view_key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some(EntityType::Viewport(vp)) = self.document.get_entity(vp_handle) {
                let vh = vp.view_height.abs().max(1e-6);
                let q = vh / (screen_height_px.max(1.0) as f64); // model units / px
                (((vp.view_target.x + vp.view_center.x) / q).round() as i64).hash(&mut h);
                (((vp.view_target.y + vp.view_center.y) / q).round() as i64).hash(&mut h);
                ((vh * 1000.0).round() as i64).hash(&mut h);
                ((vp.view_direction.x * 1000.0).round() as i64).hash(&mut h);
                ((vp.view_direction.y * 1000.0).round() as i64).hash(&mut h);
                ((vp.view_direction.z * 1000.0).round() as i64).hash(&mut h);
            }
            h.finish()
        };
        let key = (self.geometry_epoch, height_key, view_key);
        {
            let cache = self.viewport_wire_cache.borrow();
            if let Some((cached_key, ref arc)) = cache.get(&vp_handle) {
                if *cached_key == key {
                    return Arc::clone(arc);
                }
            }
        }
        let arc = Arc::new(self.model_wires_for_viewport(vp_handle, screen_height_px));
        self.viewport_wire_cache
            .borrow_mut()
            .insert(vp_handle, (key, Arc::clone(&arc)));
        arc
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

// ── Paper boundary wire ────────────────────────────────────────────────────

// ── Cohen-Sutherland line clipping ───────────────────────────────────────

/// Clip a single segment (x0,y0)→(x1,y1) against the axis-aligned rectangle
/// [xmin,xmax]×[ymin,ymax].  Returns the clipped endpoints or `None` if the
/// segment is entirely outside.
fn cs_clip(
    mut x0: f32,
    mut y0: f32,
    mut x1: f32,
    mut y1: f32,
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
) -> Option<(f32, f32, f32, f32)> {
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;

    let code = |x: f32, y: f32| -> u8 {
        let mut c = 0u8;
        if x < xmin {
            c |= LEFT;
        } else if x > xmax {
            c |= RIGHT;
        }
        if y < ymin {
            c |= BOTTOM;
        } else if y > ymax {
            c |= TOP;
        }
        c
    };

    let mut c0 = code(x0, y0);
    let mut c1 = code(x1, y1);

    loop {
        if c0 | c1 == 0 {
            return Some((x0, y0, x1, y1));
        }
        if c0 & c1 != 0 {
            return None;
        }
        let cout = if c0 != 0 { c0 } else { c1 };
        let (x, y);
        if cout & TOP != 0 {
            x = x0 + (x1 - x0) * (ymax - y0) / (y1 - y0);
            y = ymax;
        } else if cout & BOTTOM != 0 {
            x = x0 + (x1 - x0) * (ymin - y0) / (y1 - y0);
            y = ymin;
        } else if cout & RIGHT != 0 {
            y = y0 + (y1 - y0) * (xmax - x0) / (x1 - x0);
            x = xmax;
        } else {
            y = y0 + (y1 - y0) * (xmin - x0) / (x1 - x0);
            x = xmin;
        }
        if cout == c0 {
            x0 = x;
            y0 = y;
            c0 = code(x0, y0);
        } else {
            x1 = x;
            y1 = y;
            c1 = code(x1, y1);
        }
    }
}

/// Clip a projected polyline (NaN-separated segments) to the viewport rectangle.
/// Returns a new points vec with proper NaN separators at clip boundaries.
fn clip_polyline_to_rect(
    pts: &[[f32; 3]],
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
    z: f32,
) -> Vec<[f32; 3]> {
    const NAN3: [f32; 3] = [f32::NAN, f32::NAN, f32::NAN];
    let mut result: Vec<[f32; 3]> = Vec::new();
    let mut i = 0;

    while i < pts.len() {
        // Skip NaN separators.
        if pts[i][0].is_nan() || pts[i][1].is_nan() {
            i += 1;
            continue;
        }
        // Gather contiguous run of finite points.
        let start = i;
        while i < pts.len() && pts[i][0].is_finite() && pts[i][1].is_finite() {
            i += 1;
        }
        let seg = &pts[start..i];
        if seg.len() < 2 {
            continue;
        }

        // Clip each edge and track pen state to insert NaN on lift.
        let mut pen_down = false;
        for j in 0..seg.len() - 1 {
            let [x0, y0, _] = seg[j];
            let [x1, y1, _] = seg[j + 1];
            match cs_clip(x0, y0, x1, y1, xmin, ymin, xmax, ymax) {
                None => {
                    pen_down = false;
                }
                Some((cx0, cy0, cx1, cy1)) => {
                    if !pen_down {
                        if !result.is_empty() {
                            result.push(NAN3);
                        }
                        result.push([cx0, cy0, z]);
                        pen_down = true;
                    } else if let Some(&[lx, ly, _]) = result.last() {
                        if (lx - cx0).abs() > 1e-4 || (ly - cy0).abs() > 1e-4 {
                            result.push(NAN3);
                            result.push([cx0, cy0, z]);
                        }
                    }
                    result.push([cx1, cy1, z]);
                    // If the exit point was clipped, lift pen.
                    if (cx1 - x1).abs() > 1e-4 || (cy1 - y1).abs() > 1e-4 {
                        pen_down = false;
                    }
                }
            }
        }
    }
    // Remove trailing NaN.
    while result
        .last()
        .map(|p: &[f32; 3]| p[0].is_nan())
        .unwrap_or(false)
    {
        result.pop();
    }
    result
}

// ── Parallel tessellation free function ──────────────────────────────────────
//
// Takes only the `Send + Sync` data needed for tessellation so that
// `wires_for_block` can dispatch work across rayon's thread pool without
// requiring `Scene` (which contains `Rc<RefCell<...>>` and is `!Send`) to
// cross thread boundaries.

/// Tessellate a synthesised dimension-text entity through `tessellate_entity`
/// so it picks up the standard text LOD ladder (baseline / greek / full),
/// then re-color the returned wires with the dimension's resolved text colour
/// (so DIMCLRT / DIMSTYLE colours win over the synthetic Text's defaults).
pub(crate) fn tessellate_entity_dim_text(
    document: &acadrust::CadDocument,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    bg_color: [f32; 4],
    anno_scale: f32,
    e: &EntityType,
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    text_color: [f32; 4],
) -> Vec<WireModel> {
    let mut wires = tessellate_entity(
        document, selected, active_viewport, bg_color,
        anno_scale, e, None, view_aabb, world_per_pixel,
    );
    for w in &mut wires {
        // Synth dim text carries no real entity colour — paint everything
        // (including greek-LOD fill tris which read `wire.color`) with the
        // dim's text colour. Selection highlight already baked in by
        // tessellate_entity, so leave that alone.
        if !w.selected {
            w.color = text_color;
        }
    }
    wires
}

fn tessellate_entity(
    document: &acadrust::CadDocument,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    bg_color: [f32; 4],
    anno_scale: f32,
    e: &EntityType,
    block_cache: Option<&cache::block_cache::BlockCache>,
    // World-space XY view AABB (post `world_offset` subtraction). When
    // `Some`, entities whose AABB doesn't intersect this rect are skipped.
    view_aabb: Option<[f32; 4]>,
    // World units per screen pixel for LOD culling. `None` = no LOD.
    world_per_pixel: Option<f32>,
) -> Vec<WireModel> {
    let h = e.common().handle;
    let sel = selected.contains(&h);

    // Frustum + LOD cull for non-Insert, non-Viewport entities. Insert is
    // handled separately (its WCS bbox depends on the block defn AABB ×
    // Insert transform — done inside expand_insert). Viewports always emit
    // so the viewport frame stays visible regardless of zoom.
    let needs_cull = view_aabb.is_some() || world_per_pixel.is_some();
    if needs_cull {
        match e {
            EntityType::Viewport(_) | EntityType::Insert(_) => {}
            _ => {
                let ab = entity_aabb(e);
                if ab != WireModel::UNBOUNDED_AABB {
                    if let Some(view) = view_aabb {
                        if cache::block_cache::aabb_disjoint_xy(ab, view) {
                            return vec![];
                        }
                    }
                    if let Some(wpp) = world_per_pixel {
                        let w_px = (ab[2] - ab[0]).abs();
                        let h_px = (ab[3] - ab[1]).abs();
                        // Keep in sync with `cache::block_cache::MIN_PIXEL_SIZE`.
                        // Text/MText have their own LOD ladder below
                        // (baseline-line / greek / full) and must reach it
                        // even when projected size is sub-5 px.
                        let is_text = matches!(e, EntityType::Text(_) | EntityType::MText(_));
                        let is_3d_entity = matches!(
                            e,
                            EntityType::Face3D(_)
                                | EntityType::Solid3D(_)
                                | EntityType::Mesh(_)
                                | EntityType::PolyfaceMesh(_)
                                | EntityType::PolygonMesh(_)
                                | EntityType::Body(_)
                                | EntityType::Region(_)
                                | EntityType::Surface(_)
                        );
                        if !is_text && w_px.max(h_px) / wpp < 5.0 {
                            // Sub-pixel entity: emit a stub instead of
                            // nothing so it stays visible / selectable /
                            // hit-test'able at any zoom. 2-D entities
                            // get the cheap diagonal segment; 3-D
                            // entities get an AABB cube so their
                            // footprint doesn't drift when the camera
                            // crosses the LOD threshold. See #19.
                            let (entity_color, _, _, _, aci_idx) =
                                view::render::render_style_for(document, e);
                            let entity_color = view::render::adapt_to_bg(entity_color, bg_color);
                            if is_3d_entity {
                                // `ab` is already in the local frame
                                // (entity_aabb subtracted world_offset
                                // XY). The bbox z fields are still in
                                // WCS, so subtract `world_offset[2]` to
                                // match — otherwise the stub sits at a
                                // different z than the full tessellation
                                // and the geometry visibly shifts when
                                // the camera crosses the LOD threshold.
                                let bbox = e.as_entity().bounding_box();
                                let oz = 0.0_f64;
                                let z_min = (bbox.min.z - oz) as f32;
                                let z_max = (bbox.max.z - oz) as f32;
                                return vec![lod_stub_wire_3d(
                                    h.value().to_string(),
                                    entity_color,
                                    sel,
                                    aci_idx,
                                    ab,
                                    z_min,
                                    z_max,
                                )];
                            }
                            return vec![lod_stub_wire(
                                h.value().to_string(),
                                entity_color,
                                sel,
                                aci_idx,
                                ab,
                                0.0,
                                0.0,
                            )];
                        }
                    }
                }
            }
        }
    }

    if let EntityType::Viewport(vp) = e {
        // The sheet viewport (overall/id=1) is never shown — it represents the
        // paper boundary, not a user-defined content window.
        if !Scene::is_content_viewport(vp) {
            return vec![];
        }
        let is_active = active_viewport == Some(h);
        let is_locked = vp.status.locked;
        let color = if sel {
            [1.0, 1.0, 1.0, 1.0]
        } else if is_active {
            [1.0, 0.90, 0.20, 1.0]
        } else if is_locked {
            [0.90, 0.55, 0.10, 1.0]
        } else {
            [0.0, 0.75, 0.75, 1.0]
        };
        let (pattern_length, pattern) = if is_active {
            (1.5_f32, [0.8, -0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0_f32])
        } else {
            (0.0_f32, [0.0f32; 8])
        };
        let mut wires = convert::tessellate::tessellate(
            document,
            h,
            e,
            sel,
            color,
            pattern_length,
            pattern,
            1.5,
            1.0,
            world_per_pixel,
        );
        let ab = entity_aabb(e);
        for w in &mut wires {
            w.aabb = ab;
        }
        return wires;
    }

    let (entity_color, pattern_length, pattern, line_weight_px, aci) =
        view::render::render_style_for(document, e);
    let entity_color = view::render::adapt_to_bg(entity_color, bg_color);
    let lt_scale = document.header.linetype_scale as f32 * e.common().linetype_scale as f32;
    let lt_name = view::render::linetype_name_for(document, e);
    // PSLTSCALE: scale linetype dashes by viewport anno_scale so they appear uniform in paper space.
    let pslt_factor = if document.header.paper_space_linetype_scaling {
        anno_scale
    } else {
        1.0
    };
    let pattern_length = pattern_length * pslt_factor;
    let pattern = pattern.map(|v| v * pslt_factor);

    // ── Dimension baked-block fast path ─────────────────────────────────────
    //
    // AutoCAD bakes each dimension's final geometry (extension lines, dim
    // line, arrows, text MText) into a per-instance block — usually
    // `*D<n>`, but custom names like `DIMBLOCK###-4NP` also occur. When the
    // block exists we render its contents through `tessellate_entity` so
    // sub-Text/MText get the standard baseline/greek/full LOD ladder, and
    // DIMTXT × DIMSCALE isn't re-applied on already-baked geometry.
    if let EntityType::Dimension(dim) = e {
        let block_name = &dim.base().block_name;
        if !block_name.trim().is_empty() {
            if let Some(br) = document
                .block_records
                .iter()
                .find(|br| br.name.eq_ignore_ascii_case(block_name))
            {
                if !br.entity_handles.is_empty() {
                    let mut wires: Vec<WireModel> =
                        Vec::with_capacity(br.entity_handles.len());
                    for &eh in &br.entity_handles {
                        let Some(sub) = document.get_entity(eh) else { continue };
                        // Sub-entities inside *D### / DIMBLOCK## blocks
                        // typically use ByBlock color/linetype/lineweight —
                        // they should inherit from the Dimension entity.
                        let sub_color_is_byblock =
                            sub.common().color == acadrust::types::Color::ByBlock;
                        let sub_wires = tessellate_entity(
                            document, selected, active_viewport, bg_color,
                            // Block contents are baked at the final WCS size —
                            // don't let downstream paths re-apply anno_scale.
                            1.0, sub, block_cache, view_aabb, world_per_pixel,
                        );
                        for mut w in sub_wires {
                            w.name = h.value().to_string();
                            // Override ByBlock colour with the dim's resolved
                            // colour so text matches `DIMCLRT`-style behaviour
                            // (or layer colour) instead of the raw ByBlock
                            // fallback that render_style_for produces.
                            if sub_color_is_byblock {
                                w.color = if sel { WireModel::SELECTED } else { entity_color };
                                w.aci = aci;
                            }
                            wires.push(w);
                        }
                    }
                    if !wires.is_empty() {
                        let aabb = entity_aabb(e);
                        for w in &mut wires {
                            w.aabb = aabb;
                        }
                        return wires;
                    }
                }
            }
        }
        // Fall through to the synthesis path below when no block is attached.
    }

    if let EntityType::Dimension(dim) = e {
        let aabb = entity_aabb(e);
        use crate::entities::dimension::DimensionTess;
        let mut wires = dim.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            anno_scale,
            selected,
            active_viewport,
            bg_color,
            view_aabb,
            world_per_pixel,
        );
        for w in &mut wires {
            w.aci = aci;
            w.aabb = aabb;
        }
        return wires;
    }

    if let EntityType::MultiLeader(ml) = e {
        let aabb = entity_aabb(e);
        use crate::entities::multileader::MultiLeaderTess;
        let mut wires = ml.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            anno_scale,
            world_per_pixel,
        );
        for w in &mut wires {
            w.aci = aci;
            w.aabb = aabb;
        }
        return wires;
    }

    // ── Table baked-block fast path ─────────────────────────────────────────
    //
    // AutoCAD bakes a Table's final rendered geometry (cell text, gridlines,
    // fill) into a per-instance block (usually `*T###`) referenced through
    // `table.block_record_handle`. The block's text uses the *displayed*
    // height; synthesising cells from `self.rows + TableStyle` instead would
    // re-apply the table's scale factor on top of already-baked geometry.
    // When the block exists we render it directly. Same pattern as
    // Dimension's `block_name`.
    if let EntityType::Table(tab) = e {
        if let Some(br_h) = tab.block_record_handle {
            if let Some(br) = document
                .block_records
                .iter()
                .find(|br| br.handle == br_h)
            {
                if !br.entity_handles.is_empty() {
                    let mut wires: Vec<WireModel> =
                        Vec::with_capacity(br.entity_handles.len());
                    for &eh in &br.entity_handles {
                        let Some(sub) = document.get_entity(eh) else { continue };
                        let sub_color_is_byblock =
                            sub.common().color == acadrust::types::Color::ByBlock;
                        let sub_wires = tessellate_entity(
                            document, selected, active_viewport, bg_color,
                            anno_scale, sub, block_cache, view_aabb, world_per_pixel,
                        );
                        for mut w in sub_wires {
                            w.name = h.value().to_string();
                            if sub_color_is_byblock {
                                w.color = if sel { WireModel::SELECTED } else { entity_color };
                                w.aci = aci;
                            }
                            wires.push(w);
                        }
                    }
                    if !wires.is_empty() {
                        let aabb = entity_aabb(e);
                        for w in &mut wires {
                            w.aabb = aabb;
                        }
                        return wires;
                    }
                }
            }
        }
        // No baked block (e.g. a table created in-app) — synthesise coloured
        // geometry from the rows + TableStyle so fills/colours/borders/margins
        // are honoured instead of the monochrome fallback.
        let mut wires = crate::entities::table::tessellate_table(
            tab, document, sel, entity_color, line_weight_px,
        );
        if !wires.is_empty() {
            let aabb = entity_aabb(e);
            for w in &mut wires {
                w.aci = aci;
                w.aabb = aabb;
            }
            return wires;
        }
    }

    if let EntityType::Insert(ins) = e {
        // Resolve the INSERT's own style so ByBlock sub-entities can inherit it.
        let (ins_color, ins_pat_len, ins_pat, ins_lw_px, _) = view::render::render_style_for(document, e);
        let ins_color = view::render::adapt_to_bg(ins_color, bg_color);
        let ip = glam::Vec3::new(
            (ins.insert_point.x) as f32,
            (ins.insert_point.y) as f32,
            (ins.insert_point.z) as f32,
        );
        let marker = WireModel {
            name: h.value().to_string(),
            points: vec![],
            points_low: Vec::new(),
            color: entity_color,
            selected: sel,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![(ip.as_dvec3(), model::wire_model::SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            vp_scissor: None,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        };

        if let Some(cache) = block_cache {
            // Xrefs render with the same hue but faded toward `bg_color` so
            // the user can recognise external-reference geometry at a glance.
            let is_xref = document
                .block_records
                .get(&ins.block_name)
                .map(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                .unwrap_or(false);
            if let Some(mut wires) = cache::block_cache::expand_insert(
                cache,
                ins,
                h,
                ins_color,
                ins_pat_len,
                ins_pat,
                ins_lw_px,
                sel,
                pslt_factor,
                view_aabb,
                world_per_pixel,
                is_xref,
                bg_color,
            ) {
                // XCLIP: if this INSERT carries an enabled spatial filter,
                // clip the expanded block geometry to the boundary polygon so
                // only the portion inside the clip is drawn.
                if let Some(sf) = pick::xclip::insert_spatial_filter(document, ins) {
                    let poly = pick::xclip::world_clip_polygon_f64(sf, ins);
                    pick::xclip::clip_wires(&mut wires, &poly);
                }

                // Per-INSERT attribute values. The block defn carries the
                // AttributeDefinitions (templates) which expand_insert skips;
                // the AttributeEntity instances live on the Insert itself in
                // WCS and need their own tessellation so the user sees the
                // values they actually filled in. See #20.
                crate::entities::insert::append_insert_attribute_wires(
                    &mut wires,
                    document,
                    ins,
                    h,
                    sel,
                    ins_color,
                    ins_pat_len,
                    ins_pat,
                    ins_lw_px,
                    bg_color,
                    is_xref,
                    pslt_factor,
                    anno_scale,
                );
                wires.push(marker);
                return wires;
            }
        }

        // Cache miss / unavailable: fall back to the original explode path.
        // The block_cache primary path covers all typical Inserts; this
        // branch only fires for pathological cache failures.
        let br = document.block_records.get(&ins.block_name);
        let is_xref = br
            .map(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .unwrap_or(false);
        let mut wires: Vec<WireModel> = ins
            .explode_from_document(document)
            .iter()
            .cloned()
            .map(crate::modules::draw::modify::explode::normalize_insert_entity)
            .flat_map(|sub| {
                let (sub_color, sub_pattern_length, sub_pattern, sub_line_weight_px, sub_aci) =
                    view::render::render_style_for_block_sub(
                        document,
                        &sub,
                        ins_color,
                        ins_pat_len,
                        ins_pat,
                        ins_lw_px,
                    );
                let sub_color = view::render::adapt_to_bg(sub_color, bg_color);
                let sub_color = if is_xref && !sel {
                    cache::block_cache::fade_toward_bg(sub_color, bg_color)
                } else {
                    sub_color
                };
                let sub_aabb = entity_aabb(&sub);
                let sub_pattern_length = sub_pattern_length * pslt_factor;
                let sub_pattern = sub_pattern.map(|v| v * pslt_factor);
                let mut wires = convert::tessellate::tessellate(
                    document,
                    h,
                    &sub,
                    sel,
                    sub_color,
                    sub_pattern_length,
                    sub_pattern,
                    sub_line_weight_px,
                    anno_scale,
                    world_per_pixel,
                );
                for w in &mut wires {
                    w.name = h.value().to_string();
                    w.aci = sub_aci;
                    w.aabb = sub_aabb;
                }
                wires
            })
            .collect();
        crate::entities::insert::append_insert_attribute_wires(
            &mut wires,
            document,
            ins,
            h,
            sel,
            ins_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            bg_color,
            is_xref,
            pslt_factor,
            anno_scale,
        );
        wires.push(marker);
        return wires;
    }

    let aabb = entity_aabb(e);

    // Text-specific LOD ladder, keyed off the entity's glyph height in
    // pixels (anno-scaled):
    //   < 1 px  → baseline line in the text's color (text-here hint)
    //   1–5 px  → greeked OBB rect in the text's color
    //   ≥ 5 px  → full per-glyph stroke tessellation
    //
    // Applies to every entity that is "primarily a piece of text" — Text,
    // MText, ATTDEF, ATTRIB, Tolerance — so far-out drawings don't pay the
    // full glyph-tessellation cost. Composite entities (Dimension, Table,
    // MultiLeader) carry non-text geometry and have their own LOD paths.
    if let Some(wpp) = world_per_pixel {
        let text_height: Option<f64> = match e {
            EntityType::Text(t) => Some(t.height * anno_scale as f64),
            EntityType::MText(m) => Some(m.height * anno_scale as f64),
            EntityType::AttributeDefinition(a) => Some(a.height * anno_scale as f64),
            EntityType::AttributeEntity(a) => Some(a.height * anno_scale as f64),
            EntityType::Tolerance(t) => {
                // Tolerance text_height defaults to 0.18 from creation; treat
                // 0 as missing and fall back to the AutoCAD default so the
                // pixel check still kicks in for legitimately tiny dimensions.
                let raw = if t.text_height > 0.0 { t.text_height } else { 2.5 };
                Some(raw * anno_scale as f64)
            }
            _ => None,
        };
        if let Some(h_world) = text_height {
            let h_px = (h_world as f32) / wpp;
            // Wrap-expanded line count for MText (Text = 1).
            let n_lines = match e {
                EntityType::MText(m) => {
                    crate::entities::text_support::mtext_line_count(m, document, anno_scale)
                }
                _ => 1,
            };
            if h_px < 1.0 {
                let pts = crate::entities::text_support::text_baseline_points(e, anno_scale, n_lines);
                if pts.len() < 2 {
                    return vec![];
                }
                // Skip the baseline too if the line itself projects under
                // 2 px (e.g. a 1-char text seen edge-on). All wrap lines
                // share the same baseline length, so the first segment is
                // a representative sample.
                let dx = pts[1][0] - pts[0][0];
                let dy = pts[1][1] - pts[0][1];
                let len_px = (dx * dx + dy * dy).sqrt() / wpp;
                if len_px < 2.0 {
                    // Text projects to under 2 px — fall back to the
                    // generic LOD stub so the entity stays visible /
                    // selectable. #19. Text is 2-D in the XY plane so
                    // z_min = z_max = 0 keeps the historical behaviour.
                    return vec![lod_stub_wire(
                        h.value().to_string(),
                        entity_color,
                        sel,
                        aci,
                        aabb,
                        0.0,
                        0.0,
                    )];
                }
                return vec![WireModel {
                    name: h.value().to_string(),
                    points: pts,
                    points_low: Vec::new(),
                    color: entity_color,
                    selected: sel,
                    aci,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px: 1.0,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),
                }];
            }
            if h_px < 5.0 && aabb != WireModel::UNBOUNDED_AABB {
                let fill_tris = crate::entities::text_support::text_greek_obb_tris(e, anno_scale, n_lines);
                if fill_tris.is_empty() {
                    // Text greek fallback: also 2-D, keep stub at z=0.
                    return vec![lod_stub_wire(
                        h.value().to_string(),
                        entity_color,
                        sel,
                        aci,
                        aabb,
                        0.0,
                        0.0,
                    )];
                }
                // Greek text renders via the face3d fill batch, which colours
                // each tri with `wire.color`. Bake the selected colour in so
                // a selected text stays highlighted across the LOD boundary.
                // hit_test's AABB fallback handles window / crossing. #19.
                let fill_color = if sel { WireModel::SELECTED } else { entity_color };
                return vec![WireModel {
                    name: h.value().to_string(),
                    points: vec![],
                    points_low: Vec::new(),
                    color: fill_color,
                    selected: sel,
                    aci,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px: 1.0,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris,
                    fill_tris_low: Vec::new(),
                }];
            }
        }
    }

    let mut bases = convert::tessellate::tessellate(
        document,
        h,
        e,
        sel,
        entity_color,
        pattern_length,
        pattern,
        line_weight_px,
        anno_scale,
        world_per_pixel,
    );
    for b in &mut bases {
        b.aci = aci;
        b.aabb = aabb;
    }

    // Complex linetypes (with embedded shapes / text) expand the *base*
    // polyline along its tangent. Text-type entities never have a complex
    // linetype assigned, so we only consult the first wire here — multi-wire
    // returns come exclusively from MTEXT colour splits which can't trigger
    // this path.
    if let Some(clt) = crate::linetypes::complex_lt(lt_name) {
        if let Some(base) = bases.first() {
            let mut wires = text::complex_lt::apply_along(
                &base.name,
                &base.points,
                clt,
                (lt_scale * pslt_factor).max(1e-4),
                entity_color,
                sel,
                base.line_weight_px,
            );
            if !wires.is_empty() {
                for w in &mut wires {
                    w.aabb = aabb;
                }
                return wires;
            }
        }
    }

    bases
}

/// Build the 4 OBB corners (CCW: bl, br, tr, tl) of a Text / MText entity
/// in its **native frame** — for top-level entities this is world coords,
/// for block-defn subs it's block-local. No offset/transform applied.
/// Width is approximated from glyph height × character count (TEXT) or
/// from `rectangle_width` (MTEXT). Returns `None` for non-text entities.
///
/// `mtext_lines_override` lets the caller plug in a wrap-aware line count
/// (from `text_support::mtext_line_count`). Without it, MText's OBB
/// height collapses to a single line when the file omits `rectangle_height`,
/// which makes downstream per-line LOD math degenerate.

/// Build a "low-LOD stub" wire for an entity that would otherwise be culled
/// to nothing — the entity's AABB diagonal as a 2-point segment, plus the
/// AABB itself so window / crossing selection picks the entity up. The
/// stored `selected` flag tracks across zoom levels so highlight visuals
/// don't disappear when the LOD level changes. See #19.
fn lod_stub_wire(
    name: String,
    color: [f32; 4],
    selected: bool,
    aci: u8,
    aabb: [f32; 4],
    z_min: f32,
    z_max: f32,
) -> WireModel {
    let [ax, ay, bx, by] = aabb;
    let cx = (ax + bx) * 0.5;
    let cy = (ay + by) * 0.5;
    let cz = (z_min + z_max) * 0.5;
    // Mirror what tessellate.rs does for the non-stub paths: bake the
    // selection-highlight colour into the wire so a re-tessellate triggered
    // by a zoom-induced LOD change keeps the entity highlighted. Without
    // this swap the wire's `selected` flag is true but its colour stays at
    // the entity's own hue, so the user sees the highlight vanish at the
    // LOD boundary. #19.
    let stored_color = if selected { WireModel::SELECTED } else { color };
    WireModel {
        name,
        // Diagonal of the entity's 3D AABB so depth tests against
        // shaded / hidden-line geometry are correct — the stub doesn't
        // flatten to z=0 and pop in front of objects that sit at a
        // different elevation. 2D entities (text fallbacks) pass
        // z_min = z_max = 0 to keep the historical behaviour.
        points: vec![[ax, ay, z_min], [bx, by, z_max]],
        points_low: Vec::new(),
        color: stored_color,
        selected,
        aci,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: vec![[cx as f64, cy as f64, cz as f64]],
        aabb,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Sub-pixel LOD stub for 3D entities. Emits the entity's 3D AABB as a
/// 12-edge cube so the geometry occupies the same screen footprint and
/// depth range as the full tessellation, just with a tiny constant cost
/// (12 line segments). Without this, the diagonal stub used by
/// `lod_stub_wire` cuts off at two opposite bbox corners and drifts
/// visibly when the camera crosses the LOD threshold.
fn lod_stub_wire_3d(
    name: String,
    color: [f32; 4],
    selected: bool,
    aci: u8,
    aabb: [f32; 4],
    z_min: f32,
    z_max: f32,
) -> WireModel {
    let [x0, y0, x1, y1] = aabb;
    let (z0, z1) = if z_min <= z_max { (z_min, z_max) } else { (z_max, z_min) };
    let p = [
        [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0],
        [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1],
    ];
    // 12 edges = 4 bottom-face + 4 top-face + 4 vertical connectors.
    const EDGES: [(usize, usize); 12] = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];
    let mut points: Vec<[f32; 3]> = Vec::with_capacity(EDGES.len() * 3);
    for (a, b) in EDGES {
        if !points.is_empty() {
            points.push([f32::NAN; 3]);
        }
        points.push(p[a]);
        points.push(p[b]);
    }
    let stored_color = if selected { WireModel::SELECTED } else { color };
    WireModel {
        name,
        points,
        points_low: Vec::new(),
        color: stored_color,
        selected,
        aci,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        // No `key_vertices` — Face3DGpu requires 4 corners to emit a
        // fill quad, and we don't want this stub painted as a solid
        // face. The wire pass still draws its 12 edges.
        key_vertices: vec![],
        aabb,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Tessellate each visible AttributeEntity attached to an Insert and append
/// the resulting wires. AttributeEntity positions are already in WCS — the
/// INSERT only stamps the geometry once, attribute text sits at the world
/// position recorded on each ATTRIB. See #20.
#[allow(clippy::too_many_arguments)]
pub(crate) fn entity_aabb(e: &acadrust::EntityType) -> [f32; 4] {
    let bbox = e.as_entity().bounding_box();
    let min_x = (bbox.min.x) as f32;
    let min_y = (bbox.min.y) as f32;
    let max_x = (bbox.max.x) as f32;
    let max_y = (bbox.max.y) as f32;
    // A degenerate box (min == max == 0) means bounding_box() returned Default —
    // use UNBOUNDED so the wire is never wrongly pre-rejected.
    if min_x == max_x && min_y == max_y {
        return WireModel::UNBOUNDED_AABB;
    }
    [min_x, min_y, max_x, max_y]
}

/// AABB of `e` in WCS f64 (no world_offset subtraction). `None` for
/// entities whose `bounding_box()` returned the degenerate default
/// (which `entity_aabb` collapses to `UNBOUNDED_AABB`). Quadtree
/// indexing uses this so changing `world_offset` doesn't invalidate
/// the index.
fn entity_world_aabb_f64(e: &acadrust::EntityType) -> Option<[f64; 4]> {
    let bbox = e.as_entity().bounding_box();
    let (xmin, ymin, xmax, ymax) = (bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y);
    if xmin == xmax && ymin == ymax {
        return None;
    }
    if !xmin.is_finite() || !ymin.is_finite() || !xmax.is_finite() || !ymax.is_finite() {
        return None;
    }
    Some([xmin, ymin, xmax, ymax])
}

/// True if `e` is a type the quadtree should skip. `Insert` and
/// `Viewport` are sized only after extra transformation; tessellation
/// already handles them via dedicated code paths. `Block`/`BlockEnd`
/// are block-defn sentinels with no geometry.
fn is_unindexable_entity(e: &acadrust::EntityType) -> bool {
    use acadrust::EntityType as E;
    matches!(
        e,
        E::Insert(_) | E::Viewport(_) | E::Block(_) | E::BlockEnd(_)
    )
}

