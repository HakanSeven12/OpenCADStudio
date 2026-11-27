// Scene modules grouped by role:
//   convert — DXF/ACIS entities → solids & tessellated geometry
//   text    — LFF stroke + TrueType font engines and shaping
//   model   — per-entity GPU render models (wire, hatch, mesh, image, object)
//   pick    — hit-testing, selection, grips, spatial index, xclip
//   view    — camera, transforms, viewport, render pipeline driver
//   cache   — block-definition and property caches
pub mod analysis;
pub mod annotative;
pub mod assoc_array;
pub mod assoc_array_grips;
pub mod auto_constrain;
pub(crate) mod block_action_bars;
pub(crate) mod block_actions;
pub(crate) mod block_alignment;
pub(crate) mod block_authoring;
pub(crate) mod block_basepoint;
pub(crate) mod block_construction;
pub(crate) mod block_distance_parameter;
pub(crate) mod block_grips;
pub(crate) mod block_lookup;
pub(crate) mod block_metadata;
pub(crate) mod block_orientation_parameter;
pub(crate) mod block_parameter_properties;
pub(crate) mod block_parameters_manager;
pub(crate) mod block_point_parameter;
pub(crate) mod block_properties_table;
pub mod block_service;
pub mod block_visibility;
pub mod bounds;
pub(crate) mod bricscad_parameters;
pub mod cache;
pub mod compare;
pub mod convert;
pub mod creation_style;
pub(crate) mod frame;
pub(crate) use crate::entities::hyperlink;
pub mod linetype_service;
pub mod model;
pub mod pick;
pub mod pipeline;
pub(crate) mod raster_reseed;
pub(crate) mod raster_scale;
pub(crate) mod reaper;
pub(crate) mod render_graph;
pub mod section_cull;
pub mod sub_select;
pub mod text;
pub mod view;

// Topic submodules split out of this root (each contributes `impl Scene`
// blocks and/or free functions). Pure text-move from the original mod.rs.
#[cfg(test)]
mod annotation_visibility_tests;
#[cfg(test)]
mod attribute_over_wipeout_tests;
#[cfg(test)]
mod block_base_point_tests;
#[cfg(test)]
mod block_definition_handles_tests;
#[cfg(test)]
mod block_solid_fill_tests;
mod boundary;
mod caches;
mod deps_and_cull;
mod interaction_pick;
mod open_prep;
mod render_collect;
mod viewport_solids;
mod visibility_state;
use caches::SceneCaches;
pub use open_prep::{DerivedCaches, OpenTimings};
#[cfg(target_arch = "wasm32")]
pub use open_prep::build_derived_caches;
#[cfg(not(target_arch = "wasm32"))]
pub use open_prep::{build_derived_caches_with_progress, prepare_open_geometry};
#[cfg(test)]
pub(crate) use open_prep::build_derived_caches_impl;
pub(in crate::scene) use open_prep::{offset_mesh_lod_set, transform_block_mesh_lod_set};
mod camera_ops;
#[cfg(test)]
mod draw_guard_key_tests;
#[cfg(test)]
mod dwg_edit_after_open_tests;
#[cfg(test)]
mod layer_visibility_block_cache_tests;
#[cfg(test)]
mod layout_plot_settings_cache_tests;
#[cfg(test)]
mod ole_resolution_tests;
#[cfg(test)]
mod reference_only_tests;
#[cfg(test)]
mod resident_patch_adds_tests;
#[cfg(test)]
mod resident_patch_shorter_runs_tests;
#[cfg(test)]
mod solid_box_select_tests;
#[cfg(test)]
mod solid_highlight_outline_tests;
#[cfg(test)]
mod solid_outline_detail_tests;
pub use camera_ops::{add_background, copy_background, erase_background};
pub use raster_reseed::{ReseedDone, ReseedJob};
pub(crate) mod center_native;
pub(crate) mod centerline;
mod center_marks;
pub(crate) use crate::entities::centermark;
pub mod clipboard;
pub(crate) mod dimension_assoc;
pub(crate) mod dimension_assoc_chain;
pub mod drawing_vars;
pub mod explode;
pub(crate) mod mline_style;
pub use dimension_assoc::{ReferenceStatus, ResolvedReference};
pub use page_setup::{apply_default_page_setup, document_page_setups, rotated_margins};
mod dwg_native_constraints;
mod entity;
pub use camera_ops::named_view_center;
pub use camera_ops::FindZoom;
pub(crate) use entity::gradient_tint_color;
pub(crate) use entity::normalize_block_terminators;
pub(crate) use entity::wire_thumbnail_bgra;
pub use entity::CreateBlockOptions;
mod group_layer;
mod named_selections;
pub(crate) use named_selections::NamedSelectionRecall;
/// The one grip a whole picked group shows (GROUPDISPLAYMODE 1 and 2), at
/// the centre of its extents: each member carries it, so dragging it moves
/// the whole group.
pub const GROUP_GRIP_ID: usize = usize::MAX - 2;
#[cfg(test)]
mod hatch_boundary;
pub(crate) mod layout;
#[cfg(test)]
mod layout_viewport_tests;
mod limits;
mod modify;
#[cfg(test)]
mod selection_preview_tests;
#[cfg(test)]
mod sheet_underlay_tests;
#[cfg(test)]
mod sheet_paper_change_tests;
pub(crate) mod table_content;
pub(crate) mod table_fields;
pub(crate) use modify::inverse_affine;
pub(crate) use modify::remap_copied_reactors;
pub(crate) use modify::remap_copied_viewport_clips;
mod mspace;
pub mod named_parameters;
mod outline_detail;
mod page_setup;
mod paper;
mod preview;
mod project;
pub mod reference_only;
mod scene_markers;
mod selection;
mod viewport_color;
pub use selection::TableCellSelection;
pub use selection::SELECT_SIMILAR_DEFAULT;
#[cfg(test)]
mod layout_frame_wires_tests;
#[cfg(test)]
mod layout_saved_without_sheet_tests;
#[cfg(test)]
mod paper_sheet_depth_tests;
#[cfg(test)]
mod viewport_depth_tests;
pub mod viewport_dimension_pick;
#[cfg(test)]
mod viewport_freeze_block_member_tests;
pub mod viewport_ref;
#[cfg(test)]
mod viewport_underlay_tests;
pub(crate) use selection::pe_url_of;
pub mod parametric_constraints;
pub mod xref_info;
mod parametric_solve;

#[cfg(test)]
pub(crate) use boundary::SOURCES_EXAMINED;
pub(crate) use boundary::{
    boundary_entities, boundary_entities_from_sources, boundary_faces, boundary_faces_around,
    boundary_polyline_entities, entity_boundary_source_on_plane, exact_hatch_paths,
    hatch_boundary_rings, hatch_path_ring, live_segments_for_curves, remap_hatch_associations,
    ring_source_handles, rings_source_handles, separated_hatch_path_groups, sources_near_rings,
    BoundarySource,
};
pub(crate) use boundary::{drawn_hatch_path, new_hatch_paths, HatchLoopSource};

// Parallel tessellation free functions live in `convert::tess` (alongside the
// other tessellation code); re-exported here so this root and sibling topic
// modules (each does `use super::*`) keep referencing them unqualified.
pub(crate) use convert::tess::{
    entity_aabb, entity_world_aabb_f64, is_unindexable_entity, tessellate_entity_dim_text,
};

/// Keep construction-history curves in the same per-entity tessellation memo
/// as their parent, so navigation never rebuilds them on every frame.
///
/// `reference_only` is the scene's designation set: its membership decides
/// whether the produced wires carry the `WireModel::reference_only` click-
/// through stamp (see `scene::reference_only`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_entity(
    document: &Document,
    selected: &HashSet<Handle>,
    active_viewport: Option<Handle>,
    bg_color: [f32; 4],
    anno_scale: f32,
    annotation_scale_handle: Option<Handle>,
    entity: &EntityType,
    block_cache: Option<&cache::block_cache::BlockCache>,
    view_aabb: Option<[f32; 4]>,
    world_per_pixel: Option<f32>,
    viewport_lt: Option<f32>,
    reference_only: &HashSet<Handle>,
) -> Vec<WireModel> {
    let mut wires = convert::tess::tessellate_entity(
        document,
        selected,
        active_viewport,
        bg_color,
        anno_scale,
        annotation_scale_handle,
        entity,
        block_cache,
        view_aabb,
        world_per_pixel,
        viewport_lt,
        reference_only,
    );
    if matches!(entity, EntityType::Solid3D(_) | EntityType::Surface(_)) {
        let handle = entity.common().handle;
        for source in model::solid_history::loft_visible_history_entities(document, handle) {
            // Call the raw converter, not this wrapper: sources must not expand
            // the parent's history again through their shared selection handle.
            let mut construction = if let EntityType::Region(region) = &source {
                // Regions normally render through the mesh layer, so the raw
                // wire converter emits nothing. Decode every exact boundary,
                // including holes, then sample only for this cached display.
                let mut boundaries = Vec::new();
                if let Ok(ocs_solid::LoftSection::Profile { plane, wires, .. }) =
                    ocs_solid::loft_section_geometry(&[document::entities::EmbeddedEntity::Region(
                        region.clone(),
                    )])
                {
                    let (color, _, _, line_weight_px, _) =
                        view::render::render_style_for_viewport(document, &source, active_viewport);
                    for boundary in wires {
                        for curve in boundary {
                            let points = curve
                                .tessellate(64.0 / std::f64::consts::TAU)
                                .into_iter()
                                .map(|point| plane.point_at(point));
                            let (points, points_low) = convert::tessellate::points_to_ds(points);
                            let mut wire = WireModel::solid(
                                handle.value().to_string(),
                                points,
                                color,
                                selected.contains(&handle),
                            );
                            wire.points_low = points_low.into();
                            wire.line_weight_px = line_weight_px;
                            boundaries.push(wire);
                        }
                    }
                }
                boundaries
            } else {
                convert::tess::tessellate_entity(
                    document,
                    selected,
                    active_viewport,
                    bg_color,
                    anno_scale,
                    annotation_scale_handle,
                    &source,
                    block_cache,
                    view_aabb,
                    world_per_pixel,
                    viewport_lt,
                    reference_only,
                )
            };
            for wire in &mut construction {
                wire.name = handle.value().to_string();
                if !wire.selected {
                    wire.color = [1.0, 0.25, 0.25, wire.color[3]];
                }
                wire.fill_tris.clear();
                wire.fill_tris_low.clear();
            }
            wires.extend(construction);
        }
        for points in model::solid_history::composite_history_edges(document, handle) {
            let selected = selected.contains(&handle);
            let color = [1.0, 0.25, 0.25, 1.0];
            wires.push(WireModel::solid_f64(
                handle.value().to_string(),
                points,
                color,
                selected,
            ));
        }
    }
    wires
}

/// The xdata application a paper layout's "show every annotative object" lives
/// under.
pub(in crate::scene) const LAYOUT_ANNOTATION_VISIBILITY_APP: &str = "AcadAnnoAV";


/// Result of `Scene::entity_index()`. The wire path queries `tree` for
/// view-rect candidates and also always emits `unbounded_handles`
/// (entities with no usable bbox — legacy `UNBOUNDED_AABB` sentinel).
#[derive(Clone)]
pub(crate) struct EntityIndex {
    pub tree: pick::quadtree::QuadTree,
    pub unbounded_handles: Vec<Handle>,
}

/// Hash of every baked display input a memoized tessellation depends on; a
/// memo entry is reusable only under the guard it was built with.
pub(in crate::scene) fn tess_memo_guard(
    view_aabb: Option<[f32; 4]>,
    anno: f32,
    annotation_scale_handle: Option<Handle>,
    all_visible: bool,
    bg: [f32; 4],
    avp: Option<Handle>,
) -> u64 {
    let mut g: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |x: u64| g = g.rotate_left(13) ^ x;
    // wpp removed from guard: GPU analytical rendering handles
    // circles/arcs/ellipses, Point ignores wpp, and Light (the
    // only remaining wpp consumer) is rare enough that its stale
    // glyphs don't justify clearing every memoized entity on zoom.
    if let Some(v) = view_aabb {
        for c in v {
            mix(c.to_bits() as u64);
        }
    }
    mix(anno.to_bits() as u64);
    mix(annotation_scale_handle
        .map(|handle| handle.value())
        .unwrap_or(0));
    mix(all_visible as u64);
    // Direct entities and inherited/faded block colours still bake
    // the background before Batches::finalize records its inputs.
    for channel in bg {
        mix(channel.to_bits() as u64);
    }
    mix(avp.map(|h| h.value()).unwrap_or(0));
    // The SDF atlas generation is not here: a growth or re-bake stales only
    // the memoized text (see `drop_stale_text`), not every entity.
    g
}

/// SDF glyph quads bake the atlas UV of each tile, so an atlas growth or
/// re-bake (which rescale / rewind every UV) makes memoized text address the
/// wrong tile: garbage on screen, and a silent miss in the PDF export's glyph
/// lookup (#385 under #347's conditions). When the generation `cell` was
/// built under moved, the runs carrying glyphs are dropped; the rest stay,
/// so a first open's atlas growth no longer re-tessellates the whole
/// drawing (#2164).
pub(in crate::scene) fn drop_stale_text(
    scene: &Scene,
    memo: &mut HashMap<Handle, MemoRun>,
    cell: &std::cell::Cell<u64>,
) {
    let now = crate::scene::text::sdf_atlas::generation();
    if cell.get() != now {
        memo.retain(|handle, run| {
            scene
                .resolve_memo_run(*handle, run)
                .is_some_and(|hit| hit.wires().iter().all(|w| w.text_verts.is_empty()))
        });
        cell.set(now);
    }
}

/// A complex linetype bakes the viewport's PSLTSCALE factor into its wires, so
/// a new factor stales the memoized runs the dependency index files under
/// annotation geometry (complex-linetype users among them), not every entity.
pub(in crate::scene) fn drop_stale_complex_lt(
    scene: &Scene,
    memo: &mut HashMap<Handle, MemoRun>,
    cell: &std::cell::Cell<u64>,
) {
    let now = scene.viewport_complex_lt_guard();
    if cell.get() == now {
        return;
    }
    cell.set(now);
    if memo.is_empty() {
        return;
    }
    if scene.caches.dependency_index_cache.borrow().is_none() {
        let index = scene.rebuild_dependency_index();
        *scene.caches.dependency_index_cache.borrow_mut() = Some(Arc::new(index));
    }
    if let Some(index) = scene.caches.dependency_index_cache.borrow().as_ref() {
        for handle in &index.annotation_geometry.render_handles {
            memo.remove(handle);
        }
    }
}

#[derive(Clone, Default)]
pub(in crate::scene) struct DependencyTargets {
    pub(in crate::scene) render_handles: HashSet<Handle>,
    pub(in crate::scene) source_handles: HashSet<Handle>,
    /// Render handles reached through a block definition (INSERTs and other
    /// block users), as opposed to entities that carry the name themselves.
    pub(in crate::scene) block_users: HashSet<Handle>,
    pub(in crate::scene) touches_block_definition: bool,
}

impl DependencyTargets {
    pub(in crate::scene) fn extend_from(&mut self, other: &DependencyTargets) {
        self.render_handles
            .extend(other.render_handles.iter().copied());
        self.source_handles
            .extend(other.source_handles.iter().copied());
        self.block_users.extend(other.block_users.iter().copied());
        self.touches_block_definition |= other.touches_block_definition;
    }
}

#[derive(Clone, Copy)]
pub(in crate::scene) enum DependencyKind {
    Layer,
    Linetype,
    TextStyle,
    DimStyle,
}

/// The render roots an entity contributes to a dependency target. Kept as a
/// borrow rather than a collected set so the common direct-render case adds
/// its single handle without allocating.
#[derive(Clone, Copy)]
pub(in crate::scene) enum RenderHandles<'a> {
    SelfOnly(Handle),
    /// The entity lives inside a block definition; it renders through the
    /// outer references that draw that block. `None` when the definition has
    /// no root users yet.
    Block(Option<&'a HashSet<Handle>>),
}

impl RenderHandles<'_> {
    pub(in crate::scene) fn extend_into(&self, target: &mut DependencyTargets, source: Handle) {
        match self {
            RenderHandles::SelfOnly(handle) => {
                target.render_handles.insert(*handle);
                target.source_handles.insert(source);
            }
            RenderHandles::Block(roots) => {
                target.source_handles.insert(source);
                target.touches_block_definition = true;
                if let Some(roots) = roots {
                    target.render_handles.extend(roots.iter().copied());
                    target.block_users.extend(roots.iter().copied());
                }
            }
        }
    }
}

#[derive(Default)]
pub(in crate::scene) struct SceneDependencyIndex {
    pub(in crate::scene) layers: HashMap<String, DependencyTargets>,
    pub(in crate::scene) linetypes: HashMap<String, DependencyTargets>,
    pub(in crate::scene) text_styles: HashMap<String, DependencyTargets>,
    pub(in crate::scene) dim_styles: HashMap<String, DependencyTargets>,
    pub(in crate::scene) object_styles: HashMap<Handle, DependencyTargets>,
    pub(in crate::scene) points: DependencyTargets,
    pub(in crate::scene) text_geometry: DependencyTargets,
    pub(in crate::scene) annotation_geometry: DependencyTargets,
    pub(in crate::scene) signatures: HashMap<Handle, u64>,
    /// Block name (normalised) → the layout entities that draw it, nested
    /// references included.
    pub(in crate::scene) block_roots: HashMap<String, HashSet<Handle>>,
    pub(in crate::scene) wipeout_hosts: HashSet<Handle>,
}

pub(in crate::scene) fn hatch_interaction_aabb(hatch: &model::hatch_model::HatchModel) -> Option<[f64; 4]> {
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

/// Whether a wire's geometry meets a world-XY box `[min_x, min_y, max_x,
/// max_y]`. A wire with no finite stored box is measured from what it
/// carries (points, fills, snap features); one carrying nothing never meets.
pub(in crate::scene) fn wire_meets_xy_box(wire: &WireModel, query: [f64; 4]) -> bool {
    // The stored box is f32: allow its rounding at survey coordinates. Scaled
    // by the query, not the wire, so a construction line's huge far end does
    // not widen it.
    let pad = 1e-6 * query.iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let meets = |[x0, y0, x1, y1]: [f64; 4]| {
        x1 + pad >= query[0] && x0 - pad <= query[2] && y1 + pad >= query[1] && y0 - pad <= query[3]
    };
    if wire.aabb.iter().all(|value| value.is_finite()) {
        return meets(wire.aabb.map(f64::from));
    }
    fn combine<'a>(
        high: &'a [[f32; 3]],
        low: model::point_residuals::ResidualView<'a>,
    ) -> impl Iterator<Item = [f64; 2]> + 'a {
        high.iter().enumerate().map(move |(index, point)| {
            let residual = low.get(index).unwrap_or([0.0; 3]);
            [
                point[0] as f64 + residual[0] as f64,
                point[1] as f64 + residual[1] as f64,
            ]
        })
    }
    let mut aabb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in combine(&wire.points, wire.points_low.view())
        .chain(combine(
            &wire.fill_tris,
            model::point_residuals::ResidualView::Xyz(&wire.fill_tris_low),
        ))
        .chain(wire.key_vertices.iter().map(|point| [point[0], point[1]]))
        .chain(wire.snap_pts.iter().map(|(point, _)| [point.x, point.y]))
    {
        if x.is_finite() && y.is_finite() {
            aabb = [
                aabb[0].min(x),
                aabb[1].min(y),
                aabb[2].max(x),
                aabb[3].max(y),
            ];
        }
    }
    aabb.iter().all(|value| value.is_finite()) && meets(aabb)
}

pub(in crate::scene) fn mesh_interaction_aabb(set: &model::mesh_model::MeshLodSet) -> Option<[f64; 6]> {
    if let Some(aabb) = set.instance_aabb {
        return Some(aabb);
    }
    *set.cached_interaction_aabb.get_or_init(|| {
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
    })
}

pub use entity::BlockObjectMode;
pub use model::hatch_model::HatchModel;
pub use preview::{AreaPreviewRegion, AreaPreviewSource};
pub use model::image_model::ImageModel;
pub use model::mesh_model::MeshLodSet;
pub use model::object::{GripApply, GripDef};
pub use model::wire_model::{WireGeom, WireModel};
use pick::interaction_index::{InteractionCandidates, InteractionIndex, SnapQueryCategory};
pub(crate) use parametric_solve::status::{display_color, normalize_color};
pub use pick::selection_state::SelectionState;
pub use project::PlotImage;
pub use scale_list::ViewportLayers;
pub(crate) use page_setup::NO_DEVICE_STORED_NAME;
pub use pipeline::uniforms::Uniforms;
pub use pipeline::viewcube::{
    hit_test, hit_test_cardinal, hover_id, CubeRegion, NudgeDir, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD,
    VIEWCUBE_PX, VIEWCUBE_REGION_PX, VIEWCUBE_RENDER_PX,
};
use view::camera::Camera;
pub use view::camera::Projection;

use crate::entities::transform::EntityTransform;
use document::entities::{Block, BlockEnd, Insert as DxfInsert};
use document::entities::{
    BoundaryEdge, BoundaryPath, Hatch as DxfHatch, PolylineEdge, Solid as DxfSolid,
};
use document::objects::ObjectType;
use document::tables::normalize_name;
use document::types::Vector2;
use document::{Document, EntityType, Handle, TableEntry};
use glam;
use iced::time::Duration;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
pub use crate::entities::insert::BlockScalePolicy;
pub(crate) use crate::entities::insert::INSERT_ATTRIBUTE_RANK;
pub use crate::entities::viewport::vp_effective_scale;

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

/// Session-only LAYWALK visibility override, keyed by drawing space
/// (see `Scene::interaction_space_key_pub`): while a space has a walk set,
/// entities whose layer is not in the set are hidden there — like
/// per-viewport freeze, but in memory only. The layer table is never
/// touched, so the drawing stays clean and undo is unaffected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerWalkState {
    pub visible: HashMap<u64, HashSet<Handle>>,
}

/// Global counter so every Scene and every geometry mutation gets a
/// process-wide unique epoch. This prevents two different tabs (Scenes)
/// from ever sharing the same epoch value, which would cause the shared
/// GPU Pipeline to skip re-uploading geometry when switching tabs.
pub(in crate::scene) static GEOMETRY_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Process-wide monotonic id stamped each time the Model wire set is built.
/// The set is held static across camera moves, so the id stays the same every
/// frame until the geometry epoch changes — it uniquely identifies a wire
/// buffer's *content* across frames. The GPU pipeline gates wire re-upload on
/// it: an unchanged id means the world-space wire buffer is not re-sent.
/// Monotonic (never reused) → free of the ABA hazard a raw `Arc` pointer would
/// carry when an address is freed and reallocated.
pub(in crate::scene) static WIRE_CONTENT_GEN: AtomicU64 = AtomicU64::new(1);

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
    /// First-touch before-images of every parametric-constraint scope a command
    /// changed (e.g. ERASE removing an entity's constraints along with it via
    /// `refresh_parametric_constraints`'s deletion policy). `parametric_constraints`
    /// lives on `Scene`, not in `document`/`document.objects`, so neither
    /// directory above ever sees this change — it needs its own directory.
    /// Unlike the two above, there is no "didn't exist before" case: a
    /// scope's `Vec` entry, once created, is never removed.
    parametric_constraints_before: HashMap<
        parametric_constraints::ParametricScope,
        parametric_constraints::ParametricConstraintSet,
    >,
    parametric_constraints_order: Vec<parametric_constraints::ParametricScope>,
    named_parameters_before: Option<named_parameters::ParameterTable>,
    /// First-touch before-images of reference-only designations (API v9)
    /// changed by the command — almost always empty. The designation set
    /// lives on `Scene`, not in the document, so like
    /// `parametric_constraints_before` it needs its own channel. Erasing a
    /// designated entity drops its set entry here so undo restores entity
    /// and designation together.
    reference_designations_before: Vec<(Handle, bool)>,
    poisoned: bool,
}

impl UndoRecording {
    /// The command touched non-entity state a pure-entity delta can't restore.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// No entity, object, constraint, or parameter state was recorded.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
            && self.object_order.is_empty()
            && self.parametric_constraints_order.is_empty()
            && self.named_parameters_before.is_none()
            && self.reference_designations_before.is_empty()
    }

    /// Consume the recording directories in deterministic first-touch
    /// order. A `None` image means the entity/object was added by the
    /// command; every parametric-constraint scope's before-image is a real
    /// `ParametricConstraintSet` (its `Vec` entry, once created, is never
    /// removed, so there is no "didn't exist before" case there).
    pub fn into_recorded_images(
        mut self,
    ) -> (
        Vec<(Handle, Option<Arc<EntityType>>)>,
        Vec<(Handle, Option<ObjectType>)>,
        Vec<(
            parametric_constraints::ParametricScope,
            parametric_constraints::ParametricConstraintSet,
        )>,
        Option<named_parameters::ParameterTable>,
        Vec<(Handle, bool)>,
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
        let parametric_constraints = self
            .parametric_constraints_order
            .drain(..)
            .filter_map(|scope| {
                self.parametric_constraints_before
                    .remove(&scope)
                    .map(|before| (scope, before))
            })
            .collect();
        let reference_designations = std::mem::take(&mut self.reference_designations_before);
        (
            entities,
            objects,
            parametric_constraints,
            self.named_parameters_before,
            reference_designations,
        )
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
pub(in crate::scene) struct GeometryDelta {
    pub(in crate::scene) epoch: u64,
    pub(in crate::scene) changes: Vec<(Handle, ChangeKind)>,
    /// Cache-category membership captured immediately before a Removed entity
    /// leaves the document. Presence with value 0 means "known unrelated";
    /// absence means the caller removed it outside the tracked primitives and
    /// consumers must remain conservative.
    pub(in crate::scene) removed_categories: HashMap<Handle, u16>,
    pub(in crate::scene) full: bool,
    pub(in crate::scene) layer_visibility_only: bool,
}

/// The desk a paper layout lays its sheet on — the surface visible around the
/// page. Distinct from both the page (`paper_bg_color`) and the model canvas
/// (`bg_color`); if the desk took either of those colours the page edge would
/// vanish into its own surround.
pub const PAPER_DESK_COLOR: [f32; 4] = [138.0 / 255.0, 138.0 / 255.0, 138.0 / 255.0, 1.0];

pub(in crate::scene) const CACHE_CATEGORY_ANNOTATIVE: u16 = 1 << 0;
pub(in crate::scene) const CACHE_CATEGORY_HATCH: u16 = 1 << 1;
pub(in crate::scene) const CACHE_CATEGORY_WIPEOUT: u16 = 1 << 2;
pub(in crate::scene) const CACHE_CATEGORY_IMAGE: u16 = 1 << 3;
pub(in crate::scene) const CACHE_CATEGORY_MESH: u16 = 1 << 4;
pub(in crate::scene) const CACHE_CATEGORY_INTERACTION: u16 = 1 << 5;
pub(in crate::scene) const CACHE_CATEGORY_TEXT: u16 = 1 << 6;
pub(in crate::scene) const CACHE_CATEGORY_INSERT_HATCH: u16 = 1 << 7;

/// Snap points of the solids inside each block definition, by entity.
pub(in crate::scene) type BlockSolidSnaps =
    HashMap<Handle, Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)>>;

/// Mutable assembly metadata for one resident wire set. Keeping entity ranges
/// beside the flat render Vec lets a one-entity edit splice that run directly;
/// rebuilding this directory by grouping every WireModel was the dominant
/// edit-time CPU cost on dense drawings.
pub(crate) struct ResidentWireSet {
    estimated_bytes: usize,
    epoch: u64,
    gen: u64,
    /// [`resident_touch`] stamp of the last build or reuse: pruning gives up
    /// the least recently used set first.
    used: std::cell::Cell<u64>,
    wires: Arc<Vec<WireModel>>,
    layout: Option<ResidentWireLayout>,
}

/// Monotonic stamp for [`ResidentWireSet::used`].
pub(in crate::scene) fn resident_touch() -> u64 {
    static TOUCH: AtomicU64 = AtomicU64::new(0);
    TOUCH.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub(crate) struct ResidentWireLayout {
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

/// One memoized entity tessellation.
#[derive(Clone)]
pub(crate) enum MemoRun {
    /// The wires themselves: an entity with no run in the backing set
    /// (hidden, or tessellated after the set was built).
    Owned(Arc<Vec<WireModel>>),
    /// Still valid, and the same wires the backing resident set holds for the
    /// entity, so they are kept once instead of twice (#1542).
    InSet,
    /// Tessellated into the resident set being built, which becomes the
    /// backing when it is done; until then the run reads as a miss.
    Pending,
}

/// The resident set `MemoRun::InSet` runs live in. Held weakly so the set
/// stays uniquely owned and can be patched in place; `keep` holds a set an
/// abandoned patch took apart until the following full build has read it.
#[derive(Clone, Default)]
pub(crate) struct MemoBacking {
    set: std::sync::Weak<Vec<WireModel>>,
    ranges: HashMap<Handle, (usize, usize)>,
    keep: Option<Arc<Vec<WireModel>>>,
}

/// A memoized run resolved to its wires.
pub(crate) enum MemoHit {
    Owned(Arc<Vec<WireModel>>),
    Slice(Arc<Vec<WireModel>>, usize, usize),
}

impl MemoHit {
    fn wires(&self) -> &[WireModel] {
        match self {
            MemoHit::Owned(wires) => wires,
            MemoHit::Slice(set, start, len) => &set[*start..*start + *len],
        }
    }
}

/// Hands the set an abandoned resident patch was taking apart back to the
/// memo, whose `InSet` runs still read from it.
pub(in crate::scene) struct ReclaimOnDecline<'a> {
    pub(in crate::scene) scene: &'a Scene,
    pub(in crate::scene) wires: Option<Vec<WireModel>>,
    pub(in crate::scene) backed: bool,
}

impl Drop for ReclaimOnDecline<'_> {
    fn drop(&mut self) {
        if let Some(wires) = self.wires.take() {
            if self.backed {
                self.scene.keep_memo_backing(wires);
            }
        }
    }
}

/// Coalesced geometry-journal replay for the overlay base, keyed by (base
/// epoch, geometry epoch, space key): the journal changes themselves.
pub(in crate::scene) type OverlayChanges = (u64, u64, u64, Arc<Vec<(Handle, ChangeKind)>>);

/// What a query needs from the immutable overlay base: (base epoch, base
/// index, geometry-journal changes since it).
pub(in crate::scene) type OverlayBase = (
    u64,
    Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    Arc<Vec<(Handle, ChangeKind)>>,
);

/// A zero-copy overlay gather: shared wire runs plus the base and changed
/// slot maps remapping `(handle, ordinal)` keys onto run slots.
pub(in crate::scene) type OverlayGather = (
    Arc<crate::scene::pick::interaction_index::SharedWireRuns>,
    HashMap<(u64, u32), u32>,
    HashMap<(u64, u32), u32>,
);

/// Kill-switch: `OCS_NO_RESIDENT_MEMO` reverts the resident set to the old
/// full re-tessellation on every edit, in case a mutation site is ever found
/// that edits geometry without dropping its handle from the memo.
pub(in crate::scene) fn resident_memo_enabled() -> bool {
    static EN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *EN.get_or_init(|| std::env::var("OCS_NO_RESIDENT_MEMO").is_err())
}

#[derive(Clone)]
pub struct PreparedOpenGeometry {
    pub(in crate::scene) dependency_index: Option<Arc<SceneDependencyIndex>>,
    pub wires: Arc<Vec<WireModel>>,
    pub interaction_index: Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>>,
    /// Loader-computed spatial entity index, re-stamped by the receiving scene.
    pub(crate) entity_index: Option<EntityIndex>,
    /// Loader tessellations and their guard; a state mismatch clears the memo.
    pub(crate) tess_memo: HashMap<Handle, MemoRun>,
    pub(crate) tess_memo_backing: MemoBacking,
    pub tess_guard: u64,
    /// The SDF atlas generation the loader's memo was built under.
    pub tess_atlas: u64,
    /// The loader's submission-order layout for `wires`.
    ///
    /// Without it the installed set has `layout: None`, and `try_resident_patch`
    /// refuses any set that has none — so the first edit after opening a file
    /// could never patch and rebuilt the whole wire assembly instead.
    pub(crate) resident_layout: Option<ResidentWireLayout>,
    /// Loader block definitions; the receiving scene supplies its own epoch.
    pub block_defns: Vec<(u64, Arc<cache::block_cache::BlockCache>)>,
    /// Loader-computed draw-order map, re-stamped by the receiving scene.
    pub(crate) draw_depths: Option<DrawDepthCache>,
    /// Loader-computed hatch picking index with the interaction space it was
    /// built for; a receiving scene in another space rebuilds its own.
    pub(crate) handle_index: Option<(
        u64,
        Arc<crate::scene::pick::interaction_index::InteractionHandleIndex>,
    )>,
    /// Placed block hatches already built as input to the loader's handle index.
    pub(in crate::scene) insert_hatches: Option<(u64, Arc<HashMap<Handle, Vec<HatchModel>>>)>,
}

impl std::fmt::Debug for PreparedOpenGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedOpenGeometry")
            .field("wires", &self.wires.len())
            .field("interaction_index", &self.interaction_index.is_some())
            .field("tess_memo", &self.tess_memo.len())
            .field("block_defns", &self.block_defns.len())
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
    /// Whether block-text GPU buffers can differ after this patch.
    pub(crate) text_pass_changed: bool,
}

/// Combine consecutive resident edits when they happen before the renderer
/// prepares a frame. A single point acceptance can add a line and then update
/// it again while inferred constraints are applied; exposing only the final
/// patch would leave the GPU arena one version behind and force a full rebuild.
pub(in crate::scene) fn compose_wire_gpu_patches(first: &WireGpuPatch, second: WireGpuPatch) -> WireGpuPatch {
    use std::collections::HashMap as StdHashMap;

    let mut change_slots = StdHashMap::with_capacity(first.changes.len() + second.changes.len());
    let mut changes = Vec::with_capacity(first.changes.len() + second.changes.len());
    for &(handle, kind) in first.changes.iter() {
        change_slots.insert(handle, changes.len());
        changes.push((handle, kind));
    }
    for &(handle, next_kind) in second.changes.iter() {
        if let Some(&slot) = change_slots.get(&handle) {
            let prior_kind = changes[slot].1;
            changes[slot].1 = match (prior_kind, next_kind) {
                // The renderer's base set never contained this handle, so an
                // add followed by a remove has no net geometry change.
                (ChangeKind::Added, ChangeKind::Removed) => ChangeKind::Removed,
                (ChangeKind::Added, _) => ChangeKind::Added,
                (ChangeKind::Removed, ChangeKind::Removed) => ChangeKind::Removed,
                (ChangeKind::Removed, _) => ChangeKind::Modified,
                (ChangeKind::Modified, ChangeKind::Removed) => ChangeKind::Removed,
                (ChangeKind::Modified, _) => ChangeKind::Modified,
            };
        } else {
            change_slots.insert(handle, changes.len());
            changes.push((handle, next_kind));
        }
    }

    let mut runs = (*first.runs).clone();
    for &(handle, kind) in second.changes.iter() {
        if matches!(kind, ChangeKind::Removed) {
            runs.remove(&handle);
        } else if let Some(run) = second.runs.get(&handle) {
            runs.insert(handle, Arc::clone(run));
        } else {
            runs.remove(&handle);
        }
    }

    let mut index_slots =
        StdHashMap::with_capacity(first.index_edits.len() + second.index_edits.len());
    let mut index_edits = Vec::with_capacity(first.index_edits.len() + second.index_edits.len());
    for &edit in first.index_edits.iter() {
        index_slots.insert(edit.handle, index_edits.len());
        index_edits.push(edit);
    }
    for &edit in second.index_edits.iter() {
        if let Some(&slot) = index_slots.get(&edit.handle) {
            index_edits[slot].new_len = edit.new_len;
            index_edits[slot].visible = edit.visible;
        } else {
            index_slots.insert(edit.handle, index_edits.len());
            index_edits.push(edit);
        }
    }

    WireGpuPatch {
        changes: Arc::new(changes),
        runs: Arc::new(runs),
        index_edits: Arc::new(index_edits),
        new_handles_are_suffix: first.new_handles_are_suffix && second.new_handles_are_suffix,
        face_pass_changed: first.face_pass_changed || second.face_pass_changed,
        text_pass_changed: first.text_pass_changed || second.text_pass_changed,
    }
}

#[cfg(test)]
mod wire_gpu_patch_tests;

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
pub(in crate::scene) struct DrawDepthEntry {
    pub(in crate::scene) handle: u64,
    pub(in crate::scene) effective: u64,
    pub(in crate::scene) label: i32,
    pub(in crate::scene) half_label: i32,
}

// Stable order-maintenance labels. Initial builds occupy the middle half of
// this space, leaving 200 million units at either end. New CAD handles are
// monotonic, so ordinary Add extends with the existing label stride without
// changing any sibling's clip-z bias. The large integer domain preserves enough
// f32 separation for nested block children even in very large drawings.
// Deleted entries remain as tombstones so Undo/Redo restores the same slot.
pub(in crate::scene) const DRAW_DEPTH_LABEL_MIN: i32 = -1_000_000_000;
pub(in crate::scene) const DRAW_DEPTH_LABEL_MAX: i32 = 1_000_000_000;
pub(in crate::scene) const DRAW_DEPTH_INITIAL_MIN: i32 = -800_000_000;
pub(in crate::scene) const DRAW_DEPTH_INITIAL_MAX: i32 = 800_000_000;
pub(in crate::scene) const DRAW_DEPTH_LABEL_SCALE: f32 = 1.0 / 1_073_741_824.0;
// Label stride for a block that starts empty and grows by Add: wide enough for
// the depth buffer to keep neighbours apart (about 80 steps of a 24-bit buffer
// at the default draw-order scale), and narrow enough to leave room for
// thousands of Adds before a full rebuild.
pub(in crate::scene) const DRAW_DEPTH_EMPTY_BLOCK_STRIDE: i32 = 1 << 18;

/// Labels map into (0, 2): draw order only ever moves 2D geometry towards
/// the camera. A negative bias pushed lines lying on a solid's face behind
/// that face in shaded views (#3119); 0 stays the true depth of 3D geometry.
/// The scale stays 2^-30 so neighbours (and a block's text over its own
/// wipeout) keep their depth separation (#3138).
pub(in crate::scene) fn draw_depth_value(label: i32, half_label: i32) -> [f32; 2] {
    [
        (label as i64 + (1_i64 << 30)) as f32 * DRAW_DEPTH_LABEL_SCALE,
        half_label as f32 * DRAW_DEPTH_LABEL_SCALE,
    ]
}

pub(in crate::scene) fn seed_draw_depth_entries(order: Vec<(u64, u64)>) -> Vec<DrawDepthEntry> {
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

pub(in crate::scene) fn inserted_draw_depth_label(order: &[DrawDepthEntry], position: usize) -> Option<(i32, i32)> {
    match (
        position.checked_sub(1).and_then(|index| order.get(index)),
        order.get(position),
    ) {
        // A block that starts empty grows from the low end of the initial range
        // by a fixed stride. A (0, 1) seed spaced a new drawing's entities two
        // labels apart, too close for the depth buffer, so draw order, masks
        // and wipeouts had no effect in it.
        (None, None) => Some((DRAW_DEPTH_INITIAL_MIN, DRAW_DEPTH_EMPTY_BLOCK_STRIDE / 2)),
        (Some(left), None) => {
            // With a single entry there is no previous gap to repeat (index
            // `position - 2` would be `left` itself, giving a zero stride), so
            // fall back to the entry's reserved half gap.
            let stride = position
                .checked_sub(2)
                .and_then(|index| order.get(index))
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

/// The entry of `handle` at its own place in a block's order.
///
/// The order is sorted by effective key, then by handle, and an entity's key
/// is its own handle unless the block's sort table files it under another. So
/// the entry of an entity the table left alone is where a search for the
/// handle as both key and handle ends, with no walk through the order.
pub(in crate::scene) fn draw_depth_entry_at_own_key(order: &[DrawDepthEntry], handle: u64) -> Option<&DrawDepthEntry> {
    order
        .binary_search_by_key(&(handle, handle), |entry| (entry.effective, entry.handle))
        .ok()
        .map(|index| &order[index])
}

/// The entry of `handle` in a block's order: at its own place, or, for an
/// entity the sort table moved, wherever in the order that put it.
pub(in crate::scene) fn draw_depth_entry(order: &[DrawDepthEntry], handle: u64) -> Option<&DrawDepthEntry> {
    draw_depth_entry_at_own_key(order, handle)
        .or_else(|| order.iter().find(|entry| entry.handle == handle))
}

#[derive(Clone)]
pub(crate) struct DrawDepthCache {
    pub(in crate::scene) epoch: u64,
    pub(in crate::scene) depths: Arc<HashMap<u64, [f32; 2]>>,
    /// Per-block stable order labels, including deleted tombstones retained for
    /// symmetric Undo/Redo.
    pub(in crate::scene) blocks: HashMap<Handle, Vec<DrawDepthEntry>>,
    /// Reverse owner lookup retained across removal for tombstone restoration.
    pub(in crate::scene) owners: HashMap<u64, Handle>,
}

pub(in crate::scene) struct PaperViewportCache {
    pub(in crate::scene) epoch: u64,
    pub(in crate::scene) layout: String,
    pub(in crate::scene) layout_block: Handle,
    pub(in crate::scene) sheet: Handle,
    pub(in crate::scene) content: Arc<Vec<Handle>>,
    pub(in crate::scene) paper_limits: Option<((f64, f64), (f64, f64))>,
}

pub(in crate::scene) struct PaperSheetRenderCache {
    pub(in crate::scene) epoch: u64,
    pub(in crate::scene) layout: String,
    pub(in crate::scene) selected: u64,
    pub(in crate::scene) paper_bg: [f32; 4],
    pub(in crate::scene) hatches: Arc<Vec<HatchModel>>,
    pub(in crate::scene) wipeouts: Arc<Vec<HatchModel>>,
    pub(in crate::scene) images: Arc<Vec<ImageModel>>,
}

/// Bound on the geometry-delta ring. A consumer that fell more than this many
/// mutations behind (or predates the oldest retained delta) can't be replayed
/// and does a one-time full rebuild — the safe fallback, not a correctness hole.
pub(in crate::scene) const GEOMETRY_JOURNAL_CAP: usize = 256;

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

/// `raw` made fit to name a table entry (a block, layer, style or view):
/// characters [`valid_block_name`] refuses become `_`, the name is cut to
/// 255 characters, and `fallback` stands in when nothing is left. For names
/// that come from outside files.
pub(crate) fn sanitized_table_name(raw: &str, fallback: &str) -> String {
    let name: String = raw
        .trim()
        .chars()
        .map(|c| {
            if valid_block_name(c.encode_utf8(&mut [0; 4])) {
                c
            } else {
                '_'
            }
        })
        .take(255)
        .collect();
    block_service::validate_block_name(&name).unwrap_or_else(|_| fallback.to_string())
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
    pub render_mode: document::entities::ViewportRenderMode,
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
    /// `true` for the second sheet pass, drawn after the content viewports:
    /// the paper-space entities that come after the first content viewport
    /// in draw order, so they cover what the viewports show.
    pub sheet_over: bool,
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
    pub(crate) render_mode: document::entities::ViewportRenderMode,
    /// Grid display + grid-snap for this viewport alone, round-tripped through
    /// its VPort entry. The app mirrors the *active* tile's pair into the live
    /// grid/snap toggles. (#121)
    pub(crate) grid_on: bool,
    pub(crate) snap_on: bool,
}

#[derive(Clone)]
pub(in crate::scene) enum ViewSnapshot {
    Main {
        layout: String,
        tile: usize,
        camera: Camera,
    },
    Floating {
        layout: String,
        handle: Handle,
        viewport: Box<document::entities::Viewport>,
    },
}

/// Gap (pixels) between Model panes — the `pane_grid` spacing and the visible
/// divider width. The renderer derives tile rects through this same spacing so
/// the drawn viewports line up exactly with the pane_grid layout.
pub const TILE_DIVIDER_PX: f32 = 2.0;
/// Extra grab width around a Model-pane divider where its resize arrows
/// show (split across both sides).
pub const TILE_RESIZE_LEEWAY_PX: f32 = 6.0;

/// Minimum used until the viewport render-mode bar reports its natural width.
pub const MODEL_PANE_MIN_FALLBACK_PX: f32 = 50.0;


#[derive(Clone, Copy, Debug)]
pub(crate) enum NavPerfOp {
    Pan,
    Zoom,
    Rotate,
    /// A message that changed geometry before the next rendered frame.
    Edit,
}

impl NavPerfOp {
    pub(in crate::scene) fn label(self) -> &'static str {
        match self {
            Self::Pan => "pan",
            Self::Zoom => "zoom",
            Self::Rotate => "rotate",
            Self::Edit => "edit",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::scene) struct NavPerfSample {
    pub(in crate::scene) op: NavPerfOp,
    /// Message label that caused the sample.
    pub(in crate::scene) cause: &'static str,
    pub(in crate::scene) space: &'static str,
    pub(in crate::scene) mode: &'static str,
    pub(in crate::scene) started: iced::time::Instant,
    pub(in crate::scene) input_ms: f64,
    pub(in crate::scene) build_ms: f64,
}

#[derive(Clone)]
pub(in crate::scene) struct BlockMeshInherit {
    pub(in crate::scene) insert_color: [f32; 4],
    pub(in crate::scene) layer0_color: [f32; 4],
    pub(in crate::scene) insert_material: crate::scene::model::material_model::MeshMaterial,
    pub(in crate::scene) layer0_material: crate::scene::model::material_model::MeshMaterial,
}

#[derive(Clone)]
pub(in crate::scene) struct SceneLight {
    pub(in crate::scene) handle: Handle,
    /// Present when `color` must follow the current ByLayer color.
    pub(in crate::scene) color_layer: Option<String>,
    pub(in crate::scene) light_type: f32,
    pub(in crate::scene) position: [f64; 3],
    pub(in crate::scene) direction: [f32; 3],
    pub(in crate::scene) color: [f32; 3],
    pub(in crate::scene) intensity: f32,
    pub(in crate::scene) hotspot_cos: f32,
    pub(in crate::scene) falloff_cos: f32,
    pub(in crate::scene) attenuation_type: f32,
    pub(in crate::scene) attenuation_start: f32,
    pub(in crate::scene) attenuation_end: f32,
    pub(in crate::scene) cast_shadows: bool,
    pub(in crate::scene) shadow_softness: f32,
    pub(in crate::scene) shadow_map_size: u32,
    pub(in crate::scene) web_profile: [f32; 8],
    pub(in crate::scene) web_rotation: [f32; 3],
    pub(in crate::scene) web_enabled: bool,
}

/// One-shot presentation-cache refresh scope for REDRAW/REDRAWALL. Resolved to
/// concrete `ViewportInstance::instance_id`s at request time; `All` is the
/// superset and dominates. `Active` = current-input viewport (paper sheet in
/// paper space with nothing entered, D6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewportRefreshScope {
    Active,
    All,
}

/// Entity membership in document order, keyed by geometry epoch.
pub(in crate::scene) type BlockMembers = (u64, HashMap<Handle, Vec<Handle>>);

/// What count mode colours: see [`Scene::count_display`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CountDisplay {
    pub counted: HashSet<Handle>,
    pub errors: HashSet<Handle>,
    pub color: [f32; 4],
    pub error_color: [f32; 4],
}

/// Selected handles, picked segments and the selected-only annotation
/// highlight built for them.
pub(in crate::scene) type AnnotationSelectedEntry = (
    HashSet<Handle>,
    HashMap<Handle, std::collections::BTreeSet<usize>>,
    Arc<Vec<WireModel>>,
);

pub struct Scene {
    pub camera: Rc<RefCell<Camera>>,
    /// View saved immediately before the latest navigation operation. ZOOM
    /// Previous swaps with this snapshot, allowing the user to toggle between
    /// the two most recent views without touching drawing history.
    previous_view: Option<ViewSnapshot>,
    /// Model-space tiled viewport layout. One full-window tile by default;
    /// the split buttons / VPORTS subdivide the active tile.
    pub(crate) model_tiles: RefCell<Vec<ModelTile>>,
    /// Index of the active model tile (camera input + overlays target it).
    pub(crate) active_model_tile: std::cell::Cell<usize>,
    pub(in crate::scene) caches: SceneCaches,
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
    pub selection: std::sync::Arc<std::cell::RefCell<SelectionState>>,
    /// The CAD document — single source of truth for all entities.
    pub document: Document,
    /// Last chain-compatible dimension created this session.
    /// Never reconstructed from loaded file handles.
    pub last_created_dimension: Option<Handle>,
    /// File-open-prepared index for non-graphical semantic object lookups.
    pub(crate) object_data_cache: crate::entities::object_data::ObjectDataCache,
    /// The visibility state each dynamic block shows in the Block Editor,
    /// by block record.
    pub visibility_states: HashMap<Handle, usize>,
    pub(crate) block_visibility_reveal: bool,
    pub(crate) block_construction_hidden: rustc_hash::FxHashSet<Handle>,
    pub(crate) block_constraint_status_mode: bool,
    block_constraint_status_colors: [String; 4],
    constraint_status_suppressed: std::cell::Cell<bool>,
    /// Currently selected entity handles.
    pub selected: HashSet<Handle>,
    /// The selection as it stood when the last command started, so
    /// SELECTPREVIOUS can hand it back. Kept in pick order.
    previous_selected: Vec<Handle>,
    /// Polylines only part of which is selected (Ctrl+click): the picked
    /// segment indices.
    pub sub_segments: HashMap<Handle, std::collections::BTreeSet<usize>>,
    selected_order: Vec<Handle>,
    /// The parametric-constraint glyph currently selected in the viewport.
    /// Entity selection changes clear it so the two selections stay exclusive.
    pub selected_constraint: Option<crate::scene::parametric_constraints::ConstraintId>,
    /// Cells picked inside a table (the table itself is then in `selected`
    /// but draws neither its highlight nor its grips).
    pub table_cells: Option<crate::scene::TableCellSelection>,
    /// Session-only ISOLATEOBJECTS / HIDEOBJECTS state. Never written to DWG/DXF.
    pub object_isolation: ObjectIsolationState,
    /// Session-only LAYWALK per-space visible-layer sets. In-memory only;
    /// never serialized and never touches the layer table, so undo is
    /// unaffected and a document replace drops it with the rest of the
    /// transient visibility.
    pub layer_walk: LayerWalkState,
    /// Scene-scoped reference-only designations (API v9): plugin-owned
    /// display geometry the built-in edit paths refuse. In-memory only,
    /// keyed by handle, lives and dies with this scene — never written to
    /// DWG/DXF (see `scene::reference_only`).
    pub(crate) reference_only: reference_only::ReferenceOnlyDesignation,
    /// Entity handles temporarily removed from the base render while an
    /// interactive grip preview draws their live replacement.
    /// Separate from object isolation so a grip can never activate the
    /// isolation status or be captured in its undo state.
    pub preview_hidden: HashSet<Handle>,
    /// The table whose fill grip is being dragged: its picked range stays
    /// drawn over the grip preview that hides it.
    pub table_fill_drag: Option<Handle>,
    /// The cells the table fill being dragged copied from field cells, made
    /// fields again when the drag is committed.
    pub table_fill_fields: Option<(Handle, Vec<crate::entities::table::FillPair>)>,
    /// The table whose cell is open in the in-place editor: the editor's
    /// picture hides it, but its row and column labels stay drawn.
    pub table_cell_editing: Option<Handle>,
    /// Source handles replaced by an active command preview. Kept separate
    /// from grip state so either interaction can clean up only its own hides.
    command_preview_hidden: HashSet<Handle>,
    /// Existing MTEXT replaced by an in-place editor draft.
    mtext_editor_hidden: Option<Handle>,
    /// During in-place block edit (REFEDIT), the handles of the entities being
    /// edited. Everything else is rendered faded toward the background so the
    /// edited geometry stands out while the surrounding drawing stays visible
    /// for context. `None` = not editing. (#136)
    pub refedit_keep: Option<HashSet<Handle>>,
    /// REFEDIT locks the objects outside the working set (`refedit_keep`).
    pub refedit_lock: bool,
    /// COUNT mode colours: counted references in COUNTCOLOR, overlapping
    /// duplicates in COUNTERRORCOLOR, everything else faded. `None` outside
    /// count mode.
    pub count_display: Option<CountDisplay>,
    /// Entity drawn with the selection-highlight colour without being part
    /// of the real selection — used to preview a row in the cycling list box.
    pub hover_highlight: Option<Handle>,
    /// The raw selectable-group expansion of `hover_highlight`, kept in sync
    /// by [`Scene::set_hover_highlight`]. Storing it lets
    /// [`Scene::hover_highlight_handles`] — called every rendered frame —
    /// reuse the expansion instead of re-running the group scan, and lets
    /// `set_hover_highlight` compare old vs new contribution without a second
    /// expansion pass.
    hover_expanded: HashSet<Handle>,
    /// All entities related to the constraint indicator currently under the cursor.
    constraint_hover_highlights: HashSet<Handle>,
    /// Entities a selection box still being drawn would catch on release
    /// (SELECTIONPREVIEW), expanded to their selectable groups.
    box_hover_highlights: HashSet<Handle>,
    /// Objects a selection filter would find, shown before it is applied.
    filter_preview_highlights: HashSet<Handle>,
    /// Sub-entity references related to that indicator. Polyline segments are
    /// drawn separately so hovering one constraint does not light the whole chain.
    constraint_hover_refs: Vec<crate::scene::parametric_constraints::ParametricRef>,
    constraint_hover_wires: Vec<WireModel>,
    /// Dynamic UCS: the outline of the planar solid face a first point
    /// would align the UCS to, with the solid and face it outlines.
    ducs_face: Option<(Handle, u32)>,
    ducs_face_wires: Vec<WireModel>,
    /// Control nets shown by CVSHOW, by surface.
    cv_net_wires: HashMap<Handle, Vec<WireModel>>,
    /// Color used to draw the selection highlight overlay wires (SELECTIONEFFECTCOLOR).
    pub selection_color: [f32; 4],
    /// Whether selection visual effect (glow/highlight) is enabled (SELECTIONEFFECT).
    pub selection_effect: bool,
    /// Extra screen pixels the selection highlight is drawn wider.
    pub selection_widen: f32,
    /// VSEDGECOLOR: edge colour of the shaded-with-edges modes (`None`: black).
    pub edge_color: Option<[u8; 3]>,
    /// VSOCCLUDEDEDGES: HiddenLine draws the edges behind faces dashed.
    pub occluded_edges: bool,
    /// Whether entity transparency is honoured on screen. When false the
    /// wire shader forces every line opaque (a uniform toggle, no retessellate).
    pub transparency_display: bool,
    /// User-adjustable model-space lineweight preview multiplier.
    pub model_lineweight_scale: f32,
    /// Selection filter: entity-type names excluded from interactive picking.
    /// Empty = every type is selectable.
    pub selection_filter: HashSet<String>,
    /// In-progress preview wires while a command is active (rubber-band + object ghosts).
    pub preview_wires: Vec<WireModel>,
    /// Candidate wireframes produced by editable solid-history grips. The
    /// resident body remains untouched until the grip is committed.
    solid_history_preview_wires: HashMap<Handle, Vec<WireModel>>,
    /// Live hatch fill used by interactive edits such as dragging the pattern
    /// origin grip. Kept separate from the resident hatch set so moving one
    /// pattern never re-uploads every hatch in a large drawing.
    pub preview_hatches: Arc<Vec<HatchModel>>,
    /// Cached scoped document and last evaluated parameter preview for the
    /// active dynamic block grip drag.
    dynamic_preview_cache: Option<preview::block_parameter::DynamicPreviewCache>,
    /// In-progress preview SDF glyph quads (grip drag / command preview). Rides
    /// a per-frame GPU buffer separate from the epoch-cached base text so the
    /// dragged text stays visible while it's hidden from the base set (#316).
    pub preview_text: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
    /// Committed-segment wire drawn during multi-point commands (normal colour).
    pub interim_wire: Option<WireModel>,
    /// Set when an entity creation or update automatically registers a novel
    /// layer into the document layer table, signalling the UI to refresh.
    pub layer_table_dirty: bool,
    pub camera_generation: u64,
    /// Incremented whenever geometry-affecting state changes (entities, selection,
    /// preview wires, layer visibility, layout). The GPU pipeline uses this to
    /// skip re-uploading unchanged geometry buffers every frame.
    pub geometry_epoch: u64,
    /// Geometry epoch represented by the model bounds cached on the live
    /// camera. Projection and orientation changes refresh those bounds through
    /// the same filtered fit path when this falls behind `geometry_epoch`.
    projection_bounds_epoch: std::cell::Cell<u64>,
    /// Separate epoch for the (expensive) block-definition tessellation cache.
    /// Bumped together with `geometry_epoch` by `bump_geometry`, but NOT by
    /// `bump_geometry_no_blocks` — so edits that provably can't change any
    /// block definition (drawing a top-level entity, grip-moving an
    /// entity/insert) re-tessellate only the visible wires (~baseline cost)
    /// instead of rebuilding every block defn (the edit-time spike).
    pub block_epoch: u64,
    /// Process-wide identity of this scene, so that viewports of different
    /// documents sharing the renderer keep separate GPU slots (#2304).
    render_id: u64,
    render_lifetime: Arc<()>,
    /// Layer visibility changes since a `block_epoch` (`from`, `to`, and the
    /// definitions they touched): a cache built at `from` is brought to `to`
    /// by rebuilding only those definitions, not all of them (#2696).
    block_visibility_edits: Option<(u64, u64, HashSet<String>)>,
    /// Incremented when the selection / hover-highlight set changes WITHOUT a
    /// geometry change. The wire tessellation is selection-independent, so a
    /// pick only refreshes the GPU xray overlay (cheap) instead of bumping
    /// `geometry_epoch` and re-tessellating the whole model.
    pub selection_generation: u64,
    /// Incremented when only the *hover / box-preview / constraint / filter*
    /// highlight set changes, with no change to the committed selection.
    /// The xray overlay splits its caches on the two generations: a rollover
    /// or marquee tick rebuilds only the hover half, never the committed
    /// selection's GPU buffers (which are sized by the whole selection).
    pub hover_generation: u64,
    /// Bumped whenever a group object is created, edited, or removed, and by
    /// undo / redo (which can restore any of them). Invalidates
    /// [`Scene::groups_snapshot`] so the next rollover sees the new groups.
    pub groups_epoch: u64,
    /// Cached fingerprint of `selected`, recomputed lazily after mutations.
    #[cfg(any(test, not(target_arch = "wasm32")))]
    selection_fingerprint_cache: u64,
    selection_fingerprint_dirty: bool,
    /// Latest large source requested for off-thread preparation. A matching
    /// cache miss returns no candidates temporarily instead of scanning the
    /// entire scene on the UI thread.
    interaction_index_pending_key: std::cell::Cell<Option<(u64, usize)>>,
    /// Bumped when a full draw-order rebuild can move existing depths.
    /// Cached uploads compare it before reusing baked depth values.
    draw_depth_generation: std::cell::Cell<u64>,
    /// Shared empty map for 3-D wireframe, where true depth wins instead of
    /// the entity submission order used by the optimized 2-D style.
    no_draw_depths: Arc<HashMap<u64, [f32; 2]>>,
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
    /// GPU render data for solid meshes (the kernel Shell/Solid tessellation).
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
    /// Kernel B-reps used by solid operations and exact-geometry saves.
    pub solid_models: HashMap<Handle, ocs_solid::model::Brep>,
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
    /// The desk around a layout's sheet (Options › Display).
    pub desk_bg_color: [f32; 4],
    /// Dense model-space cluster half-span used for viewport recovery.
    pub local_extent_max: f32,
    /// Dense model-space cluster median used for viewport recovery.
    pub local_center: [f64; 2],
    /// Current annotation scale (CANNOSCALE equivalent).
    /// Multiplier applied to Text/MText/Dimension sizes during tessellation.
    /// 1.0 = no scaling. 50.0 = "1:50" drawing scale.
    pub annotation_scale: f32,
    /// Dimension variables typed at the command line (DIMSCALE, DIMDEC, …):
    /// overrides of the named current dimension style that new dimensions
    /// take. Dropped once another style is current.
    pub dim_overrides: Option<(String, Vec<(i16, document::xdata::XDataValue)>)>,
    /// Cached per-epoch: does annotation scale actually change wire geometry?
    /// PSLTSCALE is handled by a per-viewport GPU uniform, so only real
    /// annotative entities require a scale-specific resident set.
    annotation_affects_wires: std::cell::Cell<Option<(u64, bool)>>,
    /// Memoised [`Scene::model_space_block_handle`] answer, keyed by the
    /// structural state it is derived from: the geometry epoch (a coarse
    /// structural change can replace the whole document), the layout epoch
    /// (Layout objects inserted/removed/renamed/reordered) and the object
    /// count (a direct `document.objects` insert/remove). The scene asks for
    /// the handle from 18 call sites, several per frame, and the DWG lookup
    /// walks every object until it finds the "Model" layout.
    model_space_block_cache: std::cell::Cell<Option<(u64, u64, usize, Handle)>>,
    /// Bump on any Layout object insert/remove/rename/reorder.
    layout_epoch: u64,
    /// Layouts created in this session and not opened yet: the first time
    /// one opens it gets the LAYOUTCREATEVIEWPORT viewport.
    pub(crate) unopened_layouts: std::collections::HashSet<String>,
    /// The plot style table a layout's page setup takes when it is
    /// initialized on first opening.
    pub(crate) new_layout_plot_style: String,
    /// Bump on any annotation-scale list mutation.
    scale_epoch: u64,
    /// Bump on any named-UCS table mutation (UCS SAVE / DELETE / RENAME,
    /// record-API patch, file-open, undo Full-swap).
    ucs_epoch: u64,
    /// Tables added or changed since the drawing was opened or last saved:
    /// their stored cell geometry is marked out of date (flag 7) in the
    /// next save.
    tables_modified: HashSet<Handle>,
    /// Messages for the command line about centre objects an edit just
    /// detached from their sources; the app prints and clears them.
    pub(crate) center_notices: Vec<String>,
    /// Conservative association hint: unknown until scanned, then updated from
    /// changed entities. Retaining `true` after deletion only costs an extra scan.
    has_associative_centers: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any DIMASSOC dimension association exists.
    /// Unknown until scanned; creating one sets it, and a coarse structural
    /// change re-opens the question. Lets most drawings skip the per-edit
    /// `document.objects` scan in `refresh_associative_dimensions`.
    has_dimension_associations: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any Underlay entities exist in the document.
    pub(crate) has_underlays: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any OLE2FRAME entities exist in the document.
    pub(crate) has_ole_frames: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any Face3D entities exist in the document.
    pub(crate) has_face3d: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any section object exists in the
    /// document. The live-section lookup runs on every resident patch.
    has_section_objects: std::cell::Cell<Option<bool>>,
    /// Conservative hint for whether any table exists in the document. The
    /// table-cell pick runs on every click.
    has_tables: std::cell::Cell<Option<bool>>,
    /// Whether the current space holds objects the drawing saves as invisible,
    /// keyed by `(geometry_epoch, space block)`. Read every frame by the
    /// isolation menus, so the entity scan runs once per edit, not per frame.
    persisted_hidden_cache: std::cell::Cell<Option<(u64, Handle, bool)>>,
    /// Runtime parametric constraint sets decoded from standard graph scopes.
    pub(crate) parametric_constraints: Vec<parametric_constraints::ParametricConstraintSet>,
    /// Handles a constraint addition's solve moved that the command did not
    /// name. The result driver clears the list at the outermost result and
    /// drains it once that result is applied; a command that adds constraints
    /// without a result drains it itself.
    pub(crate) constraint_network_moves: Vec<Handle>,
    /// Scratch list a solve fills with the entities it moved, whether or not
    /// anyone asked for a report. Only a constraint addition copies it into
    /// `constraint_network_moves`; every other solve drops it.
    solve_moved: Vec<Handle>,
    /// CONSTRAINTNAMEFORMAT: what a dynamic dimension's text shows —
    /// 0 the parameter name, 1 the value, 2 `name=value`.
    pub constraint_name_format: u8,
    /// DYNCONSTRAINTDISPLAY: whether dynamic dimensions draw at all.
    pub dynamic_constraint_display: bool,
    /// Dynamic dimensions DCHIDE or DYNCONSTRAINTDISPLAY 0 keep off screen.
    hidden_dynamic_dimensions: HashSet<Handle>,
    /// The camera generation the dynamic dimensions were last scaled for.
    dynamic_dimension_camera_gen: Option<u64>,
    /// The view scale the rasters of the underlays of model space are made
    /// for: the model view's, or with a sheet in view that of its viewports
    /// (`sheet_model_view`).
    underlay_scale: raster_scale::RasterScale,
    /// The view scale of the current layout, which its OLE pictures and the
    /// underlays on its sheet are made for.
    ole_scale: raster_scale::RasterScale,
    /// The rasters a zoom step asks a worker to draw (`raster_reseed`).
    pending_reseed: Option<raster_reseed::ReseedJob>,
    /// Rasters `apply_reseed` just took: the bump it makes does not draw them
    /// again.
    fresh_images: HashSet<Handle>,
    /// Session-only visibility overrides for constraint glyphs.
    hidden_parametric_constraints: HashSet<(
        parametric_constraints::ParametricScope,
        parametric_constraints::ConstraintId,
    )>,
    /// Constraint glyphs explicitly shown through the constraint-bar commands.
    shown_parametric_constraints: HashSet<(
        parametric_constraints::ParametricScope,
        parametric_constraints::ConstraintId,
    )>,
    /// Bumped on every parametric-constraint-set mutation (add / remove /
    /// metadata edit / visibility override / whole-set restore). The
    /// constraint-glyph memoization keys on this so a stale cache can never
    /// outlive the edit that invalidated it — a missed bump is a
    /// correctness bug (stale glyphs), not just a perf miss.
    constraints_epoch: u64,
    /// Document-wide named-parameter and expression table decoded from
    /// standard associative variables.
    pub(crate) named_parameters: named_parameters::ParameterTable,
    /// Content hash of the constraint model as of the last
    /// `sync_native_parametric_graph`, letting geometry-only edits skip the
    /// graph rebuild. `None` until the first sync records one.
    native_graph_fingerprint: Option<u64>,
    /// Last viewport aspect ratio captured by the render pipeline. Used by
    /// `view_world_aabb` to compute the world-space view rect on demand.
    last_render_aspect: std::cell::Cell<f32>,
    /// World units that map to one screen pixel at the current camera +
    /// viewport size, captured each render. Drives the LOD pixel-size cull
    /// in expand_insert / tessellate_entity. 0 means "not yet set" — culling
    /// falls back to None.
    last_world_per_pixel: std::cell::Cell<f32>,
    /// Width over height of the view last drawn.
    last_view_aspect: std::cell::Cell<f32>,
    /// ViewCube hover region (0..25, face/edge/corner index), driven by the
    /// `CursorMoved` message that the cube hit-area overlay publishes. Lives
    /// here so the unified render path can read it for the active viewport
    /// without depending on the shader widget's internal `Program::State`
    /// (which can miss events under overlapping overlays).
    pub viewcube_hover: std::cell::Cell<Option<usize>>,
    /// The cube's opacity this frame: 1 while the cursor is over the
    /// navigation corner, else NAVVCUBEOPACITY.
    pub viewcube_opacity: std::cell::Cell<f32>,
    /// Wall time (ms) of the most recent wire re-tessellation
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
    /// Minimum protected resident-wire count during transient off-screen layout
    /// switches so capturing another layout cannot evict live viewports.
    pub(super) transient_prune_floor: std::cell::Cell<usize>,
    /// SDF glyph-atlas generation this scene's caches were built against.
    /// When the atlas grows or re-bakes, every cached glyph quad (resident
    /// sets, block caches, projected viewport copies) addresses the wrong
    /// tile; `update` compares against the live generation and rebuilds.
    pub(crate) last_atlas_generation: std::cell::Cell<u64>,
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
    /// Hash of the tessellation parameters `tess_memo` was built under. When
    /// the current call's parameters differ, the memo is stale and cleared.
    tess_memo_guard: std::cell::Cell<u64>,
    /// Set while `resident_wires_for` assembles a set that will become the
    /// memo's backing; new runs then go straight into it (`Pending`).
    building_resident_set: std::cell::Cell<bool>,
    /// The factor on complex linetypes in the viewport whose resident set is
    /// being read or built. `None` outside one, or where nothing depends on it.
    viewport_complex_lt: std::cell::Cell<Option<f32>>,
    /// Annotation, background and viewport signature.
    resident_tess_guard: std::cell::Cell<u64>,
    /// The SDF atlas generation the resident memo's text was built under.
    resident_tess_atlas: std::cell::Cell<u64>,
    /// The same for `tess_memo`.
    tess_memo_atlas: std::cell::Cell<u64>,
    /// The complex-linetype factor ([`Scene::viewport_complex_lt_guard`]) the
    /// resident memo's complex-linetype runs were built under.
    resident_tess_complex_lt: std::cell::Cell<u64>,
    /// The same for `tess_memo`.
    tess_memo_complex_lt: std::cell::Cell<u64>,
    /// Highest epoch whose delta has been evicted from the ring. A consumer
    /// synced older than this can't be caught up incrementally → full rebuild.
    geometry_journal_floor: std::cell::Cell<u64>,
    /// Count of resident-set rebuilds served by the incremental patch rather than
    /// a full re-tessellation. Diagnostics + a hook for the oracle test to prove
    /// the fast path is actually exercised (not silently always falling back).
    resident_patch_hits: std::cell::Cell<u64>,
    /// Count of resident sets that were assembled whole, because no patch
    /// could bring the kept set up to date. With `resident_patch_hits` it
    /// tells a test how a step updated the display.
    resident_full_builds: std::cell::Cell<u64>,
    /// Count of entities the incremental patch has tessellated, whether it
    /// then went in or declined: what a declined patch cost before the full
    /// build did the work again.
    resident_patch_tessellated: std::cell::Cell<u64>,
    /// Active delta-undo recording, or `None` when no entity-only command is
    /// capturing. Populated by the five mutation primitives via
    /// [`Scene::record_undo_before`]; consumed by the app (`take_undo_recording`)
    /// to build a cheap history delta instead of a full document clone. See
    /// [`UndoRecording`].
    undo_recording: Option<UndoRecording>,
}

pub(in crate::scene) fn section_frame(
    data: &document::entities::SectionObjectData,
) -> Option<(glam::DVec3, glam::DVec3, glam::DVec3, glam::DVec3)> {
    let first = data.vertices.first()?;
    let last = data.vertices.last()?;
    let origin = glam::DVec3::new(first.x, first.y, first.z);
    let tangent = (glam::DVec3::new(last.x, last.y, last.z) - origin).try_normalize()?;
    let raw_vertical = glam::DVec3::new(
        data.vertical_direction.x,
        data.vertical_direction.y,
        data.vertical_direction.z,
    );
    let vertical = (raw_vertical - tangent * raw_vertical.dot(tangent)).try_normalize()?;
    let base_view = vertical.cross(tangent).try_normalize()?;
    let viewing = if data.flags & 4 != 0 {
        base_view
    } else {
        -base_view
    };
    Some((origin, tangent, vertical, viewing))
}

/// A section object entity (live or not); see `Scene::active_live_section`.
pub(in crate::scene) fn is_section_object(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Extended(extended)
            if matches!(
                extended.data,
                document::entities::ExtendedEntityData::SectionObject(_)
            )
    )
}

#[cfg(test)]
mod section_hint_tests;

#[derive(Clone)]
pub(in crate::scene) struct LiveSection {
    pub(in crate::scene) data: document::entities::SectionObjectData,
    pub(in crate::scene) slice_depth: Option<f64>,
}

pub(in crate::scene) fn section_plane(
    origin: glam::DVec3,
    tangent: glam::DVec3,
    normal: glam::DVec3,
    body_to_world: Option<&document::types::Transform>,
) -> Option<ocs_geom::space::Plane> {
    let Some(transform) = body_to_world else {
        return ocs_geom::space::Plane::orthonormal(
            origin.to_array(),
            tangent.to_array(),
            normal.to_array(),
        );
    };
    let matrix = transform.matrix.m;
    let linear = document::types::Matrix3::from_rows(
        [matrix[0][0], matrix[0][1], matrix[0][2]],
        [matrix[1][0], matrix[1][1], matrix[1][2]],
        [matrix[2][0], matrix[2][1], matrix[2][2]],
    );
    let inverse = linear.inverse()?;
    let translation = document::types::Vector3::new(matrix[0][3], matrix[1][3], matrix[2][3]);
    let origin = inverse
        .transform_point(document::types::Vector3::new(origin.x, origin.y, origin.z) - translation);
    let tangent = inverse.transform_point(document::types::Vector3::new(
        tangent.x, tangent.y, tangent.z,
    ));
    let normal = linear
        .transpose()
        .transform_point(document::types::Vector3::new(normal.x, normal.y, normal.z));
    ocs_geom::space::Plane::orthonormal(
        [origin.x, origin.y, origin.z],
        [tangent.x, tangent.y, tangent.z],
        [normal.x, normal.y, normal.z],
    )
}

pub(in crate::scene) fn keep_section_positive(
    body: &ocs_solid::model::Brep,
    plane: ocs_geom::space::Plane,
) -> Result<Option<ocs_solid::model::Brep>, ()> {
    let normal = plane.normal().ok_or(())?;
    match ocs_solid::model::slice(body, plane.origin, normal) {
        Ok(Some((_, front))) => Ok(Some(front)),
        Ok(None) => {
            let bounds = ocs_solid::model::aabb(body).ok_or(())?;
            let centre = [
                (bounds.min[0] + bounds.max[0]) * 0.5,
                (bounds.min[1] + bounds.max[1]) * 0.5,
                (bounds.min[2] + bounds.max[2]) * 0.5,
            ];
            Ok((plane.distance_to(centre).ok_or(())? >= 0.0).then(|| body.clone()))
        }
        Err(_) => Err(()),
    }
}

impl Scene {
    pub fn new() -> Self {
        Self {
            camera: Rc::new(RefCell::new(Camera::default())),
            previous_view: None,
            model_tiles: RefCell::new(vec![ModelTile {
                rect: iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                camera: Camera::default(),
                render_mode: document::entities::ViewportRenderMode::Wireframe2D,
                grid_on: false,
                snap_on: false,
            }]),
            active_model_tile: std::cell::Cell::new(0),
            caches: SceneCaches::new(),
            // One pane mapped to tile 0 — matches the single default tile above.
            model_panes: iced::widget::pane_grid::State::new(0).0,
            model_pane_min_px: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            selection: std::sync::Arc::new(std::cell::RefCell::new(SelectionState::default())),
            document: Document::new(),
            last_created_dimension: None,
            object_data_cache: crate::entities::object_data::ObjectDataCache::default(),
            visibility_states: HashMap::default(),
            selected: HashSet::default(),
            previous_selected: Vec::new(),
            sub_segments: HashMap::default(),
            selected_order: Vec::new(),
            selected_constraint: None,
            table_cells: None,
            object_isolation: ObjectIsolationState::default(),
            layer_walk: LayerWalkState::default(),
            reference_only: Default::default(),
            preview_hidden: HashSet::default(),
            table_fill_drag: None,
            table_fill_fields: None,
            table_cell_editing: None,
            command_preview_hidden: HashSet::default(),
            mtext_editor_hidden: None,
            refedit_keep: None,
            refedit_lock: false,
            count_display: None,
            hover_highlight: None,
            hover_expanded: HashSet::default(),
            constraint_hover_highlights: HashSet::default(),
            box_hover_highlights: HashSet::default(),
            filter_preview_highlights: HashSet::default(),
            constraint_hover_refs: Vec::new(),
            constraint_hover_wires: Vec::new(),
            ducs_face: None,
            ducs_face_wires: Vec::new(),
            cv_net_wires: HashMap::default(),
            selection_color: crate::scene::model::wire_model::WireModel::SELECTED,
            selection_widen: 0.0,
            edge_color: None,
            occluded_edges: false,
            selection_effect: true,
            transparency_display: true,
            model_lineweight_scale: 1.0,
            selection_filter: HashSet::default(),
            preview_wires: vec![],
            solid_history_preview_wires: HashMap::default(),
            preview_hatches: Arc::new(Vec::new()),
            dynamic_preview_cache: None,
            preview_text: vec![],
            interim_wire: None,
            layer_table_dirty: false,
            camera_generation: 0,
            geometry_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            projection_bounds_epoch: std::cell::Cell::new(0),
            block_epoch: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            render_id: GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed),
            render_lifetime: Arc::new(()),
            block_visibility_edits: None,
            selection_generation: 0,
            hover_generation: 0,
            groups_epoch: 0,
            #[cfg(any(test, not(target_arch = "wasm32")))]
            selection_fingerprint_cache: 0,
            selection_fingerprint_dirty: false,
            interaction_index_pending_key: std::cell::Cell::new(None),
            draw_depth_generation: std::cell::Cell::new(0),
            no_draw_depths: Arc::new(HashMap::default()),
            current_layout: "Model".to_string(),
            block_edit_block: None,
            block_visibility_reveal: false,
            block_construction_hidden: Default::default(),
            block_constraint_status_mode: false,
            block_constraint_status_colors: ["8".into(), "2".into(), "3".into(), "1".into()],
            constraint_status_suppressed: std::cell::Cell::new(false),
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
            desk_bg_color: PAPER_DESK_COLOR,
            local_extent_max: 1e9,
            local_center: [0.0, 0.0],
            annotation_scale: 1.0,
            dim_overrides: None,
            annotation_affects_wires: std::cell::Cell::new(None),
            model_space_block_cache: std::cell::Cell::new(None),
            layout_epoch: 0,
            unopened_layouts: Default::default(),
            new_layout_plot_style: String::new(),
            scale_epoch: 0,
            ucs_epoch: 0,
            tables_modified: HashSet::default(),
            center_notices: Vec::new(),
            parametric_constraints: Vec::new(),
            constraint_network_moves: Vec::new(),
            solve_moved: Vec::new(),
            constraint_name_format: 2,
            dynamic_constraint_display: true,
            hidden_dynamic_dimensions: HashSet::default(),
            dynamic_dimension_camera_gen: None,
            underlay_scale: raster_scale::RasterScale::default(),
            ole_scale: raster_scale::RasterScale::default(),
            pending_reseed: None,
            fresh_images: HashSet::default(),
            hidden_parametric_constraints: HashSet::default(),
            shown_parametric_constraints: HashSet::default(),
            constraints_epoch: 0,
            named_parameters: named_parameters::ParameterTable::new(),
            native_graph_fingerprint: None,
            has_associative_centers: std::cell::Cell::new(None),
            has_dimension_associations: std::cell::Cell::new(None),
            has_underlays: std::cell::Cell::new(None),
            has_ole_frames: std::cell::Cell::new(None),
            has_face3d: std::cell::Cell::new(None),
            has_section_objects: std::cell::Cell::new(None),
            has_tables: std::cell::Cell::new(None),
            persisted_hidden_cache: std::cell::Cell::new(None),
            last_render_aspect: std::cell::Cell::new(16.0 / 9.0),
            last_world_per_pixel: std::cell::Cell::new(0.0),
            last_view_aspect: std::cell::Cell::new(0.0),
            viewcube_hover: std::cell::Cell::new(None),
            // NAVVCUBEOPACITY default until the first cursor move.
            viewcube_opacity: std::cell::Cell::new(0.5),
            last_tess_ms: std::cell::Cell::new(0.0),
            last_tess_wires: std::cell::Cell::new(0),
            last_model_wire_gen: std::cell::Cell::new(0),
            transient_prune_floor: std::cell::Cell::new(0),
            last_atlas_generation: std::cell::Cell::new(crate::scene::text::sdf_atlas::generation()),
            nav_last_gen: std::cell::Cell::new(0),
            nav_changed_at: std::cell::Cell::new(None),
            nav_perf_pending: std::cell::Cell::new(None),
            tess_memo_guard: std::cell::Cell::new(0),
            building_resident_set: std::cell::Cell::new(false),
            viewport_complex_lt: std::cell::Cell::new(None),
            resident_tess_guard: std::cell::Cell::new(0),
            resident_tess_atlas: std::cell::Cell::new(0),
            tess_memo_atlas: std::cell::Cell::new(0),
            resident_tess_complex_lt: std::cell::Cell::new(0),
            tess_memo_complex_lt: std::cell::Cell::new(0),
            geometry_journal_floor: std::cell::Cell::new(0),
            resident_patch_hits: std::cell::Cell::new(0),
            resident_full_builds: std::cell::Cell::new(0),
            resident_patch_tessellated: std::cell::Cell::new(0),
            undo_recording: None,
        }
    }
}

mod invalidation;


mod layout_paper;

impl Scene {
}

mod scale_list;


mod resident_wires;

/// What [`Scene::tessellation_context`] resolves: the background colour, the
/// annotation scale and its handle, the block cache, and whether the layout
/// is a paper layout.
pub(in crate::scene) type TessellationContext = (
    [f32; 4],
    f32,
    Option<Handle>,
    Arc<cache::block_cache::BlockCache>,
    bool,
);

#[cfg(test)]
thread_local! {
    /// How many times this thread resolved a tessellation context.
    pub(crate) static TESSELLATION_CONTEXTS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}


/// A page setup never initialized: no paper size and no media name (the
/// reference writes a layout so until it is first opened).
pub fn page_setup_uninitialized(layout: &document::objects::Layout) -> bool {
    layout.paper_size.trim().is_empty() && layout.paper_width <= 0.0 && layout.paper_height <= 0.0
}

/// A layout written as never opened: the page setup fields back to the
/// uninitialized state and no viewports, as the reference stores a layout
/// it has not opened yet. A layout already holding a viewport onto the model
/// (one added by automation) is plainly set up and is written as it is.
pub(crate) fn write_layout_unopened(document: &mut document::Document, layout_name: &str) {
    let block = document.objects.values().find_map(|object| match object {
        ObjectType::Layout(layout) if layout.name == layout_name => Some(layout.block_record),
        _ => None,
    });
    let has_model_view = document.entities().any(|entity| {
        matches!(entity, EntityType::Viewport(viewport)
            if Some(viewport.common.owner_handle) == block
                && !Scene::is_sheet_viewport(document, viewport))
    });
    if has_model_view {
        return;
    }
    let fresh = document::objects::Layout::new(layout_name);
    let mut block = Handle::NULL;
    for object in document.objects.values_mut() {
        if let ObjectType::Layout(layout) = object {
            if layout.name == layout_name {
                block = layout.block_record;
                layout.paper_size = fresh.paper_size.clone();
                layout.paper_width = fresh.paper_width;
                layout.paper_height = fresh.paper_height;
                layout.plot_rotation = fresh.plot_rotation;
                layout.plot_paper_units = fresh.plot_paper_units;
                layout.min_limits = fresh.min_limits;
                layout.max_limits = fresh.max_limits;
                layout.min_extents = fresh.min_extents;
                layout.max_extents = fresh.max_extents;
            }
        }
    }
    drop_layout_viewports(document, block);
}

/// Size a sheet (overall paper) viewport to the paper limits `min`..`max`,
/// framing the whole sheet with a small margin.
pub(in crate::scene) fn fit_sheet_viewport(vp: &mut document::entities::Viewport, min: (f64, f64), max: (f64, f64)) {
    let height = (max.1 - min.1).abs().max(1.0);
    // Paper-space center is an (x, y) point with z = 0.
    let center = document::types::Vector3::new((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0, 0.0);
    vp.center = center;
    vp.width = (max.0 - min.0).abs().max(1.0);
    vp.height = height;
    vp.view_target = center;
    vp.view_center = document::types::Vector3::ZERO;
    vp.view_height = height * 1.1;
}

/// Takes a layout's sheet (overall paper) viewport out of its block (entity
/// and membership). Viewports onto the model are the user's drawing and stay:
/// a file may carry them on a layout whose page setup was never filled in.
pub(crate) fn drop_layout_viewports(document: &mut document::Document, block: Handle) {
    if block.is_null() {
        return;
    }
    let viewports: Vec<Handle> = document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Viewport(viewport)
                if viewport.common.owner_handle == block
                    && Scene::is_sheet_viewport(document, viewport) =>
            {
                Some(viewport.common.handle)
            }
            _ => None,
        })
        .collect();
    for handle in &viewports {
        document.remove_entity(*handle);
    }
    if let Some(record) = document
        .block_records
        .iter_mut()
        .find(|record| record.handle == block)
    {
        record
            .entity_handles
            .retain(|handle| !viewports.contains(handle));
    }
    for object in document.objects.values_mut() {
        if let ObjectType::Layout(layout) = object {
            if layout.block_record == block {
                layout
                    .viewports
                    .retain(|handle| !viewports.contains(handle));
                if viewports.contains(&layout.viewport) {
                    layout.viewport = Handle::NULL;
                }
            }
        }
    }
}

/// LAYOUTCREATEVIEWPORT: a layout opened for the first time gets a viewport.
static LAYOUT_CREATE_VIEWPORT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// A selection box drawn right to left on the screen is a crossing, left to
/// right a window — for a dragged box and for two typed corners alike. A box
/// with no width on the screen (its corners within half a pixel across) is a
/// crossing too, as in the reference.
pub fn selection_box_crossing(anchor: iced::Point, current: iced::Point) -> bool {
    current.x < anchor.x + 0.5
}

/// PICKLOCKED: objects on a locked layer can be selected (they are never
/// editable). Off takes them out of every selection; they stay visible and
/// snappable.
#[cfg(not(test))]
static PICK_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Each test thread owns its PICKLOCKED value. In the shipped app this is a
/// process-wide setting, but in tests every app that is constructed applies
/// its own settings: with a shared atomic, one test could flip the flag under
/// another test that had just set it and was asserting the result.
#[cfg(test)]
thread_local! {
    static PICK_LOCKED_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub fn pick_locked() -> bool {
    #[cfg(test)]
    {
        PICK_LOCKED_TEST.with(|cell| cell.get())
    }
    #[cfg(not(test))]
    {
        PICK_LOCKED.load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub fn set_pick_locked(on: bool) {
    #[cfg(test)]
    PICK_LOCKED_TEST.with(|cell| cell.set(on));
    #[cfg(not(test))]
    PICK_LOCKED.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn layout_create_viewport() -> bool {
    LAYOUT_CREATE_VIEWPORT.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_layout_create_viewport(on: bool) {
    LAYOUT_CREATE_VIEWPORT.store(on, std::sync::atomic::Ordering::Relaxed);
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod section_tests;

#[cfg(test)]
mod journal_tests;

/// The associative families append their re-solved handles to a
/// `bump_entities` change list through one shared membership structure: each
/// handle is appended once, in family order, and a handle the edit already
/// named — or an earlier family appended — is never appended again.
#[cfg(test)]
mod associative_refresh_dedup_tests;

#[cfg(test)]
mod viewport_hidden_handles_tests;

/// Delta-undo round-trips: a recorded entity-only edit must be exactly
/// invertible (undo restores the pre-edit images) and re-appliable (redo
/// restores the post-edit images), including handle preservation and no
/// duplication of a re-inserted handle in its owning block record.
#[cfg(test)]
mod delta_undo_tests;

#[cfg(test)]
mod redraw_tests;

#[cfg(test)]
mod layout_cache_tests;

#[cfg(test)]
mod update_entities_batch_tests;

pub(crate) fn is_point_cloud(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Extended(extended)
            if matches!(extended.data, document::entities::ExtendedEntityData::PointCloudEx(_))
    )
}

/// A point cloud whose Locked property is set: edits leave it alone.
pub(crate) fn is_locked_point_cloud(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Extended(extended)
            if matches!(&extended.data, document::entities::ExtendedEntityData::PointCloudEx(data) if data.locked)
    )
}

#[cfg(test)]
mod world_box_handle_tests;

/// The per-viewport wipeout cache must outlive an edit that cannot have moved
/// a mask, and must not outlive one that can.
#[cfg(test)]
mod wipeout_viewport_cache_tests;

#[cfg(test)]
mod block_incremental_invalidation;

// A test sets this to make the next `add_entity` panic when the entity is
// in the document and has no undo record and no display model yet: a stop
// in the middle of a change.
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_PANIC_AFTER_DOCUMENT_ADD: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// A double-single coordinate (`hi` + `lo`) as one f64 point.
fn split_sum(hi: [f32; 3], lo: [f32; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| hi[i] as f64 + lo[i] as f64)
}

/// An f64 point as a double-single coordinate (`hi`, `lo`).
fn split_f64(point: [f64; 3]) -> ([f32; 3], [f32; 3]) {
    let hi = point.map(|v| v as f32);
    let lo = [0, 1, 2].map(|i| (point[i] - hi[i] as f64) as f32);
    (hi, lo)
}

/// The placement that takes `base`'s image plane onto `clone`'s, read from
/// three of their corners: the affine map the gather applied to an image
/// inside a block reference. `None` for a degenerate quad.
fn image_clone_placement(
    base: &ImageModel,
    clone: &ImageModel,
) -> Option<impl Fn([f64; 3]) -> [f64; 3]> {
    let corner = |m: &ImageModel, i: usize| split_sum(m.corners[i], m.corners_low[i]);
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (b0, k0) = (corner(base, 0), corner(clone, 0));
    let (e1, e2) = (sub(corner(base, 1), b0), sub(corner(base, 2), b0));
    let (f1, f2) = (sub(corner(clone, 1), k0), sub(corner(clone, 2), k0));
    let (a, b, c) = (dot(e1, e1), dot(e1, e2), dot(e2, e2));
    let det = a * c - b * b;
    if !(det > 1.0e-12 * a * c) {
        return None;
    }
    Some(move |p: [f64; 3]| {
        let d = sub(p, b0);
        let (r1, r2) = (dot(d, e1), dot(d, e2));
        let (u, v) = ((c * r1 - b * r2) / det, (a * r2 - b * r1) / det);
        [0, 1, 2].map(|i| k0[i] + u * f1[i] + v * f2[i])
    })
}
