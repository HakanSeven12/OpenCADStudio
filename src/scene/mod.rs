pub mod acad_to_truck;
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
mod render;
mod selection;
pub mod solid3d_tess;
pub mod tessellate;
pub mod transform;
pub mod truck_tess;
pub mod viewport_pane;
pub mod wire_model;

use camera::Camera;
pub use camera::Projection;
pub use hatch_model::HatchModel;
pub use image_model::ImageModel;
pub use mesh_model::MeshModel;
pub use object::{GripApply, GripDef};
pub use pipeline::uniforms::Uniforms;
pub use pipeline::viewcube::{
    hit_test, CubeRegion, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
pub use selection::SelectionState;
pub use wire_model::WireModel;

use crate::command::EntityTransform;
use acadrust::entities::{BoundaryEdge, BoundaryPath, Hatch as DxfHatch, PolylineEdge, Solid as DxfSolid};
use truck_modeling::{
    base::{BoundedCurve, ParametricCurve},
    BSplineCurve as TruckBSpline, KnotVec, Point3,
};
use acadrust::entities::{Block, BlockEnd, Insert as DxfInsert};
use acadrust::objects::ObjectType;
use acadrust::types::Vector2;
use acadrust::{CadDocument, EntityType, Handle, TableEntry};
use glam;

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

pub struct Scene {
    pub camera: Rc<RefCell<Camera>>,
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
    /// Keyed by `geometry_epoch`; invalidated automatically when the epoch changes.
    /// Uses `Arc` so `build_primitive()` avoids a full Vec clone during navigation.
    wire_cache: RefCell<Option<(u64, Arc<Vec<WireModel>>)>>,
    /// Index built from every SortEntitiesTable in the document.
    /// Maps block_handle → (entity_handle.value() → sort_handle.value()).
    /// Replaces the O(objects) linear scan inside `wires_for_block()` with an O(1) lookup.
    sort_cache: RefCell<Option<(u64, HashMap<Handle, HashMap<u64, u64>>)>>,
    /// Cached hatch fill models, keyed by geometry_epoch.
    hatch_cache: RefCell<Option<(u64, Arc<Vec<HatchModel>>)>>,
    /// Cached wipeout fill models, keyed by geometry_epoch.
    wipeout_cache: RefCell<Option<(u64, Arc<Vec<HatchModel>>)>>,
    /// Cached image models, keyed by geometry_epoch.
    image_cache: RefCell<Option<(u64, Arc<Vec<ImageModel>>)>>,
    /// Cached mesh models, keyed by geometry_epoch.
    mesh_cache: RefCell<Option<(u64, Arc<Vec<MeshModel>>)>>,
    /// Per-viewport wire cache for paper-space rendering.
    /// Maps vp_handle → (geometry_epoch, Arc<Vec<WireModel>>).
    viewport_wire_cache: RefCell<HashMap<Handle, (u64, Arc<Vec<WireModel>>)>>,
    /// Cached tessellation of paper-space layout block entities (title block, annotations, etc.).
    /// Separate from `wire_cache` so paper_canvas_wires() doesn't re-tessellate on every frame.
    paper_sheet_cache: RefCell<Option<(u64, Arc<Vec<WireModel>>)>>,
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
    pub meshes: HashMap<Handle, MeshModel>,
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
}

impl Scene {
    pub fn new() -> Self {
        Self {
            camera: Rc::new(RefCell::new(Camera::default())),
            selection: Rc::new(RefCell::new(SelectionState::default())),
            document: CadDocument::new(),
            selected: HashSet::new(),
            preview_wires: vec![],
            interim_wire: None,
            camera_generation: 0,
            geometry_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            wire_cache: RefCell::new(None),
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
        }
    }

    pub fn bump_geometry(&mut self) {
        self.geometry_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
    }

    /// Compute scene centroid from DXF header extents and store as world_offset.
    /// Must be called after `self.document` is set so all geometry is offset-corrected.
    pub fn compute_and_set_world_offset(&mut self) {
        let h = &self.document.header;
        let min = h.model_space_extents_min;
        let max = h.model_space_extents_max;
        // Sentinel: DXF files with no geometry have EXTMIN = 1e20, EXTMAX = -1e20.
        // Fall back to no offset so the scene stays at the origin.
        let valid = min.x < max.x && min.y < max.y;
        if valid {
            self.world_offset = [
                (min.x + max.x) * 0.5,
                (min.y + max.y) * 0.5,
                (min.z + max.z) * 0.5,
            ];
            // 10× the EXTMIN→EXTMAX half-diagonal gives fit_all() a threshold to
            // reject origin-stuck/corrupted entities far outside the drawing area.
            let hw = ((max.x - min.x) * 0.5) as f32;
            let hh = ((max.y - min.y) * 0.5) as f32;
            let hz = ((max.z - min.z) * 0.5).max(1.0) as f32;
            self.local_extent_max = hw.max(hh).max(hz) * 10.0;
        } else {
            self.world_offset = [0.0; 3];
            self.local_extent_max = 1e9;
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
        if vp.id == 1 { return false; }
        if vp.id > 1  { return true;  }
        // id=0 or id<0 — distinguish by geometry
        let scale = if vp.view_height.abs() > 1e-9 {
            vp.height / vp.view_height
        } else {
            1.0
        };
        let at_origin = vp.center.x.abs() < 0.5 && vp.center.y.abs() < 0.5;
        let scale_one = (scale - 1.0).abs() < 0.02;
        !(at_origin && scale_one)
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
                if l.name == self.current_layout { Some(l) } else { None }
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
                            if l.name != "Model" { Some((l.tab_order, l.name.as_str())) }
                            else { None }
                        } else {
                            None
                        }
                    })
                    .collect();
                paper_layouts.sort_by_key(|(o, n)| (*o, *n));

                if let Some(pos) = paper_layouts.iter().position(|(_, n)| *n == self.current_layout) {
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

        self.document.objects.values().find_map(|obj| {
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
                    let scale = if vp.custom_scale.abs() > 1e-9 {
                        vp.custom_scale
                    } else if vp.view_height.abs() > 1e-9 {
                        vp.height / vp.view_height
                    } else {
                        1.0
                    };
                    return Some(scale);
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
        self.document.entities().filter(|e| {
            if let EntityType::Viewport(vp) = e {
                Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block
            } else {
                false
            }
        }).count()
    }

    /// Sorted list of layout names: "Model" first, then paper layouts by tab order.
    pub fn layout_names(&self) -> Vec<String> {
        let mut names = vec!["Model".to_string()];
        // Deduplicate by name: prefer the entry with a non-null block_record (the
        // real layout from the file) over the default placeholder created by
        // CadDocument::new().
        let mut by_name: std::collections::HashMap<String, (i16, Handle)> =
            Default::default();
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

    /// Cached tessellation of the current layout block's paper-space entities.
    /// Shared by both `entity_wires_arc()` and `paper_canvas_wires()` so a single
    /// cache miss triggers only one tessellation pass, not two.
    fn paper_sheet_wires_arc(&self) -> Arc<Vec<WireModel>> {
        {
            let cache = self.paper_sheet_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        let arc = Arc::new(self.wires_for_block(layout_block));
        *self.paper_sheet_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Build WireModels from all document entities for the current layout.
    /// Returns a shared `Arc` so `build_primitive()` can skip the clone during
    /// navigation frames where no preview wires are active.
    pub(super) fn entity_wires_arc(&self) -> Arc<Vec<WireModel>> {
        {
            let cache = self.wire_cache.borrow();
            if let Some((cached_epoch, ref arc)) = *cache {
                if cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        // Reuse the paper_sheet_cache to avoid a duplicate tessellation pass
        // when both entity_wires_arc() and paper_canvas_wires() are called in the same frame.
        let mut wires = (*self.paper_sheet_wires_arc()).clone();
        if self.current_layout != "Model" {
            wires.extend(self.viewport_content_wires(layout_block, None, None));
        }
        let arc = Arc::new(wires);
        *self.wire_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
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

    pub(super) fn meshes_arc(&self) -> Arc<Vec<MeshModel>> {
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
        use acadrust::objects::ObjectType;

        // ── Ensure sort-order index is current ────────────────────────────
        // Replaces the old O(objects) find_map with one rebuild per epoch,
        // after which every wires_for_block call is an O(1) HashMap lookup.
        {
            let needs_rebuild = self.sort_cache.borrow()
                .as_ref()
                .map(|(e, _)| *e != self.geometry_epoch)
                .unwrap_or(true);

            if needs_rebuild {
                let mut idx: HashMap<Handle, HashMap<u64, u64>> = HashMap::new();
                for obj in self.document.objects.values() {
                    if let ObjectType::SortEntitiesTable(t) = obj {
                        if !t.is_empty() {
                            let map = t.entries()
                                .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                                .collect();
                            idx.insert(t.block_owner_handle, map);
                        }
                    }
                }
                *self.sort_cache.borrow_mut() = Some((self.geometry_epoch, idx));
            }
        }

        // Collect visible entities sequentially (filter needs &self).
        let visible: Vec<&EntityType> = self.document
            .entities()
            .filter(|e| {
                let c = e.common();
                if c.invisible {
                    return false;
                }
                if self
                    .document
                    .layers
                    .get(&c.layer)
                    .map(|l| l.flags.off || l.flags.frozen)
                    .unwrap_or(false)
                {
                    return false;
                }
                self.belongs_to_visible_block(e.common().handle, c.owner_handle, block_handle)
            })
            .collect();

        // Tessellate in parallel across all available CPU cores.
        use rayon::prelude::*;
        let doc = &self.document;
        let sel = &self.selected;
        let avp = self.active_viewport;
        // Paper-space entities live in sheet coordinates (mm), not model-world
        // coordinates, so world_offset must not be subtracted from them.
        let woff = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };
        let bg = if self.current_layout == "Model" { self.bg_color } else { self.paper_bg_color };
        let mut wires: Vec<WireModel> = visible
            .into_par_iter()
            .flat_map(|e| tessellate_entity(doc, sel, avp, woff, bg, e))
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
        if let Some(br) = self.document.block_records.iter().find(|br| br.handle == block_handle) {
            if !br.entity_handles.is_empty() {
                return br.entity_handles.contains(&entity_handle);
            }
        }

        // entity_handles not populated: fall back to "not listed in any other block".
        !self
            .document
            .block_records
            .iter()
            .filter(|br| br.handle != block_handle)
            .any(|br| br.entity_handles.contains(&entity_handle))
    }

    /// Full tessellation pipeline for one entity.
    fn tessellate_one(&self, e: &EntityType) -> Vec<WireModel> {
        let bg = if self.current_layout == "Model" { self.bg_color } else { self.paper_bg_color };
        tessellate_entity(&self.document, &self.selected, self.active_viewport, self.world_offset, bg, e)
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

    /// Compute the axis-aligned bounding box of all model-space entities by
    /// collecting their `key_vertices`.  Returns `None` when there are no
    /// vertices (empty drawing).
    pub fn model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let model_block = self.model_space_block_handle();
        if model_block.is_null() {
            return None;
        }
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;
        for entity in self.document.entities() {
            let c = entity.common();
            if c.owner_handle != model_block || c.invisible {
                continue;
            }
            for wire in self.tessellate_one(entity) {
                for &[x, y, z] in &wire.key_vertices {
                    if x.is_finite() && y.is_finite() && z.is_finite() {
                        min = min.min(glam::Vec3::new(x, y, z));
                        max = max.max(glam::Vec3::new(x, y, z));
                        any = true;
                    }
                }
            }
        }
        if any { Some((min, max)) } else { None }
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
                if let EntityType::Viewport(vp) = e { Some(vp) } else { None }
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
            let view_up    = cam_frame.rotation * glam::Vec3::Y;

            // ── Scale & viewport parameters ───────────────────────────────
            // view_height is always correct; custom_scale is unreliable in DWG files
            // (acadrust DWG reader never populates it, so it stays at 1.0).
            let scale = if vp.view_height.abs() > 1e-9 {
                (vp.height / vp.view_height) as f32
            } else if vp.custom_scale.abs() > 1e-9 {
                vp.custom_scale as f32
            } else {
                1.0
            };

            // view_target is in raw model coords; wire points have world_offset
            // subtracted, so bring target into the same wire-space.
            let target = glam::Vec3::new(
                (vp.view_target.x - self.world_offset[0]) as f32,
                (vp.view_target.y - self.world_offset[1]) as f32,
                (vp.view_target.z - self.world_offset[2]) as f32,
            );
            let pcx = vp.center.x as f32;
            let pcy = vp.center.y as f32;
            let pcz = vp.center.z as f32;
            let hw = (vp.width / 2.0) as f32;
            let hh = (vp.height / 2.0) as f32;

            // ── Use cached tessellation (model_wires_for_viewport_arc) ────
            // This eliminates the per-frame tessellate_one() loop that was here
            // previously; tessellation is now O(1) on navigation frames.
            let model_wires = self.model_wires_for_viewport_arc(vp_handle);

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

            for wire in model_wires.iter() {
                // Project 3-D model points onto view plane → paper space.
                let projected_pts: Vec<[f32; 3]> = wire.points.iter().map(|&[mx, my, mz]| {
                    if mx.is_nan() || my.is_nan() || mz.is_nan() {
                        return [f32::NAN; 3];
                    }
                    let mp = glam::Vec3::new(mx, my, mz) - target;
                    // view_center is the 2-D DCS offset of the display centre from
                    // view_target; subtract it so model origin maps to viewport centre.
                    let u = mp.dot(view_right) - vp.view_center.x as f32;
                    let v = mp.dot(view_up)    - vp.view_center.y as f32;
                    if use_perspective {
                        let d_vd = mp.dot(cam_frame.rotation * glam::Vec3::Z);
                        let fwd = camera_dist - d_vd;
                        if fwd <= 0.001 {
                            return [f32::NAN; 3];
                        }
                        let factor = camera_dist / fwd;
                        [pcx + u * factor * scale, pcy + v * factor * scale, pcz]
                    } else {
                        [pcx + u * scale, pcy + v * scale, pcz]
                    }
                }).collect();

                // Fast AABB pre-reject.
                let any_near = projected_pts.iter().any(|&[x, y, _]| {
                    x.is_finite() && y.is_finite()
                        && x >= vp_x0 - 1.0 && x <= vp_x1 + 1.0
                        && y >= vp_y0 - 1.0 && y <= vp_y1 + 1.0
                });
                let (min_x, max_x, min_y, max_y) = projected_pts.iter()
                    .filter(|p| p[0].is_finite())
                    .fold(
                        (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY),
                        |(mnx, mxx, mny, mxy), &[x, y, _]| {
                            (mnx.min(x), mxx.max(x), mny.min(y), mxy.max(y))
                        },
                    );
                let aabb_hits = max_x >= vp_x0 && min_x <= vp_x1
                             && max_y >= vp_y0 && min_y <= vp_y1;
                if !any_near && !aabb_hits {
                    continue;
                }

                let clipped = clip_polyline_to_rect(
                    &projected_pts, vp_x0, vp_y0, vp_x1, vp_y1, pcz,
                );
                if clipped.is_empty() {
                    continue;
                }

                let adapted = render::adapt_to_bg(wire.color, self.paper_bg_color);
                let [r, g, b, a] = adapted;
                let mut out = wire.clone();
                out.points = clipped;
                out.color = [r * 0.80, g * 0.80, b * 0.80, a * 0.85];
                out.line_weight_px = wire.line_weight_px;
                projected.push(out);
            }

            // Store in cache, then extend result.
            self.paper_projected_cache.borrow_mut()
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
        let scale = if vp.custom_scale.abs() > 1e-9 {
            vp.custom_scale as f32
        } else if vp.view_height.abs() > 1e-9 {
            (vp.height / vp.view_height) as f32
        } else {
            1.0
        };
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
        // Use the viewport's own camera so that the pan axes match the 3-D view
        // orientation (important for tilted/rotated MSPACE views).
        // camera_for_viewport already encodes the correct distance/scale via view_height,
        // so no additional scale division is needed.
        let vp_cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return,
        };
        let model_delta = vp_cam.screen_delta_to_world(screen_dx, screen_dy, bounds);

        if let Some(acadrust::EntityType::Viewport(vp)) =
            self.document.get_entity_mut(vp_handle)
        {
            if vp.status.locked { return; }
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
        if let Some(acadrust::EntityType::Viewport(vp)) =
            self.document.get_entity_mut(vp_handle)
        {
            if vp.status.locked { return; }
            // Zoom in = shrink view_height → higher scale → objects appear larger.
            let factor = (1.0_f64 - 0.15 * steps as f64).clamp(0.1, 10.0);

            if let Some(cp) = cursor_paper {
                // Compute the model-space point under the cursor before zoom.
                let scale_before = if vp.custom_scale.abs() > 1e-9 {
                    vp.custom_scale as f32
                } else if vp.view_height.abs() > 1e-9 {
                    (vp.height / vp.view_height) as f32
                } else {
                    1.0
                };
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
        if let Some(acadrust::EntityType::Viewport(vp)) =
            self.document.get_entity_mut(vp_handle)
        {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = -eye.y as f64;
            vp.view_direction.z = eye.z as f64;
        }
    }

    /// Snap the active viewport's view direction to a canonical yaw/pitch.
    /// No-op when there is no active viewport or it is locked.
    pub fn snap_active_viewport_to_angles(&mut self, yaw: f32, pitch: f32) {
        let vp_handle = match self.active_viewport {
            Some(h) => h,
            None => return,
        };
        let cos_p = pitch.cos();
        let eye = glam::Vec3::new(cos_p * yaw.sin(), cos_p * yaw.cos(), pitch.sin());
        if let Some(acadrust::EntityType::Viewport(vp)) =
            self.document.get_entity_mut(vp_handle)
        {
            if vp.status.locked {
                return;
            }
            vp.view_direction.x = eye.x as f64;
            vp.view_direction.y = eye.y as f64;
            vp.view_direction.z = eye.z as f64;
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

    /// Return (yaw, pitch) of the active viewport's camera, or None if PSPACE.
    pub fn active_viewport_yaw_pitch(&self) -> Option<(f32, f32)> {
        let h = self.active_viewport?;
        let cam = self.camera_for_viewport(h)?;
        Some((cam.yaw, cam.pitch))
    }

    /// Return the handle of the user viewport whose bounding rectangle contains
    /// the given paper-space point, or `None` if no viewport matches.
    pub fn viewport_at_paper_point(&self, px: f32, py: f32) -> Option<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document
            .entities()
            .find_map(|e| {
                let EntityType::Viewport(vp) = e else { return None; };
                if !Self::is_content_viewport(vp) || vp.common.owner_handle != layout_block || !vp.status.is_on {
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
            let EntityType::Viewport(vp) = e else { return None; };
            if Self::is_content_viewport(vp) && vp.common.owner_handle == layout_block && vp.status.is_on {
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
                if l.name == name_a { order_a = Some(l.tab_order); }
                if l.name == name_b { order_b = Some(l.tab_order); }
            }
        }
        if let (Some(oa), Some(ob)) = (order_a, order_b) {
            for obj in self.document.objects.values_mut() {
                if let ObjectType::Layout(l) = obj {
                    if l.name == name_a { l.tab_order = ob; }
                    else if l.name == name_b { l.tab_order = oa; }
                }
            }
        }
    }

    // ── Entity management ─────────────────────────────────────────────────

    pub fn add_entity(&mut self, mut entity: EntityType) -> Handle {
        let hatch_offset = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };
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
        let mesh_seed = match &entity {
            EntityType::Solid3D(s3d) => {
                let color = self.render_style(&entity).0;
                solid3d_tess::tessellate_solid3d(s3d, color)
            }
            EntityType::Region(r) => {
                let color = self.render_style(&entity).0;
                solid3d_tess::tessellate_region(r, color)
            }
            EntityType::Body(b) => {
                let color = self.render_style(&entity).0;
                solid3d_tess::tessellate_body(b, color)
            }
            _ => None,
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
        let color = layer_entry.map(|l| &l.color).unwrap_or(&acadrust::types::Color::WHITE);
        let [r, g, b, _] = crate::scene::tessellate::aci_to_rgba(color);
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
        self.document.block_records.add(block_record).map_err(|e| e.to_string())?;

        let mut block = Block::new(
            name,
            acadrust::types::Vector3::ZERO,
        );
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
        let hatch_offset = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };

        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };

        let mut models: Vec<HatchModel> = self
            .hatches
            .iter()
            .filter(|(&handle, _)| {
                let Some(entity) = self.document.get_entity(handle) else {
                    return true;
                };
                let c = entity.common();
                !c.invisible
                    && !layer_hidden(&c.layer)
                    && self.belongs_to_visible_block(handle, c.owner_handle, layout_block)
            })
            .map(|(&handle, model)| {
                let mut m = if let Some(EntityType::Hatch(dxf)) = self.document.get_entity(handle) {
                    let mut m = model.clone();
                    match &mut m.pattern {
                        hatch_model::HatchPattern::Pattern(_) => {
                            m.angle_offset = dxf.pattern_angle as f32;
                            m.scale = dxf.pattern_scale as f32;
                        }
                        hatch_model::HatchPattern::Gradient { angle_deg, .. } => {
                            *angle_deg = dxf.pattern_angle.to_degrees() as f32;
                        }
                        hatch_model::HatchPattern::Solid => {}
                    }
                    m
                } else {
                    model.clone()
                };
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
        for entity in self.document.entities() {
            let (common, fills) = match entity {
                EntityType::LwPolyline(pl) => {
                    (&pl.common, wide_lwpolyline_fills(pl))
                }
                EntityType::Polyline2D(pl) => {
                    (&pl.common, wide_polyline2d_fills(pl))
                }
                _ => continue,
            };
            if fills.is_empty() { continue; }
            if common.invisible || layer_hidden(&common.layer) { continue; }
            if !self.belongs_to_visible_block(common.handle, common.owner_handle, layout_block) {
                continue;
            }
            let base_color = self.render_style(entity).0;
            let selected = self.selected.contains(&common.handle);
            let color = if selected { [0.15, 0.55, 1.00, 1.0] } else { base_color };
            for boundary in fills {
                models.push(HatchModel {
                    boundary,
                    pattern: hatch_model::HatchPattern::Solid,
                    name: "SOLID".into(),
                    color,
                    angle_offset: 0.0,
                    scale: 1.0,
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
        // Paper-space entities are already in small coordinates — don't shift them.
        let world_offset = if is_paper { [0.0; 3] } else { self.world_offset };
        let mut models = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Wipeout(wo) = entity else { continue };
            if entity.common().invisible {
                continue;
            }
            if self.document.layers
                .get(&entity.common().layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
            {
                continue;
            }
            let boundary = Self::wipeout_boundary_2d(wo, world_offset);
            if boundary.len() >= 3 {
                let mut fill_color = bg_color;
                if self.selected.contains(&wo.common.handle) {
                    fill_color = [0.15, 0.55, 1.00, 0.35];
                }
                models.push(HatchModel {
                    boundary,
                    pattern: hatch_model::HatchPattern::Solid,
                    name: "WIPEOUT_FILL".into(),
                    color: fill_color,
                    angle_offset: 0.0,
                    scale: 1.0,
                });
            }
        }
        models
    }

    /// Compute the 2D (XY) boundary polygon for a Wipeout entity.
    fn wipeout_boundary_2d(wo: &acadrust::entities::Wipeout, world_offset: [f64; 3]) -> Vec<[f32; 2]> {
        use acadrust::entities::WipeoutClipType;

        let [wox, woy, _woz] = world_offset;
        let is_polygon = wo.clipping_enabled
            && wo.clip_boundary_vertices.len() >= 3
            && matches!(wo.clip_type, WipeoutClipType::Polygonal);

        if is_polygon {
            let ox = (wo.insertion_point.x - wox) as f32;
            let oy = (wo.insertion_point.y - woy) as f32;
            wo.clip_boundary_vertices.iter().map(|v| {
                let wx = (wo.u_vector.x * v.x * wo.size.x + wo.v_vector.x * v.y * wo.size.y) as f32;
                let wy = (wo.u_vector.y * v.x * wo.size.x + wo.v_vector.y * v.y * wo.size.y) as f32;
                [ox + wx, oy + wy]
            }).collect()
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

    fn hatch_model_from_dxf(dxf: &DxfHatch, color: [f32; 4], world_offset: [f64; 3]) -> Option<HatchModel> {
        let [ox, oy, _oz] = world_offset;
        let path = dxf
            .paths
            .iter()
            .find(|p| p.flags.is_external())
            .or_else(|| dxf.paths.first())?;

        let mut boundary: Vec<[f32; 2]> = Vec::new();

        for edge in &path.edges {
            match edge {
                BoundaryEdge::Polyline(poly) => {
                    let verts = &poly.vertices;
                    let count = verts.len();
                    let seg_count = if poly.is_closed {
                        count
                    } else {
                        count.saturating_sub(1)
                    };
                    for i in 0..seg_count {
                        let v0 = &verts[i];
                        let v1 = &verts[(i + 1) % count];
                        let bulge = v0.z;
                        if bulge.abs() < 1e-9 {
                            boundary.push([(v0.x - ox) as f32, (v0.y - oy) as f32]);
                        } else {
                            let p0 = [(v0.x - ox) as f32, (v0.y - oy) as f32];
                            let p1 = [(v1.x - ox) as f32, (v1.y - oy) as f32];
                            let angle = 4.0 * (bulge as f32).atan();
                            let dx = p1[0] - p0[0];
                            let dy = p1[1] - p0[1];
                            let d = (dx * dx + dy * dy).sqrt();
                            let r = (d / 2.0) / (angle / 2.0).sin().abs();
                            let mx = (p0[0] + p1[0]) * 0.5;
                            let my = (p0[1] + p1[1]) * 0.5;
                            let len = d.max(1e-9);
                            let px = -dy / len;
                            let py = dx / len;
                            let sign = if bulge > 0.0 { 1.0_f32 } else { -1.0_f32 };
                            let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
                            let cx = mx - sign * px * (r - h);
                            let cy = my - sign * py * (r - h);
                            let a0 = (p0[1] - cy).atan2(p0[0] - cx);
                            let a1 = (p1[1] - cy).atan2(p1[0] - cx);
                            let (sa, mut ea) = if bulge > 0.0 { (a0, a1) } else { (a1, a0) };
                            if ea < sa {
                                ea += std::f32::consts::TAU;
                            }
                            let span = ea - sa;
                            let segs = ((span.abs() / std::f32::consts::TAU) * 16.0)
                                .ceil()
                                .max(4.0) as u32;
                            for j in 0..segs {
                                let t = sa + span * (j as f32 / segs as f32);
                                boundary.push([cx + r * t.cos(), cy + r * t.sin()]);
                            }
                        }
                    }
                    if poly.is_closed {
                        if let Some(&first) = boundary.first() {
                            boundary.push(first);
                        }
                    }
                }
                BoundaryEdge::Line(line) => {
                    boundary.push([(line.start.x - ox) as f32, (line.start.y - oy) as f32]);
                    boundary.push([(line.end.x - ox) as f32, (line.end.y - oy) as f32]);
                }
                BoundaryEdge::CircularArc(arc) => {
                    let cx = (arc.center.x - ox) as f32;
                    let cy = (arc.center.y - oy) as f32;
                    let r = arc.radius as f32;
                    let (sa, ea) = if arc.counter_clockwise {
                        (arc.start_angle as f32, arc.end_angle as f32)
                    } else {
                        (arc.end_angle as f32, arc.start_angle as f32)
                    };
                    let mut end = ea;
                    if end < sa {
                        end += std::f32::consts::TAU;
                    }
                    let span = end - sa;
                    let segs = ((span / std::f32::consts::TAU) * 32.0).ceil().max(4.0) as u32;
                    for i in 0..=segs {
                        let t = sa + span * (i as f32 / segs as f32);
                        boundary.push([cx + r * t.cos(), cy + r * t.sin()]);
                    }
                }
                BoundaryEdge::EllipticArc(ell) => {
                    let cx = (ell.center.x - ox) as f32;
                    let cy = (ell.center.y - oy) as f32;
                    let maj_x = ell.major_axis_endpoint.x as f32;
                    let maj_y = ell.major_axis_endpoint.y as f32;
                    let r_maj = (maj_x * maj_x + maj_y * maj_y).sqrt();
                    let r_min = r_maj * ell.minor_axis_ratio as f32;
                    let rot = maj_y.atan2(maj_x);
                    let (sa, ea) = if ell.counter_clockwise {
                        (ell.start_angle as f32, ell.end_angle as f32)
                    } else {
                        (ell.end_angle as f32, ell.start_angle as f32)
                    };
                    let mut end = ea;
                    if end < sa {
                        end += std::f32::consts::TAU;
                    }
                    let span = end - sa;
                    let segs = ((span / std::f32::consts::TAU) * 32.0).ceil().max(4.0) as u32;
                    for i in 0..=segs {
                        let t = sa + span * (i as f32 / segs as f32);
                        let lx = r_maj * t.cos();
                        let ly = r_min * t.sin();
                        boundary.push([
                            cx + lx * rot.cos() - ly * rot.sin(),
                            cy + lx * rot.sin() + ly * rot.cos(),
                        ]);
                    }
                }
                BoundaryEdge::Spline(spline) => {
                    // Evaluate the B-spline curve into smooth boundary points.
                    // Fall back to control-point polyline for degenerate inputs.
                    let degree = spline.degree as usize;
                    let cps: Vec<Point3> = spline.control_points
                        .iter()
                        .map(|p| Point3::new(p.x, p.y, 0.0))
                        .collect();
                    let knot_vec = if !spline.knots.is_empty() {
                        KnotVec::from(spline.knots.clone())
                    } else if cps.len() >= 2 {
                        KnotVec::uniform_knot(degree, cps.len() - 1)
                    } else {
                        KnotVec::from(vec![])
                    };
                    let ok = cps.len() >= 2
                        && degree >= 1
                        && knot_vec.len() == cps.len() + degree + 1;
                    if ok {
                        let bspl = TruckBSpline::new(knot_vec, cps);
                        let (t0, t1) = bspl.range_tuple();
                        let segs = 16u32;
                        for i in 0..=segs {
                            let t = t0 + (t1 - t0) * (i as f64 / segs as f64);
                            let p = bspl.subs(t);
                            boundary.push([(p.x - ox) as f32, (p.y - oy) as f32]);
                        }
                    } else {
                        for cp in &spline.control_points {
                            boundary.push([(cp.x - ox) as f32, (cp.y - oy) as f32]);
                        }
                    }
                }
            }
        }

        if boundary.is_empty() {
            return None;
        }
        boundary.truncate(64);

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
        Some(HatchModel {
            boundary,
            pattern,
            name,
            color,
            angle_offset: 0.0,
            scale: 1.0,
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

        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((h.common.handle, e.clone())),
                EntityType::Solid(s) => Some((s.common.handle, e.clone())),
                _ => None,
            })
            .collect();

        for (handle, kind) in entries {
            // Paper-space entities live in sheet coordinates — world_offset must not
            // be applied to them.  Only model-space entities need the shift.
            let owner = kind.common().owner_handle;
            let offset = if owner == model_block { self.world_offset } else { [0.0; 3] };
            let model = match &kind {
                EntityType::Hatch(dxf) => {
                    let color = tessellate::aci_to_rgba(&dxf.common.color);
                    Self::hatch_model_from_dxf(dxf, color, offset)
                }
                EntityType::Solid(solid) => {
                    let color = tessellate::aci_to_rgba(&solid.common.color);
                    Some(Self::solid_hatch_model(solid, color, offset))
                }
                _ => None,
            };
            if let Some(m) = model {
                self.hatches.insert(handle, m);
            }
        }
        self.bump_geometry();
    }

    /// Tessellate all `Solid3D` entities in the current document into
    /// GPU-ready `MeshModel`s and store them in `self.meshes`.
    ///
    /// Called after loading a document or after undo/redo so that every
    /// `Solid3D` entity is represented in the mesh cache.
    pub fn populate_meshes_from_document(&mut self) {
        self.meshes.clear();
        // Collect all ACIS-bearing entities: Solid3D, Region, Body.
        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) =>
                    Some((e.common().handle, e.clone())),
                _ => None,
            })
            .collect();
        for (handle, entity) in entries {
            let color = if let Some(e) = self.document.get_entity(handle) {
                tessellate::aci_to_rgba(&e.common().color)
            } else {
                [0.7, 0.7, 0.7, 1.0]
            };
            let model = match &entity {
                EntityType::Solid3D(s) => solid3d_tess::tessellate_solid3d(s, color),
                EntityType::Region(r)  => solid3d_tess::tessellate_region(r, color),
                EntityType::Body(b)    => solid3d_tess::tessellate_body(b, color),
                _ => None,
            };
            if let Some(m) = model {
                self.meshes.insert(handle, m);
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
    fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4], world_offset: [f64; 3]) -> HatchModel {
        let [ox, oy, _oz] = world_offset;
        let boundary = vec![
            [(solid.first_corner.x - ox) as f32,  (solid.first_corner.y - oy) as f32],
            [(solid.second_corner.x - ox) as f32, (solid.second_corner.y - oy) as f32],
            [(solid.fourth_corner.x - ox) as f32, (solid.fourth_corner.y - oy) as f32],
            [(solid.third_corner.x - ox) as f32,  (solid.third_corner.y - oy) as f32],
        ];
        HatchModel {
            boundary,
            pattern: hatch_model::HatchPattern::Solid,
            name: "SOLID".into(),
            color,
            angle_offset: 0.0,
            scale: 1.0,
        }
    }

    pub fn add_hatch(&mut self, model: HatchModel) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::hatch_model::HatchPattern::Solid
        );
        let verts: Vec<Vector2> = model
            .boundary
            .iter()
            .map(|&[x, y]| Vector2::new(x as f64, y as f64))
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
                    if g.entities.is_empty() { Some(g.handle) } else { None }
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
                ObjectType::Group(g) if handles.iter().any(|h| g.contains(*h)) => {
                    Some(g.handle)
                }
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
                ObjectType::Group(g)
                    if g.selectable && handles.iter().any(|h| g.contains(*h)) =>
                {
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
        let hatch_offset = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };
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
        self.bump_geometry();
    }

    pub fn copy_entities(&mut self, handles: &[Handle], t: &EntityTransform) -> Vec<Handle> {
        let hatch_offset = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };
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
                    let color = tessellate::aci_to_rgba(&dxf.common.color);
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
        if let Some(entity) = self.document.get_entity_mut(handle) {
            dispatch::apply_grip(entity, grip_id, apply);
        }
        // Rebuild GPU hatch/solid model when a boundary vertex or corner moves.
        let hatch_offset = if self.current_layout == "Model" { self.world_offset } else { [0.0; 3] };
        match self.document.get_entity(handle) {
            Some(EntityType::Hatch(dxf)) => {
                let color = tessellate::aci_to_rgba(&dxf.common.color);
                if let Some(model) = Self::hatch_model_from_dxf(dxf, color, hatch_offset) {
                    self.hatches.insert(handle, model);
                } else {
                    self.hatches.remove(&handle);
                }
            }
            Some(EntityType::Solid(solid)) => {
                let color = tessellate::aci_to_rgba(&solid.common.color);
                self.hatches.insert(handle, Self::solid_hatch_model(solid, color, hatch_offset));
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
        cam.target = Vec3::new(view.target.x as f32, view.target.y as f32, view.target.z as f32);
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
    fn apply_camera_from_view_entry(&mut self, view: &acadrust::tables::View, model_space: bool) -> bool {
        if view.height.abs() < 1e-9 {
            return false;
        }
        let vd = glam::Vec3::new(
            view.direction.x as f32,
            view.direction.y as f32,
            view.direction.z as f32,
        ).normalize_or(glam::Vec3::Z);
        let pitch = vd.z.clamp(-1.0, 1.0).asin();
        let yaw = if vd.x.abs() < 1e-6 && vd.y.abs() < 1e-6 {
            0.0_f32
        } else {
            vd.x.atan2(-vd.y)
        };
        let rotation  = camera::yaw_pitch_to_quat(yaw, pitch);
        let view_right = rotation * glam::Vec3::X;
        let view_up    = rotation * glam::Vec3::Y;
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
        let target = base
            + view_right * view.center.x as f32
            + view_up    * view.center.y as f32;
        let fov_y    = 45.0_f32.to_radians();
        let distance = ((view.height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);
        let mut cam = self.camera.borrow_mut();
        cam.target     = target;
        cam.rotation   = rotation;
        cam.distance   = distance;
        cam.yaw        = yaw;
        cam.pitch      = pitch;
        cam.fov_y      = fov_y;
        cam.projection = camera::Projection::Orthographic;
        drop(cam);
        self.camera_generation += 1;
        true
    }

    /// Set the model-space camera from the VPORT table's *Active entry.
    /// Returns true if the entry was found and the camera was set.
    fn apply_active_vport_camera(&mut self) -> bool {
        // Prefer our named View entry — survives DWG save without being overridden.
        let saved_view = self.document.views.iter().find(|v| v.name == "H7CAD_Camera_Model").cloned();
        if let Some(view) = saved_view {
            return self.apply_camera_from_view_entry(&view, true);
        }
        let vp = match self.document.vports.iter().find(|v| v.name == "*Active") {
            Some(v) => v.clone(),
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
        let rotation = camera::yaw_pitch_to_quat(yaw, pitch);
        let view_right = rotation * glam::Vec3::X;
        let view_up    = rotation * glam::Vec3::Y;

        // view_target is WCS; wire-space subtracts world_offset.
        let base = glam::Vec3::new(
            (vp.view_target.x - self.world_offset[0]) as f32,
            (vp.view_target.y - self.world_offset[1]) as f32,
            (vp.view_target.z - self.world_offset[2]) as f32,
        );
        let target = base
            + view_right * vp.view_center.x as f32
            + view_up    * vp.view_center.y as f32;

        let fov_y = 45.0_f32.to_radians();
        let distance = ((vp.view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);

        let mut cam = self.camera.borrow_mut();
        cam.target     = target;
        cam.rotation   = rotation;
        cam.distance   = distance;
        cam.yaw        = yaw;
        cam.pitch      = pitch;
        cam.fov_y      = fov_y;
        cam.projection = camera::Projection::Orthographic;
        drop(cam);

        self.camera_generation += 1;
        true
    }

    /// Set the paper-space camera from the sheet viewport's stored view.
    /// Returns true if a valid sheet viewport was found and the camera was set.
    fn apply_sheet_viewport_camera(&mut self) -> bool {
        // Prefer our named View entry — survives DWG save without being overridden.
        let view_name = format!("H7CAD_Camera_{}", self.current_layout);
        let saved_view = self.document.views.iter().find(|v| v.name == view_name).cloned();
        if let Some(view) = saved_view {
            return self.apply_camera_from_view_entry(&view, false);
        }

        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return false;
        }

        let sheet_vp = self.document.entities()
            .filter_map(|e| if let EntityType::Viewport(vp) = e { Some(vp) } else { None })
            .find(|vp| {
                vp.common.owner_handle == layout_block && !Self::is_content_viewport(vp)
            });

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
        let rotation = camera::yaw_pitch_to_quat(yaw, pitch);
        let view_right = rotation * glam::Vec3::X;
        let view_up    = rotation * glam::Vec3::Y;

        // Paper-space entities have no world_offset applied, so target is raw.
        let base = glam::Vec3::new(
            vp.view_target.x as f32,
            vp.view_target.y as f32,
            vp.view_target.z as f32,
        );
        let target = base
            + view_right * vp.view_center.x as f32
            + view_up    * vp.view_center.y as f32;

        let fov_y = 45.0_f32.to_radians();
        let distance = ((vp.view_height as f32 / 2.0) / (fov_y * 0.5).tan()).max(0.001);

        let mut cam = self.camera.borrow_mut();
        cam.target     = target;
        cam.rotation   = rotation;
        cam.distance   = distance;
        cam.yaw        = yaw;
        cam.pitch      = pitch;
        cam.fov_y      = fov_y;
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
            if let Some(vp) = self.document.vports.iter_mut().find(|v| v.name == "*Active") {
                vp.view_target    = target_wcs;
                vp.view_center    = acadrust::types::Vector2::ZERO;
                vp.view_direction = vd3;
                vp.view_height    = view_height as f64;
            }

            // Also write to View table — survives DWG save without override.
            self.write_camera_view_entry("H7CAD_Camera_Model", target_wcs, vd3, view_height);
            true
        } else {
            let target_wcs = acadrust::types::Vector3 {
                x: cam.target.x as f64,
                y: cam.target.y as f64,
                z: cam.target.z as f64,
            };
            let view_name = format!("H7CAD_Camera_{}", self.current_layout);

            // Write back to the sheet viewport entity.
            let layout_block = self.current_layout_block_handle();
            if !layout_block.is_null() {
                let sheet_handle = self.document.entities()
                    .filter_map(|e| if let EntityType::Viewport(vp) = e { Some(vp) } else { None })
                    .find(|vp| vp.common.owner_handle == layout_block && !Self::is_content_viewport(vp))
                    .map(|vp| vp.common.handle);

                if let Some(handle) = sheet_handle {
                    if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(handle) {
                        vp.view_target    = target_wcs;
                        vp.view_center    = acadrust::types::Vector3::ZERO;
                        vp.view_direction = vd3;
                        vp.view_height    = view_height as f64;
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
        let existing_handle = self.document.views.iter().find(|v| v.name == name).map(|v| v.handle);
        let mut entry = acadrust::tables::View::new(name);
        entry.handle    = existing_handle.unwrap_or_else(|| self.document.allocate_handle());
        entry.target    = target;
        entry.direction = direction;
        entry.height    = height as f64;
        entry.width     = height as f64;
        entry.center    = acadrust::types::Vector3::ZERO;
        self.document.views.add_or_replace(entry);
    }

    /// Restore the camera from the file's saved view (called once on open).
    /// Falls back to fit_all() if no saved view is available.
    pub fn restore_saved_camera(&mut self) {
        let restored = if self.current_layout == "Model" {
            self.apply_active_vport_camera()
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

        let mut min = glam::Vec3::splat(f32::MAX);
        let mut max = glam::Vec3::splat(f32::MIN);
        // Reject points beyond 10× the drawing's EXTMIN→EXTMAX half-size.
        // These come from origin-stuck entities (y ≈ -world_offset.y after subtraction),
        // Ray/XLine far-points with bad direction vectors, or corrupt coordinates.
        let lim = self.local_extent_max;
        for wire in &wires {
            for &[x, y, z] in &wire.points {
                if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    continue;
                }
                if x.abs() > lim || y.abs() > lim || z.abs() > lim {
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

        Some(iced::Rectangle { x: x0, y: y0, width: w, height: h })
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
        let layout_block = self.current_layout_block_handle();
        let mut wires = self.paper_sheet_wires();
        wires.extend(self.viewport_content_wires(layout_block, None, self.active_viewport));
        if let Some(iw) = &self.interim_wire {
            wires.push(iw.clone());
        }
        wires.extend(self.preview_wires.iter().cloned());
        let arc = Arc::new(wires);
        *self.paper_canvas_cache.borrow_mut() = Some((self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Hatch fills for the paper-space canvas.
    pub fn paper_canvas_hatches(&self) -> Arc<Vec<HatchModel>> {
        self.hatch_models_arc()
    }

    /// Wipeout (opaque background fill) models for the paper-space canvas.
    pub fn paper_canvas_wipeouts(&self) -> Arc<Vec<HatchModel>> {
        self.wipeout_models_arc()
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

        let rotation = camera::yaw_pitch_to_quat(yaw, pitch);
        let view_right = rotation * glam::Vec3::X;
        let view_up    = rotation * glam::Vec3::Y;

        // view_target is in raw model/WCS coords; the GPU renderer works in
        // wire-space (model - world_offset), so subtract world_offset here.
        // view_center is a 2-D DCS offset of the display centre from view_target.
        let base_target = glam::Vec3::new(
            (vp.view_target.x - self.world_offset[0]) as f32,
            (vp.view_target.y - self.world_offset[1]) as f32,
            (vp.view_target.z - self.world_offset[2]) as f32,
        );
        let target = base_target
            + view_right * vp.view_center.x as f32
            + view_up    * vp.view_center.y as f32;

        let fov_y = 45.0_f32.to_radians();
        let view_height = if vp.view_height.abs() > 1e-9 {
            vp.view_height as f32
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
    /// global layer visibility and the viewport's per-viewport layer freeze list.
    fn model_wires_for_viewport(&self, vp_handle: Handle) -> Vec<WireModel> {
        use std::collections::HashSet as HSet;

        let frozen: HSet<Handle> = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp.frozen_layers.iter().cloned().collect(),
            _ => HSet::new(),
        };

        let model_block = self.model_space_block_handle();

        self.document
            .entities()
            .filter(|e| {
                let c = e.common();
                if c.invisible || matches!(e, EntityType::Viewport(_)) {
                    return false;
                }
                if !self.belongs_to_visible_block(c.handle, c.owner_handle, model_block) {
                    return false;
                }
                if self
                    .document
                    .layers
                    .get(&c.layer)
                    .map(|l| l.flags.off || l.flags.frozen)
                    .unwrap_or(false)
                {
                    return false;
                }
                if !frozen.is_empty() {
                    if let Some(lh) = self.document.layers.get(&c.layer).map(|l| l.handle) {
                        if frozen.contains(&lh) {
                            return false;
                        }
                    }
                }
                true
            })
            .flat_map(|e| tessellate_entity(
                &self.document,
                &self.selected,
                self.active_viewport,
                self.world_offset,
                self.bg_color,
                e,
            ))
            .collect()
    }

    pub(super) fn model_wires_for_viewport_arc(&self, vp_handle: Handle) -> Arc<Vec<WireModel>> {
        {
            let cache = self.viewport_wire_cache.borrow();
            if let Some((cached_epoch, ref arc)) = cache.get(&vp_handle) {
                if *cached_epoch == self.geometry_epoch {
                    return Arc::clone(arc);
                }
            }
        }
        let arc = Arc::new(self.model_wires_for_viewport(vp_handle));
        self.viewport_wire_cache
            .borrow_mut()
            .insert(vp_handle, (self.geometry_epoch, Arc::clone(&arc)));
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
    mut x0: f32, mut y0: f32,
    mut x1: f32, mut y1: f32,
    xmin: f32, ymin: f32, xmax: f32, ymax: f32,
) -> Option<(f32, f32, f32, f32)> {
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;

    let code = |x: f32, y: f32| -> u8 {
        let mut c = 0u8;
        if x < xmin { c |= LEFT; }
        else if x > xmax { c |= RIGHT; }
        if y < ymin { c |= BOTTOM; }
        else if y > ymax { c |= TOP; }
        c
    };

    let mut c0 = code(x0, y0);
    let mut c1 = code(x1, y1);

    loop {
        if c0 | c1 == 0 { return Some((x0, y0, x1, y1)); }
        if c0 & c1 != 0 { return None; }
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
        if cout == c0 { x0 = x; y0 = y; c0 = code(x0, y0); }
        else           { x1 = x; y1 = y; c1 = code(x1, y1); }
    }
}

/// Clip a projected polyline (NaN-separated segments) to the viewport rectangle.
/// Returns a new points vec with proper NaN separators at clip boundaries.
fn clip_polyline_to_rect(
    pts: &[[f32; 3]],
    xmin: f32, ymin: f32, xmax: f32, ymax: f32,
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
                None => { pen_down = false; }
                Some((cx0, cy0, cx1, cy1)) => {
                    if !pen_down {
                        if !result.is_empty() { result.push(NAN3); }
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
    while result.last().map(|p: &[f32; 3]| p[0].is_nan()).unwrap_or(false) {
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

fn tessellate_entity(
    document: &acadrust::CadDocument,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    world_offset: [f64; 3],
    bg_color: [f32; 4],
    e: &EntityType,
) -> Vec<WireModel> {
    let h = e.common().handle;
    let sel = selected.contains(&h);

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
        let mut wire = tessellate::tessellate(
            document, h, e, sel, color, pattern_length, pattern, 1.5, world_offset,
        );
        wire.aabb = entity_aabb(e, world_offset);
        return vec![wire];
    }

    let (entity_color, pattern_length, pattern, line_weight_px, aci) =
        render::render_style_for(document, e);
    let entity_color = render::adapt_to_bg(entity_color, bg_color);
    let lt_scale = document.header.linetype_scale as f32 * e.common().linetype_scale as f32;
    let lt_name = render::linetype_name_for(document, e);

    if let EntityType::Dimension(dim) = e {
        let aabb = entity_aabb(e, world_offset);
        let mut wires = tessellate::tessellate_dimension(
            document, h, dim, sel, entity_color, line_weight_px, world_offset,
        );
        for w in &mut wires {
            w.aci = aci;
            w.aabb = aabb;
        }
        return wires;
    }

    if let EntityType::Insert(ins) = e {
        let is_mirrored = ins.x_scale() * ins.y_scale() < 0.0;
        // Resolve the INSERT's own style so ByBlock sub-entities can inherit it.
        let (ins_color, ins_pat_len, ins_pat, ins_lw_px, _) =
            render::render_style_for(document, e);
        let ins_color = render::adapt_to_bg(ins_color, bg_color);
        return ins
            .explode_from_document(document)
            .iter()
            .cloned()
            .map(crate::modules::home::modify::explode::normalize_insert_entity)
            .map(|sub| crate::modules::home::modify::explode::fix_mirrored_arc(sub, is_mirrored))
            .flat_map(|sub| {
                let (sub_color, sub_pattern_length, sub_pattern, sub_line_weight_px, sub_aci) =
                    render::render_style_for_block_sub(
                        document, &sub,
                        ins_color, ins_pat_len, ins_pat, ins_lw_px,
                    );
                let sub_color = render::adapt_to_bg(sub_color, bg_color);
                let sub_aabb = entity_aabb(&sub, world_offset);
                let mut wire = tessellate::tessellate(
                    document,
                    h,
                    &sub,
                    sel,
                    sub_color,
                    sub_pattern_length,
                    sub_pattern,
                    sub_line_weight_px,
                    world_offset,
                );
                wire.name = h.value().to_string();
                wire.aci = sub_aci;
                wire.aabb = sub_aabb;
                vec![wire]
            })
            .collect();
    }

    let aabb = entity_aabb(e, world_offset);
    let mut base = tessellate::tessellate(
        document, h, e, sel, entity_color, pattern_length, pattern, line_weight_px, world_offset,
    );
    base.aci = aci;
    base.aabb = aabb;

    if let Some(clt) = crate::linetypes::complex_lt(lt_name) {
        let mut wires = complex_lt::apply_along(
            &base.name,
            &base.points,
            clt,
            lt_scale.max(1e-4),
            entity_color,
            sel,
            base.line_weight_px,
        );
        if !wires.is_empty() {
            for w in &mut wires { w.aabb = aabb; }
            return wires;
        }
    }

    vec![base]
}

fn entity_aabb(e: &acadrust::EntityType, world_offset: [f64; 3]) -> [f32; 4] {
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

/// Generate solid-fill boundary polygons for each wide segment of an LWPolyline.
/// Returns one polygon per segment that has non-zero width; empty if the polyline
/// has zero `constant_width` and all vertex widths are zero.
fn wide_lwpolyline_fills(
    pl: &acadrust::entities::LwPolyline,
) -> Vec<Vec<[f32; 2]>> {
    let hw_const = (pl.constant_width / 2.0) as f32;
    let verts = &pl.vertices;
    let n = verts.len();
    if n < 2 {
        return vec![];
    }
    let seg_count = if pl.is_closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..seg_count {
        let v0 = &verts[i];
        let v1 = &verts[(i + 1) % n];
        let hw0 = if v0.start_width > 1e-9 { v0.start_width as f32 / 2.0 } else { hw_const };
        let hw1 = if v0.end_width > 1e-9 { v0.end_width as f32 / 2.0 } else { hw_const };
        if hw0 < 1e-6 && hw1 < 1e-6 {
            continue;
        }
        let p0 = [v0.location.x as f32, v0.location.y as f32];
        let p1 = [v1.location.x as f32, v1.location.y as f32];
        if let Some(poly) = polyline_segment_fill(p0, p1, hw0, hw1, v0.bulge as f32) {
            out.push(poly);
        }
    }
    out
}

/// Generate solid-fill boundary polygons for each wide segment of a Polyline2D.
fn wide_polyline2d_fills(
    pl: &acadrust::entities::Polyline2D,
) -> Vec<Vec<[f32; 2]>> {
    let hw_default = (pl.start_width.max(pl.end_width) / 2.0) as f32;
    let verts = &pl.vertices;
    let n = verts.len();
    if n < 2 {
        return vec![];
    }
    let seg_count = if pl.is_closed() { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..seg_count {
        let v0 = &verts[i];
        let v1 = &verts[(i + 1) % n];
        let hw0 = if v0.start_width > 1e-9 { v0.start_width as f32 / 2.0 } else { hw_default };
        let hw1 = if v0.end_width > 1e-9 { v0.end_width as f32 / 2.0 } else { hw_default };
        if hw0 < 1e-6 && hw1 < 1e-6 {
            continue;
        }
        let p0 = [v0.location.x as f32, v0.location.y as f32];
        let p1 = [v1.location.x as f32, v1.location.y as f32];
        if let Some(poly) = polyline_segment_fill(p0, p1, hw0, hw1, v0.bulge as f32) {
            out.push(poly);
        }
    }
    out
}

/// Compute the filled boundary polygon for one polyline segment.
/// For straight segments: a rectangle/trapezoid.
/// For arc segments: an arc band (outer arc + reversed inner arc).
/// Returns `None` if the segment is degenerate.
fn polyline_segment_fill(
    p0: [f32; 2],
    p1: [f32; 2],
    hw0: f32,
    hw1: f32,
    bulge: f32,
) -> Option<Vec<[f32; 2]>> {
    if bulge.abs() < 1e-9 {
        // Straight segment — rectangle or trapezoid
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-9 {
            return None;
        }
        let nx = -dy / len;
        let ny = dx / len;
        Some(vec![
            [p0[0] + hw0 * nx, p0[1] + hw0 * ny],
            [p1[0] + hw1 * nx, p1[1] + hw1 * ny],
            [p1[0] - hw1 * nx, p1[1] - hw1 * ny],
            [p0[0] - hw0 * nx, p0[1] - hw0 * ny],
        ])
    } else {
        // Arc segment — arc band polygon
        let angle = 4.0 * (bulge as f64).atan();
        let dx = (p1[0] - p0[0]) as f64;
        let dy = (p1[1] - p0[1]) as f64;
        let d = (dx * dx + dy * dy).sqrt();
        if d < 1e-9 {
            return None;
        }
        let r = (d / 2.0) / (angle / 2.0).sin().abs();
        let mx = ((p0[0] + p1[0]) * 0.5) as f64;
        let my = ((p0[1] + p1[1]) * 0.5) as f64;
        let px = -dy / d;
        let py = dx / d;
        let sign = if bulge > 0.0 { 1.0_f64 } else { -1.0_f64 };
        let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
        let cx = (mx - sign * px * (r - h)) as f32;
        let cy = (my - sign * py * (r - h)) as f32;
        let a0 = ((p0[1] - cy) as f32).atan2((p0[0] - cx) as f32);
        let a1 = ((p1[1] - cy) as f32).atan2((p1[0] - cx) as f32);
        let (sa, mut ea) = if bulge > 0.0 { (a0, a1) } else { (a1, a0) };
        if ea < sa {
            ea += std::f32::consts::TAU;
        }
        let span = ea - sa;
        let segs = ((span.abs() / std::f32::consts::TAU) * 12.0)
            .ceil()
            .max(4.0) as u32;
        let r = r as f32;
        let hw = (hw0 + hw1) * 0.5;
        let r_outer = r + hw;
        let r_inner = (r - hw).max(0.0);
        let mut boundary = Vec::with_capacity((segs as usize + 1) * 2);
        for j in 0..=segs {
            let t = sa + span * (j as f32 / segs as f32);
            boundary.push([cx + r_outer * t.cos(), cy + r_outer * t.sin()]);
        }
        for j in (0..=segs).rev() {
            let t = sa + span * (j as f32 / segs as f32);
            boundary.push([cx + r_inner * t.cos(), cy + r_inner * t.sin()]);
        }
        boundary.truncate(64);
        Some(boundary)
    }
}

