pub mod acad_to_truck;
pub mod block_cache;
mod camera;
pub mod complex_lt;
pub mod cxf;
pub mod dispatch;
pub mod grip;
pub mod hatch_model;
pub mod hatch_patterns;
pub mod hit_test;
pub mod image_model;
pub mod mesh_model;
pub mod object;
pub mod paper_canvas;
pub mod pipeline;
pub mod properties;
pub mod quadtree;

/// Result of `Scene::entity_index()`. The wire path queries `tree` for
/// view-rect candidates and also always emits `unbounded_handles`
/// (entities with no usable bbox — legacy `UNBOUNDED_AABB` sentinel).
pub(super) struct EntityIndex {
    pub tree: quadtree::QuadTree,
    pub unbounded_handles: Vec<Handle>,
}
pub(crate) mod render;
mod selection;
pub mod solid3d_tess;
pub mod tess_util;
pub mod tessellate;
pub mod transform;
pub mod truck_tess;
pub mod viewport_pane;
pub mod wire_model;

use camera::Camera;
pub use camera::Projection;
pub use hatch_model::HatchModel;
pub use image_model::ImageModel;
pub use mesh_model::MeshLodSet;
pub use object::{GripApply, GripDef};
pub use pipeline::uniforms::Uniforms;
pub use pipeline::viewcube::{
    hit_test, hover_id, CubeRegion, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
pub use selection::SelectionState;
pub use wire_model::WireModel;

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
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Global counter so every Scene and every geometry mutation gets a
/// process-wide unique epoch. This prevents two different tabs (Scenes)
/// from ever sharing the same epoch value, which would cause the shared
/// GPU Pipeline to skip re-uploading geometry when switching tabs.
static GEOMETRY_EPOCH: AtomicU64 = AtomicU64::new(1);

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
    pub world_offset: [f64; 3],
    pub local_extent_max: f32,
    pub hatches: HashMap<Handle, HatchModel>,
    pub images: HashMap<Handle, ImageModel>,
    pub meshes: HashMap<Handle, MeshLodSet>,
    /// Number of entities removed by the corrupt-entity guard during load.
    /// Reported back to the UI so the user knows when a file had parser-junk
    /// entities silently dropped.
    pub corrupt_dropped: usize,
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
    // and Civil-3D-style top-level extents that span only an Insert's
    // bounding box rather than the actual MSPACE geometry. Any of those
    // leave the precision-preserving offset wrong, so direct MSPACE
    // entities render at huge magnitudes and f32 wires lose precision.
    //
    // Cross-check the header against a per-entity AABB scan of MSPACE
    // (same `bounding_box()` API and same SANE_EXTENT/zero-placeholder
    // filters that `block_cache::build_defn` already uses for block defns)
    // and prefer the entity-scan when the header center drifts more than
    // 10× its own half-span away from the entity centroid.
    let (world_offset, local_extent_max) = compute_world_offset(doc, model_block);

    use rayon::prelude::*;

    // Single pass over entities — collect only handles per kind. No clones.
    // Heavy tessellation runs in parallel below, reading entities via
    // `doc.get_entity(h)` (O(1) HashMap lookup).
    let mut hatch_handles: Vec<Handle> = Vec::new();
    let mut image_handles: Vec<Handle> = Vec::new();
    let mut mesh_handles: Vec<Handle> = Vec::new();
    for e in doc.entities() {
        let h = e.common().handle;
        match e {
            EntityType::Hatch(_) | EntityType::Solid(_) => hatch_handles.push(h),
            EntityType::RasterImage(_) => image_handles.push(h),
            EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) => {
                mesh_handles.push(h)
            }
            _ => {}
        }
    }

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
            let owner = e.common().owner_handle;
            let offset = if owner == model_block {
                world_offset
            } else {
                [0.0; 3]
            };
            let (raw, ..) = render::render_style_for(doc, e);
            let color = render::adapt_to_bg(raw, LOAD_BG);
            let model = match e {
                EntityType::Hatch(dxf) => Scene::hatch_model_from_dxf(dxf, color, offset),
                EntityType::Solid(solid) => Some(Scene::solid_hatch_model(solid, color, offset)),
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
    let facet_res = doc.header.facet_resolution;
    let meshes: HashMap<Handle, MeshLodSet> = mesh_handles
        .par_iter()
        .filter_map(|&handle| {
            let e = doc.get_entity(handle)?;
            let (raw, ..) = render::render_style_for(doc, e);
            let color = render::adapt_to_bg(raw, LOAD_BG);
            crate::entities::solid3d::tessellate_volume(e, color, facet_res)
                .map(|m| (handle, offset_mesh_lod_set(m, world_offset)))
        })
        .collect();

    DerivedCaches {
        world_offset,
        local_extent_max,
        hatches,
        images,
        meshes,
        corrupt_dropped: 0,
    }
}

/// Pick the model-space precision-preserving offset and the `fit_all`
/// outlier-rejection limit.
///
/// Tries header `$EXTMIN/$EXTMAX` first, then cross-checks against a direct
/// MSPACE entity AABB scan. The entity scan wins when the header is invalid
/// (sentinel / sub-empty) or when the header center has drifted more than
/// 10× its own half-span from the entity centroid (a stale-extents DXF).
fn compute_world_offset(
    doc: &acadrust::CadDocument,
    model_block: Handle,
) -> ([f64; 3], f32) {
    // Mirrors `block_cache::SANE_EXTENT` — wire coords past this magnitude
    // are treated as corruption rather than precision-relevant geometry.
    const SANE_EXTENT: f64 = 1.0e8;

    // The filter here MUST agree with `belongs_to_visible_block` (the
    // render-time filter): if rendering treats an entity as MSPACE but
    // we skip it here, our offset misses the geometry that's actually on
    // screen, and direct WCS-coordinate wires drag f32 precision to its
    // knees. Likewise, including block-defn entities that the render
    // path strictly drops would pull the centroid toward block-local
    // origins.
    //
    // Authoritative path: if the model BlockRecord enumerates its
    // entities via `entity_handles`, use that set directly. Falls back
    // to the legacy permissive interpretation when no BlockRecord
    // enumerates anything (legacy DXF without group-code 330) — match
    // `belongs_to_visible_block`'s permissive default for that case.
    let model_br = doc
        .block_records
        .iter()
        .find(|br| br.handle == model_block);
    let mspace_set: Option<std::collections::HashSet<Handle>> = model_br
        .filter(|br| !br.entity_handles.is_empty())
        .map(|br| br.entity_handles.iter().copied().collect());
    let any_enumerated = doc
        .block_records
        .iter()
        .any(|br| !br.entity_handles.is_empty());
    let owned_by_other_block: std::collections::HashSet<Handle> = if mspace_set.is_none() {
        doc.block_records
            .iter()
            .filter(|br| br.handle != model_block)
            .flat_map(|br| br.entity_handles.iter().copied())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    // ── Pass 1: collect per-entity centroids ─────────────────────────────
    // Min/max midpoint is wrecked by a single bogus entity at WCS distance
    // (e.g. a Ray with bad direction, an orphan reference at WCS x=-510k
    // when the real drawing sits at WCS x=+510k — the midpoint lands
    // halfway between, far from both clusters). Per-entity centroids let
    // us take the median, which is immune to single outliers regardless
    // of how far they sit.
    let mut centers: Vec<[f64; 3]> = Vec::new();
    for e in doc.entities() {
        let c = e.common();
        let h = c.handle;
        let include = if let Some(ref set) = mspace_set {
            set.contains(&h)
        } else if c.owner_handle == model_block {
            true
        } else if !c.owner_handle.is_null() {
            false
        } else if owned_by_other_block.contains(&h) {
            false
        } else {
            // owner null + h not enumerated by any block: legacy permissive
            // when no block enumerated at all, strict drop otherwise (same
            // as belongs_to_visible_block).
            !any_enumerated
        };
        if !include {
            continue;
        }
        // Skip block-defn sentinels and AttributeDefinition — same as
        // block_cache::build_defn. Their bboxes don't represent drawable
        // MSPACE geometry.
        if matches!(
            e,
            EntityType::Block(_)
                | EntityType::BlockEnd(_)
                | EntityType::AttributeDefinition(_)
        ) {
            continue;
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
            continue;
        }
        let cx = (bmin.x + bmax.x) * 0.5;
        let cy = (bmin.y + bmax.y) * 0.5;
        let cz = (bmin.z + bmax.z) * 0.5;
        if !cx.is_finite() || !cy.is_finite() || !cz.is_finite() {
            continue;
        }
        if cx.abs() > SANE_EXTENT || cy.abs() > SANE_EXTENT {
            continue;
        }
        centers.push([cx, cy, cz]);
    }
    let entity_ok = !centers.is_empty();

    // Median of per-entity centroids → robust drawing center.
    // For local_extent_max: 95th-percentile distance from the median × 2
    // gives the half-span of the dense cluster while leaving room for
    // legitimate outliers (sparse leaders, dimensions, scattered annotations).
    let median = |v: &mut Vec<f64>| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    let percentile = |v: &mut Vec<f64>, frac: f64| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let i = ((v.len() as f64 - 1.0) * frac).round() as usize;
        v[i]
    };
    let (ecx, ecy, ecz, espan_max) = if entity_ok {
        let mut xs: Vec<f64> = centers.iter().map(|c| c[0]).collect();
        let mut ys: Vec<f64> = centers.iter().map(|c| c[1]).collect();
        let mut zs: Vec<f64> = centers.iter().map(|c| c[2]).collect();
        let mx = median(&mut xs);
        let my = median(&mut ys);
        let mz = median(&mut zs);
        let mut dx: Vec<f64> = centers.iter().map(|c| (c[0] - mx).abs()).collect();
        let mut dy: Vec<f64> = centers.iter().map(|c| (c[1] - my).abs()).collect();
        let p95 = percentile(&mut dx, 0.95).max(percentile(&mut dy, 0.95));
        (mx, my, mz, (p95 * 2.0).max(1.0) as f32)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    // ── Pass 2: read header extents ──────────────────────────────────────
    let h = &doc.header;
    let hmin = h.model_space_extents_min;
    let hmax = h.model_space_extents_max;
    let header_ok = hmin.x < hmax.x
        && hmin.y < hmax.y
        && hmin.x.abs() < SANE_EXTENT
        && hmax.x.abs() < SANE_EXTENT
        && hmin.y.abs() < SANE_EXTENT
        && hmax.y.abs() < SANE_EXTENT;

    // Entity-derived offset is preferred whenever it's available — the
    // median-of-centroids ignores Ray/orphan/duplicate-block-defn outliers
    // that the header EXTMIN/EXTMAX (a min/max midpoint) bakes in. Header
    // is the fallback only when the entity scan found nothing.
    if entity_ok {
        ([ecx, ecy, ecz], espan_max)
    } else if header_ok {
        let offset = [
            (hmin.x + hmax.x) * 0.5,
            (hmin.y + hmax.y) * 0.5,
            (hmin.z + hmax.z) * 0.5,
        ];
        let hw = ((hmax.x - hmin.x) * 0.5) as f32;
        let hh = ((hmax.y - hmin.y) * 0.5) as f32;
        let hz = ((hmax.z - hmin.z) * 0.5).max(1.0) as f32;
        (offset, hw.max(hh).max(hz) * 10.0)
    } else {
        ([0.0; 3], 1e9_f32)
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
}

/// One pane of the Model-space tiled viewport layout: the normalized screen
/// rectangle it fills and the camera it last had. The active tile uses the
/// live `Scene::camera` (so orbit/pan/zoom drive it); inactive tiles keep a
/// snapshot here, swapped in when they become active.
#[derive(Clone)]
pub(crate) struct ModelTile {
    pub(crate) rect: iced::Rectangle,
    pub(crate) camera: Camera,
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
fn offset_mesh_lod_set(mut set: MeshLodSet, world_offset: [f64; 3]) -> MeshLodSet {
    let [ox, oy, oz] = world_offset;
    let (fx, fy, fz) = (ox as f32, oy as f32, oz as f32);
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for lod in &mut set.lods {
        for v in &mut lod.verts {
            v[0] -= fx;
            v[1] -= fy;
            v[2] -= fz;
            if v[0] < min_x { min_x = v[0]; }
            if v[1] < min_y { min_y = v[1]; }
            if v[0] > max_x { max_x = v[0]; }
            if v[1] > max_y { max_y = v[1]; }
        }
    }
    if min_x.is_finite() {
        set.world_aabb = [min_x, min_y, max_x, max_y];
    }
    set
}

/// Stable hash of a `Camera`'s pose for use as a per-tile cache key.
/// Two cameras with bit-identical target / rotation / distance hash the
/// same; any orbit / pan / zoom on a tile bumps it.
fn camera_state_hash(c: &Camera) -> u64 {
    fn h(state: u64, x: f32) -> u64 {
        state.rotate_left(13) ^ x.to_bits() as u64
    }
    let mut s: u64 = 0xcbf2_9ce4_8422_2325;
    s = h(s, c.target.x);
    s = h(s, c.target.y);
    s = h(s, c.target.z);
    s = h(s, c.rotation.x);
    s = h(s, c.rotation.y);
    s = h(s, c.rotation.z);
    s = h(s, c.rotation.w);
    s = h(s, c.distance);
    s
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
    /// In-progress preview wires while a command is active (rubber-band + object ghosts).
    pub preview_wires: Vec<WireModel>,
    /// Committed-segment wire drawn during multi-point commands (normal colour).
    pub interim_wire: Option<WireModel>,
    pub camera_generation: u64,
    /// Incremented whenever geometry-affecting state changes (entities, selection,
    /// preview wires, layer visibility, layout). The GPU pipeline uses this to
    /// skip re-uploading unchanged geometry buffers every frame.
    pub geometry_epoch: u64,
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
    /// carries `(geometry_epoch, camera_state_hash)` so a cache hit is
    /// also rejected when the same tile's camera moved or the document
    /// geometry changed.
    model_tile_wire_cache:
        RefCell<HashMap<usize, ((u64, u64), Arc<Vec<WireModel>>)>>,
    /// Index built from every SortEntitiesTable in the document.
    /// Maps block_handle → (entity_handle.value() → sort_handle.value()).
    /// Replaces the O(objects) linear scan inside `wires_for_block()` with an O(1) lookup.
    sort_cache: RefCell<Option<(u64, HashMap<Handle, HashMap<u64, u64>>)>>,
    /// Cached hatch fill models, keyed by geometry_epoch. View culling
    /// is handled at draw time via `hatch_skip_flags` in the pipeline,
    /// not at build time — that lets the GPU buffer stay stable across
    /// pan/zoom while still skipping out-of-view hatches.
    hatch_cache: RefCell<Option<(u64, Arc<Vec<HatchModel>>)>>,
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
    viewport_wire_cache: RefCell<HashMap<Handle, ((u64, u32), Arc<Vec<WireModel>>)>>,
    /// Cached tessellation of paper-space layout block entities (title block, annotations, etc.).
    /// Separate from `wire_cache` so paper_canvas_wires() doesn't re-tessellate on every frame.
    /// Keyed by `(geometry_epoch, camera_generation)` — paper view changes
    /// on zoom too, so culled wire output depends on camera.
    paper_sheet_cache: RefCell<Option<((u64, u64), Arc<Vec<WireModel>>)>>,
    /// Per-viewport projected wire cache for the paper canvas (2-D Iced widget).
    /// Stores projected + clipped wires in paper-space coordinates.
    /// Maps vp_handle → (geometry_epoch, Vec<WireModel>).
    paper_projected_cache: RefCell<HashMap<Handle, (u64, Vec<WireModel>)>>,
    /// Full paper-canvas wire list (sheet + inactive-viewport projections + interim/preview).
    /// Keyed by geometry_epoch — valid for all navigation frames (pan/zoom do not bump epoch).
    /// Returns Arc so paper_canvas_wires() is O(1) on cache hits.
    paper_canvas_cache: RefCell<Option<(u64, Arc<Vec<WireModel>>)>>,
    /// Active layout name — "Model" or a paper space layout name.
    pub current_layout: String,
    /// GPU render data for hatch fills, keyed by the DXF entity Handle.
    pub hatches: HashMap<Handle, HatchModel>,
    /// GPU render data for solid meshes (truck Shell/Solid tessellation).
    pub meshes: HashMap<Handle, MeshLodSet>,
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
    /// Scene centroid subtracted from all coordinates before f32 conversion.
    /// Eliminates f32 precision loss at large world coordinates (e.g. UTM 4,000,000 m).
    pub world_offset: [f64; 3],
    /// Largest local-space coordinate expected from real geometry, derived from
    /// EXTMIN/EXTMAX (10× safety margin). Used by fit_all() to ignore garbage
    /// entity coordinates (origin-stuck entities, bad Ray/XLine direction vectors).
    pub local_extent_max: f32,
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
    block_defn_cache: RefCell<Option<(u64, Arc<block_cache::BlockCache>)>>,
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
            }]),
            active_model_tile: std::cell::Cell::new(0),
            selection: Rc::new(RefCell::new(SelectionState::default())),
            document: CadDocument::new(),
            selected: HashSet::new(),
            preview_wires: vec![],
            interim_wire: None,
            camera_generation: 0,
            geometry_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            wire_cache: RefCell::new(None),
            model_tile_wire_cache: RefCell::new(HashMap::new()),
            sort_cache: RefCell::new(None),
            hatch_cache: RefCell::new(None),
            wipeout_cache: RefCell::new(None),
            image_cache: RefCell::new(None),
            mesh_cache: RefCell::new(None),
            viewport_wire_cache: RefCell::new(HashMap::new()),
            paper_sheet_cache: RefCell::new(None),
            paper_projected_cache: RefCell::new(HashMap::new()),
            paper_canvas_cache: RefCell::new(None),
            current_layout: "Model".to_string(),
            hatches: HashMap::new(),
            meshes: HashMap::new(),
            images: HashMap::new(),
            active_viewport: None,
            bg_color: [0.11, 0.11, 0.11, 1.0],
            paper_bg_color: [1.0, 1.0, 1.0, 1.0],
            world_offset: [0.0; 3],
            local_extent_max: 1e9,
            annotation_scale: 1.0,
            model_extents_cache: RefCell::new(None),
            entity_block_map_cache: RefCell::new(None),
            block_defn_cache: RefCell::new(None),
            entity_index_cache: RefCell::new(None),
            last_render_aspect: std::cell::Cell::new(16.0 / 9.0),
            last_world_per_pixel: std::cell::Cell::new(0.0),
            viewcube_hover: std::cell::Cell::new(None),
        }
    }

    /// Compute the current camera's world-space XY view AABB with
    /// `world_offset` already subtracted (so the result is in the same f32
    /// space as emitted wire points). Adds a 25% margin around the
    /// frustum to absorb pan inertia and avoid clipped-edge popping.
    pub(super) fn view_world_aabb(&self) -> Option<[f32; 4]> {
        if self.current_layout != "Model" {
            // Paper-space viewport composition handles its own culling; the
            // top-level paper canvas is small enough not to need it.
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
        let cx = cam.target.x;
        let cy = cam.target.y;
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
    /// Also returns `None` in paper space: `set_render_pixel_scale` is only
    /// fed by the shader pipeline (`build_primitive`), and the paper layout
    /// renders through `PaperCanvas` (an Iced 2-D canvas) which never touches
    /// it. Whatever value is cached would be a stale model-world wpp and,
    /// applied to mm-sheet entity AABBs, would cull every paper-space
    /// annotation. Matches the same skip already in `view_world_aabb`.
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
    pub(super) fn block_cache_arc(&self) -> Arc<block_cache::BlockCache> {
        {
            let cache = self.block_defn_cache.borrow();
            if let Some((epoch, ref arc)) = *cache {
                if epoch == self.geometry_epoch {
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
        let built = block_cache::BlockCache::build(&self.document, anno, bg);
        let arc = Arc::new(built);
        *self.block_defn_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    pub fn bump_geometry(&mut self) {
        self.geometry_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
    }

    /// Re-evaluate every cached mesh's color through `render_style` so a
    /// `BACKGROUND` change picks up the new `adapt_to_bg` result without
    /// re-tessellating ACIS geometry. Caller must bump `geometry_epoch`
    /// afterwards so the GPU re-uploads the now-updated colour data.
    pub fn recolor_meshes(&mut self) {
        // Cache colour lookups by handle to avoid borrowing the document
        // re-entrantly through `render_style` inside a `&mut self` loop.
        let colors: HashMap<Handle, [f32; 4]> = self
            .meshes
            .keys()
            .filter_map(|&h| {
                self.document
                    .get_entity(h)
                    .map(|e| (h, self.render_style(e).0))
            })
            .collect();
        for (h, set) in self.meshes.iter_mut() {
            if let Some(&c) = colors.get(h) {
                for lod in &mut set.lods {
                    lod.color = c;
                }
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
                if Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block {
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
                    if Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block {
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
                    Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block
                } else {
                    false
                }
            })
            .count()
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
                if Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block {
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
        let mut by_name: std::collections::HashMap<String, (i16, Handle)> = Default::default();
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
        let cam_key = camera_state_hash(cam);
        let key = (self.geometry_epoch, cam_key);
        {
            let cache = self.model_tile_wire_cache.borrow();
            if let Some((cached_key, ref arc)) = cache.get(&tile_idx) {
                if *cached_key == key {
                    return Arc::clone(arc);
                }
            }
        }
        // Compute the tile's own view AABB and world-per-pixel from its
        // camera + pixel size (mirrors `view_world_aabb` /
        // `world_per_pixel`, neither of which knows about anything beyond
        // the live `self.camera`).
        let view_aabb = if self.camera_generation == 0 {
            None
        } else {
            let h = cam.ortho_size();
            let w = h * cam_aspect.max(0.01);
            let margin = 1.25_f32;
            let cx = cam.target.x;
            let cy = cam.target.y;
            Some([cx - w * margin, cy - h * margin, cx + w * margin, cy + h * margin])
        };
        let wpp = if tile_pixel_height > 0.0 {
            Some((2.0 * cam.ortho_size()) / tile_pixel_height)
        } else {
            None
        };
        let block = self.model_space_block_handle();
        let arc = Arc::new(self.wires_for_block_culled(block, view_aabb, wpp, None, None));
        self.model_tile_wire_cache
            .borrow_mut()
            .insert(tile_idx, (key, Arc::clone(&arc)));
        arc
    }

    /// Cached tessellation of the current layout block's paper-space entities.
    /// Shared by both `entity_wires_arc()` and `paper_canvas_wires()` so a single
    /// cache miss triggers only one tessellation pass, not two.
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
        let arc = Arc::new(self.wires_for_block(layout_block));
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

    pub(super) fn hatch_models_arc(&self) -> Arc<Vec<HatchModel>> {
        {
            let cache = self.hatch_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let arc = Arc::new(self.synced_hatch_models());
        *self.hatch_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
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
        let arc = Arc::new(self.images.values().cloned().collect());
        *self.image_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
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
        let arc = Arc::new(self.meshes.values().cloned().collect());
        *self.mesh_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Hatches eligible for click / box / lasso hit-testing in the current
    /// layout. Filters out block-internal source hatches (stored in
    /// `self.hatches` at block-local coords for the block-defn position,
    /// which doesn't project correctly through the offset-rel view_proj
    /// and was causing the wrong hatch to be selected on click).
    pub fn visible_hatches_for_click(&self) -> HashMap<Handle, HatchModel> {
        let layout_block = self.current_layout_block_handle();
        self.hatches
            .iter()
            .filter_map(|(&h, m)| {
                let owner = self.document.get_entity(h)?.common().owner_handle;
                if self.belongs_to_visible_block(h, owner, layout_block) {
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
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };
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
                .map(crate::modules::home::modify::explode::normalize_insert_entity)
            {
                let EntityType::Hatch(dxf) = sub else {
                    continue;
                };
                if dxf.common.invisible || layer_hidden(&dxf.common.layer) {
                    continue;
                }
                let color = self.render_style(&EntityType::Hatch(dxf.clone())).0;
                if let Some(model) = Self::hatch_model_from_dxf(&dxf, color, hatch_offset) {
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
                let mut idx: HashMap<Handle, HashMap<u64, u64>> = HashMap::new();
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
            let [ox, oy, _] = self.world_offset;
            let view_wcs: [f64; 4] = [
                local_view[0] as f64 + ox,
                local_view[1] as f64 + oy,
                local_view[2] as f64 + ox,
                local_view[3] as f64 + oy,
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
        use rayon::prelude::*;
        let doc = &self.document;
        let sel = &self.selected;
        let avp = self.active_viewport;
        // A paper-space content viewport renders MODEL block entities while
        // the user is sitting in a paper layout — that path expects
        // `world_offset` subtraction even though `current_layout != "Model"`.
        // Decide based on the block being tessellated, not the layout.
        let is_model_block = block_handle == self.model_space_block_handle();
        let woff = if is_model_block {
            self.world_offset
        } else {
            [0.0; 3]
        };
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
        let blk_ref: &block_cache::BlockCache = &blk_cache;
        // Zoom-adaptive curve sampling for top-level Edge tessellation. Target
        // ~0.5 px chord height — far-out arcs that used to emit hundreds of
        // segments now collapse to a handful. The guard clears the override
        // when this scope exits so off-render tessellation (snap previews,
        // hit-test, block_cache rebuild) sees the default.
        struct CurveTolGuard;
        impl Drop for CurveTolGuard {
            fn drop(&mut self) {
                crate::scene::truck_tess::set_curve_tol_override(None);
            }
        }
        let _tol_guard = wpp.map(|w| {
            crate::scene::truck_tess::set_curve_tol_override(Some((w * 0.5) as f64));
            CurveTolGuard
        });
        let mut wires: Vec<WireModel> = visible
            .into_par_iter()
            .flat_map(|e| {
                tessellate_entity(
                    doc,
                    sel,
                    avp,
                    woff,
                    bg,
                    anno,
                    e,
                    Some(blk_ref),
                    view_aabb,
                    wpp,
                )
            })
            .collect();

        // Apply draw order via the cached index (O(1) block lookup).
        {
            let cache = self.sort_cache.borrow();
            if let Some((_, ref idx)) = *cache {
                if let Some(sort_map) = idx.get(&block_handle) {
                    wires.sort_by_key(|w| {
                        let key = Self::handle_from_wire_name(&w.name)
                            .map(|h| h.value())
                            .unwrap_or(u64::MAX);
                        sort_map.get(&key).copied().unwrap_or(u64::MAX / 2)
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
        let mut map: HashMap<Handle, Handle> = HashMap::new();
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
        let mut tree = quadtree::QuadTree::new(root);
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
            self.world_offset,
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
        let [ox, oy, _] = self.world_offset;
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
                        if ax.is_finite() && bx.is_finite() {
                            min = min.min(glam::Vec3::new(ax + ox as f32, ay + oy as f32, 0.0));
                            max = max.max(glam::Vec3::new(bx + ox as f32, by + oy as f32, 0.0));
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
        let oz = self.world_offset[2] as f32;
        for entity in self.document.entities() {
            let c = entity.common();
            if c.owner_handle != model_block || c.invisible {
                continue;
            }
            for wire in self.tessellate_one(entity) {
                for &[x, y, z] in &wire.key_vertices {
                    if x.is_finite() && y.is_finite() && z.is_finite() {
                        min = min.min(glam::Vec3::new(
                            x + ox as f32,
                            y + oy as f32,
                            z + oz,
                        ));
                        max = max.max(glam::Vec3::new(
                            x + ox as f32,
                            y + oy as f32,
                            z + oz,
                        ));
                        any = true;
                    }
                }
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
                Self::is_content_viewport(vp)
                    && vp.common.owner_handle == paper_block
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

            // ── Scale, target, view_center — saved-view-then-fallback ─────
            //
            // Honor the file's saved view (view_target + view_center +
            // view_height) whenever the WCS region it points at overlaps
            // model content. Some DWG files (typical for AutoCAD
            // viewports the user never explicitly panned/zoomed into)
            // arrive with view_target = (0, 0, 0) and a stale view_center
            // pointing at empty WCS — in that case AutoCAD silently
            // auto-fits to the model on first display; mirror that so
            // UTM-scale drawings don't open with blank viewports.
            let mut effective_view_center = (vp.view_center.x, vp.view_center.y);
            let mut effective_view_height = vp.view_height as f32;

            // Project the saved view's WCS rect and test against
            // `world_offset`-centered IQR cluster (`local_extent_max`).
            let saved_center_wcs_x = vp.view_target.x + vp.view_center.x;
            let saved_center_wcs_y = vp.view_target.y + vp.view_center.y;
            let saved_half_h = (effective_view_height as f64) * 0.5;
            let saved_half_w = saved_half_h * (vp.width / vp.height.max(1.0));
            let cluster_half = self.local_extent_max.max(1.0) as f64;
            let cluster_min_x = self.world_offset[0] - cluster_half;
            let cluster_max_x = self.world_offset[0] + cluster_half;
            let cluster_min_y = self.world_offset[1] - cluster_half;
            let cluster_max_y = self.world_offset[1] + cluster_half;
            let saved_overlaps = saved_center_wcs_x + saved_half_w >= cluster_min_x
                && saved_center_wcs_x - saved_half_w <= cluster_max_x
                && saved_center_wcs_y + saved_half_h >= cluster_min_y
                && saved_center_wcs_y - saved_half_h <= cluster_max_y
                && effective_view_height > 1e-9;

            if !saved_overlaps {
                // Saved view points at empty space — auto-fit to the
                // outlier-immune content cluster (world_offset ±
                // local_extent_max).
                let margin = 1.05_f64;
                let fit_h = cluster_half * 2.0 * margin;
                let fit_w = fit_h * (vp.width / vp.height.max(1.0));
                let scale_w = vp.width / fit_w;
                let scale_h = vp.height / fit_h;
                let fit_scale = scale_w.min(scale_h).max(1e-12);
                effective_view_height = (vp.height / fit_scale) as f32;
                effective_view_center = (self.world_offset[0], self.world_offset[1]);
            }

            // When the saved view did not overlap the model cluster we just
            // forced `effective_view_height` to a fit value above — that
            // override must win regardless of the configured priority,
            // otherwise broken files render blank under custom_scale-first.
            let scale = if !saved_overlaps {
                vp.height as f32 / effective_view_height
            } else {
                vp_effective_scale(
                    vp.custom_scale,
                    effective_view_height as f64,
                    vp.height,
                ) as f32
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
            let display_center_x = vp.view_target.x + effective_view_center.0;
            let display_center_y = vp.view_target.y + effective_view_center.1;
            let display_center_z = vp.view_target.z;
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
            let [wo_x, wo_y, wo_z] = self.world_offset;

            for wire in model_wires.iter() {
                let projected_pts: Vec<[f32; 3]> = wire
                    .points
                    .iter()
                    .map(|&[mx, my, mz]| {
                        if mx.is_nan() || my.is_nan() || mz.is_nan() {
                            return [f32::NAN; 3];
                        }
                        // wire stored offset-rel; reconstruct WCS in f64
                        // then subtract display center in WCS → small
                        // f64 mp_proj with full precision.
                        let mp_x = (mx as f64 + wo_x) - display_center_x;
                        let mp_y = (my as f64 + wo_y) - display_center_y;
                        let mp_z = (mz as f64 + wo_z) - display_center_z;
                        let u = mp_x * view_right_d.0
                            + mp_y * view_right_d.1
                            + mp_z * view_right_d.2;
                        let v = mp_x * view_up_d.0
                            + mp_y * view_up_d.1
                            + mp_z * view_up_d.2;
                        if use_perspective {
                            let d_vd = mp_x * view_fwd_d.0
                                + mp_y * view_fwd_d.1
                                + mp_z * view_fwd_d.2;
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
                            [
                                (pcx_d + u * scale_d) as f32,
                                (pcy_d + v * scale_d) as f32,
                                pcz,
                            ]
                        }
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

                let adapted = render::adapt_to_bg(wire.color, self.paper_bg_color);
                let [r, g, b, a] = adapted;
                let mut out = wire.clone();
                out.points = clipped;
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
    pub fn paper_to_model(&self, paper_pt: glam::Vec3) -> glam::Vec3 {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return paper_pt,
        };
        let vp = match self.document.get_entity(vp_handle) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return paper_pt,
        };
        let scale =
            vp_effective_scale(vp.custom_scale, vp.view_height, vp.height) as f32;
        if scale.abs() < 1e-9 {
            return paper_pt;
        }
        let tx = vp.view_target.x as f32;
        let ty = vp.view_target.y as f32;
        let pcx = vp.center.x as f32;
        let pcy = vp.center.y as f32;
        glam::Vec3::new(
            (paper_pt.x - pcx) / scale + tx,
            (paper_pt.y - pcy) / scale + ty,
            paper_pt.z,
        )
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
        // but view_direction convention (matching snap_active_viewport_to_angles) is
        //   (cos(p)*sin(y), +cos(p)*cos(y), sin(p))  ← Y has opposite sign.
        // Negate Y when writing back so camera_for_viewport round-trips correctly.
        let eye = cam.rotation * glam::Vec3::Z;
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = -eye.y as f64;
            vp.view_direction.z = eye.z as f64;
        }
    }

    /// Snap the active viewport's view direction to `eye_dir` (unit
    /// vector from target toward camera). Twist angle is left at its
    /// current value so the up-sense is preserved across successive
    /// snaps. No-op when there is no active viewport or it is locked.
    pub fn snap_active_viewport_to_direction(&mut self, eye_dir: glam::Vec3) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        let eye = eye_dir.normalize_or(glam::Vec3::Z);
        if let Some(acadrust::EntityType::Viewport(vp)) = self.document.get_entity_mut(vp_handle) {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = eye.y as f64;
            vp.view_direction.z = eye.z as f64;
        }
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

    /// View-rotation matrix for the active viewport (MSPACE), or the
    /// paper-space camera's matrix when not in MSPACE.
    /// Used by ViewCube hit-testing so clicks map to the correct camera.
    pub fn active_view_rotation_mat(&self) -> glam::Mat4 {
        if let Some(h) = self.active_viewport {
            if let Some(cam) = self.camera_for_viewport(h) {
                return cam.view_rotation_mat();
            }
        }
        self.camera.borrow().view_rotation_mat()
    }

    /// Return the handle of the user viewport whose bounding rectangle contains
    /// the given paper-space point, or `None` if no viewport matches.
    pub fn viewport_at_paper_point(&self, px: f32, py: f32) -> Option<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document.entities().find_map(|e| {
            let EntityType::Viewport(vp) = e else {
                return None;
            };
            if !Self::is_content_viewport(vp)
                || vp.common.owner_handle != layout_block
                || !vp.status.is_on
            {
                return None;
            }
            let hw = (vp.width / 2.0) as f32;
            let hh = (vp.height / 2.0) as f32;
            let cx = vp.center.x as f32;
            let cy = vp.center.y as f32;
            if px >= cx - hw && px <= cx + hw && py >= cy - hh && py <= cy + hh {
                Some(vp.common.handle)
            } else {
                None
            }
        })
    }

    /// Return the handle of the first active user viewport in the current layout,
    /// or `None` if there are none.  Used by the MS command.
    pub fn first_user_viewport(&self) -> Option<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document.entities().find_map(|e| {
            let EntityType::Viewport(vp) = e else {
                return None;
            };
            if Self::is_content_viewport(vp)
                && vp.common.owner_handle == layout_block
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
            self.document.remove_entity(*h);
        }

        // Remove the Layout object itself.
        self.document.objects.remove(&layout_handle);

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
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color, hatch_offset)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color, hatch_offset))
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
            EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_)
        ) {
            let color = self.render_style(&entity).0;
            let woff = self.world_offset;
            crate::entities::solid3d::tessellate_volume(&entity, color, facet_res)
                .map(|m| offset_mesh_lod_set(m, woff))
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
            self.bump_geometry();
        }
        handle
    }

    /// Returns the RGBA color for the given layer name.
    pub fn layer_color(&self, layer: &str) -> [f32; 4] {
        let layer_entry = self.document.layers.get(layer);
        let color = layer_entry
            .map(|l| &l.color)
            .unwrap_or(&acadrust::types::Color::WHITE);
        let [r, g, b, _] = crate::scene::tess_util::aci_to_rgba(color);
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

        let local = EntityTransform::Translate(-base);
        for (old_handle, mut entity) in source_entities {
            dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::home::modify::explode::normalize_entity_for_block(entity);
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

    fn synced_hatch_models(&self) -> Vec<HatchModel> {
        let layout_block = self.current_layout_block_handle();
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };

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
                            hatch_model::HatchPattern::Pattern(_) => {
                                m.angle_offset = dxf.pattern_angle as f32;
                                let anno = if self.current_layout == "Model" {
                                    self.annotation_scale
                                } else {
                                    1.0
                                };
                                m.scale = dxf.pattern_scale as f32 * anno;
                            }
                            hatch_model::HatchPattern::Gradient { angle_deg, .. } => {
                                *angle_deg = dxf.pattern_angle.to_degrees() as f32;
                            }
                            hatch_model::HatchPattern::Solid => {}
                        }
                    }
                }
                if self.selected.contains(&handle) {
                    m.color = [0.15, 0.55, 1.00, m.color[3]];
                }
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
            for sub in ins
                .explode_from_document(&self.document)
                .into_iter()
                .map(crate::modules::home::modify::explode::normalize_insert_entity)
            {
                let EntityType::Hatch(dxf) = sub else {
                    continue;
                };
                if dxf.common.invisible || layer_hidden(&dxf.common.layer) {
                    continue;
                }
                let color = self.render_style(&EntityType::Hatch(dxf.clone())).0;
                if let Some(mut model) = Self::hatch_model_from_dxf(&dxf, color, hatch_offset) {
                    if selected {
                        model.color = [0.15, 0.55, 1.00, model.color[3]];
                    }
                    models.push(model);
                }
            }
        }

        // Wide LWPolyline and Polyline2D fills
        let [ox, oy, _] = hatch_offset;
        let ox = ox as f32;
        let oy = oy as f32;
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
            for mut boundary in fills {
                // Wires subtract world_offset during tessellation; fills must
                // match or the band drifts away from the centerline on
                // drawings far from origin.
                for p in boundary.iter_mut() {
                    p[0] -= ox;
                    p[1] -= oy;
                }
                models.push(HatchModel {
                    boundary: Arc::new(boundary),
                    pattern: hatch_model::HatchPattern::Solid,
                    name: "SOLID".into(),
                    color,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin: [0.0; 2],
                    vp_scissor: None,
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
        let model_block = self.model_space_block_handle();
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
            let world_offset = if wo.common.owner_handle == model_block {
                self.world_offset
            } else {
                [0.0; 3]
            };
            let boundary = Self::wipeout_boundary_2d(wo, world_offset);
            if boundary.len() >= 3 {
                let mut fill_color = bg_color;
                if self.selected.contains(&wo.common.handle) {
                    fill_color = [0.15, 0.55, 1.00, 0.35];
                }
                models.push(HatchModel {
                    boundary: Arc::new(boundary),
                    pattern: hatch_model::HatchPattern::Solid,
                    name: "WIPEOUT_FILL".into(),
                    color: fill_color,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin: [0.0; 2],
                    vp_scissor: None,
                });
            }
        }
        models
    }

    /// Compute the 2D (XY) boundary polygon for a Wipeout entity.
    fn wipeout_boundary_2d(
        wo: &acadrust::entities::Wipeout,
        world_offset: [f64; 3],
    ) -> Vec<[f32; 2]> {
        use acadrust::entities::WipeoutClipType;

        let [wox, woy, _woz] = world_offset;
        let is_polygon = wo.clipping_enabled
            && wo.clip_boundary_vertices.len() >= 3
            && matches!(wo.clip_type, WipeoutClipType::Polygonal);

        if is_polygon {
            let ox = (wo.insertion_point.x - wox) as f32;
            let oy = (wo.insertion_point.y - woy) as f32;
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
            let ox = (wo.insertion_point.x - wox) as f32;
            let oy = (wo.insertion_point.y - woy) as f32;
            let oz = wo.insertion_point.z as f32;
            let ux = (wo.u_vector.x * wo.size.x) as f32;
            let uy = (wo.u_vector.y * wo.size.x) as f32;
            let vx = (wo.v_vector.x * wo.size.y) as f32;
            let vy = (wo.v_vector.y * wo.size.y) as f32;
            let _ = oz;
            vec![
                [ox, oy],
                [ox + ux, oy + uy],
                [ox + ux + vx, oy + uy + vy],
                [ox + vx, oy + vy],
            ]
        }
    }

    fn hatch_model_from_dxf(
        dxf: &DxfHatch,
        color: [f32; 4],
        world_offset: [f64; 3],
    ) -> Option<HatchModel> {
        let [ox, oy, _oz] = world_offset;
        let normal = (dxf.normal.x, dxf.normal.y, dxf.normal.z);
        // Build the boundary in f64 first so the precision-preserving
        // origin computation below sees full WCS precision. We only cast
        // to f32 once at the end, after subtracting the AABB centre, so
        // the stored offsets are small-magnitude with high f32 precision
        // even on large UTM-scale drawings.
        let to_xy = |x: f64, y: f64| -> [f64; 2] {
            let (wx, wy, _) =
                crate::scene::transform::ocs_point_to_wcs((x, y, dxf.elevation), normal);
            [wx - ox, wy - oy]
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
                            let segs = tess_util::arc_segments(
                                arc.radius,
                                arc.sweep.abs(),
                                tess_util::fill_chord_tol(arc.radius),
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
                        let (sa, span) = tess_util::arc_signed_span(
                            arc.start_angle,
                            arc.end_angle,
                            arc.counter_clockwise,
                        );
                        let segs = tess_util::arc_segments(
                            arc.radius,
                            span.abs(),
                            tess_util::fill_chord_tol(arc.radius),
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
                        let (sa, span) = tess_util::arc_signed_span(
                            ell.start_angle,
                            ell.end_angle,
                            ell.counter_clockwise,
                        );
                        let segs = tess_util::arc_segments(
                            r_maj,
                            span.abs(),
                            tess_util::fill_chord_tol(r_maj),
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
                        let tol = tess_util::fill_chord_tol(diag.max(1.0));

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
        boundary.truncate(hatch_model::MAX_HATCH_BOUNDARY_VERTS);

        let pattern = if dxf.gradient_color.is_enabled() {
            let color2 = dxf
                .gradient_color
                .colors
                .get(1)
                .and_then(|e| e.color.rgb())
                .map(|(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
                .unwrap_or(color);
            let angle_deg = dxf.pattern_angle.to_degrees() as f32;
            hatch_model::HatchPattern::Gradient { angle_deg, color2 }
        } else if dxf.is_solid {
            hatch_model::HatchPattern::Solid
        } else {
            let pat_name = &dxf.pattern.name;
            if let Some(entry) = crate::scene::hatch_patterns::find(pat_name) {
                entry.gpu.clone()
            } else {
                hatch_model::HatchPattern::Pattern(vec![hatch_model::PatFamily {
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

        let model_block = self.model_space_block_handle();
        let world_offset = self.world_offset;

        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((h.common.handle, e.clone())),
                EntityType::Solid(s) => Some((s.common.handle, e.clone())),
                _ => None,
            })
            .collect();

        use rayon::prelude::*;
        self.hatches = entries
            .into_par_iter()
            .filter_map(|(handle, kind)| {
                // Paper-space entities live in sheet coordinates — world_offset must not
                // be applied to them.  Only model-space entities need the shift.
                let owner = kind.common().owner_handle;
                let offset = if owner == model_block {
                    world_offset
                } else {
                    [0.0; 3]
                };
                let model = match &kind {
                    EntityType::Hatch(dxf) => {
                        let color = tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color, offset)
                    }
                    EntityType::Solid(solid) => {
                        let color = tess_util::aci_to_rgba(&solid.common.color);
                        Some(Self::solid_hatch_model(solid, color, offset))
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
        // Resolve color through `render_style` so the same bg adaptation
        // wires use kicks in (pure black on dark bg → white, pure white
        // on light bg → black). Without this, ACIS meshes ignore
        // `adapt_to_bg` and stay invisible against matching bg colours.
        let entries: Vec<(Handle, EntityType, [f32; 4])> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) => {
                    let color = self.render_style(e).0;
                    Some((e.common().handle, e.clone(), color))
                }
                _ => None,
            })
            .collect();

        use rayon::prelude::*;
        let facet_res = self.document.header.facet_resolution;
        let woff = self.world_offset;
        self.meshes = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color)| {
                crate::entities::solid3d::tessellate_volume(&entity, color, facet_res)
                    .map(|m| (handle, offset_mesh_lod_set(m, woff)))
            })
            .collect();

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
    fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4], world_offset: [f64; 3]) -> HatchModel {
        let [ox, oy, _oz] = world_offset;
        let boundary = vec![
            [
                (solid.first_corner.x - ox) as f32,
                (solid.first_corner.y - oy) as f32,
            ],
            [
                (solid.second_corner.x - ox) as f32,
                (solid.second_corner.y - oy) as f32,
            ],
            [
                (solid.fourth_corner.x - ox) as f32,
                (solid.fourth_corner.y - oy) as f32,
            ],
            [
                (solid.third_corner.x - ox) as f32,
                (solid.third_corner.y - oy) as f32,
            ],
        ];
        HatchModel {
            boundary: std::sync::Arc::new(boundary),
            pattern: hatch_model::HatchPattern::Solid,
            name: "SOLID".into(),
            color,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin: [0.0; 2],
            vp_scissor: None,
        }
    }

    pub fn add_hatch(&mut self, model: HatchModel) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::hatch_model::HatchPattern::Solid
        );
        let [wx, wy] = model.world_origin;
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
        if let Some(entry) = crate::scene::hatch_patterns::find(&model.name) {
            dxf.pattern = crate::scene::hatch_patterns::build_dxf_pattern(entry);
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
        self.selected = HashSet::new();
        self.preview_wires = vec![];
        self.current_layout = "Model".to_string();
        self.hatches = HashMap::new();
        self.meshes = HashMap::new();
        *self.camera.borrow_mut() = Camera::default();
        self.camera_generation += 1;
        self.bump_geometry();
    }

    // ── Preview wire ──────────────────────────────────────────────────────

    pub fn set_preview_wires(&mut self, wires: Vec<WireModel>) {
        self.preview_wires = wires;
        self.bump_geometry();
    }

    pub fn clear_preview_wire(&mut self) {
        self.preview_wires = vec![];
        self.interim_wire = None;
        self.bump_geometry();
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
        self.interim_wire = Some(w);
        self.bump_geometry();
    }

    // ── Selection ─────────────────────────────────────────────────────────

    pub fn select_entity(&mut self, handle: Handle, exclusive: bool) {
        if exclusive {
            self.selected.clear();
        }
        self.selected.insert(handle);
        self.bump_geometry();
    }

    pub fn deselect_all(&mut self) {
        self.selected.clear();
        self.bump_geometry();
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
        let pairs: std::collections::HashSet<(&'static str, String)> = self
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
            self.bump_geometry();
        }
        added
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
        self.bump_geometry();
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
        use crate::scene::object::PropValue;
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
        self.bump_geometry();
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
        self.bump_geometry();
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
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };
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
        for &h in handles {
            if let Some(entity) = self.document.get_entity_mut(h) {
                dispatch::apply_transform(entity, t);
            }
            if self.hatches.contains_key(&h) {
                let existing_color = self.hatches[&h].color;
                let new_model = if let Some(EntityType::Hatch(dxf)) = self.document.get_entity(h) {
                    Self::hatch_model_from_dxf(dxf, existing_color, hatch_offset)
                } else {
                    None
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
        self.bump_geometry();
    }

    pub fn copy_entities(&mut self, handles: &[Handle], t: &EntityTransform) -> Vec<Handle> {
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };
        let clones: Vec<EntityType> = handles
            .iter()
            .filter_map(|&h| self.document.get_entity(h).cloned())
            .collect();
        let mut new_handles = Vec::with_capacity(clones.len());
        for mut entity in clones {
            dispatch::apply_transform(&mut entity, t);
            entity.common_mut().handle = Handle::NULL;
            let h = self.document.add_entity(entity).unwrap_or(Handle::NULL);
            if !h.is_null() {
                let new_model = if let Some(EntityType::Hatch(dxf)) = self.document.get_entity(h) {
                    let color = tess_util::aci_to_rgba(&dxf.common.color);
                    Self::hatch_model_from_dxf(dxf, color, hatch_offset)
                } else {
                    None
                };
                if let Some(model) = new_model {
                    self.hatches.insert(h, model);
                }
            }
            new_handles.push(h);
        }
        self.bump_geometry();
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
            dispatch::apply_grip(entity, grip_id, apply);
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
        let hatch_offset = if self.current_layout == "Model" {
            self.world_offset
        } else {
            [0.0; 3]
        };
        match self.document.get_entity(handle) {
            Some(EntityType::Hatch(dxf)) => {
                let color = tess_util::aci_to_rgba(&dxf.common.color);
                if let Some(model) = Self::hatch_model_from_dxf(dxf, color, hatch_offset) {
                    self.hatches.insert(handle, model);
                } else {
                    self.hatches.remove(&handle);
                }
            }
            Some(EntityType::Solid(solid)) => {
                let color = tess_util::aci_to_rgba(&solid.common.color);
                self.hatches
                    .insert(handle, Self::solid_hatch_model(solid, color, hatch_offset));
            }
            _ => {}
        }
        self.bump_geometry();
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
        cam.target = Vec3::new(
            view.target.x as f32,
            view.target.y as f32,
            view.target.z as f32,
        );
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

    /// Apply camera state from an acadrust View table entry.
    /// `model_space`: if true, subtracts world_offset from target (wire-space).
    fn apply_camera_from_view_entry(
        &mut self,
        view: &acadrust::tables::View,
        model_space: bool,
    ) -> bool {
        if view.height.abs() < 1e-9 {
            return false;
        }
        let vd = glam::Vec3::new(
            view.direction.x as f32,
            view.direction.y as f32,
            view.direction.z as f32,
        )
        .normalize_or(glam::Vec3::Z);
        let pitch = vd.z.clamp(-1.0, 1.0).asin();
        let yaw = if vd.x.abs() < 1e-6 && vd.y.abs() < 1e-6 {
            0.0_f32
        } else {
            vd.x.atan2(-vd.y)
        };
        let rotation = camera::yaw_pitch_to_quat(yaw, pitch, 0.0);
        let view_right = rotation * glam::Vec3::X;
        let view_up = rotation * glam::Vec3::Y;
        let base = if model_space {
            glam::Vec3::new(
                (view.target.x - self.world_offset[0]) as f32,
                (view.target.y - self.world_offset[1]) as f32,
                (view.target.z - self.world_offset[2]) as f32,
            )
        } else {
            glam::Vec3::new(
                view.target.x as f32,
                view.target.y as f32,
                view.target.z as f32,
            )
        };
        let target = base + view_right * view.center.x as f32 + view_up * view.center.y as f32;
        let fov_y = 45.0_f32.to_radians();
        let distance = ((view.height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);
        let mut cam = self.camera.borrow_mut();
        cam.target = target;
        cam.rotation = rotation;
        cam.distance = distance;
        cam.yaw = yaw;
        cam.pitch = pitch;
        cam.fov_y = fov_y;
        cam.projection = camera::Projection::Orthographic;
        drop(cam);
        self.camera_generation += 1;
        true
    }

    /// Set the model-space camera from the VPORT table's *Active entry.
    /// Returns true if the entry was found and the camera was set.
    fn apply_active_vport_camera(&mut self) -> bool {
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

    /// Decode a VPort table entry into a `Camera`. Returns `None` if the
    /// entry has a zero view_height (i.e. is uninitialised).
    fn camera_from_vport(&self, vp: &acadrust::tables::VPort) -> Option<Camera> {
        if vp.view_height.abs() < 1e-9 {
            return None;
        }
        let vd = glam::Vec3::new(
            vp.view_direction.x as f32,
            vp.view_direction.y as f32,
            vp.view_direction.z as f32,
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
        let rotation = camera::yaw_pitch_to_quat(yaw, pitch, 0.0);
        let view_right = rotation * glam::Vec3::X;
        let view_up = rotation * glam::Vec3::Y;
        // view_target is WCS; wire-space subtracts world_offset.
        let base = glam::Vec3::new(
            (vp.view_target.x - self.world_offset[0]) as f32,
            (vp.view_target.y - self.world_offset[1]) as f32,
            (vp.view_target.z - self.world_offset[2]) as f32,
        );
        let target =
            base + view_right * vp.view_center.x as f32 + view_up * vp.view_center.y as f32;
        let fov_y = 45.0_f32.to_radians();
        let distance = ((vp.view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);
        Some(Camera {
            target,
            rotation,
            distance,
            fov_y,
            projection: camera::Projection::Orthographic,
            yaw,
            pitch,
        })
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
            x: (cam.target.x as f64) + self.world_offset[0],
            y: (cam.target.y as f64) + self.world_offset[1],
            z: (cam.target.z as f64) + self.world_offset[2],
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

    /// Restore `model_tiles` from `*OCS_Tile_*` VPort entries that a previous
    /// save left in the document. Returns true on success — the caller skips
    /// `apply_active_vport_camera` in that case because the active tile's
    /// camera has already been loaded into `self.camera`.
    fn restore_model_tiles_from_vports(&mut self) -> bool {
        let mut indexed: Vec<(usize, acadrust::tables::VPort)> = self
            .document
            .vports
            .iter()
            .filter_map(|v| {
                let idx: usize = v
                    .name
                    .strip_prefix("*OCS_Tile_")
                    .and_then(|s| s.parse().ok())?;
                Some((idx, v.clone()))
            })
            .collect();
        if indexed.len() < 2 {
            return false;
        }
        indexed.sort_by_key(|(i, _)| *i);
        let mut tiles = Vec::with_capacity(indexed.len());
        for (_, vp) in &indexed {
            let Some(cam) = self.camera_from_vport(vp) else {
                return false;
            };
            tiles.push(ModelTile {
                rect: Self::vport_to_tile_rect(vp.lower_left, vp.upper_right),
                camera: cam,
            });
        }
        // Active tile = the one whose stored camera fields match `*Active`.
        // The save path writes `*Active` to mirror the active tile's view, so
        // this matches by view_target (the most discriminating field).
        let active_idx = self
            .document
            .vports
            .iter()
            .find(|v| v.name == "*Active")
            .and_then(|act| {
                indexed.iter().position(|(_, vp)| {
                    let dx = vp.view_target.x - act.view_target.x;
                    let dy = vp.view_target.y - act.view_target.y;
                    let dz = vp.view_target.z - act.view_target.z;
                    dx * dx + dy * dy + dz * dz < 1e-12
                })
            })
            .unwrap_or(0);
        let active_cam = tiles[active_idx].camera.clone();
        *self.model_tiles.borrow_mut() = tiles;
        self.active_model_tile.set(active_idx);
        *self.camera.borrow_mut() = active_cam;
        self.camera_generation += 1;
        true
    }

    /// Persist `model_tiles` to the VPort table. The active tile is also
    /// mirrored to `*Active` (already written by `sync_camera_to_document`);
    /// every tile (including a single full-screen one) is written as a
    /// `*OCS_Tile_<i>` entry so the layout survives roundtrip.
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
        // Remove every previous tile entry — names are reassigned by index.
        let to_remove: Vec<String> = self
            .document
            .vports
            .iter()
            .filter(|v| v.name.starts_with("*OCS_Tile_"))
            .map(|v| v.name.clone())
            .collect();
        for name in to_remove {
            self.document.vports.remove(&name);
        }
        // Skip writing tile entries for the trivial single-tile config —
        // `*Active` alone is enough and keeps the file noise-free.
        let tiles = self.model_tiles.borrow().clone();
        if tiles.len() <= 1 {
            return;
        }
        for (i, tile) in tiles.iter().enumerate() {
            let (ll, ur) = Self::tile_rect_to_vport(tile.rect);
            let name = format!("*OCS_Tile_{i}");
            let mut entry = self.vport_from_camera(&name, &tile.camera, ll, ur);
            entry.handle = self.document.allocate_handle();
            self.document.vports.add_or_replace(entry);
        }
    }

    /// Set the paper-space camera from the sheet viewport's stored view.
    /// Returns true if a valid sheet viewport was found and the camera was set.
    fn apply_sheet_viewport_camera(&mut self) -> bool {
        // Prefer our named View entry — survives DWG save without being overridden.
        let view_name = format!("OpenCADStudio_Camera_{}", self.current_layout);
        let saved_view = self
            .document
            .views
            .iter()
            .find(|v| v.name == view_name)
            .cloned();
        if let Some(view) = saved_view {
            return self.apply_camera_from_view_entry(&view, false);
        }

        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return false;
        }

        let sheet_vp = self
            .document
            .entities()
            .filter_map(|e| {
                if let EntityType::Viewport(vp) = e {
                    Some(vp)
                } else {
                    None
                }
            })
            .find(|vp| vp.common.owner_handle == layout_block && !Self::is_content_viewport(vp));

        let vp = match sheet_vp {
            Some(v) => v,
            None => return false,
        };

        if vp.view_height.abs() < 1e-9 {
            return false;
        }

        let vd = glam::Vec3::new(
            vp.view_direction.x as f32,
            vp.view_direction.y as f32,
            vp.view_direction.z as f32,
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
        let rotation = camera::yaw_pitch_to_quat(yaw, pitch, 0.0);
        let view_right = rotation * glam::Vec3::X;
        let view_up = rotation * glam::Vec3::Y;

        // Paper-space entities have no world_offset applied, so target is raw.
        let base = glam::Vec3::new(
            vp.view_target.x as f32,
            vp.view_target.y as f32,
            vp.view_target.z as f32,
        );
        let target =
            base + view_right * vp.view_center.x as f32 + view_up * vp.view_center.y as f32;

        let fov_y = 45.0_f32.to_radians();
        let distance = ((vp.view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);

        let mut cam = self.camera.borrow_mut();
        cam.target = target;
        cam.rotation = rotation;
        cam.distance = distance;
        cam.yaw = yaw;
        cam.pitch = pitch;
        cam.fov_y = fov_y;
        cam.projection = camera::Projection::Orthographic;
        drop(cam);

        self.camera_generation += 1;
        true
    }

    /// Write the current camera back into the document (VPort or sheet viewport)
    /// so it is saved with the file. Returns true if the document was modified.
    pub fn sync_camera_to_document(&mut self) -> bool {
        let cam = self.camera.borrow().clone();
        let view_dir = cam.rotation * glam::Vec3::Z;
        let view_height = cam.ortho_size() * 2.0;
        let vd3 = acadrust::types::Vector3 {
            x: view_dir.x as f64,
            y: view_dir.y as f64,
            z: view_dir.z as f64,
        };

        if self.current_layout == "Model" {
            let target_wcs = acadrust::types::Vector3 {
                x: (cam.target.x as f64) + self.world_offset[0],
                y: (cam.target.y as f64) + self.world_offset[1],
                z: (cam.target.z as f64) + self.world_offset[2],
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
            }

            // Persist the tiled layout (multiple `*OCS_Tile_<i>` entries) so
            // the panel split survives save→reopen. Single-tile configs skip
            // this — `*Active` alone is enough.
            self.save_model_tiles_to_vports();

            // Also write to View table — survives DWG save without override.
            self.write_camera_view_entry("OpenCADStudio_Camera_Model", target_wcs, vd3, view_height);
            true
        } else {
            let target_wcs = acadrust::types::Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: cam.target.z as f64,
            };
            let view_name = format!("OpenCADStudio_Camera_{}", self.current_layout);

            // Write back to the sheet viewport entity.
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
                        vp.common.owner_handle == layout_block && !Self::is_content_viewport(vp)
                    })
                    .map(|vp| vp.common.handle);

                if let Some(handle) = sheet_handle {
                    if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(handle) {
                        vp.view_target = target_wcs;
                        vp.view_center = acadrust::types::Vector3::ZERO;
                        vp.view_direction = vd3;
                        vp.view_height = view_height as f64;
                    }
                }
            }

            // Also write to View table — survives DWG save without override.
            self.write_camera_view_entry(&view_name, target_wcs, vd3, view_height);
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
            self.apply_sheet_viewport_camera()
        };
        if !restored {
            self.fit_all();
        }
    }

    pub fn fit_all(&mut self) {
        let wires = self.entity_wires();
        if wires.is_empty() {
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
                if x.abs() > lim || y.abs() > lim {
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
        if cents.is_empty() {
            return;
        }

        // IQR-based reject only kicks in with enough samples for the quartiles
        // to be meaningful. Below that, the absolute `lim` filter is the only
        // gate (legacy behavior).
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
            (-lim, lim, -lim, lim)
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
                if x.abs() > lim || y.abs() > lim {
                    continue;
                }
                min = min.min(glam::Vec3::new(x, y, z));
                max = max.max(glam::Vec3::new(x, y, z));
            }
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
        tiles[active] = ModelTile {
            rect: a,
            camera: cam_now.clone(),
        };
        tiles.insert(
            active + 1,
            ModelTile {
                rect: b,
                camera: cam_now,
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
        let tiles: Vec<ModelTile> = rects
            .into_iter()
            .map(|rect| ModelTile {
                rect,
                camera: cam_now.clone(),
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
                        render_mode: model_mode,
                        active: i == active,
                    }
                })
                .collect();
        }
        let layout_block = self.current_layout_block_handle();
        let mut out: Vec<ViewportInstance> = Vec::new();
        for e in self.document.entities() {
            let EntityType::Viewport(vp) = e else {
                continue;
            };
            if !Self::is_content_viewport(vp)
                || vp.common.owner_handle != layout_block
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
            });
        }
        out
    }

    /// Convert a paper-space Viewport entity's position/size into a pixel
    /// `Rectangle` relative to the top-left of the paper canvas.
    ///
    /// Uses the same camera-based ortho transform as `PaperCanvas::draw()` so
    /// that the overlay lands exactly over the drawn viewport border regardless
    /// of zoom or pan level.
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
        let tx = cam.target.x;
        let ty = cam.target.y;
        drop(cam);

        // Mirror the to_px closure in PaperCanvas::draw().
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

    // ── ViewportPane helpers ──────────────────────────────────────────────

    /// Paper-space entity wires only (title blocks, frames, borders).
    /// Does NOT include viewport content projection — that is handled by
    /// individual ViewportPane::Paper widgets layered on top.
    /// All wires needed to render the paper-space canvas (2D widget path).
    /// Includes paper entities, paper boundary, inactive viewport projections
    /// (excluding the active MSPACE viewport), plus interim/preview wires.
    pub fn paper_canvas_wires(&self) -> Arc<Vec<WireModel>> {
        {
            let cache = self.paper_canvas_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        // The unified GPU shader draws every content viewport (active and
        // inactive) directly through its own camera + scissor, so the
        // paper canvas no longer needs to CPU-project model content onto
        // the sheet. `paper_sheet_wires()` keeps the title-block /
        // annotation / viewport-border 2-D pass that GPU does not handle.
        let mut wires = self.paper_sheet_wires();
        if let Some(iw) = &self.interim_wire {
            wires.push(iw.clone());
        }
        wires.extend(self.preview_wires.iter().cloned());
        let arc = Arc::new(wires);
        *self.paper_canvas_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Hatch fills for the paper-space 2-D canvas. The GPU-rendered
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
                if let hatch_model::HatchPattern::Pattern(_) = &m.pattern {
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

    /// Wipeout fills for the paper-space 2-D canvas. Same rationale as
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
            let boundary = Self::wipeout_boundary_2d(wo, [0.0; 3]);
            if boundary.len() < 3 {
                continue;
            }
            let mut fill_color = bg_color;
            if self.selected.contains(&wo.common.handle) {
                fill_color = [0.15, 0.55, 1.00, 0.35];
            }
            models.push(HatchModel {
                boundary: Arc::new(boundary),
                pattern: hatch_model::HatchPattern::Solid,
                name: "WIPEOUT_FILL".into(),
                color: fill_color,
                angle_offset: 0.0,
                scale: 1.0,
                world_origin: [0.0; 2],
                vp_scissor: None,
            });
        }
        Arc::new(models)
    }

    pub(super) fn paper_sheet_wires(&self) -> Vec<WireModel> {
        (*self.paper_sheet_wires_arc()).clone()
    }

    /// Build a Camera oriented and scaled to match a paper-space Viewport entity.
    /// Used by `ViewportPane::Paper` to render model-space content through the
    /// viewport's own view direction and scale.
    fn camera_for_viewport(&self, vp_handle: Handle) -> Option<camera::Camera> {
        let vp = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp,
            _ => return None,
        };

        let vd = glam::Vec3::new(
            vp.view_direction.x as f32,
            vp.view_direction.y as f32,
            vp.view_direction.z as f32,
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

        let rotation = camera::yaw_pitch_to_quat(yaw, pitch, 0.0);

        // view_target is in raw model/WCS coords; the GPU renderer works in
        // wire-space (model - world_offset), so subtract world_offset here.
        // view_center is a 2-D DCS offset of the display centre from view_target.
        //
        // UTM / coordinate-shifted drawings often arrive with
        // `view_target = (0, 0, 0)` and a stale `view_center` from before
        // the file was geo-referenced; the saved view points at empty
        // WCS while the actual model sits ~`world_offset` away. The CPU
        // projection path in `viewport_content_wires` already auto-fits
        // to the content cluster in that case — apply the same fallback
        // here so the GPU-rendered viewport shows the model instead of
        // an empty rectangle.
        let mut effective_target_wcs = (
            vp.view_target.x + vp.view_center.x,
            vp.view_target.y + vp.view_center.y,
        );
        let mut effective_view_height = vp.view_height.abs().max(1e-9);
        let aspect_d = (vp.width / vp.height.max(1.0)).max(1e-9);
        let cluster_half = self.local_extent_max.max(1.0) as f64;
        let saved_half_h = effective_view_height * 0.5;
        let saved_half_w = saved_half_h * aspect_d;
        let cluster_min_x = self.world_offset[0] - cluster_half;
        let cluster_max_x = self.world_offset[0] + cluster_half;
        let cluster_min_y = self.world_offset[1] - cluster_half;
        let cluster_max_y = self.world_offset[1] + cluster_half;
        let saved_overlaps = effective_target_wcs.0 + saved_half_w >= cluster_min_x
            && effective_target_wcs.0 - saved_half_w <= cluster_max_x
            && effective_target_wcs.1 + saved_half_h >= cluster_min_y
            && effective_target_wcs.1 - saved_half_h <= cluster_max_y;
        if !saved_overlaps {
            let margin = 1.05_f64;
            effective_view_height = cluster_half * 2.0 * margin;
            effective_target_wcs = (self.world_offset[0], self.world_offset[1]);
        }

        let base_target = glam::Vec3::new(
            (effective_target_wcs.0 - self.world_offset[0]) as f32,
            (effective_target_wcs.1 - self.world_offset[1]) as f32,
            (vp.view_target.z - self.world_offset[2]) as f32,
        );
        // `effective_target_wcs` already folded `view_center` in above
        // (CPU path does the same via `display_center_*`), so no extra
        // `view_right * view_center` shift here — that would double-count.
        let target = base_target;

        let fov_y = 45.0_f32.to_radians();
        let view_height = if effective_view_height > 1e-9 {
            effective_view_height as f32
        } else {
            vp.height as f32
        };
        // ortho_size = distance * tan(fov_y/2)  =>  distance = view_height/2 / tan(fov_y/2)
        let distance = ((view_height / 2.0) / (fov_y * 0.5).tan()).max(0.001);

        Some(camera::Camera {
            target,
            rotation,
            distance,
            fov_y,
            projection: camera::Projection::Orthographic,
            yaw,
            pitch,
        })
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
        use std::collections::HashSet as HSet;

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
            _ => (HSet::new(), 1.0_f32, 1.0_f32),
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
        let half_w = vp_ortho_h * vp_aspect.max(0.01);
        let margin = 1.25_f32;
        let view_aabb = Some([
            cam.target.x - half_w * margin,
            cam.target.y - vp_ortho_h * margin,
            cam.target.x + half_w * margin,
            cam.target.y + vp_ortho_h * margin,
        ]);
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
        let key = (self.geometry_epoch, height_key);
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
    world_offset: [f64; 3],
    bg_color: [f32; 4],
    anno_scale: f32,
    e: &EntityType,
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    text_color: [f32; 4],
) -> Vec<WireModel> {
    let mut wires = tessellate_entity(
        document, selected, active_viewport, world_offset, bg_color,
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
    world_offset: [f64; 3],
    bg_color: [f32; 4],
    anno_scale: f32,
    e: &EntityType,
    block_cache: Option<&block_cache::BlockCache>,
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
                let ab = entity_aabb(e, world_offset);
                if ab != WireModel::UNBOUNDED_AABB {
                    if let Some(view) = view_aabb {
                        if block_cache::aabb_disjoint_xy(ab, view) {
                            return vec![];
                        }
                    }
                    if let Some(wpp) = world_per_pixel {
                        let w_px = (ab[2] - ab[0]).abs();
                        let h_px = (ab[3] - ab[1]).abs();
                        // Keep in sync with `block_cache::MIN_PIXEL_SIZE`.
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
                                render::render_style_for(document, e);
                            let entity_color = render::adapt_to_bg(entity_color, bg_color);
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
                                let oz = world_offset[2];
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
        let mut wires = tessellate::tessellate(
            document,
            h,
            e,
            sel,
            color,
            pattern_length,
            pattern,
            1.5,
            world_offset,
            1.0,
        );
        let ab = entity_aabb(e, world_offset);
        for w in &mut wires {
            w.aabb = ab;
        }
        return wires;
    }

    let (entity_color, pattern_length, pattern, line_weight_px, aci) =
        render::render_style_for(document, e);
    let entity_color = render::adapt_to_bg(entity_color, bg_color);
    let lt_scale = document.header.linetype_scale as f32 * e.common().linetype_scale as f32;
    let lt_name = render::linetype_name_for(document, e);
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
                            document, selected, active_viewport, world_offset, bg_color,
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
                        let aabb = entity_aabb(e, world_offset);
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
        let aabb = entity_aabb(e, world_offset);
        use crate::entities::dimension::DimensionTess;
        let mut wires = dim.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            world_offset,
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
        let aabb = entity_aabb(e, world_offset);
        use crate::entities::multileader::MultiLeaderTess;
        let mut wires = ml.tessellate(
            document,
            h,
            sel,
            entity_color,
            line_weight_px,
            world_offset,
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
                            document, selected, active_viewport, world_offset, bg_color,
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
                        let aabb = entity_aabb(e, world_offset);
                        for w in &mut wires {
                            w.aabb = aabb;
                        }
                        return wires;
                    }
                }
            }
        }
    }

    if let EntityType::Insert(ins) = e {
        // Resolve the INSERT's own style so ByBlock sub-entities can inherit it.
        let (ins_color, ins_pat_len, ins_pat, ins_lw_px, _) = render::render_style_for(document, e);
        let ins_color = render::adapt_to_bg(ins_color, bg_color);
        let [ox, oy, oz] = world_offset;
        let ip = glam::Vec3::new(
            (ins.insert_point.x - ox) as f32,
            (ins.insert_point.y - oy) as f32,
            (ins.insert_point.z - oz) as f32,
        );
        let marker = WireModel {
            name: h.value().to_string(),
            points: vec![],
            color: entity_color,
            selected: sel,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![(ip, wire_model::SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            vp_scissor: None,
            fill_tris: vec![],
        };

        if let Some(cache) = block_cache {
            // Xrefs render with the same hue but faded toward `bg_color` so
            // the user can recognise external-reference geometry at a glance.
            let is_xref = document
                .block_records
                .get(&ins.block_name)
                .map(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                .unwrap_or(false);
            if let Some(mut wires) = block_cache::expand_insert(
                cache,
                ins,
                h,
                ins_color,
                ins_pat_len,
                ins_pat,
                ins_lw_px,
                sel,
                world_offset,
                pslt_factor,
                view_aabb,
                world_per_pixel,
                is_xref,
                bg_color,
            ) {
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
                    world_offset,
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
            .map(crate::modules::home::modify::explode::normalize_insert_entity)
            .flat_map(|sub| {
                let (sub_color, sub_pattern_length, sub_pattern, sub_line_weight_px, sub_aci) =
                    render::render_style_for_block_sub(
                        document,
                        &sub,
                        ins_color,
                        ins_pat_len,
                        ins_pat,
                        ins_lw_px,
                    );
                let sub_color = render::adapt_to_bg(sub_color, bg_color);
                let sub_color = if is_xref && !sel {
                    block_cache::fade_toward_bg(sub_color, bg_color)
                } else {
                    sub_color
                };
                let sub_aabb = entity_aabb(&sub, world_offset);
                let sub_pattern_length = sub_pattern_length * pslt_factor;
                let sub_pattern = sub_pattern.map(|v| v * pslt_factor);
                let mut wires = tessellate::tessellate(
                    document,
                    h,
                    &sub,
                    sel,
                    sub_color,
                    sub_pattern_length,
                    sub_pattern,
                    sub_line_weight_px,
                    world_offset,
                    anno_scale,
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
            world_offset,
        );
        wires.push(marker);
        return wires;
    }

    let aabb = entity_aabb(e, world_offset);

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
                let pts = crate::entities::text_support::text_baseline_points(e, anno_scale, world_offset, n_lines);
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
                }];
            }
            if h_px < 5.0 && aabb != WireModel::UNBOUNDED_AABB {
                let fill_tris = crate::entities::text_support::text_greek_obb_tris(e, anno_scale, world_offset, n_lines);
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
                }];
            }
        }
    }

    let mut bases = tessellate::tessellate(
        document,
        h,
        e,
        sel,
        entity_color,
        pattern_length,
        pattern,
        line_weight_px,
        world_offset,
        anno_scale,
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
            let mut wires = complex_lt::apply_along(
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
        color: stored_color,
        selected,
        aci,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: vec![[cx, cy, cz]],
        aabb,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
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
    }
}

/// Tessellate each visible AttributeEntity attached to an Insert and append
/// the resulting wires. AttributeEntity positions are already in WCS — the
/// INSERT only stamps the geometry once, attribute text sits at the world
/// position recorded on each ATTRIB. See #20.
#[allow(clippy::too_many_arguments)]
pub(crate) fn entity_aabb(e: &acadrust::EntityType, world_offset: [f64; 3]) -> [f32; 4] {
    let bbox = e.as_entity().bounding_box();
    let [ox, oy, _] = world_offset;
    let min_x = (bbox.min.x - ox) as f32;
    let min_y = (bbox.min.y - oy) as f32;
    let max_x = (bbox.max.x - ox) as f32;
    let max_y = (bbox.max.y - oy) as f32;
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

