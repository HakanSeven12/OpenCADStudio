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
pub mod cache;
pub mod compare;
pub mod convert;
pub mod creation_style;
pub(crate) mod frame;
pub(crate) mod hyperlink;
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

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum BlockScalePolicy {
    #[default]
    FromInsert,
    Applied,
}

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
mod camera_ops;
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
pub(crate) mod centermark;
pub(crate) mod dimension_assoc;
pub(crate) mod dimension_assoc_chain;
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
/// The one grip a whole picked group shows (GROUPDISPLAYMODE 1 and 2), at
/// the centre of its extents: each member carries it, so dragging it moves
/// the whole group.
pub const GROUP_GRIP_ID: usize = usize::MAX - 2;
#[cfg(test)]
mod hatch_boundary;
mod layout;
#[cfg(test)]
mod layout_viewport_tests;
mod limits;
mod modify;
#[cfg(test)]
mod selection_preview_tests;
#[cfg(test)]
mod sheet_underlay_tests;
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
const LAYOUT_ANNOTATION_VISIBILITY_APP: &str = "AcadAnnoAV";

impl Scene {
    /// True for the modeler solid families whose wire path is an empty shell
    /// (their geometry lives in the mesh pipeline): B-rep corners for those
    /// come from the mesh edge list instead of wire points.
    fn is_solid_shell_entity(entity: &EntityType) -> bool {
        matches!(
            entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
        )
    }

    /// B-rep snap points for a solid: edge endpoints as `Vertex` hints and
    /// edge centres as `EdgeMidpoint` hints (both from the tessellated edge
    /// list), plus face centres as `FaceCenter` hints (from the kernel B-rep
    /// faces). Empty for non-solids and for solids with no usable data.
    pub(crate) fn solid_snap_points(
        &self,
        handle: Handle,
    ) -> Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)> {
        use crate::scene::model::wire_model::SnapHint;
        let Some(entity) = self.document.get_entity(handle) else {
            return Vec::new();
        };
        if !Self::is_solid_shell_entity(entity) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(set) = self
            .meshes
            .get(&handle)
            .or_else(|| self.block_meshes.get(&handle))
        {
            let (edges, lows) = set.geometry_edges();
            let mut seen = HashSet::default();
            for (a, b) in crate::scene::model::mesh_model::solid_edge_segments(edges, lows) {
                for (point, hint) in [(a, SnapHint::Vertex), (b, SnapHint::Vertex)] {
                    let key = [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
                    if seen.insert(key) {
                        out.push((point, hint));
                    }
                }
                out.push(((a + b) * 0.5, SnapHint::EdgeMidpoint));
            }
        }
        out.extend(
            Self::solid_face_centers(entity, self.solid_models.get(&handle))
                .into_iter()
                .map(|p| (p, SnapHint::FaceCenter)),
        );
        out
    }

    /// Face centres for a solid entity: kernel B-rep faces averaged over
    /// their boundary loops. Prefers the cached kernel body (no re-parse);
    /// falls back to parsing the entity's ACIS data, which shares the cost
    /// profile of the mesh build at the same epoch. No body ⇒ no centres.
    fn solid_face_centers(
        entity: &EntityType,
        cached: Option<&ocs_solid::model::Brep>,
    ) -> Vec<glam::DVec3> {
        use crate::scene::convert::solid3d_tess::{region_record, solid_record};
        use crate::scene::model::solid_model::face_centers;
        if let Some(body) = cached {
            return face_centers(body)
                .into_iter()
                .map(glam::DVec3::from_array)
                .collect();
        }
        match entity {
            EntityType::Solid3D(s) => solid_record(s),
            EntityType::Region(r) => region_record(r),
            _ => None,
        }
        .map(|body| face_centers(&body))
        .unwrap_or_default()
        .into_iter()
        .map(glam::DVec3::from_array)
        .collect()
    }

    /// Attach solid B-rep snaps to the entity's shell wire (the empty wire
    /// carrying only the insertion snap). History/construction wires keep
    /// their own points and are left alone; non-solids are untouched.
    /// Idempotent: a wire already carrying 3D snaps is skipped, so shared
    /// memo entries can pass through every assembly path safely.
    pub(crate) fn attach_solid_snaps(&self, handle: Handle, wires: &mut [WireModel]) {
        use crate::scene::model::wire_model::SnapHint;
        let points = self.solid_snap_points(handle);
        if points.is_empty() {
            return;
        }
        for wire in wires.iter_mut() {
            if !wire.points.is_empty() {
                continue;
            }
            if wire.snap_pts.iter().any(|(_, hint)| {
                matches!(
                    hint,
                    SnapHint::Vertex
                        | SnapHint::EdgeMidpoint
                        | SnapHint::FaceCenter
                        | SnapHint::Knot
                )
            }) {
                continue;
            }
            wire.snap_pts.extend(points.iter().copied());
        }
    }

    /// Live solid-face snaps for a command point: Nearest-to-face (screen
    /// nearest point on a mesh triangle) and Perpendicular-to-face (foot from
    /// `base` onto a triangle). Mesh triangles carry no B-rep face grouping,
    /// so faces are individual triangles here — the pre-baked `FaceCenter`
    /// hints (true B-rep faces) are the separate discrete counterpart.
    ///
    /// Top-level model meshes only: block-local sets live in a different
    /// space, and xclipped views are not honored (the wire channel the
    /// pre-baked 3D hints ride is clipped; this direct mesh read is not).
    /// Worst case in those views is a snap with no geometry behind it —
    /// never a corrupt point.
    pub(crate) fn solid_face_snaps(
        &self,
        cursor_screen: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        aperture_px: f32,
        base: Option<glam::DVec3>,
        want_perp: bool,
        want_nearest: bool,
    ) -> Option<crate::snap::SnapResult> {
        use crate::snap::{
            closest_point_on_tri_2d, foot_on_triangle, snap_better, snap_priority, snap_tier,
            SnapType,
        };
        if !want_perp && !want_nearest {
            return None;
        }
        if !(aperture_px.is_finite() && aperture_px > 0.0) {
            return None;
        }
        let perp_base = want_perp.then_some(base).flatten();
        // Eye-plane guard: a triangle behind the perspective eye mirrors onto
        // visible pixels and would otherwise win a snap for a hidden face (#64).
        let project = |world: glam::DVec3| -> Option<[f32; 2]> {
            let ndc = crate::scene::view::projection::project_ndc(view_rot, eye, world)?;
            Some([
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            ])
        };
        let in_bounds = |s: [f32; 2]| {
            s[0] >= 0.0 && s[0] <= bounds.width && s[1] >= 0.0 && s[1] <= bounds.height
        };
        let radius2 = aperture_px * aperture_px;
        let cursor = [cursor_screen.x, cursor_screen.y];
        // (tier, d2, sub, eye-depth², handle, hit): engine ordering, then
        // nearer-to-eye (coincident faces share pixels — the top face must
        // beat the bottom one), then the handle as a final deterministic
        // tie-break (mesh iteration order is not stable).
        let mut best: Option<(u8, f32, u8, f64, u64, crate::snap::SnapResult)> = None;
        let mut consider = |snap_type: SnapType,
                            world: glam::DVec3,
                            screen: [f32; 2],
                            d2: f32,
                            handle: Handle| {
            if !(d2 < radius2) || !in_bounds(screen) {
                return;
            }
            let (tier, sub) = (snap_tier(snap_type), snap_priority(snap_type));
            let depth2 = (world - eye).length_squared();
            let better = match &best {
                None => true,
                Some((bt, bd2, bs, bdepth2, bh, _)) => {
                    snap_better(tier, d2, sub, (*bt, *bd2, *bs))
                        || (tier == *bt
                            && (d2 - *bd2).abs() <= 1e-4
                            && sub == *bs
                            && (depth2 < *bdepth2 || (depth2 == *bdepth2 && handle.value() < *bh)))
                }
            };
            if better {
                best = Some((
                    tier,
                    d2,
                    sub,
                    depth2,
                    handle.value(),
                    crate::snap::SnapResult {
                        world,
                        screen: iced::Point::new(screen[0], screen[1]),
                        snap_type,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                        extension_origin: None,
                        extension_dir: None,
                        viewport: None,
                        source: Some(crate::command::DimensionAssociationSource::inferred(handle)),
                        secondary_source: None,
                        model_point: None,
                    },
                ));
            }
        };
        for (handle, set) in self.meshes.iter() {
            let Some(lod) = set.geometry_lods().first() else {
                continue;
            };
            if lod.indices.len() < 3 {
                continue;
            }
            // Screen-space cull from the mesh bounds before touching triangles.
            // It is only sound while every corner has a real projection: a
            // corner behind the eye mirrors across the screen, so the union of
            // the projected corners bounds nothing (#64).
            let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
            let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
            let mut cullable = true;
            for corner in [
                [set.world_aabb[0], set.world_aabb[1], set.z_aabb[0]],
                [set.world_aabb[2], set.world_aabb[1], set.z_aabb[0]],
                [set.world_aabb[0], set.world_aabb[3], set.z_aabb[0]],
                [set.world_aabb[2], set.world_aabb[3], set.z_aabb[0]],
                [set.world_aabb[0], set.world_aabb[1], set.z_aabb[1]],
                [set.world_aabb[2], set.world_aabb[1], set.z_aabb[1]],
                [set.world_aabb[0], set.world_aabb[3], set.z_aabb[1]],
                [set.world_aabb[2], set.world_aabb[3], set.z_aabb[1]],
            ] {
                let Some(s) = project(glam::DVec3::new(
                    corner[0] as f64,
                    corner[1] as f64,
                    corner[2] as f64,
                )) else {
                    cullable = false;
                    break;
                };
                x0 = x0.min(s[0]);
                y0 = y0.min(s[1]);
                x1 = x1.max(s[0]);
                y1 = y1.max(s[1]);
            }
            if cullable
                && (cursor[0] < x0 - aperture_px
                    || cursor[0] > x1 + aperture_px
                    || cursor[1] < y0 - aperture_px
                    || cursor[1] > y1 + aperture_px)
            {
                continue;
            }
            for tri in lod.indices.chunks_exact(3) {
                let mut v = [glam::DVec3::ZERO; 3];
                let mut ok = true;
                for (slot, index) in v.iter_mut().zip(tri.iter()) {
                    let (Some(high), low) = (
                        lod.verts.get(*index as usize),
                        lod.verts_low
                            .get(*index as usize)
                            .copied()
                            .unwrap_or([0.0; 3]),
                    ) else {
                        ok = false;
                        break;
                    };
                    *slot = glam::DVec3::new(
                        high[0] as f64 + low[0] as f64,
                        high[1] as f64 + low[1] as f64,
                        high[2] as f64 + low[2] as f64,
                    );
                    if !slot.is_finite() {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let [a, b, c] = v;
                let (Some(sa), Some(sb), Some(sc)) = (project(a), project(b), project(c)) else {
                    continue;
                };
                // Per-triangle screen reject before any 3D work.
                let (tx0, ty0) = (sa[0].min(sb[0]).min(sc[0]), sa[1].min(sb[1]).min(sc[1]));
                let (tx1, ty1) = (sa[0].max(sb[0]).max(sc[0]), sa[1].max(sb[1]).max(sc[1]));
                if cursor[0] < tx0 - aperture_px
                    || cursor[0] > tx1 + aperture_px
                    || cursor[1] < ty0 - aperture_px
                    || cursor[1] > ty1 + aperture_px
                {
                    continue;
                }
                if want_nearest {
                    let (q, d2, w) = closest_point_on_tri_2d(cursor, sa, sb, sc);
                    if d2 < radius2 {
                        let world = a * w[0] as f64 + b * w[1] as f64 + c * w[2] as f64;
                        consider(SnapType::NearestFace, world, q, d2, *handle);
                    }
                }
                if let Some(base3d) = perp_base {
                    if let Some(foot) = foot_on_triangle(base3d, a, b, c) {
                        if let Some(sf) = project(foot) {
                            let dx = sf[0] - cursor[0];
                            let dy = sf[1] - cursor[1];
                            consider(
                                SnapType::FacePerpendicular,
                                foot,
                                sf,
                                dx * dx + dy * dy,
                                *handle,
                            );
                        }
                    }
                }
            }
        }
        best.map(|(_, _, _, _, _, hit)| hit)
    }
}

/// Result of `Scene::entity_index()`. The wire path queries `tree` for
/// view-rect candidates and also always emits `unbounded_handles`
/// (entities with no usable bbox — legacy `UNBOUNDED_AABB` sentinel).
#[derive(Clone)]
pub(super) struct EntityIndex {
    pub tree: pick::quadtree::QuadTree,
    pub unbounded_handles: Vec<Handle>,
}

/// Hash of every baked display input a memoized tessellation depends on; a
/// memo entry is reusable only under the guard it was built with.
fn tess_memo_guard(
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
fn drop_stale_text(
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
fn drop_stale_complex_lt(
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
    if scene.dependency_index_cache.borrow().is_none() {
        let index = scene.rebuild_dependency_index();
        *scene.dependency_index_cache.borrow_mut() = Some(Arc::new(index));
    }
    if let Some(index) = scene.dependency_index_cache.borrow().as_ref() {
        for handle in &index.annotation_geometry.render_handles {
            memo.remove(handle);
        }
    }
}

#[derive(Clone, Default)]
struct DependencyTargets {
    render_handles: HashSet<Handle>,
    source_handles: HashSet<Handle>,
    /// Render handles reached through a block definition (INSERTs and other
    /// block users), as opposed to entities that carry the name themselves.
    block_users: HashSet<Handle>,
    touches_block_definition: bool,
}

impl DependencyTargets {
    fn extend_from(&mut self, other: &DependencyTargets) {
        self.render_handles
            .extend(other.render_handles.iter().copied());
        self.source_handles
            .extend(other.source_handles.iter().copied());
        self.block_users.extend(other.block_users.iter().copied());
        self.touches_block_definition |= other.touches_block_definition;
    }
}

#[derive(Clone, Copy)]
enum DependencyKind {
    Layer,
    Linetype,
    TextStyle,
    DimStyle,
}

/// The render roots an entity contributes to a dependency target. Kept as a
/// borrow rather than a collected set so the common direct-render case adds
/// its single handle without allocating.
#[derive(Clone, Copy)]
enum RenderHandles<'a> {
    SelfOnly(Handle),
    /// The entity lives inside a block definition; it renders through the
    /// outer references that draw that block. `None` when the definition has
    /// no root users yet.
    Block(Option<&'a HashSet<Handle>>),
}

impl RenderHandles<'_> {
    fn extend_into(&self, target: &mut DependencyTargets, source: Handle) {
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
struct SceneDependencyIndex {
    layers: HashMap<String, DependencyTargets>,
    linetypes: HashMap<String, DependencyTargets>,
    text_styles: HashMap<String, DependencyTargets>,
    dim_styles: HashMap<String, DependencyTargets>,
    object_styles: HashMap<Handle, DependencyTargets>,
    points: DependencyTargets,
    text_geometry: DependencyTargets,
    annotation_geometry: DependencyTargets,
    signatures: HashMap<Handle, u64>,
    /// Block name (normalised) → the layout entities that draw it, nested
    /// references included.
    block_roots: HashMap<String, HashSet<Handle>>,
    wipeout_hosts: HashSet<Handle>,
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

/// Whether a wire's geometry meets a world-XY box `[min_x, min_y, max_x,
/// max_y]`. A wire with no finite stored box is measured from what it
/// carries (points, fills, snap features); one carrying nothing never meets.
fn wire_meets_xy_box(wire: &WireModel, query: [f64; 4]) -> bool {
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

fn mesh_interaction_aabb(set: &model::mesh_model::MeshLodSet) -> Option<[f64; 6]> {
    if let Some(aabb) = set.instance_aabb {
        return Some(aabb);
    }
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
pub use model::wire_model::{WireGeom, WireModel};
use pick::interaction_index::{InteractionCandidates, InteractionIndex, SnapQueryCategory};
pub use pick::selection_state::SelectionState;
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
struct GeometryDelta {
    epoch: u64,
    changes: Vec<(Handle, ChangeKind)>,
    /// Cache-category membership captured immediately before a Removed entity
    /// leaves the document. Presence with value 0 means "known unrelated";
    /// absence means the caller removed it outside the tracked primitives and
    /// consumers must remain conservative.
    removed_categories: HashMap<Handle, u16>,
    full: bool,
    layer_visibility_only: bool,
}

/// The desk a paper layout lays its sheet on — the surface visible around the
/// page. Distinct from both the page (`paper_bg_color`) and the model canvas
/// (`bg_color`); if the desk took either of those colours the page edge would
/// vanish into its own surround.
pub const PAPER_DESK_COLOR: [f32; 4] = [138.0 / 255.0, 138.0 / 255.0, 138.0 / 255.0, 1.0];

const CACHE_CATEGORY_ANNOTATIVE: u16 = 1 << 0;
const CACHE_CATEGORY_HATCH: u16 = 1 << 1;
const CACHE_CATEGORY_WIPEOUT: u16 = 1 << 2;
const CACHE_CATEGORY_IMAGE: u16 = 1 << 3;
const CACHE_CATEGORY_MESH: u16 = 1 << 4;
const CACHE_CATEGORY_INTERACTION: u16 = 1 << 5;
const CACHE_CATEGORY_TEXT: u16 = 1 << 6;
const CACHE_CATEGORY_INSERT_HATCH: u16 = 1 << 7;

/// Snap points of the solids inside each block definition, by entity.
type BlockSolidSnaps =
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
fn resident_touch() -> u64 {
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
struct ReclaimOnDecline<'a> {
    scene: &'a Scene,
    wires: Option<Vec<WireModel>>,
    backed: bool,
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
type OverlayChanges = (u64, u64, u64, Arc<Vec<(Handle, ChangeKind)>>);

/// What a query needs from the immutable overlay base: (base epoch, base
/// index, geometry-journal changes since it).
type OverlayBase = (
    u64,
    Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    Arc<Vec<(Handle, ChangeKind)>>,
);

/// A zero-copy overlay gather: shared wire runs plus the base and changed
/// slot maps remapping `(handle, ordinal)` keys onto run slots.
type OverlayGather = (
    Arc<crate::scene::pick::interaction_index::SharedWireRuns>,
    HashMap<(u64, u32), u32>,
    HashMap<(u64, u32), u32>,
);

/// Kill-switch: `OCS_NO_RESIDENT_MEMO` reverts the resident set to the old
/// full re-tessellation on every edit, in case a mutation site is ever found
/// that edits geometry without dropping its handle from the memo.
fn resident_memo_enabled() -> bool {
    static EN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *EN.get_or_init(|| std::env::var("OCS_NO_RESIDENT_MEMO").is_err())
}

#[derive(Clone)]
pub struct PreparedOpenGeometry {
    dependency_index: Option<Arc<SceneDependencyIndex>>,
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
    insert_hatches: Option<(u64, Arc<HashMap<Handle, Vec<HatchModel>>>)>,
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
fn compose_wire_gpu_patches(first: &WireGpuPatch, second: WireGpuPatch) -> WireGpuPatch {
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
// Label stride for a block that starts empty and grows by Add: wide enough for
// the depth buffer to keep neighbours apart (about 80 steps of a 24-bit buffer
// at the default draw-order scale), and narrow enough to leave room for
// thousands of Adds before a full rebuild.
const DRAW_DEPTH_EMPTY_BLOCK_STRIDE: i32 = 1 << 18;

/// The rank an insert's attributes are drawn at: `insert depth + rank * half`.
/// Block members draw at their own ranks, all inside (0, 2) of the insert's
/// half gap, so this one is above every member (an attribute is drawn after the
/// block's contents, over its wipeout) and still under the next sibling, which
/// starts two half gaps further on.
pub(crate) const INSERT_ATTRIBUTE_RANK: f32 = 1.95;

/// Labels map into (0, 2): draw order only ever moves 2D geometry towards
/// the camera. A negative bias pushed lines lying on a solid's face behind
/// that face in shaded views (#3119); 0 stays the true depth of 3D geometry.
/// The scale stays 2^-30 so neighbours (and a block's text over its own
/// wipeout) keep their depth separation (#3138).
fn draw_depth_value(label: i32, half_label: i32) -> [f32; 2] {
    [
        (label as i64 + (1_i64 << 30)) as f32 * DRAW_DEPTH_LABEL_SCALE,
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
fn draw_depth_entry_at_own_key(order: &[DrawDepthEntry], handle: u64) -> Option<&DrawDepthEntry> {
    let _ = (order, handle);
    None
}

/// The entry of `handle` in a block's order: at its own place, or, for an
/// entity the sort table moved, wherever in the order that put it.
fn draw_depth_entry(order: &[DrawDepthEntry], handle: u64) -> Option<&DrawDepthEntry> {
    draw_depth_entry_at_own_key(order, handle)
        .or_else(|| order.iter().find(|entry| entry.handle == handle))
}

#[derive(Clone)]
pub(crate) struct DrawDepthCache {
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
/// `view_height` is canonical. `custom_scale` is the fallback when it is
/// missing or zero.
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

/// Pre-built entity caches returned by [`build_derived_caches`].
/// Produced in the file-load background task so the UI thread only assigns.
#[derive(Debug, Clone)]
pub struct DerivedCaches {
    pub read_stats: Option<document::ReadStats>,
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
    /// The FINALIZING phase: `prepare_open_geometry` — first full Model
    /// tessellation plus interaction index. Previously unmeasured, which made
    /// it indistinguishable from parse time in an open that appeared to stall.
    pub finalize_ms: u32,
}

/// Build hatch / image / mesh caches from a document without needing `&mut Scene`.
/// Intended to run on a background thread during file load.
#[cfg(target_arch = "wasm32")]
pub fn build_derived_caches(doc: &Document) -> DerivedCaches {
    build_derived_caches_impl(doc, None, None)
}

/// Build open-time caches while reporting monotonic progress in 0..=10000.
///
/// The callback is UI-agnostic and may run from Rayon workers. Callers should
/// keep it cheap, normally just updating atomics.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_derived_caches_with_progress(
    doc: &Document,
    progress: &(dyn Fn(u16) + Sync),
    material_base_dir: Option<&std::path::Path>,
) -> DerivedCaches {
    build_derived_caches_impl(doc, Some(progress), material_base_dir)
}

fn build_derived_caches_impl(
    doc: &Document,
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
            if let document::objects::ObjectType::Layout(l) = obj {
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
    //
    // The per-entity `bounding_box()` behind the centroid is the expensive
    // part, so the scan itself runs in parallel; the order-preserving collect
    // keeps the handle lists and centroids in document order, and the cheap
    // gather below stays serial.
    #[derive(Clone, Copy)]
    enum CacheKind {
        None,
        Hatch,
        Image,
        Mesh,
    }
    let entity_refs: Vec<&EntityType> = doc.entities().collect();
    let entity_total = entity_refs.len().max(1);
    let scanned_count = std::sync::atomic::AtomicUsize::new(0);
    let scanned: Vec<(CacheKind, Option<[f64; 3]>)> = entity_refs
        .par_iter()
        .map(|&e| {
            let kind = match e {
                EntityType::Hatch(_) | EntityType::Solid(_) => CacheKind::Hatch,
                EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_) => {
                    CacheKind::Image
                }
                EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_) => CacheKind::Mesh,
                _ => CacheKind::None,
            };
            let centroid = offset_centroid(e, model_block, &prep);
            let done = scanned_count.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0x1fff == 0 {
                if let Some(progress) = progress {
                    progress(((done as u64 * 4000) / entity_total as u64) as u16);
                }
            }
            (kind, centroid)
        })
        .collect();
    for (e, (kind, centroid)) in entity_refs.iter().zip(scanned) {
        let h = e.common().handle;
        match kind {
            CacheKind::Hatch => hatch_handles.push(h),
            CacheKind::Image => image_handles.push(h),
            CacheKind::Mesh => mesh_handles.push(h),
            CacheKind::None => {}
        }
        if let Some(c) = centroid {
            centers.push(c);
        }
    }
    drop(entity_refs);
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
                    Some(document::objects::ObjectType::UnderlayDefinition(def)) => {
                        // Paper-space underlays adjust to the white sheet.
                        let owner = u.common.owner_handle;
                        let background = if owner.is_null() || owner == model_block {
                            LOAD_BG
                        } else {
                            [1.0, 1.0, 1.0, 1.0]
                        };
                        ImageModel::from_underlay(u, def, background, None).map(|m| (handle, m))
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

    // FACETRES biases one shared chordal tolerance for all solids.
    // Top-level (layout-owned) solids are offset into the render frame; block
    // definition solids keep block-local coords for per-INSERT instancing. (#123)
    let facet_res = doc.header.facet_resolution;
    let chordal_deflection = crate::entities::solid3d::display_deflection(&doc.header, facet_res);
    let isolines = crate::entities::solid3d::clamp_header_isolines(doc.header.isolines);
    // Real layout blocks come from the Layout objects' block_record handles —
    // `BlockRecord::is_layout()` is unreliable here (it flags ordinary blocks).
    let layout_blocks: std::collections::HashSet<Handle> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            document::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
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
            let result = crate::entities::solid3d::tessellate_volume(
                e,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(|mut mesh| {
                material.apply_to_with_face_overrides(&mut mesh, doc, material_base_dir);
                crate::scene::model::visual_style_model::apply_mesh_visual_style(&mut mesh, doc, e);
                let mesh = if top_level {
                    offset_mesh_lod_set(mesh)
                } else {
                    mesh
                };
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
        xrefs: Vec::new(),
        prepared_geometry: None,
        timings: OpenTimings::default(),
    }
}

/// Prepare the expensive first Model wire set and spatial interaction index on
/// the loader thread. The temporary `Scene` never crosses threads (it contains
/// `Rc`/`RefCell` state); only its Send-safe document and immutable prepared
/// geometry are returned.
#[cfg(not(target_arch = "wasm32"))]
pub fn prepare_open_geometry(
    doc: Document,
    caches: &mut DerivedCaches,
    model_bg: [f32; 4],
) -> (Document, PreparedOpenGeometry) {
    let mut scene = Scene::new();
    scene.document = doc;
    // Solid meshes give solid wires their corner snaps; lent to the scene
    // for the build and handed back below.
    scene.meshes = std::mem::take(&mut caches.meshes);
    scene.block_meshes = std::mem::take(&mut caches.block_meshes);
    scene.local_extent_max = caches.local_extent_max;
    scene.local_center = caches.local_center;
    scene.bg_color = model_bg;
    scene.sync_annotation_scale_from_header();
    scene.current_layout = "Model".to_string();
    let camera = scene.camera.borrow().clone();
    let perf = crate::perf::enabled();
    let t_wires = perf.then(iced::time::Instant::now);
    let wires = scene.model_tile_wires_arc(0, &camera, 1.0, 1.0);
    let wires_ms = crate::perf::elapsed_ms(t_wires);
    // The temporary scene is discarded after this function. Keep its spatial
    // entity index so opening a large drawing does not rebuild the same O(N)
    // index synchronously on the UI thread during the 99.5% finalization step.
    let t_entity_index = perf.then(iced::time::Instant::now);
    let _ = scene.entity_index();
    let entity_index =
        std::mem::take(&mut *scene.entity_index_cache.borrow_mut()).map(|(_, index)| index);
    let entity_index_ms = crate::perf::elapsed_ms(t_entity_index);
    let t_index = perf.then(iced::time::Instant::now);
    let interaction_index = if scene.interaction_index_worthwhile(&wires) {
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
        let build_ms = crate::perf::elapsed_ms(t_index);
        if perf {
            crate::perf_record!("[perf] open-index build={:.1}ms", build_ms);
        }
        Some(index)
    } else {
        None
    };
    if perf {
        crate::perf_record!(
            "[perf] open-finalize wires={:.1}ms index={:.1}ms entity_index={:.1}ms wire_models={}",
            wires_ms,
            crate::perf::elapsed_ms(t_index),
            entity_index_ms,
            wires.len(),
        );
    }
    // The hatch picking index walks every entity and needs the hatch models,
    // which stay out of the wire tessellation above. Built here it no longer
    // freezes the UI thread right after the open; the models are lent to the
    // scene for this one build and handed back untouched.
    let t_handles = perf.then(iced::time::Instant::now);
    scene.hatches = std::mem::take(&mut caches.hatches);
    let _ = scene.interaction_handle_index();
    caches.hatches = std::mem::take(&mut scene.hatches);
    let handle_index = scene
        .interaction_handle_index_cache
        .borrow_mut()
        .take()
        .map(|(_, space_key, index)| (space_key, index));
    let insert_hatches = scene
        .insert_hatch_cache
        .borrow_mut()
        .take()
        .map(|(_, space_key, hatches)| (space_key, hatches));
    if perf {
        crate::perf_record!(
            "[perf] open-handle-index={:.1}ms",
            crate::perf::elapsed_ms(t_handles),
        );
    }
    // Taken, not cloned: the scene is discarded on the next line.
    let tess_memo = std::mem::take(&mut *scene.resident_tess_memo.borrow_mut());
    let tess_memo_backing = std::mem::take(&mut *scene.resident_memo_backing.borrow_mut());
    let tess_guard = scene.resident_tess_guard.get();
    let tess_atlas = scene.resident_tess_atlas.get();
    let draw_depths = scene.draw_depth_cache.borrow_mut().take();
    // The layout the loader already built for exactly these wires. Taken by
    // identity so it cannot be paired with a different assembly.
    let resident_layout = scene
        .resident_wire_sets
        .borrow_mut()
        .drain()
        .find_map(|(_, set)| {
            Arc::ptr_eq(&set.wires, &wires)
                .then_some(set.layout)
                .flatten()
        });
    let block_defns: Vec<(u64, Arc<cache::block_cache::BlockCache>)> =
        std::mem::take(&mut *scene.block_defn_cache.borrow_mut())
            .into_iter()
            .map(|(key, (_epoch, cache))| (key, cache))
            .collect();
    // Layer changes are interactive. Build their reverse lookup beside the
    // other loader indexes so the first pick doesn't scan the whole DWG.
    let dependency_index = scene
        .dependency_index_cache
        .get_mut()
        .take()
        .unwrap_or_else(|| Arc::new(scene.rebuild_dependency_index()));
    let doc = std::mem::replace(&mut scene.document, Document::new());
    caches.meshes = std::mem::take(&mut scene.meshes);
    caches.block_meshes = std::mem::take(&mut scene.block_meshes);
    (
        doc,
        PreparedOpenGeometry {
            dependency_index: Some(dependency_index),
            wires,
            interaction_index,
            entity_index,
            tess_memo,
            tess_memo_backing,
            tess_guard,
            tess_atlas,
            block_defns,
            draw_depths,
            resident_layout,
            handle_index,
            insert_hatches,
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

fn offset_prep(doc: &document::Document, model_block: Handle) -> OffsetPrep {
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
    header: &document::document::HeaderVariables,
) -> ([f64; 2], f32) {
    const SANE_EXTENT: f64 = CLUSTER_SANE_EXTENT;
    let entity_ok = !centers.is_empty();

    // 95th-percentile distance from the median × 2 gives the half-span of the
    // dense cluster while leaving room for legitimate outliers (sparse leaders,
    // dimensions, scattered annotations).
    let median = |v: &mut Vec<f64>| -> f64 {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let percentile = |v: &mut Vec<f64>, frac: f64| -> f64 {
        v.sort_by(|a, b| a.total_cmp(b));
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
enum ViewSnapshot {
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

/// Build a lightweight placement over immutable block-local geometry.
fn transform_block_mesh_lod_set(
    set: &MeshLodSet,
    xform: &document::types::Transform,
) -> MeshLodSet {
    use document::types::Vector3;
    let source = set.instance_source.clone().unwrap_or_else(|| {
        std::sync::Arc::new(model::mesh_model::MeshInstanceSource {
            handle: set.entity_handle().unwrap_or(Handle::new(0)),
            lods: set.lods.clone(),
            edge_verts: set.edge_verts.clone(),
            edge_verts_low: set.edge_verts_low.clone(),
            curved_gens: set.curved_gens.clone(),
            display_lods: set.display_lods.clone(),
        })
    });
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for x in [set.world_aabb[0], set.world_aabb[2]] {
        for y in [set.world_aabb[1], set.world_aabb[3]] {
            for z in [set.z_aabb[0], set.z_aabb[1]] {
                let point = xform.apply(Vector3::new(x as f64, y as f64, z as f64));
                for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                    bounds[axis] = bounds[axis].min(value);
                    bounds[axis + 3] = bounds[axis + 3].max(value);
                }
            }
        }
    }
    MeshLodSet {
        lods: Vec::new(),
        effective_layer: set.effective_layer.clone(),
        material: set.material.clone(),
        face_materials: set.face_materials.clone(),
        visual_style: set.visual_style.clone(),
        complete: set.complete,
        edge_verts: Vec::new(),
        edge_verts_low: Vec::new(),
        edge_vert_colors: Vec::new(),
        curved_gens: Vec::new(),
        metrics: set.metrics,
        world_aabb: [
            bounds[0] as f32,
            bounds[1] as f32,
            bounds[3] as f32,
            bounds[4] as f32,
        ],
        z_aabb: [bounds[2] as f32, bounds[5] as f32],
        instance_source: Some(source),
        instance_transform: Some(*xform),
        instance_handle: None,
        instance_color: set.display_color(),
        edge_color: set.edge_color,
        instance_aabb: bounds
            .iter()
            .all(|value| value.is_finite())
            .then_some(bounds),
        display_lods: Default::default(),
    }
}

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
type BlockMembers = (u64, HashMap<Handle, Vec<Handle>>);

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
type AnnotationSelectedEntry = (
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
    // The selected part is independent of box/rollover hover changes.
    annotation_selected_cache: RefCell<HashMap<u64, AnnotationSelectedEntry>>,
    /// The step of scale the outlines of highlighted solids were last built
    /// for in each view (its viewport handle and model tile; see
    /// `outline_detail`). A view that is being zoomed keeps its step until
    /// it rests.
    outline_steps: RefCell<HashMap<(Handle, Option<usize>), Option<i32>>>,
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
    pub selection: std::sync::Arc<std::cell::RefCell<SelectionState>>,
    /// The CAD document — single source of truth for all entities.
    pub document: Document,
    /// Last chain-compatible dimension created this session.
    /// Never reconstructed from loaded file handles.
    pub last_created_dimension: Option<Handle>,
    /// File-open-prepared index for non-graphical semantic object lookups.
    pub(crate) object_data_cache: crate::entities::object_data::ObjectDataCache,
    /// Native AcDbLight/Sun inputs, separated by rendered block and viewport
    /// frozen-layer signature. Invalidated only when a light entity changes;
    /// ordinary geometry edits never rescan the drawing's lights.
    lighting_cache: RefCell<HashMap<(Handle, u64), Vec<SceneLight>>>,
    /// The visibility state each dynamic block shows in the Block Editor,
    /// by block record.
    pub visibility_states: HashMap<Handle, usize>,
    pub(crate) block_visibility_reveal: bool,
    pub(crate) block_construction_hidden: rustc_hash::FxHashSet<Handle>,
    pub(crate) block_constraint_status_mode: bool,
    block_constraint_status_colors: [String; 4],
    constraint_status_cache: RefCell<parametric_solve::status::Cache>,
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
    /// Lazily rebuilt snapshot of every selectable group's member list, keyed
    /// by `(groups_epoch, PICKSTYLE group bit)`. Group-selection expansion used
    /// to scan ALL document objects on every pointer move, box-preview tick and
    /// rendered frame; with the snapshot those walk only the groups.
    groups_snapshot: RefCell<
        Option<(
            u64,
            bool,
            std::sync::Arc<Vec<(Handle, std::sync::Arc<Vec<Handle>>)>>,
        )>,
    >,
    /// Cached fingerprint of `selected`, recomputed lazily after mutations.
    #[cfg(any(test, not(target_arch = "wasm32")))]
    selection_fingerprint_cache: u64,
    selection_fingerprint_dirty: bool,
    /// Cached per-frame `ViewportData` interaction sets (bit-encoded
    /// `(selection_generation, hover_generation)` keys → selected handles,
    /// hover handles, selected signature). Building them used to clone the
    /// selected set, expand the hover set and hash the selection on EVERY
    /// rendered frame per viewport — idle CPU that scaled with the selection
    /// size even when nothing moved. The two generations cover every input:
    /// selection mutations, sub-segment picks and table-cell picks all bump
    /// the selection generation; hover sets bump the hover one.
    viewport_interaction_sets: RefCell<
        Option<(
            u64,
            u64,
            std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
            std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
            u64,
        )>,
    >,
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
    /// Geometry-journal replay for the overlay base, keyed on (base epoch,
    /// geometry epoch, space key). Pointer moves consult the overlay base
    /// several times per gesture; replay the journal once per key instead
    /// of once per consult.
    interaction_overlay_changes: RefCell<Option<OverlayChanges>>,
    /// Auxiliary broad phase for hatch-only objects.
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
    /// [`Scene::block_solid_snaps`] for one geometry epoch.
    block_solid_snaps_cache: RefCell<Option<(u64, Arc<BlockSolidSnaps>)>>,
    /// Bumped when a full draw-order rebuild can move existing depths.
    /// Cached uploads compare it before reusing baked depth values.
    draw_depth_generation: std::cell::Cell<u64>,
    /// Shared empty map for 3-D wireframe, where true depth wins instead of
    /// the entity submission order used by the optimized 2-D style.
    no_draw_depths: Arc<HashMap<u64, [f32; 2]>>,
    /// Cached hatch fill models, keyed by target block and geometry_epoch. View culling
    /// is handled at draw time via `hatch_skip_flags` in the pipeline,
    /// not at build time — that lets the GPU buffer stay stable across
    /// pan/zoom while still skipping out-of-view hatches.
    /// Keyed by geometry and selection in paper space. Model-space selection
    /// tint is drawn through the small live overlay so selecting one hatch
    /// does not rebuild and re-upload every resident hatch.
    hatch_cache: RefCell<HashMap<(Handle, String), (u64, u64, Arc<Vec<HatchModel>>)>>,
    /// For each `hatch_cache` entry, the top-level reference that draws each
    /// model (`Handle::NULL` for a fill drawn directly), index for index. Held
    /// with the list it describes, so an edit of one reference can replace
    /// just its fills instead of rebuilding every model.
    hatch_hosts:
        RefCell<HashMap<(Handle, String), (std::sync::Weak<Vec<HatchModel>>, Arc<Vec<Handle>>)>>,
    /// Small selected-hatch tint overlay, keyed independently from the
    /// resident hatch set. The rendered preview cache retains the source Arcs
    /// so pointer identity is ABA-safe when command/grip previews change.
    selected_hatch_overlay_cache: RefCell<Option<(u64, u64, Arc<Vec<HatchModel>>)>>,
    combined_preview_hatch_cache: RefCell<
        Option<(
            Arc<Vec<HatchModel>>,
            Arc<Vec<HatchModel>>,
            Arc<Vec<HatchModel>>,
        )>,
    >,
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
    /// The box around the vertices of each mesh the current renderer mesh
    /// source draws, by the mesh's address: the references of a block share
    /// one. The box and lasso tests tell the solids clear of a selection from
    /// it. Guarded by the weak source like the lookup above.
    #[allow(clippy::type_complexity)]
    mesh_pick_boxes_cache: RefCell<
        Option<(
            usize,
            std::sync::Weak<Vec<MeshLodSet>>,
            Arc<rustc_hash::FxHashMap<usize, [glam::DVec3; 2]>>,
        )>,
    >,
    /// Per-viewport (VP-frozen-layer) filtered fill / image / mesh sets, keyed by
    /// the order-independent signature of the viewport's frozen-layer set. A
    /// content viewport that freezes layers must hide their hatches, 2-D solids,
    /// images/OLE and 3-D solids too — not just wires — so these hold the
    /// filtered variants. Viewports sharing a frozen set share one entry (like
    /// the resident wire set). Empty for a viewport with no frozen layers (it
    /// reuses the unfiltered `*_arc` sets directly).
    frozen_hatch_cache: RefCell<HashMap<(Handle, String, u64), (u64, u64, Arc<Vec<HatchModel>>)>>,
    frozen_wipeout_cache: RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<HatchModel>>)>>,
    frozen_image_cache: RefCell<HashMap<(Handle, u64), (u64, Arc<Vec<ImageModel>>)>>,
    /// Point clouds shown by the model content, keyed by target block and
    /// frozen-layer signature, per geometry epoch.
    point_cloud_cache: RefCell<
        HashMap<(Handle, u64), (u64, Arc<crate::scene::model::point_cloud::PointCloudSet>)>,
    >,
    frozen_mesh_cache: RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<MeshLodSet>>)>>,
    /// Viewports that carry layer color/alpha/linetype/lineweight overrides.
    /// Cached per geometry epoch so ordinary viewports can share render data.
    viewport_style_override_cache: RefCell<Option<(u64, HashSet<Handle>)>>,
    /// Model-space INSERTs that carry a view-representation binding, as
    /// `(insert, bound viewport)` pairs, cached for the geometry epoch and
    /// model-space block they were collected for. Every content viewport asks
    /// which references hide inside it on every frame; without this the answer
    /// came from a full document scan per viewport.
    view_rep_reference_cache: RefCell<Option<(u64, Handle, Arc<Vec<(Handle, Handle)>>)>>,
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
    display_plot_style_cache:
        RefCell<HashMap<(String, String), Option<Arc<crate::io::plot_style::PlotStyleTable>>>>,
    styled_wire_cache: RefCell<HashMap<(u64, String), (u64, Arc<Vec<WireModel>>)>>,
    styled_hatch_cache: RefCell<HashMap<(u64, usize, usize, String, u32), Arc<Vec<HatchModel>>>>,
    styled_wire_fill_cache: RefCell<HashMap<(u64, String, u32, u64), Arc<Vec<HatchModel>>>>,
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
    /// Cached model-space bounding box, keyed by geometry_epoch.
    /// Avoids re-tessellating all entities on every ZOOM E / auto-fit call.
    model_extents_cache: RefCell<Option<(u64, Option<(glam::Vec3, glam::Vec3)>)>>,
    /// Memoised [`Scene::model_space_block_handle`] answer, keyed by the
    /// structural state it is derived from: the geometry epoch (a coarse
    /// structural change can replace the whole document), the layout epoch
    /// (Layout objects inserted/removed/renamed/reordered) and the object
    /// count (a direct `document.objects` insert/remove). The scene asks for
    /// the handle from 18 call sites, several per frame, and the DWG lookup
    /// walks every object until it finds the "Model" layout.
    model_space_block_cache: std::cell::Cell<Option<(u64, u64, usize, Handle)>>,
    /// Reverse map: entity_handle → block_record_handle, built from entity_handles lists.
    /// Keyed by geometry_epoch. Eliminates the O(B) fallback scan in belongs_to_visible_block.
    entity_block_map_cache: RefCell<Option<(u64, HashMap<Handle, Handle>)>>,
    /// Candidate handles per block, in document order and keyed by geometry epoch.
    block_members_cache: RefCell<Option<BlockMembers>>,
    leaders_by_annotation_cache: RefCell<Option<(u64, HashMap<Handle, Vec<Handle>>)>>,
    /// Insert/Viewport/Block/BlockEnd handles omitted by the spatial index.
    unindexable_cache: RefCell<Option<(u64, Vec<Handle>)>>,
    /// `(epoch, layout block, present type names, list exposed to the UI)`.
    /// The set lets pure additions update without rebuilding the list.
    layout_type_names_cache: RefCell<
        Option<(
            u64,
            Handle,
            std::collections::BTreeSet<String>,
            std::sync::Arc<Vec<String>>,
        )>,
    >,
    /// Bump on any Layout object insert/remove/rename/reorder.
    layout_epoch: u64,
    /// Layouts created in this session and not opened yet: the first time
    /// one opens it gets the LAYOUTCREATEVIEWPORT viewport.
    pub(crate) unopened_layouts: std::collections::HashSet<String>,
    /// The plot style table a layout's page setup takes when it is
    /// initialized on first opening.
    pub(crate) new_layout_plot_style: String,
    layout_names_cache: RefCell<Option<(u64, std::sync::Arc<Vec<String>>)>>,
    /// Bump on any annotation-scale list mutation.
    scale_epoch: u64,
    scale_picker_cache: RefCell<
        Option<(
            u64,
            f64,
            Option<bool>,
            String,
            std::sync::Arc<Vec<(String, f32, f64)>>,
        )>,
    >,
    /// Bump on any named-UCS table mutation (UCS SAVE / DELETE / RENAME,
    /// record-API patch, file-open, undo Full-swap).
    ucs_epoch: u64,
    ucs_names_cache: RefCell<Option<(u64, std::sync::Arc<[String]>)>>,
    /// Reverse dependencies from layer/style/block definitions to the top-level
    /// entities whose resident wire runs actually change. Kept independent from
    /// `geometry_epoch`: a layer colour toggle can reuse the index, invalidate
    /// only its dependants, and avoid a whole-document scan on every toggle.
    dependency_index_cache: RefCell<Option<Arc<SceneDependencyIndex>>>,
    /// Tables added or changed since the drawing was opened or last saved:
    /// their stored cell geometry is marked out of date (flag 7) in the
    /// next save.
    tables_modified: HashSet<Handle>,
    /// Boundary source → associative hatch handles. Source edits reuse this
    /// index instead of scanning the document during every drag step.
    associative_hatch_source_cache: RefCell<Option<HashMap<Handle, Vec<Handle>>>>,
    /// Centre-line source → carrier lines. Built on first use and dropped
    /// whenever an entity or an association may have changed, so an ordinary
    /// edit neither walks every line nor parses its xdata.
    associative_centerline_source_cache:
        RefCell<Option<crate::scene::centerline::CenterLineSourceIndex>>,
    /// Centre-mark source → carrier lines. See
    /// [`Self::associative_centerline_source_cache`].
    associative_centermark_source_cache: RefCell<Option<HashMap<Handle, Vec<Handle>>>>,
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
    /// Cached resolved document render environment (fog and background image), keyed by (geometry_epoch, document.objects.len()).
    document_render_env_cache: RefCell<
        Option<(
            u64,
            usize,
            crate::scene::view::render::CachedDocumentRenderEnvironment,
        )>,
    >,
    /// Memoized background image resolution to avoid repeated filesystem probes on each render frame.
    background_image_cache:
        RefCell<HashMap<String, Option<crate::scene::model::image_model::DecodedImage>>>,
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
    /// Memoised constraint-glyph placements for the latest [`GlyphKey`](parametric_constraints::GlyphKey)
    /// (Task 3; overlay/hit consumers adopt it in Task 4). Single-slot memo:
    /// within one frame view + hit-test + dwell + click share the key (hits),
    /// camera motion misses once per frame by design, and edits just miss on
    /// the stale entry — no leak possible. `RefCell` + `Arc` shape mirrors
    /// `layout_names_cache`; a missed constraint bump serves stale glyphs, so
    /// every set mutation must bump `constraints_epoch`.
    glyph_cache: RefCell<
        Option<(
            parametric_constraints::GlyphKey,
            Arc<[parametric_constraints::GlyphEntry]>,
        )>,
    >,
    /// Document-wide named-parameter and expression table decoded from
    /// standard associative variables.
    pub(crate) named_parameters: named_parameters::ParameterTable,
    /// Content hash of the constraint model as of the last
    /// `sync_native_parametric_graph`, letting geometry-only edits skip the
    /// graph rebuild. `None` until the first sync records one.
    native_graph_fingerprint: Option<u64>,
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
    /// Pending REDRAW force set, keyed by viewport `instance_id`. Populated by
    /// `request_refresh` and drained per-id by `refresh_consume` from whichever
    /// builder owns that instance. Never touches geometry_epoch/block_epoch.
    pending_refresh: std::cell::RefCell<rustc_hash::FxHashSet<u64>>,
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
    tess_memo: RefCell<HashMap<Handle, MemoRun>>,
    /// Hash of the tessellation parameters `tess_memo` was built under. When
    /// the current call's parameters differ, the memo is stale and cleared.
    tess_memo_guard: std::cell::Cell<u64>,
    /// One unrestricted block-stub expansion, kept so repeated whole-set
    /// snap/pick queries over the same wires do not redo it. Keyed by the
    /// source wire set's identity and `geometry_epoch`.
    ///
    /// Measured need: on a paper-space viewport the resident-identity gate in
    /// `interaction_candidates_near_impl` fails, so every pointer move took
    /// the unindexed path and re-expanded the same 47 353 wires into the same
    /// 202 830 at ~75 ms. 355 such moves in one recorded session.
    ///
    /// The source is held weakly, not by address: a dropped `Arc` can be
    /// replaced at the same address within one epoch, and a bare pointer
    /// compare would then hand back another set's expansion.
    stub_expansion_cache: RefCell<
        Option<(
            std::sync::Weak<Vec<WireModel>>,
            u64,
            u64,
            Arc<Vec<WireModel>>,
        )>,
    >,
    /// Viewport wire sets with the block lines on their frozen layers left
    /// out, one for each resident set and frozen-set signature: the resident
    /// set they were made from (held weakly, its address in the key) and the
    /// result, `None` when the set holds no such line.
    #[allow(clippy::type_complexity)]
    viewport_frozen_member_cache: RefCell<
        HashMap<(usize, u64), (std::sync::Weak<Vec<WireModel>>, Option<Arc<Vec<WireModel>>>)>,
    >,
    /// Camera-independent model tessellation, separate from the culled memo.
    resident_tess_memo: RefCell<HashMap<Handle, MemoRun>>,
    /// The resident set `resident_tess_memo`'s `InSet` runs read from.
    resident_memo_backing: RefCell<MemoBacking>,
    /// Set while `resident_wires_for` assembles a set that will become the
    /// memo's backing; new runs then go straight into it (`Pending`).
    building_resident_set: std::cell::Cell<bool>,
    /// The factor on complex linetypes in the viewport whose resident set is
    /// being read or built. `None` outside one, or where nothing depends on it.
    viewport_complex_lt: std::cell::Cell<Option<f32>>,
    /// The factor each viewport's complex linetypes are baked at. It stays put
    /// while the view moves, so a wheel tick doesn't rebuild the resident set.
    viewport_complex_lt_baked: RefCell<HashMap<Handle, f32>>,
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
    ///
    /// Keyed by the viewport whose resident set it belongs to. It used to be a
    /// single slot, and on a paper layout with two content viewports that was
    /// the whole reason dragging was slow: each viewport's full rebuild
    /// cleared the other's pending handoff, so neither ever had one, every
    /// `arena-base` test failed, and every dragged pointer move paid a full
    /// ~90 ms `arena-build` per viewport instead of an in-place patch. The
    /// sets are independent and already carry their own generations, so they
    /// get their own entries.
    model_wire_gpu_patch: RefCell<HashMap<Option<Handle>, (u64, u64, Arc<WireGpuPatch>, bool)>>,
    /// Active delta-undo recording, or `None` when no entity-only command is
    /// capturing. Populated by the five mutation primitives via
    /// [`Scene::record_undo_before`]; consumed by the app (`take_undo_recording`)
    /// to build a cheap history delta instead of a full document clone. See
    /// [`UndoRecording`].
    undo_recording: Option<UndoRecording>,
}

fn section_frame(
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
fn is_section_object(entity: &EntityType) -> bool {
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
struct LiveSection {
    data: document::entities::SectionObjectData,
    slice_depth: Option<f64>,
}

fn section_plane(
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

fn keep_section_positive(
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
            sdf_text_cache: RefCell::new(HashMap::default()),
            annotation_highlight_cache: RefCell::new(HashMap::default()),
            outline_steps: RefCell::new(HashMap::default()),
            annotation_selected_cache: RefCell::new(HashMap::default()),
            last_sdf_text: RefCell::new(HashMap::default()),
            // One pane mapped to tile 0 — matches the single default tile above.
            model_panes: iced::widget::pane_grid::State::new(0).0,
            model_pane_min_px: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            selection: std::sync::Arc::new(std::cell::RefCell::new(SelectionState::default())),
            document: Document::new(),
            last_created_dimension: None,
            object_data_cache: crate::entities::object_data::ObjectDataCache::default(),
            lighting_cache: RefCell::new(HashMap::default()),
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
            groups_snapshot: RefCell::new(None),
            #[cfg(any(test, not(target_arch = "wasm32")))]
            selection_fingerprint_cache: 0,
            selection_fingerprint_dirty: false,
            viewport_interaction_sets: RefCell::new(None),
            wire_cache: RefCell::new(None),
            interaction_index_cache: RefCell::new(Vec::new()),
            interaction_index_pending_key: std::cell::Cell::new(None),
            interaction_base_index_cache: RefCell::new(None),
            interaction_overlay_index_cache: RefCell::new(None),
            interaction_overlay_changes: RefCell::new(None),
            interaction_handle_index_cache: RefCell::new(None),
            sort_cache: RefCell::new(None),
            draw_depth_cache: RefCell::new(None),
            block_solid_snaps_cache: RefCell::new(None),
            draw_depth_generation: std::cell::Cell::new(0),
            no_draw_depths: Arc::new(HashMap::default()),
            hatch_cache: RefCell::new(HashMap::default()),
            hatch_hosts: RefCell::new(HashMap::default()),
            selected_hatch_overlay_cache: RefCell::new(None),
            combined_preview_hatch_cache: RefCell::new(None),
            wipeout_cache: RefCell::new(HashMap::default()),
            image_cache: RefCell::new(HashMap::default()),
            mesh_cache: RefCell::new(HashMap::default()),
            interaction_mesh_cache: RefCell::new(None),
            mesh_pick_lookup_cache: RefCell::new(None),
            mesh_pick_boxes_cache: RefCell::new(None),
            frozen_hatch_cache: RefCell::new(HashMap::default()),
            frozen_wipeout_cache: RefCell::new(HashMap::default()),
            frozen_image_cache: RefCell::new(HashMap::default()),
            point_cloud_cache: RefCell::new(HashMap::default()),
            frozen_mesh_cache: RefCell::new(HashMap::default()),
            viewport_style_override_cache: RefCell::new(None),
            view_rep_reference_cache: RefCell::new(None),
            insert_hatch_cache: RefCell::new(None),
            paper_sheet_cache: RefCell::new(HashMap::default()),
            paper_viewport_cache: RefCell::new(HashMap::default()),
            paper_sheet_render_cache: RefCell::new(HashMap::default()),
            display_plot_style_cache: RefCell::new(HashMap::default()),
            styled_wire_cache: RefCell::new(HashMap::default()),
            styled_hatch_cache: RefCell::new(HashMap::default()),
            styled_wire_fill_cache: RefCell::new(HashMap::default()),
            paper_projected_cache: RefCell::new(HashMap::default()),
            current_layout: "Model".to_string(),
            block_edit_block: None,
            block_visibility_reveal: false,
            block_construction_hidden: Default::default(),
            block_constraint_status_mode: false,
            block_constraint_status_colors: ["8".into(), "2".into(), "3".into(), "1".into()],
            constraint_status_cache: Default::default(),
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
            model_extents_cache: RefCell::new(None),
            model_space_block_cache: std::cell::Cell::new(None),
            entity_block_map_cache: RefCell::new(None),
            block_members_cache: RefCell::new(None),
            leaders_by_annotation_cache: RefCell::new(None),
            unindexable_cache: RefCell::new(None),
            layout_type_names_cache: RefCell::new(None),
            layout_epoch: 0,
            unopened_layouts: Default::default(),
            new_layout_plot_style: String::new(),
            layout_names_cache: RefCell::new(None),
            scale_epoch: 0,
            scale_picker_cache: RefCell::new(None),
            ucs_epoch: 0,
            ucs_names_cache: RefCell::new(None),
            dependency_index_cache: RefCell::new(None),
            tables_modified: HashSet::default(),
            associative_hatch_source_cache: RefCell::new(None),
            associative_centerline_source_cache: RefCell::new(None),
            associative_centermark_source_cache: RefCell::new(None),
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
            glyph_cache: RefCell::new(None),
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
            document_render_env_cache: RefCell::new(None),
            background_image_cache: RefCell::new(HashMap::default()),
            block_defn_cache: RefCell::new(HashMap::default()),
            entity_index_cache: RefCell::new(None),
            last_render_aspect: std::cell::Cell::new(16.0 / 9.0),
            last_world_per_pixel: std::cell::Cell::new(0.0),
            last_view_aspect: std::cell::Cell::new(0.0),
            viewcube_hover: std::cell::Cell::new(None),
            // NAVVCUBEOPACITY default until the first cursor move.
            viewcube_opacity: std::cell::Cell::new(0.5),
            pending_refresh: std::cell::RefCell::new(rustc_hash::FxHashSet::default()),
            last_tess_ms: std::cell::Cell::new(0.0),
            last_tess_wires: std::cell::Cell::new(0),
            last_model_wire_gen: std::cell::Cell::new(0),
            last_atlas_generation: std::cell::Cell::new(crate::scene::text::sdf_atlas::generation()),
            nav_last_gen: std::cell::Cell::new(0),
            nav_changed_at: std::cell::Cell::new(None),
            nav_perf_pending: std::cell::Cell::new(None),
            resident_wire_sets: RefCell::new(HashMap::default()),
            split_cache: RefCell::new(HashMap::default()),
            tess_memo: RefCell::new(HashMap::default()),
            tess_memo_guard: std::cell::Cell::new(0),
            stub_expansion_cache: RefCell::new(None),
            viewport_frozen_member_cache: RefCell::new(HashMap::default()),
            resident_tess_memo: RefCell::new(HashMap::default()),
            resident_memo_backing: RefCell::new(MemoBacking::default()),
            building_resident_set: std::cell::Cell::new(false),
            viewport_complex_lt: std::cell::Cell::new(None),
            viewport_complex_lt_baked: RefCell::new(HashMap::default()),
            resident_tess_guard: std::cell::Cell::new(0),
            resident_tess_atlas: std::cell::Cell::new(0),
            tess_memo_atlas: std::cell::Cell::new(0),
            resident_tess_complex_lt: std::cell::Cell::new(0),
            tess_memo_complex_lt: std::cell::Cell::new(0),
            geometry_deltas: RefCell::new(std::collections::VecDeque::new()),
            pending_removed_categories: RefCell::new(HashMap::default()),
            geometry_journal_floor: std::cell::Cell::new(0),
            resident_patch_hits: std::cell::Cell::new(0),
            model_wire_gpu_patch: RefCell::new(HashMap::default()),
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
    /// Rasterise the underlays again when the view's scale has moved to
    /// another quarter-octave step and stayed there briefly (not on every
    /// frame of a zoom), and likewise the OLE pictures of the current layout
    /// and the underlays on its sheet.
    pub fn refresh_underlay_resolution(&mut self, now: iced::time::Instant) {
        let generation = self.camera_generation;
        if let Some((wpp, view)) = self.model_view_scale() {
            let layout = self.current_layout.clone();
            if self
                .underlay_scale
                .settle(wpp, view, &layout, generation, now)
                && self
                    .document
                    .entities()
                    .any(|e| matches!(e, EntityType::Underlay(_)))
            {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // With a sheet in view, the pages of the model alone: one
                    // on a sheet follows that sheet's view (below).
                    let mut sheets = HashSet::default();
                    if layout != "Model" {
                        sheets = self.layout_block_handles();
                        sheets.remove(&self.model_space_block_handle());
                    }
                    self.queue_reseed(|e| {
                        matches!(e, EntityType::Underlay(u)
                            if !sheets.contains(&u.common.owner_handle))
                    });
                }
                #[cfg(target_arch = "wasm32")]
                self.reseed_underlays();
            }
        }
        if let Some(wpp) = self.layout_world_per_pixel() {
            let view = self.layout_plan_view_rect();
            let layout = self.current_layout.clone();
            let sheet_underlays = self.sheet_may_hold_underlays();
            if self.ole_scale.settle(wpp, view, &layout, generation, now)
                && (self.has_ole_frames() || sheet_underlays)
            {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let sheet = self.current_layout_block_handle();
                    self.queue_reseed(|e| match e {
                        EntityType::Ole2Frame(_) => true,
                        EntityType::Underlay(u) => {
                            sheet_underlays && u.common.owner_handle == sheet
                        }
                        _ => false,
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    if self.has_ole_frames() {
                        self.reseed_ole_frames();
                    }
                    if sheet_underlays {
                        self.reseed_underlays();
                    }
                }
            }
        }
    }

    /// The view has moved to another scale step than the underlay or OLE
    /// rasters were made for, or out of their sharp region (frames are
    /// requested until they are made again).
    pub fn underlay_resolution_stale(&self) -> bool {
        let underlays = self.has_underlays()
            && self.model_view_scale().is_some_and(|(wpp, view)| {
                self.underlay_scale.stale(wpp, view, &self.current_layout)
            });
        underlays
            || ((self.has_ole_frames() || self.sheet_may_hold_underlays())
                && self.layout_world_per_pixel().is_some_and(|wpp| {
                    self.ole_scale
                        .stale(wpp, self.layout_plan_view_rect(), &self.current_layout)
                }))
    }

    /// The scale the model is seen at now (model units per pixel) and the
    /// plan rectangle of it in sight: those of the model view, or with a
    /// sheet in view those of its viewports. `None` when the model is not in
    /// sight, or before a frame has told the size of the view.
    fn model_view_scale(&self) -> Option<(f32, Option<[f64; 4]>)> {
        match self.world_per_pixel() {
            Some(wpp) => Some((wpp, self.plan_view_rect())),
            None => self.sheet_model_view(),
        }
    }

    /// A paper layout is in view and the drawing has underlays: some may lie
    /// on its sheet, where they follow the sheet's view.
    fn sheet_may_hold_underlays(&self) -> bool {
        self.current_layout != "Model" && self.has_underlays()
    }

    /// World units per screen pixel of the current layout's view: the model
    /// camera's in Model, the sheet camera's in a paper layout. `None` until
    /// a render captured real bounds.
    fn layout_world_per_pixel(&self) -> Option<f32> {
        let v = self.last_world_per_pixel.get();
        (v > 0.0 && v.is_finite()).then_some(v)
    }

    /// World XY rectangle of the model view when it looks straight down
    /// (orthographic plan view); `None` otherwise.
    fn plan_view_rect(&self) -> Option<[f64; 4]> {
        if self.current_layout != "Model" {
            return None;
        }
        self.layout_plan_view_rect()
    }

    /// World XY rectangle of the current layout's view when it looks straight
    /// down (orthographic plan view) — model space in Model, the sheet in a
    /// paper layout; `None` otherwise.
    fn layout_plan_view_rect(&self) -> Option<[f64; 4]> {
        Self::plan_rect_of(&self.camera.borrow(), self.view_aspect())
    }

    /// World XY rectangle a camera shows in a view `aspect` wide for its
    /// height, when it looks straight down (orthographic plan view); `None`
    /// otherwise.
    pub(super) fn plan_rect_of(cam: &view::camera::Camera, aspect: f64) -> Option<[f64; 4]> {
        if !matches!(cam.projection, view::camera::Projection::Orthographic) {
            return None;
        }
        if (cam.rotation * glam::Vec3::NEG_Z).z > -0.9999 {
            return None;
        }
        let h = f64::from(cam.ortho_size());
        let w = h * aspect;
        let right = (cam.rotation * glam::Vec3::X).as_dvec3();
        let up = (cam.rotation * glam::Vec3::Y).as_dvec3();
        let (hx, hy) = (
            (right.x * w).abs() + (up.x * h).abs(),
            (right.y * w).abs() + (up.y * h).abs(),
        );
        let t = cam.target;
        let rect = [t.x - hx, t.y - hy, t.x + hx, t.y + hy];
        rect.iter().all(|v| v.is_finite()).then_some(rect)
    }

    /// Width over height of the view last drawn (1 before the first frame).
    pub(crate) fn view_aspect(&self) -> f64 {
        let aspect = f64::from(self.last_view_aspect.get());
        if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        }
    }

    /// The root drawing's geographic location (GEODATA), when it has one.
    /// Xref and paper-space GEODATA never become the scene CRS by handle order.
    pub(crate) fn geo_data(&self) -> Option<&document::objects::GeoData> {
        crate::io::crs::model_space_geo_data(&self.document)
    }

    pub(crate) fn refresh_object_data_cache(&mut self) {
        self.object_data_cache = crate::entities::object_data::build_cache(&self.document);
    }

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
        self.last_view_aspect.set(width_px / height_px);
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
        let mut key = annotation_scale_handle
            .map(|handle| handle.value())
            .unwrap_or(0);
        for component in bg {
            key = key.rotate_left(13) ^ component.to_bits() as u64;
        }
        key = key.rotate_left(13) ^ all_visible as u64;
        key = key.rotate_left(13) ^ viewport.map(|handle| handle.value()).unwrap_or(0);
        // Loading a linetype does not move the block epoch, and a cache built
        // before the drawing had a complex one kept no path for its children.
        key = key.rotate_left(13) ^ crate::io::linetypes::defines_complex_lt(&self.document) as u64;
        let stale = {
            let cache = self.block_defn_cache.borrow();
            match cache.get(&key) {
                Some((epoch, arc)) if *epoch == self.block_epoch => return Arc::clone(arc),
                Some((epoch, arc)) => Some((*epoch, Arc::clone(arc))),
                None => None,
            }
        };
        // Only layers were switched since this cache was built: rebuild the
        // definitions that hold their objects, keep the rest.
        if let (Some((epoch, old)), Some((from, to, names))) =
            (stale, self.block_visibility_edits.as_ref())
        {
            if epoch == *from && *to == self.block_epoch {
                let started = crate::perf::enabled().then(iced::time::Instant::now);
                let solid_snaps = self.block_solid_snaps();
                let mut built = old.with_rebuilt_blocks(
                    &self.document,
                    names,
                    1.0,
                    annotation_scale_handle,
                    all_visible,
                    bg,
                    viewport,
                    &self.draw_depth_map(),
                    &solid_snaps,
                );
                if self.constraint_shading_active() {
                    built.disable_prototypes();
                }
                if started.is_some() {
                    crate::perf_record!(
                        "[perf] blk_cache-rebuild total={:.1}ms changed={}",
                        crate::perf::elapsed_ms(started),
                        names.len(),
                    );
                }
                let arc = Arc::new(built);
                let mut cache = self.block_defn_cache.borrow_mut();
                self.bound_block_defn_cache(&mut cache);
                cache.insert(key, (self.block_epoch, Arc::clone(&arc)));
                return arc;
            }
        }
        // Block definitions are cached at block-local size (annotation scale
        // 1.0). An annotative block scales as ONE unit at the INSERT level, so
        // its internal geometry / text / attributes must NOT be scaled
        // individually because that would apply the same scale twice.
        let solid_snaps = self.block_solid_snaps();
        let mut built = cache::block_cache::BlockCache::build_with_solid_snaps(
            &self.document,
            1.0,
            annotation_scale_handle,
            all_visible,
            bg,
            viewport,
            &self.draw_depth_map(),
            &solid_snaps,
        );
        if self.constraint_shading_active() {
            built.disable_prototypes();
        }
        let arc = Arc::new(built);
        let mut cache = self.block_defn_cache.borrow_mut();
        cache.retain(|_, (epoch, _)| *epoch == self.block_epoch);
        self.bound_block_defn_cache(&mut cache);
        cache.insert(key, (self.block_epoch, Arc::clone(&arc)));
        arc
    }

    /// Snap points of the solids inside block definitions. Parsing their
    /// bodies for face centres took over half a second on a drawing with many
    /// block solids, and every viewport's block cache asked again (#2861), so
    /// the map is kept until the geometry changes.
    fn block_solid_snaps(&self) -> Arc<BlockSolidSnaps> {
        if let Some((epoch, snaps)) = self.block_solid_snaps_cache.borrow().as_ref() {
            if *epoch == self.geometry_epoch {
                return Arc::clone(snaps);
            }
        }
        let snaps: Arc<BlockSolidSnaps> = Arc::new(
            self.block_meshes
                .keys()
                .map(|handle| (*handle, self.solid_snap_points(*handle)))
                .filter(|(_, points)| !points.is_empty())
                .collect(),
        );
        *self.block_solid_snaps_cache.borrow_mut() =
            Some((self.geometry_epoch, Arc::clone(&snaps)));
        snaps
    }

    /// Each entry holds every block definition built for one viewport, and
    /// layout switches leave the block epoch alone: one entry per viewport
    /// ever shown piled up until memory ran out (#3260). Keep what one frame
    /// draws (model tiles count as 4) plus a little slack.
    fn bound_block_defn_cache(
        &self,
        cache: &mut HashMap<u64, (u64, Arc<cache::block_cache::BlockCache>)>,
    ) {
        if cache.len() > self.frame_wire_set_count().max(4) + 2 {
            cache.clear();
        }
    }

    /// Install loader-thread geometry into this scene's Model resident cache.
    ///
    /// The target scene has a different epoch from the temporary loader scene,
    /// so only immutable geometry is transferred and re-stamped here. The
    /// optional interaction index is cached against the exact same `Arc`.
    pub fn install_prepared_open_geometry(&self, prepared: PreparedOpenGeometry) {
        // The loader tessellated before the constraints were read, so its
        // wires still hold the dynamic dimensions that must stay hidden.
        if !self.hidden_dynamic_dimensions.is_empty() {
            return;
        }
        *self.dependency_index_cache.borrow_mut() = prepared.dependency_index;
        let block = self.model_space_block_handle();
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        let key = self.resident_wire_key(
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
                estimated_bytes: Self::resident_wire_bytes(&prepared.wires),
                epoch: self.geometry_epoch,
                gen,
                used: std::cell::Cell::new(resident_touch()),
                wires: Arc::clone(&prepared.wires),
                layout: prepared.resident_layout,
            },
        );
        // Adopt the loader thread's tessellation memo so the first edit patches
        // the entities it touched instead of rebuilding every one of them.
        if !prepared.tess_memo.is_empty() {
            *self.resident_tess_memo.borrow_mut() = prepared.tess_memo;
            *self.resident_memo_backing.borrow_mut() = prepared.tess_memo_backing;
            self.resident_tess_guard.set(prepared.tess_guard);
            self.resident_tess_atlas.set(prepared.tess_atlas);
            // The loader tessellates model space, outside any viewport.
            self.resident_tess_complex_lt.set(0);
        }
        // Reuse the loader's draw-order map with this scene's epoch.
        if let Some(mut depths) = prepared.draw_depths {
            depths.epoch = self.geometry_epoch;
            *self.draw_depth_cache.borrow_mut() = Some(depths);
        }
        // Same for the block definitions, stamped with this scene's block
        // epoch: the document has not been touched between the loader
        // finishing and this call, so they describe it exactly.
        if !prepared.block_defns.is_empty() {
            let epoch = self.block_epoch;
            let mut cache = self.block_defn_cache.borrow_mut();
            for (key, defns) in prepared.block_defns {
                cache.insert(key, (epoch, defns));
            }
        }
        if let Some(index) = prepared.interaction_index {
            self.cache_interaction_index(self.geometry_epoch, prepared.wires, index);
        }
        if let Some(index) = prepared.entity_index {
            *self.entity_index_cache.borrow_mut() = Some((self.geometry_epoch, index));
        }
        if let Some((space_key, index)) = prepared.handle_index {
            *self.interaction_handle_index_cache.borrow_mut() =
                Some((self.geometry_epoch, space_key, index));
        }
        if let Some((space_key, hatches)) = prepared.insert_hatches {
            *self.insert_hatch_cache.borrow_mut() = Some((self.geometry_epoch, space_key, hatches));
        }
    }

}

mod invalidation;

impl Scene {

    /// Prepare display geometry without changing the entity or resident caches.
    pub(crate) fn prepare_solid_model_display(
        &self,
        handle: Handle,
        solid: &ocs_solid::model::Brep,
    ) -> Option<(MeshLodSet, Vec<document::entities::Wire>, [f64; 3])> {
        let color = self
            .document
            .get_entity(handle)
            .map(|e| self.render_style(e).0)
            .unwrap_or([0.8, 0.8, 0.85, 1.0]);
        let facet_resolution = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_resolution);
        let (isolines, planar_isolines) = match self.document.get_entity(handle) {
            Some(EntityType::Surface(surface)) => (
                crate::entities::solid3d::surface_isoline_counts(surface),
                Some(crate::entities::solid3d::surface_property_state(surface).isolines),
            ),
            _ => (
                [crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines); 2],
                None,
            ),
        };
        let (mut set, wires, center) = crate::scene::model::solid_model::display_from_solid(
            solid,
            color,
            facet_resolution,
            chordal_deflection,
            isolines,
            planar_isolines,
        )?;
        let name = handle.value().to_string();
        for mesh in &mut set.lods {
            mesh.name = name.clone();
        }
        if let Some(entity) = self.document.get_entity(handle) {
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
        Some((set, wires, center))
    }

    /// Commit a prepared display without tessellating the body a second time.
    pub(crate) fn register_prepared_solid_model(
        &mut self,
        handle: Handle,
        solid: ocs_solid::model::Brep,
        display: (MeshLodSet, Vec<document::entities::Wire>, [f64; 3]),
    ) -> bool {
        let (mut set, wires, center) = display;
        // New command results can be prepared before their handle exists.
        // Apply the committed entity's actual style without retessellating.
        if let Some(entity) = self.document.get_entity(handle) {
            let color = self.render_style(entity).0;
            for mesh in &mut set.lods {
                mesh.name = handle.value().to_string();
                mesh.color = color;
            }
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
        if let Some(entity) = self.document.get_entity_mut(handle) {
            let reference = document::types::Vector3::new(center[0], center[1], center[2]);
            match entity {
                EntityType::Solid3D(entity) => {
                    entity.point_of_reference = reference;
                    entity.wires = wires;
                    entity.silhouettes.clear();
                }
                EntityType::Surface(entity) => {
                    entity.point_of_reference = reference;
                    entity.wires = wires;
                    entity.silhouettes.clear();
                }
                _ => {}
            }
        }
        self.meshes.insert(handle, set);
        self.solid_models.insert(handle, solid);
        self.sync_solid_reference_point(handle);
        self.bump_entities(&[(handle, ChangeKind::Modified)]);
        true
    }

    /// Cache and display a Model-tab kernel solid. A failed preparation leaves
    /// the previous entity, body and mesh untouched.
    pub fn register_solid_model(&mut self, handle: Handle, solid: ocs_solid::model::Brep) -> bool {
        let Some(display) = self.prepare_solid_model_display(handle, &solid) else {
            return false;
        };
        if !display.0.complete
            && matches!(
                self.document.solid_history_operation(handle),
                Some(document::objects::SolidHistoryOperation::Loft(_))
            )
        {
            return false;
        }
        self.register_prepared_solid_model(handle, solid, display)
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
                            material.diffuse = crate::scene::cache::block_cache::refedit_fade(
                                material.diffuse,
                                bg,
                            );
                        }
                    }
                    if self.meshes.contains_key(&h) {
                        material.diffuse = self.count_color(h, material.diffuse, bg);
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
            crate::scene::analysis::apply(set);
        }
    }

    /// Re-tessellates the shaded models and recolours them: the surface analysis turned on or
    /// off, or its options changed.
    pub fn invalidate_solid_meshes(&mut self) {
        self.populate_meshes_from_document();
        self.recolor_meshes();
        self.bump_geometry();
    }

    /// Shows the control net of the NURBS surface `handle` (CVSHOW): its rows and columns of
    /// control vertices drawn over the model until hidden.
    pub fn show_control_net(&mut self, handle: Handle, net: &solidcore_ops::surface_cvs::Net) {
        let mut wires = Vec::new();
        let at = |i: usize, j: usize| net.points[i * net.cols + j];
        let line = |points: Vec<[f64; 3]>, k: usize| {
            let mut wire = WireModel::solid_f64(
                format!("cv-net:{}:{k}", handle.value()),
                points,
                WireModel::HOVER,
                false,
            );
            wire.set_fixed_screen_width(1.5);
            wire
        };
        for i in 0..net.rows {
            wires.push(line((0..net.cols).map(|j| at(i, j)).collect(), wires.len()));
        }
        for j in 0..net.cols {
            wires.push(line((0..net.rows).map(|i| at(i, j)).collect(), wires.len()));
        }
        self.cv_net_wires.insert(handle, wires);
    }

    /// Hides every control net shown (CVHIDE).
    pub fn hide_control_nets(&mut self) {
        self.cv_net_wires.clear();
    }

    /// Whether the control net of `handle` is shown.
    pub fn control_net_shown(&self, handle: Handle) -> bool {
        self.cv_net_wires.contains_key(&handle)
    }

    /// Recolour only the named cached solids after a property edit.
    pub fn recolor_meshes_for_handles(&mut self, handles: &[Handle]) {
        let bg = self.bg_color;
        let materials: HashMap<Handle, crate::scene::model::material_model::MeshMaterial> = handles
            .iter()
            .filter(|handle| {
                self.meshes.contains_key(handle) || self.block_meshes.contains_key(handle)
            })
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
                        material.diffuse =
                            crate::scene::cache::block_cache::refedit_fade(material.diffuse, bg);
                    }
                    if self.meshes.contains_key(&handle) {
                        material.diffuse = self.count_color(handle, material.diffuse, bg);
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
                crate::scene::analysis::apply(set);
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

    /// Enter / leave the COUNT colouring; a no-op when nothing changed.
    /// Entering or leaving fades or restores the whole drawing; a change
    /// inside count mode recolours only the objects whose colour changed.
    pub fn set_count_display(&mut self, display: Option<CountDisplay>) {
        if self.count_display == display {
            return;
        }
        let old = std::mem::replace(&mut self.count_display, display);
        let (Some(old), Some(new)) = (old, self.count_display.as_ref()) else {
            self.hatch_cache.borrow_mut().clear();
            self.recolor_meshes();
            self.bump_geometry_no_blocks();
            return;
        };
        let recoloured = (old.color, old.error_color) != (new.color, new.error_color);
        let class = |d: &CountDisplay, h: &Handle| (d.errors.contains(h), d.counted.contains(h));
        let changed: Vec<Handle> = old
            .counted
            .iter()
            .chain(&old.errors)
            .chain(&new.counted)
            .chain(&new.errors)
            .copied()
            .collect::<HashSet<Handle>>()
            .into_iter()
            .filter(|h| recoloured || class(&old, h) != class(new, h))
            .collect();
        self.recolor_meshes_for_handles(&changed);
        let changes: Vec<(Handle, ChangeKind)> = changed
            .into_iter()
            .map(|h| (h, ChangeKind::Modified))
            .collect();
        self.bump_entities_restyled(&changes);
    }

    /// The colour count mode gives an object drawn as part of `handle` (a
    /// top-level entity): the count colours, else faded; unchanged outside
    /// count mode.
    pub(crate) fn count_color(&self, handle: Handle, color: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
        let color = if self.visibility_revealed(handle) {
            crate::scene::cache::block_cache::fade_toward_bg(color, bg)
        } else {
            color
        };
        let Some(count) = &self.count_display else {
            return color;
        };
        let [r, g, b, _] = if count.errors.contains(&handle) {
            count.error_color
        } else if count.counted.contains(&handle) {
            count.color
        } else {
            return crate::scene::cache::block_cache::fade_toward_bg(color, bg);
        };
        [r, g, b, color[3]]
    }

    /// Fade the colours of wires that belong to entities outside the REFEDIT
    /// keep set (no-op when not editing). The geometry is untouched, so
    /// hit-testing still works on faded entities. In count mode the counted
    /// references and the duplicates take the count colours instead.
    fn apply_refedit_fade(&self, wires: &mut Vec<WireModel>, bg: [f32; 4]) {
        // Block copies kept as stubs draw in their prototype's colour; while
        // copies are recoloured one by one they are expanded instead.
        if (self.count_display.is_some()
            || self.refedit_keep.is_some()
            || self.block_visibility_reveal)
            && wires.iter().any(|wire| wire.stub.is_some())
        {
            *wires = crate::scene::model::instance_model::expand_stubs(wires).into_owned();
        }
        if let Some(count) = &self.count_display {
            for w in wires.iter_mut() {
                let (color, class) = match Self::handle_from_wire_name(&w.name) {
                    Some(h) if count.errors.contains(&h) => (count.error_color, 2u64),
                    Some(h) if count.counted.contains(&h) => (count.color, 1),
                    _ => (
                        crate::scene::cache::block_cache::fade_toward_bg(w.color, bg),
                        0,
                    ),
                };
                w.color = color;
                // Instances of one definition draw in one colour: counted and
                // duplicate references get instance groups of their own.
                // ponytail: tags the two top bits of the source id, which the
                // id counter never reaches.
                if let Some(instance) = w.render_instance.as_mut() {
                    instance.source_id |= class << 62;
                }
            }
        }
        if self.block_visibility_reveal {
            for wire in wires.iter_mut() {
                if Self::handle_from_wire_name(&wire.name)
                    .is_some_and(|handle| self.visibility_revealed(handle))
                {
                    wire.color = crate::scene::cache::block_cache::fade_toward_bg(wire.color, bg);
                    for vertex in &mut wire.text_verts {
                        vertex.color =
                            crate::scene::cache::block_cache::fade_toward_bg(vertex.color, bg);
                    }
                }
            }
        }
        let Some(keep) = &self.refedit_keep else {
            return;
        };
        for w in wires.iter_mut() {
            let handle = Self::handle_from_wire_name(&w.name);
            // A DWF underlay's page stays as it is outside the working set,
            // like an image or a PDF page; only its frame fades. A DGN
            // underlay's content fades with everything else.
            let dwf_page = w.is_display_only()
                && handle.is_some_and(|h| {
                    matches!(
                        self.document.get_entity(h),
                        Some(EntityType::Underlay(u))
                            if u.underlay_type == document::entities::UnderlayType::Dwf
                    )
                });
            let keep_bright = dwf_page || handle.is_some_and(|h| keep.contains(&h));
            if !keep_bright {
                w.color = crate::scene::cache::block_cache::refedit_fade(w.color, bg);
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
            self.persist_current_layout_state();
            self.current_layout = name;
            self.load_current_layout_state();
            self.sync_active_space_to_document();
            self.recolor_meshes();
            *self.wire_cache.borrow_mut() = None;
            *self.interaction_mesh_cache.borrow_mut() = None;
            *self.mesh_pick_lookup_cache.borrow_mut() = None;
            *self.mesh_pick_boxes_cache.borrow_mut() = None;
            *self.insert_hatch_cache.borrow_mut() = None;
        }
    }

    pub fn load_current_layout_state(&mut self) {
        self.display_plot_style_cache
            .borrow_mut()
            .retain(|(layout, _), _| layout != &self.current_layout);
        self.styled_wire_cache.borrow_mut().clear();
        self.styled_hatch_cache.borrow_mut().clear();
        self.styled_wire_fill_cache.borrow_mut().clear();
        if self.current_layout == "Model" {
            return;
        }
        if let Some((flags, insertion_base, min_extents, max_extents, min_limits, max_limits)) =
            self.document.objects.values().find_map(|object| {
                let ObjectType::Layout(layout) = object else {
                    return None;
                };
                (layout.name == self.current_layout).then_some((
                    layout.flags,
                    layout.insertion_base,
                    layout.min_extents,
                    layout.max_extents,
                    layout.min_limits,
                    layout.max_limits,
                ))
            })
        {
            self.document.header.paper_space_linetype_scaling = flags & 1 != 0;
            self.document.header.paper_space_limit_check = flags & 2 != 0;
            self.document.header.paper_space_insertion_base =
                document::types::Vector3::new(insertion_base.0, insertion_base.1, insertion_base.2);
            self.document.header.paper_space_extents_min =
                document::types::Vector3::new(min_extents.0, min_extents.1, min_extents.2);
            self.document.header.paper_space_extents_max =
                document::types::Vector3::new(max_extents.0, max_extents.1, max_extents.2);
            self.document.header.paper_space_limits_min =
                document::types::Vector2::new(min_limits.0, min_limits.1);
            self.document.header.paper_space_limits_max =
                document::types::Vector2::new(max_limits.0, max_limits.1);
        }
    }

    pub fn persist_current_layout_state(&mut self) {
        if self.current_layout == "Model" {
            return;
        }
        let header = &self.document.header;
        let psltscale = header.paper_space_linetype_scaling;
        let plimcheck = header.paper_space_limit_check;
        let insertion_base = header.paper_space_insertion_base;
        let min_extents = header.paper_space_extents_min;
        let max_extents = header.paper_space_extents_max;
        let min_limits = header.paper_space_limits_min;
        let max_limits = header.paper_space_limits_max;
        for object in self.document.objects.values_mut() {
            let ObjectType::Layout(layout) = object else {
                continue;
            };
            if layout.name == self.current_layout {
                layout.flags = if psltscale {
                    layout.flags | 1
                } else {
                    layout.flags & !1
                };
                layout.flags = if plimcheck {
                    layout.flags | 2
                } else {
                    layout.flags & !2
                };
                layout.insertion_base = (insertion_base.x, insertion_base.y, insertion_base.z);
                layout.min_extents = (min_extents.x, min_extents.y, min_extents.z);
                layout.max_extents = (max_extents.x, max_extents.y, max_extents.z);
                layout.min_limits = (min_limits.x, min_limits.y);
                layout.max_limits = (max_limits.x, max_limits.y);
                break;
            }
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

    /// Record the entered viewport (or the sheet, in paper space) as the
    /// current layout's last active viewport, which reopens it (#1670).
    pub fn sync_active_viewport_to_document(&mut self) {
        let Some(handle) = self.current_layout_object_handle() else {
            return;
        };
        let sheet = match self.document.objects.get(&handle) {
            Some(ObjectType::Layout(layout)) => {
                Self::layout_sheet_viewport_handle(&self.document, layout)
            }
            _ => return,
        };
        let active = self.active_viewport.unwrap_or(sheet);
        if let Some(ObjectType::Layout(layout)) = self.document.objects.get_mut(&handle) {
            layout.viewport = active;
        }
    }

    /// The model viewport the current layout was saved with entered: its
    /// last active viewport when that is an on viewport other than the sheet.
    pub fn saved_entered_viewport(&self) -> Option<Handle> {
        let handle = self.current_layout_object_handle()?;
        let Some(ObjectType::Layout(layout)) = self.document.objects.get(&handle) else {
            return None;
        };
        let viewport = layout.viewport;
        let sheet = Self::layout_sheet_viewport_handle(&self.document, layout);
        if viewport.is_null() || viewport == sheet {
            return None;
        }
        match self.document.get_entity(viewport) {
            Some(EntityType::Viewport(vp))
                if vp.common.owner_handle == layout.block_record && !vp.status.is_off =>
            {
                Some(viewport)
            }
            _ => None,
        }
    }

}

mod layout_paper;

impl Scene {
}

mod scale_list;

impl Scene {
    /// True when a top-level entity must be omitted for a session-only object
    /// visibility command or an interactive replacement preview.
    fn entity_temporarily_hidden(&self, handle: Handle) -> bool {
        self.object_isolation.hides(handle)
            || self.preview_hidden.contains(&handle)
            || self.command_preview_hidden.contains(&handle)
            || self.mtext_editor_hidden == Some(handle)
            || self.hidden_dynamic_dimensions.contains(&handle)
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
            // Visibility is a scene concern. Normal edit bookkeeping can
            // regenerate table pictures, prune fields and solve associations.
            let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
            self.geometry_epoch = epoch;
            self.invalidate_projection_bounds();
            for (handle, _) in &changes {
                self.tess_memo.borrow_mut().remove(handle);
                self.resident_tess_memo.borrow_mut().remove(handle);
            }
            self.push_geometry_delta(epoch, changes, false);
        }
    }

    /// Hide only the source replaced by the MTEXT editor, independently from
    /// command previews and table-cell previews.
    pub(crate) fn set_mtext_editor_hidden(&mut self, handle: Option<Handle>) {
        if self.mtext_editor_hidden == handle {
            return;
        }
        let old = std::mem::replace(&mut self.mtext_editor_hidden, handle);
        let changes: Vec<_> = old
            .into_iter()
            .chain(handle)
            .map(|h| (h, ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
            self.geometry_epoch = epoch;
            self.invalidate_projection_bounds();
            for (h, _) in &changes {
                self.tess_memo.borrow_mut().remove(h);
                self.resident_tess_memo.borrow_mut().remove(h);
            }
            self.push_geometry_delta(epoch, changes, false);
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
        //
        // The raw expansion of the new hover is computed once and stored
        // (`hover_expanded`); the old → new comparison reuses the stored old
        // expansion instead of re-running the group scan a second time, and
        // `hover_highlight_handles` (every rendered frame) reads the stored
        // set too.
        let expanded = handle
            .map(|handle| self.handles_expanded_for_selectable_groups(&[handle]))
            .unwrap_or_default();
        let contribution = |expanded: &HashSet<Handle>, selected: &HashSet<Handle>| {
            expanded
                .iter()
                .filter(|handle| !selected.contains(*handle))
                .copied()
                .collect::<HashSet<_>>()
        };
        let changed = contribution(&self.hover_expanded, &self.selected)
            != contribution(&expanded, &self.selected);
        self.hover_highlight = handle;
        self.hover_expanded = expanded;
        if changed {
            self.bump_hover();
        }
    }

    /// Handles that currently contribute the rollover colour. The stored hover
    /// remains the picked member for hit-test/UI bookkeeping; rendering uses
    /// the stored group expansion (`hover_expanded`) — no per-frame scan.
    pub fn hover_highlight_handles(&self) -> HashSet<Handle> {
        let mut handles = self.hover_expanded.clone();
        handles.extend(self.constraint_hover_highlights.iter().copied());
        handles.extend(self.box_hover_highlights.iter().copied());
        handles.extend(self.filter_preview_highlights.iter().copied());
        handles
    }

    /// Cached interaction sets for [`crate::scene::view`]'s `ViewportData`:
    /// `(selected handles, hover handles, selected signature)` keyed by the
    /// bit-encoded `(selection_generation, hover_generation)`. Per rendered
    /// frame per viewport this used to rebuild a fresh selected-set clone,
    /// expand the hover set and hash the selection — idle CPU that scaled
    /// with the selection size even when nothing moved. Cache invalidation
    /// rides the two generations: selection mutations, sub-segment picks and
    /// table-cell picks bump the selection generation; hover / marquee /
    /// constraint / filter sets bump the hover one.
    fn cached_viewport_interaction_sets(
        &self,
        show_interaction: bool,
    ) -> (
        std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
        std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
        u64,
    ) {
        let sel_key = self
            .selection_generation
            .wrapping_mul(2)
            .wrapping_add(u64::from(!show_interaction));
        let hover_key = self
            .hover_generation
            .wrapping_mul(2)
            .wrapping_add(u64::from(!show_interaction));
        {
            let cache = self.viewport_interaction_sets.borrow();
            if let Some((cached_sel, cached_hover, selected, hover, sig)) = cache.as_ref() {
                if *cached_sel == sel_key && *cached_hover == hover_key {
                    return (
                        std::sync::Arc::clone(selected),
                        std::sync::Arc::clone(hover),
                        *sig,
                    );
                }
            }
        }
        let selected = if show_interaction {
            let cells = self.table_cells_handle();
            self.selected
                .iter()
                .copied()
                .filter(|handle| self.picked_segments(*handle).is_none())
                .filter(|handle| Some(*handle) != cells)
                .collect()
        } else {
            rustc_hash::FxHashSet::default()
        };
        let hover = if show_interaction {
            self.hover_highlight_handles()
        } else {
            rustc_hash::FxHashSet::default()
        };
        let sig = if show_interaction {
            self.selected_set_sig()
        } else {
            0
        };
        let selected = std::sync::Arc::new(selected);
        let hover = std::sync::Arc::new(hover);
        *self.viewport_interaction_sets.borrow_mut() = Some((
            sel_key,
            hover_key,
            std::sync::Arc::clone(&selected),
            std::sync::Arc::clone(&hover),
            sig,
        ));
        (selected, hover, sig)
    }

    #[cfg(test)]
    pub(crate) fn box_hover_highlights(&self) -> &HashSet<Handle> {
        &self.box_hover_highlights
    }

    /// Roll over everything a selection box still being drawn would catch,
    /// before the release commits it. An empty slice clears the preview.
    pub fn set_box_hover_highlights(&mut self, handles: &[Handle]) {
        self.set_box_hover_highlights_limited(handles, usize::MAX);
    }

    pub fn set_box_hover_highlights_limited(&mut self, handles: &[Handle], limit: usize) {
        // Over the limit the preview used to be dropped *entirely* — a huge
        // marquee gave no feedback at all (and expanding tens of thousands of
        // candidates per tick was pure waste). Cap to the limit instead,
        // sorted ascending by handle so the highlighted subset stays stable
        // across ticks instead of flickering with the hash-set order. Group
        // expansion can multiply the count, so the cap applies to the
        // expanded set too.
        let mut handles: Vec<Handle> = handles.to_vec();
        if handles.len() > limit {
            handles.sort_unstable_by_key(|handle| handle.value());
            handles.dedup();
            handles.truncate(limit);
        }
        let mut handles = if handles.is_empty() {
            HashSet::default()
        } else {
            self.handles_expanded_for_selectable_groups(&handles)
        };
        if handles.len() > limit {
            let mut ordered: Vec<Handle> = handles.iter().copied().collect();
            ordered.sort_unstable_by_key(|handle| handle.value());
            handles.clear();
            handles.extend(ordered[..limit].iter().copied());
        }
        if handles != self.box_hover_highlights {
            self.box_hover_highlights = handles;
            // Box preview only ever changes the marquee highlight; bumping the
            // selection generation would rebuild the committed selection's GPU
            // overlay on every ~16 ms tick.
            self.bump_hover();
        }
    }

    /// Highlight what a selection filter would find (#2611). Unlike the box
    /// preview, pointer moves don't clear it; an empty slice does.
    pub fn set_filter_preview_highlights(&mut self, handles: &[Handle]) {
        let handles: HashSet<Handle> = handles.iter().copied().collect();
        if handles != self.filter_preview_highlights {
            self.filter_preview_highlights = handles;
            self.bump_hover();
        }
    }

    pub fn set_constraint_hover_highlights(
        &mut self,
        references: &[crate::scene::parametric_constraints::ParametricRef],
    ) {
        let mut references = references.to_vec();
        references.dedup();
        if references == self.constraint_hover_refs {
            return;
        }

        let mut handles = HashSet::default();
        let mut wires = Vec::new();
        for reference in &references {
            let Some(segment) = reference.segment_index() else {
                handles.insert(reference.entity);
                continue;
            };
            let Some(entity) = self.document.get_entity(reference.entity) else {
                continue;
            };
            let Some(mut curve) = crate::entities::curve::entity_curve(entity) else {
                handles.insert(reference.entity);
                continue;
            };
            let Some(segment_curve) = curve.curve.segments().into_iter().nth(segment) else {
                continue;
            };
            curve.curve = segment_curve;
            let mut wire = WireModel::solid_f64(
                format!("constraint-segment:{}:{segment}", reference.entity.value()),
                curve.tessellate(8.0),
                WireModel::HOVER,
                false,
            );
            wire.set_fixed_screen_width(2.0);
            wires.push(wire);
        }

        self.constraint_hover_refs = references;
        self.constraint_hover_highlights = handles;
        self.constraint_hover_wires = wires;
        self.bump_hover();
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
        // Isolation hides entities the memoised glyph placements read
        // (`entity_temporarily_hidden`), so invalidate them too.
        self.bump_constraints_epoch();
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
        self.selected_order.clear();
        self.bump_selection_set();
        // Newly hidden entities change `entity_temporarily_hidden`, which the
        // memoised glyph placements read — invalidate them too.
        self.bump_constraints_epoch();
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
        // Restoring isolated entities changes `entity_temporarily_hidden`,
        // which the memoised glyph placements read — invalidate them too.
        // (`reset_transient_visibility` needs no bump: it only runs on
        // doc-replace, where `load_parametric_constraints_from_document`
        // already bumps the epoch for the whole-set replace.)
        self.bump_constraints_epoch();
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Top-level objects of the current space that the drawing itself saves
    /// as invisible (`EntityCommon::invisible`, DXF 60) — e.g. objects another
    /// CAD program hid with HIDEOBJECTS and wrote to the file. Only objects
    /// owned by the space count: inside block definitions (and in the block
    /// editor) the flag drives dynamic-block visibility states.
    pub fn persisted_hidden_objects(&self) -> Vec<Handle> {
        if self.block_edit_block.is_some() {
            return Vec::new();
        }
        let space = self.interaction_block_handle();
        if space.is_null() {
            return Vec::new();
        }
        self.document
            .entities()
            .filter(|entity| {
                let common = entity.common();
                common.invisible
                    && !common.handle.is_null()
                    && self.belongs_to_visible_block(common.handle, common.owner_handle, space)
            })
            .map(|entity| entity.common().handle)
            .collect()
    }

    /// True when UNISOLATEOBJECTS has something to show: a session filter
    /// from ISOLATEOBJECTS / HIDEOBJECTS, or objects saved as invisible in
    /// the current space.
    pub fn has_hidden_objects(&self) -> bool {
        if self.object_isolation.is_active() {
            return true;
        }
        let space = if self.block_edit_block.is_some() {
            Handle::NULL
        } else {
            self.interaction_block_handle()
        };
        if let Some((epoch, cached_space, any)) = self.persisted_hidden_cache.get() {
            if cached_space == space {
                if epoch == self.geometry_epoch {
                    return any;
                }
                // The status bar asks on every frame and the answer is a
                // scan of every object in the space. Fold the edits since
                // the cached answer instead where they cannot change it:
                // with no hidden object before, only an edited object that
                // is now hidden can make one; with one before, only adds
                // leave it standing. Anything else scans again.
                if let Some(changes) = self.replay_since(epoch) {
                    let unchanged = if any {
                        changes
                            .iter()
                            .all(|(_, kind)| matches!(kind, ChangeKind::Added))
                    } else {
                        changes.iter().all(|(handle, _)| {
                            self.document
                                .get_entity(*handle)
                                .is_none_or(|entity| !entity.common().invisible)
                        })
                    };
                    if unchanged {
                        self.persisted_hidden_cache
                            .set(Some((self.geometry_epoch, space, any)));
                        return any;
                    }
                }
            }
        }
        let any = !self.persisted_hidden_objects().is_empty();
        self.persisted_hidden_cache
            .set(Some((self.geometry_epoch, space, any)));
        any
    }

    /// Clear the saved invisible flag of every object
    /// [`Self::persisted_hidden_objects`] reports. Returns each changed
    /// object's previous state for undo.
    pub fn show_persisted_hidden_objects(&mut self) -> Vec<(Handle, Arc<EntityType>)> {
        let mut originals = Vec::new();
        for handle in self.persisted_hidden_objects() {
            let Some(original) = self.document.get_entity_arc(handle) else {
                continue;
            };
            if let Some(entity) = self.document.get_entity_mut(handle) {
                entity.common_mut().invisible = false;
                originals.push((handle, original));
            }
        }
        let changes: Vec<_> = originals
            .iter()
            .map(|(handle, _)| (*handle, ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            self.bump_constraints_epoch();
            self.bump_entities(&changes);
        }
        originals
    }

    /// A newly opened/replaced document starts with no session visibility.
    /// Persisted `EntityCommon::invisible` values stay untouched and continue
    /// to serve file/dynamic-block visibility. Covers LAYWALK too: the walk
    /// is a session override, never a drawing property.
    pub fn reset_transient_visibility(&mut self) {
        self.block_visibility_reveal = false;
        self.object_isolation = ObjectIsolationState::default();
        self.layer_walk = LayerWalkState::default();
        self.preview_hidden.clear();
        self.command_preview_hidden.clear();
        self.mtext_editor_hidden = None;
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
                matches!(
                    self.document.get_entity(*handle),
                    Some(EntityType::Viewport(_))
                )
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Viewport(_))
                    )
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
                matches!(
                    self.document.get_entity(*handle),
                    Some(EntityType::Viewport(_))
                )
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Viewport(_))
                    )
                })
            })
    }

    fn viewport_scale_handle(&self, viewport: Handle) -> Option<Handle> {
        if let Some(handle) = self.document.viewport_annotation_scale(viewport) {
            if matches!(
                self.document.objects.get(&handle),
                Some(ObjectType::Scale(_))
            ) {
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
                    Some((*handle, (self.effective_scale_factor(scale) - factor).abs()))
                }
                _ => None,
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(handle, _)| handle)
    }

    /// The name of the annotation scale a viewport shows its annotative
    /// objects at.
    pub(crate) fn viewport_annotation_scale_name(&self, viewport: Handle) -> Option<String> {
        match self
            .document
            .objects
            .get(&self.viewport_scale_handle(viewport)?)
        {
            Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
            _ => None,
        }
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
        // Same viewport the pill number comes from (active → selected →
        // first-content fallback), so label, number and badge always describe
        // one viewport (#48). Degenerate case: with zero Scale objects the
        // name falls back to paper/"1:1" while the number stays factual.
        // The render paths use displayed_annotation_scale_HANDLE, which stays
        // explicit-only — untouched by this fix.
        self.target_viewport_handle()
            .and_then(|viewport| self.viewport_scale_handle(viewport))
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                _ => None,
            })
            .or_else(|| {
                self.paper_annotation_scale_handle().and_then(|handle| {
                    match self.document.objects.get(&handle) {
                        Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                        _ => None,
                    }
                })
            })
            .unwrap_or_else(|| "1:1".to_string())
    }

    /// The viewport(s) of the current paper layout, excluding the layout's
    /// own sheet viewport (id 1). Used by commands that act on a layout
    /// viewport without the user picking one.
    pub fn layout_viewport_handles(&self) -> Vec<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Viewport(vp) if vp.id > 1 && vp.common.owner_handle == layout_block => {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    /// The effective paper-to-model scale (paper units per model unit) that
    /// `viewport` is displaying at right now.
    pub fn viewport_effective_scale(&self, viewport: Handle) -> Option<f64> {
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };
        Some(crate::scene::vp_effective_scale(
            vp.custom_scale,
            vp.view_height,
            vp.height,
        ))
    }

    /// Display name of the named scale object driving `viewport`, if any.
    pub fn viewport_display_scale_name(&self, viewport: Handle) -> Option<String> {
        let scale_handle = self.viewport_scale_handle(viewport)?;
        match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
            _ => None,
        }
    }

    pub fn set_viewport_scale_named(&mut self, name: &str) -> Option<Handle> {
        let viewport = self.scale_target_viewport()?;
        self.set_viewport_scale_named_for(viewport, name)
    }

    /// The viewport a scale picked from the list applies to: the active or a
    /// selected one, else the layout's only viewport. With several and none
    /// picked there is none, so no viewport changes behind the user's back.
    pub fn scale_target_viewport(&self) -> Option<Handle> {
        self.explicit_viewport_handle().or_else(|| {
            let (_, _, content) = self.paper_viewport_handles();
            match content.as_slice() {
                [only] => Some(*only),
                _ => None,
            }
        })
    }

    /// Whether `viewport`'s display is locked against zoom and scale changes.
    pub fn viewport_display_locked(&self, viewport: Handle) -> bool {
        matches!(
            self.document.get_entity(viewport),
            Some(EntityType::Viewport(vp)) if vp.status.locked
        )
    }

    pub fn set_viewport_scale_named_for(&mut self, viewport: Handle, name: &str) -> Option<Handle> {
        let scale_handle = self.scale_handle_ensuring(name)?;

        // Honor legacy standard ratios without converting custom/file ratios twice.
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => self.effective_scale_factor(scale),
            _ => return None,
        };

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
        } else {
            return None;
        }

        self.document
            .set_viewport_annotation_scale(viewport, scale_handle);

        // Viewport scale changed: the dimensions drawn on the sheet
        // through it measure the same model geometry at a new paper
        // size and have to be re-placed.
        self.notify_viewport_changed(viewport);
        self.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();

        Some(scale_handle)
    }

    /// Scale the active viewport to `factor` paper units per model unit
    /// (ZOOM nXP). A named scale of that factor is linked as its annotation
    /// scale; otherwise the viewport keeps a custom scale.
    pub fn set_viewport_scale_factor(&mut self, factor: f64) -> bool {
        let Some(viewport) = self.explicit_viewport_handle() else {
            return false;
        };
        self.set_viewport_scale_factor_for(viewport, factor)
    }

    /// Scale `viewport` to `factor` paper units per model unit, as
    /// [`Self::set_viewport_scale_factor`] does for the active one.
    pub fn set_viewport_scale_factor_for(&mut self, viewport: Handle, factor: f64) -> bool {
        let named = self
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::Scale(scale)
                    if !scale.is_temporary
                        && (self.effective_scale_factor(scale) - factor).abs()
                            <= 1.0e-9 * factor.max(1.0) =>
                {
                    Some(scale.name.clone())
                }
                _ => None,
            });
        if let Some(name) = named {
            return self.set_viewport_scale_named_for(viewport, &name).is_some();
        }
        match self.document.get_entity_mut(viewport) {
            Some(EntityType::Viewport(vp)) if !vp.status.locked && factor > 1.0e-9 => {
                vp.custom_scale = factor;
                vp.view_height = vp.height / factor;
            }
            _ => return false,
        }
        self.notify_viewport_changed(viewport);
        self.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        true
    }

    pub fn viewport_annotation_scale_synced(&self) -> Option<bool> {
        // Same viewport as the pill number and label (#48).
        let viewport = self.target_viewport_handle()?;
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };

        let scale = self.viewport_scale_handle(viewport)?;
        let ObjectType::Scale(scale) = self.document.objects.get(&scale)? else {
            return None;
        };

        let effective = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);

        let expected = self.effective_scale_factor(scale);

        Some((effective - expected).abs() <= 1.0e-6 * expected.max(1.0))
    }
    pub fn sync_viewport_annotation_scale(&mut self) -> bool {
        // Same fallback as the pill and the displayed scale name: an active
        // viewport wins, then a picked one, then the first with content.
        let Some(viewport) = self.target_viewport_handle() else {
            return false;
        };
        let Some(scale_handle) = self.viewport_scale_handle(viewport) else {
            return false;
        };
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => self.effective_scale_factor(scale),
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
        // Viewport scale changed: the dimensions drawn on the sheet
        // through it measure the same model geometry at a new paper
        // size and have to be re-placed.
        self.notify_viewport_changed(viewport);
        self.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        true
    }

    /// Sorted list of layout names: "Model" first, then paper layouts by tab order.
    pub fn layout_names(&self) -> Vec<String> {
        let mut names = vec!["Model".to_string()];
        // Deduplicate by name: prefer the entry with a non-null block_record (the
        // real layout from the file) over the default placeholder created by
        // Document::new().
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

    pub fn bump_layout_epoch(&mut self) {
        self.layout_epoch += 1;
    }

    pub fn cached_layout_names(&self) -> std::sync::Arc<Vec<String>> {
        if let Some((epoch, names)) = self.layout_names_cache.borrow().as_ref() {
            if *epoch == self.layout_epoch {
                return std::sync::Arc::clone(names);
            }
        }
        let names = std::sync::Arc::new(self.layout_names());
        *self.layout_names_cache.borrow_mut() =
            Some((self.layout_epoch, std::sync::Arc::clone(&names)));
        names
    }

    pub fn bump_scale_epoch(&mut self) {
        self.scale_epoch += 1;
    }

    /// Current parametric-constraint-set epoch (cache key input for the
    /// memoised `cached_glyph_placements` accessor).
    #[allow(dead_code)]
    pub(crate) fn constraints_epoch(&self) -> u64 {
        self.constraints_epoch
    }

    /// Drops the named parameters in `names` that no constraint drives any more.
    ///
    /// A parameter declared by a constraint goes with that constraint, but a
    /// name any constraint set still drives stays. The scan spans every set
    /// because a block set may name a drawing-level parameter. Names that only
    /// formulas read are not consulted, so the rule errs toward keeping.
    pub(crate) fn drop_unread_named_parameters(&mut self, names: Vec<String>) {
        let unread: Vec<String> = names
            .into_iter()
            .filter(|name| {
                !self
                    .parametric_constraints
                    .iter()
                    .flat_map(|set| set.constraints.iter())
                    .any(|constraint| {
                        matches!(
                            &constraint.driving_param,
                            Some(crate::scene::named_parameters::DrivingValue::Named(used))
                                if used == name
                        )
                    })
            })
            .collect();
        if unread.is_empty() {
            return;
        }
        self.record_undo_named_parameters_before();
        for name in &unread {
            self.named_parameters_mut().remove(name);
        }
    }

    /// Invalidate constraint-glyph consumers. Call on EVERY mutation of the
    /// constraint set or its visibility overrides — a missed bump serves
    /// stale glyphs.
    pub(crate) fn bump_constraints_epoch(&mut self) {
        self.constraints_epoch += 1;
    }

    pub fn cached_scale_picker_list(&self) -> std::sync::Arc<Vec<(String, f32, f64)>> {
        let factor = self.default_scale_unit_factor();
        let imperial = self.prefers_imperial_scales();
        let current = self.document.header.current_annotation_scale.clone();
        if let Some((epoch, cached_factor, cached_imperial, cached_current, list)) =
            self.scale_picker_cache.borrow().as_ref()
        {
            if *epoch == self.scale_epoch
                && *cached_factor == factor
                && *cached_imperial == imperial
                && *cached_current == current
            {
                return std::sync::Arc::clone(list);
            }
        }
        let list = std::sync::Arc::new(self.scale_picker_list());
        *self.scale_picker_cache.borrow_mut() = Some((
            self.scale_epoch,
            factor,
            imperial,
            current,
            std::sync::Arc::clone(&list),
        ));
        list
    }

    pub fn bump_ucs_epoch(&mut self) {
        self.ucs_epoch += 1;
    }

    /// Named-UCS names for the ViewCube picker, memoised behind `ucs_epoch`
    /// (#34, mirrors `cached_layout_names`). Same empty-name filter the view
    /// blocks used when building the list inline every frame.
    pub fn cached_ucs_names(&self) -> std::sync::Arc<[String]> {
        if let Some((epoch, names)) = self.ucs_names_cache.borrow().as_ref() {
            if *epoch == self.ucs_epoch {
                return std::sync::Arc::clone(names);
            }
        }
        let names: std::sync::Arc<[String]> = self
            .document
            .ucss
            .iter()
            .map(|u| u.name.clone())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .into();
        *self.ucs_names_cache.borrow_mut() = Some((self.ucs_epoch, std::sync::Arc::clone(&names)));
        names
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
    #[doc(hidden)]
    pub fn model_tile_wires_arc(
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
}

mod resident_wires;

/// What [`Scene::tessellation_context`] resolves: the background colour, the
/// annotation scale and its handle, the block cache, and whether the layout
/// is a paper layout.
type TessellationContext = (
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

impl Scene {
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
        wires.extend(self.viewport_content_wires(layout_block, None, None, false, None));
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
        let wires = self.entity_wires_arc();
        match crate::scene::model::instance_model::expand_stubs(&wires) {
            std::borrow::Cow::Borrowed(_) => wires,
            std::borrow::Cow::Owned(expanded) => Arc::new(expanded),
        }
    }

    /// Return paper entities and projected model-viewport entities separately.
    /// A plot-only render override is applied to cloned wires; viewport entities
    /// and their saved display modes remain unchanged.
    pub fn plot_wire_groups(
        &self,
        render_mode_override: Option<document::entities::ViewportRenderMode>,
    ) -> (Vec<WireModel>, Vec<WireModel>) {
        let _constraint_display = self.suppress_constraint_shading();
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
            let mut wires =
                crate::scene::model::instance_model::expand_stubs(&self.entity_wires_arc())
                    .into_owned();
            // Model space plots in plan, so solids show their top view.
            let mode = render_mode_override.unwrap_or_else(|| self.active_model_tile_render_mode());
            wires.extend(self.mesh_plot_wires(
                &self.meshes_arc(),
                [0.0, 0.0, 1.0],
                None,
                Some(mode),
            ));
            apply_mode(&mut wires, mode);
            // On screen the wire shader multiplies dash patterns by this ratio;
            // a plot has no shader, so apply it to the patterns themselves.
            if let Some(ratio) = view::render::model_linetype_ratio(&self.document) {
                for wire in wires.iter_mut().filter(|wire| wire.pattern_length > 0.0) {
                    wire.pattern_length *= ratio;
                    wire.pattern = wire.pattern.map(|value| value * ratio);
                }
            }
            return (wires, Vec::new());
        }
        let paper_block = self.current_layout_block_handle();
        let (_, _, viewport_handles) = self.paper_viewport_handles();
        let mut model_wires = Vec::new();
        for handle in viewport_handles.iter().copied() {
            let Some(EntityType::Viewport(viewport)) = self.document.get_entity(handle) else {
                continue;
            };
            if viewport.common.owner_handle != paper_block || !viewport.is_on() {
                continue;
            }
            // The viewport's shade plot: 1 wireframe, 2 hidden, 3 rendered;
            // anything else plots as displayed.
            let mode = render_mode_override.unwrap_or(match viewport.shade_plot_mode {
                1 => document::entities::ViewportRenderMode::Wireframe2D,
                2 => document::entities::ViewportRenderMode::HiddenLine,
                3 => document::entities::ViewportRenderMode::GouraudShaded,
                _ => viewport.render_mode,
            });
            let mut wires =
                self.viewport_content_wires(paper_block, Some(handle), None, true, Some(mode));
            apply_mode(&mut wires, mode);
            model_wires.extend(wires);
        }
        let paper =
            crate::scene::model::instance_model::expand_stubs(&self.paper_sheet_wires_arc())
                .into_owned();
        (paper, model_wires)
    }

    pub(crate) fn plot_wire_depths(&self, wires: &[WireModel]) -> Vec<f32> {
        let depths = self.draw_depth_map();
        wires
            .iter()
            .map(|wire| pipeline::wire_gpu::wire_draw_depth(wire, depths.as_ref()))
            .collect()
    }

    /// Invalidate only the draw-order depth cache without dropping geometry tessellation.
    pub fn invalidate_draw_depth(&self) {
        *self.draw_depth_cache.borrow_mut() = None;
    }

    /// Per-entity stable draw-order depth, keyed by entity handle value.
    /// A full build assigns sparse labels in effective draw order. Incremental
    /// Add/Remove then changes only the named handle: existing siblings retain
    /// their depth, avoiding an O(all entities) map rewrite and GPU const upload.
    /// See [`Self::draw_depth_generation`].
    pub(in crate::scene) fn draw_depth_generation(&self) -> u64 {
        self.draw_depth_generation.get()
    }

    #[doc(hidden)]
    pub fn draw_depth_map(&self) -> Arc<HashMap<u64, [f32; 2]>> {
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
                                    .and_then(|order| draw_depth_entry(order, value));
                                let Some(entry) = restored else {
                                    incremental_ok = false;
                                    break;
                                };
                                depths
                                    .insert(value, draw_depth_value(entry.label, entry.half_label));
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

        // From here the labels are assigned afresh, so an existing entity's
        // depth can move. Anything holding a baked depth has to rebuild.
        self.draw_depth_generation
            .set(self.draw_depth_generation.get().wrapping_add(1));

        use document::objects::ObjectType;
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
        let total_entries: usize = by_block.values().map(Vec::len).sum();
        let mut depth_map: HashMap<u64, [f32; 2]> =
            HashMap::with_capacity_and_hasher(total_entries, Default::default());
        let mut owners: HashMap<u64, Handle> =
            HashMap::with_capacity_and_hasher(total_entries, Default::default());
        let mut blocks: HashMap<Handle, Vec<DrawDepthEntry>> =
            HashMap::with_capacity_and_hasher(by_block.len(), Default::default());
        for (block, mut order) in by_block {
            order.sort_unstable_by_key(|(handle, effective)| (*effective, *handle));
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

    #[cfg(test)]
    pub(super) fn hatch_models_arc(&self) -> Arc<Vec<HatchModel>> {
        self.hatch_models_arc_for_view(true)
    }

    /// Whether `handle` is an INSERT that can place a hatch fill, because a
    /// block it references, directly or nested, holds one. An INSERT of plain
    /// geometry can move without touching any hatch cache. `memo` is the
    /// per-block answer shared across the handles of one validity check.
    fn insert_may_place_hatches(
        &self,
        handle: Handle,
        memo: &RefCell<std::collections::HashMap<String, bool>>,
    ) -> bool {
        let Some(entity @ EntityType::Insert(_)) = self.document.get_entity(handle) else {
            return false;
        };
        render_graph::entity_render_block_uses(&self.document, entity, self.annotation_scale)
            .into_iter()
            .filter(|block_use| block_use.active)
            .any(|block_use| {
                render_graph::block_contains_hatch(
                    &self.document,
                    &block_use.insert.block_name,
                    &mut memo.borrow_mut(),
                )
            })
    }

    /// Bring a stale resident hatch list up to date by redoing only the
    /// INSERTs that changed since it was built. `None` whenever that is not
    /// provably equivalent to a full rebuild (no replayable journal, a direct
    /// fill or an unknown reference changed), and the caller rebuilds.
    fn patch_hatch_models(
        &self,
        key: &(Handle, String),
        target_block: Handle,
        scale: Option<Handle>,
        sel_sig: u64,
    ) -> Option<Arc<Vec<HatchModel>>> {
        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let (epoch, cached) = {
            let cache = self.hatch_cache.borrow();
            let (epoch, cached_sel, arc) = cache.get(key)?;
            if *cached_sel != sel_sig {
                return None;
            }
            (*epoch, Arc::clone(arc))
        };
        let hosts = {
            let held = self.hatch_hosts.borrow();
            let (weak, hosts) = held.get(key)?;
            if !weak
                .upgrade()
                .is_some_and(|live| Arc::ptr_eq(&live, &cached))
                || hosts.len() != cached.len()
            {
                return None;
            }
            Arc::clone(hosts)
        };
        let changes = self.replay_since(epoch)?;
        let known_hosts: rustc_hash::FxHashSet<Handle> = hosts
            .iter()
            .copied()
            .filter(|host| !host.is_null())
            .collect();
        let memo = RefCell::new(std::collections::HashMap::new());
        let mut redo: rustc_hash::FxHashSet<Handle> = rustc_hash::FxHashSet::default();
        let mut rebuilt_inserts: rustc_hash::FxHashSet<Handle> = rustc_hash::FxHashSet::default();
        let mut rebuilt_direct = Vec::new();
        for (handle, _) in &changes {
            if self.hatches.contains_key(handle) {
                // Direct hatches can be updated by handle. A hatch inside a
                // block can affect many placements and still needs the full
                // path.
                let entity = self.document.get_entity(*handle)?;
                if !self.belongs_to_visible_block(
                    *handle,
                    entity.common().owner_handle,
                    target_block,
                ) {
                    return None;
                }
                redo.insert(*handle);
                rebuilt_direct.push(*handle);
                continue;
            }
            match self.document.get_entity(*handle) {
                // Gone: harmless when it drew fills as a reference, unknown
                // otherwise (it may have been a direct fill).
                None => {
                    if !known_hosts.contains(handle) {
                        return None;
                    }
                    redo.insert(*handle);
                }
                Some(EntityType::Insert(_)) => {
                    redo.insert(*handle);
                    rebuilt_inserts.insert(*handle);
                }
                // Another kind of reference (a dimension's arrowhead block,
                // say) is not rebuilt per host here.
                Some(entity) => {
                    if known_hosts.contains(handle)
                        || render_graph::entity_render_block_uses(
                            &self.document,
                            entity,
                            self.annotation_scale,
                        )
                        .into_iter()
                        .filter(|block_use| block_use.active)
                        .any(|block_use| {
                            render_graph::block_contains_hatch(
                                &self.document,
                                &block_use.insert.block_name,
                                &mut memo.borrow_mut(),
                            )
                        })
                    {
                        return None;
                    }
                }
            }
        }
        let fresh = if rebuilt_inserts.is_empty() {
            Vec::new()
        } else {
            self.hatch_models_for_hosts(
                target_block,
                &rebuilt_inserts,
                scale,
                self.annotation_all_visible(),
            )
        };
        let mut models = Vec::with_capacity(cached.len());
        let mut new_hosts = Vec::with_capacity(cached.len());
        for (model, host) in cached.iter().zip(hosts.iter()) {
            if redo.contains(host) {
                continue;
            }
            models.push(model.clone());
            new_hosts.push(*host);
        }
        if !rebuilt_direct.is_empty() {
            let all_visible = self.annotation_all_visible();
            let bg = if self.current_layout == "Model" {
                self.bg_color
            } else {
                self.paper_bg_color
            };
            let depths = self.draw_depth_map();
            for handle in rebuilt_direct {
                if !self.hatch_fill_shown(handle, target_block, None, scale, all_visible) {
                    continue;
                }
                let (backdrop, fill) = self.synced_hatch_model(
                    handle,
                    &self.hatches[&handle],
                    scale,
                    None,
                    bg,
                    &depths,
                    false,
                );
                for model in backdrop.into_iter().chain(std::iter::once(fill)) {
                    models.push(model);
                    new_hosts.push(handle);
                }
            }
        }
        let fresh_count = fresh.len();
        for (host, model) in fresh {
            new_hosts.push(host);
            models.push(model);
        }
        let arc = Arc::new(models);
        self.hatch_cache.borrow_mut().insert(
            key.clone(),
            (self.geometry_epoch, sel_sig, Arc::clone(&arc)),
        );
        self.hatch_hosts
            .borrow_mut()
            .insert(key.clone(), (Arc::downgrade(&arc), Arc::new(new_hosts)));
        if let Some(started) = started {
            crate::perf_record!(
                "[perf] hatch-models patch={:.1}ms hosts={} fresh={} models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                redo.len(),
                fresh_count,
                arc.len(),
            );
        }
        Some(arc)
    }

    fn hatch_models_arc_for_view(&self, requested_tint_selected: bool) -> Arc<Vec<HatchModel>> {
        if self.constraint_shading_active() {
            let _ = self.block_constraint_report(self.block_edit_block.unwrap());
        }
        // Model-space selection is rendered as a tiny overlay. Keep the
        // resident model set stable across select/deselect so its expensive
        // tessellation/style sync and GPU buffers remain warm.
        let tint_selected = requested_tint_selected && self.current_layout != "Model";
        // Hatch models bake the selection tint (issue #71), so they depend on
        // the selected set in paper layouts, but NOT on hover. Keying on
        // `selection_generation` (which also bumps on every hover) made each
        // hover-over a new entity rebuild every hatch model.
        let sel_sig = if tint_selected {
            self.selected_hatch_sig()
        } else {
            0
        };
        let target_block = self.content_render_block_handle();
        let hatch_memo = RefCell::new(std::collections::HashMap::new());
        let key = (
            target_block,
            if self.constraint_shading_active() {
                format!("{}:constraint-status", self.current_layout)
            } else {
                self.current_layout.clone()
            },
        );
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
                                        || self.insert_may_place_hatches(h, &hatch_memo)
                                        // An INSERT that drew fills before
                                        // the edit (repointed to a plain
                                        // block) still changes the list.
                                        || self.hatch_hosts.borrow().get(&key).is_some_and(
                                            |(_, hosts)| hosts.contains(&h),
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
        // Out of date, but when the journal reaches back to the cached list
        // and only some INSERTs changed, redo just those references' fills.
        if !tint_selected && !self.constraint_shading_active() {
            if let Some(arc) = self.patch_hatch_models(&key, target_block, scale, sel_sig) {
                return arc;
            }
        }
        let build_started = crate::perf::enabled().then(iced::time::Instant::now);
        let (models, hosts) = self.synced_hatch_models_hosted(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
            tint_selected,
        );
        let arc = Arc::new(models);
        self.hatch_hosts
            .borrow_mut()
            .insert(key.clone(), (Arc::downgrade(&arc), Arc::new(hosts)));
        let build_ms = crate::perf::elapsed_ms(build_started);
        if build_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-models rebuild={build_ms:.1}ms models={} source_hatches={} selected={} tint_selected={tint_selected}",
                arc.len(),
                self.hatches.len(),
                self.selected.len(),
            );
        }
        self.hatch_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, sel_sig, Arc::clone(&arc)));
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

    /// Whether a wipeout-model set cached at `cached_epoch` is still current.
    ///
    /// The displayed set also contains masks expanded from INSERTs. COPY and
    /// grip edits modify or add INSERT handles without changing the child
    /// WIPEOUT entity, so both categories count, removed INSERTs included via
    /// the recorded removal bits. Moving plain geometry touches neither and
    /// leaves the set exactly as it was.
    ///
    /// Both cache paths ask through here, because they did not: the
    /// per-viewport one compared `geometry_epoch` outright, so a drag of plain
    /// geometry in a paper-space viewport rebuilt every mask in the drawing —
    /// 96 ms a frame, 153 frames of one measured drag, 14.8 s in all — while
    /// the same drag in model space kept them. Sharing the predicate is what
    /// stops the two drifting apart again.
    fn wipeout_cache_current(&self, cached_epoch: u64, cached_empty: bool) -> bool {
        let any_wipeout = std::cell::OnceCell::<bool>::new();
        let dependencies = self.dependency_index_cache.borrow();
        let mask_hosts = dependencies
            .as_ref()
            .filter(|_| cached_empty && self.block_edit_block.is_none())
            .map(|index| &index.wipeout_hosts);
        self.category_cache_valid(
            cached_epoch,
            CACHE_CATEGORY_WIPEOUT | CACHE_CATEGORY_INSERT_HATCH,
            |h| {
                // An INSERT only matters when a block it places holds a mask;
                // moving plain geometry leaves the set as it was.
                match self.document.get_entity(h) {
                    Some(EntityType::Wipeout(_)) => true,
                    // An empty cache has no old placements to remove. If the
                    // live graph also has no path from this entity to a mask,
                    // this edit cannot add one to the cached view.
                    Some(_) if mask_hosts.is_some() => {
                        mask_hosts.is_some_and(|hosts| hosts.contains(&h))
                    }
                    // Without a single mask in the drawing no INSERT can place
                    // one; skip walking the block tree of every edited INSERT.
                    Some(EntityType::Insert(_))
                        if !*any_wipeout.get_or_init(|| {
                            self.document
                                .entities()
                                .any(|e| matches!(e, EntityType::Wipeout(_)))
                        }) =>
                    {
                        false
                    }
                    // No host list is kept: the INSERT may have placed a mask
                    // before the edit (its block repointed), so any INSERT
                    // edit counts.
                    Some(EntityType::Insert(_)) => true,
                    _ => false,
                }
            },
        )
    }

    pub(crate) fn wipeout_models_arc(&self) -> Arc<Vec<HatchModel>> {
        let target_block = self.content_render_block_handle();
        let key = (target_block, self.current_layout.clone());
        {
            let reuse = {
                let cache = self.wipeout_cache.borrow();
                match cache.get(&key) {
                    Some((cached_epoch, arc))
                        if self.wipeout_cache_current(*cached_epoch, arc.is_empty()) =>
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
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let arc = Arc::new(self.wipeout_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        if let Some(started) = rebuild_started {
            crate::perf_record!(
                "[perf] wipeout-models rebuild={:.1}ms models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                arc.len(),
            );
        }
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
                            |h| self.is_image_entity(h),
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

    /// Splice a re-delivered raster's fresh model into every cached gathered
    /// image list. The reseed takes no geometry bump — the entity, its draw
    /// order and every wire are untouched — so the lists stay valid and only
    /// their rasters are refreshed; without this the gather would re-walk the
    /// whole drawing on the UI thread right after the view settled (#3339).
    /// A raster replacement changes pixels, size and the triangles the
    /// raster is drawn with (one with a zoomed-in region has the whole
    /// picture and the region in one texture) — the gathered placement, clip
    /// and draw depth stay valid — so lists that are current are patched in
    /// place; lists already stale for other reasons, and lists holding an
    /// instanced clone (an image inside a block reference), are evicted so
    /// the next gather rebuilds them. The sheet of a layout keeps a list of
    /// its own pictures, patched the same way.
    pub(crate) fn patch_image_caches(&self, prev_epoch: u64, handles: &[Handle]) {
        if handles.is_empty() {
            return;
        }
        let perf = crate::perf::enabled();
        let started = perf.then(iced::time::Instant::now);
        let a = self.patch_image_cache_entries(&self.image_cache, prev_epoch, handles);
        let b = self.patch_image_cache_entries(&self.frozen_image_cache, prev_epoch, handles);
        let c = self.patch_paper_sheet_images(prev_epoch, handles);
        let counts = (a.0 + b.0 + c.0, a.1 + b.1 + c.1, a.2 + b.2 + c.2);
        if perf {
            crate::perf_record!(
                "[perf] image-patch   {:>7.2}ms entries={} lists_patched={} evicted={}",
                started.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0),
                counts.0,
                counts.1,
                counts.2,
            );
        }
    }

    /// Returns (entries seen, lists patched, lists evicted for a rebuild).
    fn patch_image_cache_entries<K>(
        &self,
        cache: &RefCell<HashMap<K, (u64, Arc<Vec<ImageModel>>)>>,
        prev_epoch: u64,
        handles: &[Handle],
    ) -> (usize, usize, usize) {
        let epoch = self.geometry_epoch;
        let mut seen = 0usize;
        let mut patched = 0usize;
        let mut stale = 0usize;
        cache.borrow_mut().retain(|_, (entry_epoch, arc)| {
            seen += 1;
            // A list the patch cannot reason about — one that predates other
            // changes, or holds an instanced clone whose transform the gather
            // owns — is evicted: the reseed records no geometry change, so the
            // journal replay would otherwise revalidate it with the old pixels.
            if *entry_epoch != prev_epoch || Self::holds_instanced_image(arc, handles) {
                stale += 1;
                return false;
            }
            if self.splice_fresh_images(arc, handles) {
                *entry_epoch = epoch;
                patched += 1;
            }
            true
        });
        (seen, patched, stale)
    }

    /// The same for the list the sheet of each layout keeps of its own
    /// pictures (`paper_sheet_render_models_for_view`). A sheet whose list
    /// holds none of `handles` is left as it is, whatever its epoch: the
    /// reseed has nothing for it.
    fn patch_paper_sheet_images(
        &self,
        prev_epoch: u64,
        handles: &[Handle],
    ) -> (usize, usize, usize) {
        let epoch = self.geometry_epoch;
        let mut seen = 0usize;
        let mut patched = 0usize;
        let mut stale = 0usize;
        self.paper_sheet_render_cache
            .borrow_mut()
            .retain(|_, sheet| {
                seen += 1;
                let holds = sheet
                    .images
                    .iter()
                    .any(|model| model.entity.is_some_and(|entity| handles.contains(&entity)));
                if !holds {
                    return true;
                }
                if sheet.epoch != prev_epoch || Self::holds_instanced_image(&sheet.images, handles)
                {
                    stale += 1;
                    return false;
                }
                if self.splice_fresh_images(&mut sheet.images, handles) {
                    sheet.epoch = epoch;
                    patched += 1;
                }
                true
            });
        (seen, patched, stale)
    }

    /// `list` holds one of `handles` as an instanced clone (an image inside
    /// a block reference), whose transform the gather owns.
    fn holds_instanced_image(list: &[ImageModel], handles: &[Handle]) -> bool {
        list.iter().any(|model| {
            model.render_instance.is_some()
                && model.entity.is_some_and(|entity| handles.contains(&entity))
        })
    }

    /// Gives the entries of `list` that serve `handles` the rasters the scene
    /// has for them now, and drops those whose raster did not come back.
    /// Returns whether the list held any.
    fn splice_fresh_images(&self, list: &mut Arc<Vec<ImageModel>>, handles: &[Handle]) -> bool {
        let mut touched = false;
        Arc::make_mut(list).retain_mut(|model| {
            let Some(entity) = model.entity else {
                return true;
            };
            if !handles.contains(&entity) {
                return true;
            }
            match self.images.get(&entity) {
                Some(fresh) => {
                    if !Arc::ptr_eq(&model.pixels, &fresh.pixels) {
                        model.file_path = fresh.file_path.clone();
                        model.pixels = Arc::clone(&fresh.pixels);
                        model.width = fresh.width;
                        model.height = fresh.height;
                        // The triangles go with the texture: they say
                        // which part of it each one draws.
                        model.verts.clone_from(&fresh.verts);
                    }
                    touched = true;
                    true
                }
                None => {
                    // The raster did not come back; the cached list drops
                    // the entry the rebuild would not gather either.
                    touched = true;
                    false
                }
            }
        });
        touched
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
        self.collect_images(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            false,
            |image, _| image,
        )
    }

    pub fn paper_plot_images(&self) -> Vec<crate::io::pdf_export::PlotImage> {
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        self.placed_images(
            self.current_layout_block_handle(),
            None,
            scale,
            self.annotation_all_visible(),
            None,
            true,
        )
    }

    fn placed_images(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        plotting: bool,
    ) -> Vec<crate::io::pdf_export::PlotImage> {
        self.collect_images(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            plotting,
            |image, context| crate::io::pdf_export::PlotImage {
                image,
                clips: context.clips.clone(),
            },
        )
    }

    fn collect_images<T>(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        plotting: bool,
        mut collect: impl FnMut(ImageModel, &render_graph::InstanceContext) -> T,
    ) -> Vec<T> {
        if self.images.is_empty() {
            return Vec::new();
        }
        let depth_map = self.draw_depth_map();
        let visibility_reveal = if plotting {
            HashSet::default()
        } else {
            self.visibility_reveal_handles()
        };
        // Plot/export passes draw the drawing as stored, not as walked;
        // live renders get the walk only when they show the interaction space.
        let layer_walk = (!plotting)
            .then(|| self.render_space_layer_walk(target_block, viewport))
            .flatten();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut models = Vec::new();
        let mut image_sources = rustc_hash::FxHashMap::default();
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
                (!plotting
                    || self
                        .document
                        .layers
                        .get(&entity.common().layer)
                        .is_none_or(|layer| layer.is_plottable))
                    && (context.is_instanced()
                        || !self.entity_temporarily_hidden(entity.common().handle))
            },
            |entity, context| {
                let handle = entity.common().handle;
                let Some(model) = self.images.get(&handle) else {
                    return;
                };
                // A plot draws a PDF underlay's page at print resolution, not
                // at the resolution it is shown at (#2822).
                let mut placed = plotting
                    .then(|| self.print_underlay_image(entity))
                    .flatten()
                    .unwrap_or_else(|| model.clone());
                // Stamp the real handle: a seed can build the model before
                // the entity's handle is assigned, so the builder cannot.
                placed.entity = Some(handle);
                if context.is_instanced() {
                    for (corner, low) in placed.corners.iter_mut().zip(&mut placed.corners_low) {
                        let point = document::types::Vector3::new(
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
                        let point = document::types::Vector3::new(
                            vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                            vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                            vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
                        );
                        let point = context.transform.apply(point);
                        vertex.pos = [point.x as f32, point.y as f32, point.z as f32];
                        vertex.pos_low = [
                            (point.x - vertex.pos[0] as f64) as f32,
                            (point.y - vertex.pos[1] as f64) as f32,
                            (point.z - vertex.pos[2] as f64) as f32,
                        ];
                    }
                    let matrix = &context.transform.matrix.m;
                    let linear = [
                        matrix[0][0].to_bits(),
                        matrix[0][1].to_bits(),
                        matrix[0][2].to_bits(),
                        matrix[1][0].to_bits(),
                        matrix[1][1].to_bits(),
                        matrix[1][2].to_bits(),
                        matrix[2][0].to_bits(),
                        matrix[2][1].to_bits(),
                        matrix[2][2].to_bits(),
                    ];
                    let source_id = *image_sources
                        .entry((handle.value(), linear))
                        .or_insert_with(crate::scene::model::instance_model::next_source_id);
                    placed.render_instance =
                        Some(crate::scene::model::instance_model::RenderInstance {
                            source_id,
                            translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                        });
                }
                if !plotting
                    && self.visibility_revealed(if context.is_instanced() {
                        context.root_handle
                    } else {
                        handle
                    })
                {
                    placed.opacity *= 0.3;
                }
                placed.draw_depth = context.draw_depth(handle, depth_map.as_ref());
                models.push(collect(placed, context));
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

    /// Bring a stale mesh list up to date by redoing only the INSERTs that
    /// changed since it was built. `None` whenever that is not provably the
    /// same as a full rebuild (no replayable journal, a top-level solid or a
    /// section changed), and the caller rebuilds.
    fn patch_mesh_models(
        &self,
        key: &(Handle, String),
        target_block: Handle,
        scale: Option<Handle>,
    ) -> Option<Arc<Vec<MeshLodSet>>> {
        if self.constraint_shading_active() || self.active_live_section(target_block).is_some() {
            return None;
        }
        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let (epoch, cached) = {
            let cache = self.mesh_cache.borrow();
            let (epoch, arc) = cache.get(key)?;
            (*epoch, Arc::clone(arc))
        };
        let changes = self.replay_since(epoch)?;
        let mut redo: HashSet<Handle> = HashSet::default();
        let mut inserts: HashSet<Handle> = HashSet::default();
        for (handle, _) in &changes {
            if self.meshes.contains_key(handle) || self.block_meshes.contains_key(handle) {
                return None;
            }
            match self.document.get_entity(*handle) {
                Some(EntityType::Insert(_)) => {
                    redo.insert(*handle);
                    inserts.insert(*handle);
                }
                // Section objects cut the solids they cross.
                Some(EntityType::Extended(_)) => return None,
                // Anything else places no block mesh.
                Some(_) => {}
                None => {
                    // Gone: a top-level solid leaves the list through the full
                    // rebuild; a reference takes its own placements with it.
                    if cached.iter().any(|set| {
                        set.instance_handle.is_none() && set.entity_handle() == Some(*handle)
                    }) {
                        return None;
                    }
                    if cached
                        .iter()
                        .any(|set| set.instance_handle == Some(*handle))
                    {
                        redo.insert(*handle);
                    }
                }
            }
        }
        let fresh = if inserts.is_empty() {
            Vec::new()
        } else {
            self.instanced_block_meshes(
                target_block,
                None,
                scale,
                self.annotation_all_visible(),
                None,
                None,
                Some(&inserts),
            )
        };
        let fresh_count = fresh.len();
        let mut models: Vec<MeshLodSet> = cached
            .iter()
            .filter(|set| !set.instance_handle.is_some_and(|host| redo.contains(&host)))
            .cloned()
            .collect();
        models.extend(fresh);
        let arc = Arc::new(models);
        self.mesh_cache
            .borrow_mut()
            .insert(key.clone(), (self.geometry_epoch, Arc::clone(&arc)));
        if let Some(started) = started {
            crate::perf_record!(
                "[perf] mesh-models patch={:.1}ms hosts={} fresh={} models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                redo.len(),
                fresh_count,
                arc.len(),
            );
        }
        Some(arc)
    }

    pub(super) fn meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        if self.constraint_shading_active() {
            let _ = self.block_constraint_report(self.block_edit_block.unwrap());
        }
        let target_block = self.content_render_block_handle();
        let key = (
            target_block,
            if self.constraint_shading_active() {
                format!("{}:constraint-status", self.current_layout)
            } else {
                self.current_layout.clone()
            },
        );
        {
            let reuse = {
                let mesh_memo = RefCell::new(std::collections::HashMap::new());
                let cache = self.mesh_cache.borrow();
                match cache.get(&key) {
                    // Top-level solids seed self.meshes; the instanced_block part
                    // is driven by INSERTs, so an INSERT edit (e.g. a move) must
                    // also invalidate. Block-definition edits route through
                    // bump_geometry (a full delta) and invalidate regardless.
                    // An INSERT can only place a mesh when a block it uses,
                    // directly or nested, holds one; moving an INSERT of plain
                    // geometry keeps this warm.
                    Some((cached_epoch, arc))
                        if self.category_cache_valid(*cached_epoch, CACHE_CATEGORY_MESH, |h| {
                            self.meshes.contains_key(&h)
                                // An INSERT that placed solids before the
                                // edit (repointed to a plain block).
                                || arc.iter().any(|set| set.instance_handle == Some(h))
                                || match self.document.get_entity(h) {
                                    Some(entity @ EntityType::Insert(_)) => {
                                        !self.block_meshes.is_empty()
                                            && render_graph::entity_render_block_uses(
                                                &self.document,
                                                entity,
                                                self.annotation_scale,
                                            )
                                            .into_iter()
                                            .filter(|block_use| block_use.active)
                                            .any(|block_use| {
                                                render_graph::block_contains_mesh(
                                                    &self.document,
                                                    &block_use.insert.block_name,
                                                    &mut mesh_memo.borrow_mut(),
                                                )
                                            })
                                    }
                                    Some(EntityType::Extended(extended)) => matches!(
                                        extended.data,
                                        document::entities::ExtendedEntityData::SectionObject(_)
                                    ),
                                    _ => false,
                                }
                        }) =>
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
        if let Some(arc) = self.patch_mesh_models(&key, target_block, scale) {
            return arc;
        }
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let mut models = self.mesh_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        );
        self.shade_constraint_meshes(target_block, &mut models);
        if let Some(started) = rebuild_started {
            crate::perf_record!(
                "[perf] mesh-models rebuild={:.1}ms models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                models.len(),
            );
        }
        let arc = Arc::new(models);
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

    pub(crate) fn interaction_block_handle(&self) -> Handle {
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
        let base = self.interaction_space_base_key();
        // Fold the session LAYWALK set in like the viewport freeze fold:
        // render caches keyed here must never reuse an un-walked build.
        let walk = Self::layer_walk_sig(self.layer_walk.visible.get(&base));
        base ^ walk.rotate_left(11)
    }

    /// The space identity WITHOUT any walk: block ^ viewport ^ per-viewport
    /// freeze. `LayerWalkState` entries are keyed by this value, so a walk
    /// can be set or cleared without ever moving the key it lives under.
    /// During block edit the viewport is ignored, so a walk stored under this
    /// key applies to the edited block space — matching the other gates.
    fn interaction_space_base_key(&self) -> u64 {
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

    /// Order-independent signature of an optional walk set, folded like the
    /// frozen-set folds so iteration order cannot change a cache key. `None`
    /// and the empty set differ: a hide-everything walk never aliases the
    /// un-walked key.
    fn layer_walk_sig(walk: Option<&HashSet<Handle>>) -> u64 {
        let Some(visible) = walk else {
            return 0;
        };
        visible
            .iter()
            .fold(visible.len() as u64 + 1, |signature, handle| {
                signature ^ handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15)
            })
    }

    /// The walk set governing the current interaction space, if any. An
    /// empty set is a valid walk (it hides every layered entity).
    fn interaction_layer_walk(&self) -> Option<&HashSet<Handle>> {
        // Asked once per entity by every visibility gate: without any walk,
        // skip the space key, whose layout lookup scans every object.
        if self.layer_walk.visible.is_empty() {
            return None;
        }
        self.layer_walk
            .visible
            .get(&self.interaction_space_base_key())
    }

    /// Walk set for a render of `block` seen through `viewport` (`None` =
    /// the sheet / tiles directly). Only the current interaction space
    /// carries a walk: a graph that serves a foreign viewport or an export
    /// pass must keep drawing the un-walked content.
    fn render_space_layer_walk(
        &self,
        block: Handle,
        viewport: Option<Handle>,
    ) -> Option<&HashSet<Handle>> {
        let walk = self.interaction_layer_walk()?;
        let interaction_viewport = self
            .active_viewport
            .filter(|_| self.block_edit_block.is_none());
        if viewport.filter(|handle| !handle.is_null()) != interaction_viewport {
            return None;
        }
        (block == self.interaction_block_handle()).then_some(walk)
    }

    /// Public handle on the key the LAYWALK state is stored under (see
    /// `interaction_space_base_key`): stable with and without an active walk,
    /// so the app can set / clear / compare space identities.
    pub fn interaction_space_key_pub(&self) -> u64 {
        self.interaction_space_base_key()
    }

    /// The visible-layer set walked in space `space`, if that space is walked.
    pub fn space_layer_walk(&self, space: u64) -> Option<&HashSet<Handle>> {
        self.layer_walk.visible.get(&space)
    }

    /// Install or remove (`None`) the walk set for space `space`. Pure state:
    /// the caller bumps the geometry epoch so every derived cache rebuilds.
    pub fn set_space_layer_walk(&mut self, space: u64, visible: Option<HashSet<Handle>>) {
        match visible {
            Some(visible) => {
                self.layer_walk.visible.insert(space, visible);
            }
            None => {
                self.layer_walk.visible.remove(&space);
            }
        }
    }

    /// Drop walks for every space but `keep` — the app guarantees only the
    /// interactively walked space is ever hidden.
    pub fn clear_other_space_layer_walks(&mut self, keep: u64) {
        self.layer_walk.visible.retain(|space, _| *space == keep);
    }

    /// Drop every walk set. The pallet's cleanup path calls this on every
    /// close, so a drawing can never be left half-hidden.
    pub fn clear_all_layer_walks(&mut self) {
        self.layer_walk.visible.clear();
    }

    /// The entity handles of the block record being interacted with: model
    /// space, or the current layout's paper-space block. Per-layer actions
    /// (LAYWALK's zoom and select) work on this set, never on block-definition
    /// geometry.
    pub fn interaction_space_handles(&self) -> Vec<Handle> {
        let block = self.interaction_block_handle();
        self.document
            .block_records
            .iter()
            .find(|record| record.handle == block)
            .map(|record| record.entity_handles.clone())
            .unwrap_or_default()
    }

    /// Whether any space currently has a walk set.
    pub fn layer_walk_active(&self) -> bool {
        !self.layer_walk.visible.is_empty()
    }

    /// Whether the current interaction space is walked.
    pub fn layer_walk_active_for_current_space(&self) -> bool {
        self.layer_walk
            .visible
            .contains_key(&self.interaction_space_base_key())
    }

    /// Convenience clone of the current space's walk set for snapshot/undo-free restores.
    pub fn current_space_walk_visible(&self) -> Option<HashSet<Handle>> {
        self.space_layer_walk(self.interaction_space_base_key())
            .cloned()
    }

    /// True when the current space's walk hides this layer — the predicate
    /// every visibility gate ANDs in next to its off/frozen test. No set,
    /// or a layer outside any space (unknown record), never hides.
    pub fn layer_walk_hidden(&self, layer: Option<&document::tables::layer::Layer>) -> bool {
        let Some(layer) = layer else {
            return false;
        };
        self.interaction_layer_walk()
            .is_some_and(|walk| !walk.contains(&layer.handle))
    }

    fn interaction_layer_frozen(&self, layer: &str) -> bool {
        // The interaction-space LAYWALK hides layers like a VP freeze would,
        // so every per-viewport-freeze consumer honours it through this gate.
        if self.layer_walk_hidden(self.document.layers.get(layer)) {
            return true;
        }
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
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            lookup.entry(handle).or_default().push(index as u32);
        }
        let lookup = Arc::new(lookup);
        *self.mesh_pick_lookup_cache.borrow_mut() =
            Some((source, Arc::downgrade(meshes), Arc::clone(&lookup)));
        lookup
    }

    /// The box around the vertices of each mesh `meshes` draws, in the mesh's
    /// own coordinates, by the mesh's address. A mesh with no vertices, or
    /// with one that is not finite, has none. Measured once for a mesh list:
    /// a selection box asked every solid of the drawing for its vertices on
    /// every pointer move.
    fn mesh_pick_boxes(
        &self,
        meshes: &Arc<Vec<MeshLodSet>>,
    ) -> Arc<rustc_hash::FxHashMap<usize, [glam::DVec3; 2]>> {
        let source = Arc::as_ptr(meshes) as usize;
        {
            let cache = self.mesh_pick_boxes_cache.borrow();
            if let Some((ptr, weak, boxes)) = cache.as_ref() {
                if *ptr == source
                    && weak
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, meshes))
                {
                    return Arc::clone(boxes);
                }
            }
        }
        let mut boxes = rustc_hash::FxHashMap::default();
        let mut measured = rustc_hash::FxHashSet::default();
        for set in meshes.iter() {
            let Some(mesh) = set.geometry_lods().first() else {
                continue;
            };
            let address = std::ptr::from_ref(mesh) as usize;
            if measured.insert(address) {
                if let Some(local) = pick::hit_test::mesh_local_box(mesh) {
                    boxes.insert(address, local);
                }
            }
        }
        let boxes = Arc::new(boxes);
        *self.mesh_pick_boxes_cache.borrow_mut() =
            Some((source, Arc::downgrade(meshes), Arc::clone(&boxes)));
        boxes
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
        let live_section = self.active_live_section(target_block);
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
            .filter_map(|(&handle, set)| {
                let mut set = if let Some(section) = live_section.as_ref() {
                    match self.sectioned_body(handle, section) {
                        Ok(Some(body)) => self
                            .prepare_solid_model_display(handle, &body)
                            .map(|display| display.0)
                            .unwrap_or_else(|| set.clone()),
                        Ok(None) => return None,
                        Err(()) => set.clone(),
                    }
                } else {
                    set.clone()
                };
                if let Some(entity) = self.document.get_entity(handle) {
                    let style = crate::scene::view::render::render_style_for_viewport(
                        &self.document,
                        entity,
                        viewport,
                    );
                    // Colour 7 lines contrast with what the solid is drawn
                    // on; shaded faces keep the colour so they still read as
                    // lit. White edges on a white sheet hid a wireframe solid
                    // in a layout viewport. (#1446)
                    let edge = crate::scene::view::render::adapt_to_bg(style.0, self.current_bg());
                    set.edge_color = Some(edge);
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
                if self.visibility_revealed(handle) {
                    for mesh in &mut set.lods {
                        mesh.color = crate::scene::cache::block_cache::fade_toward_bg(
                            mesh.color,
                            self.current_bg(),
                        );
                    }
                    if let Some(material) = set.material.as_mut() {
                        material.diffuse = crate::scene::cache::block_cache::fade_toward_bg(
                            material.diffuse,
                            self.current_bg(),
                        );
                    }
                    // The edges carry their own colour now; fade them too.
                    set.edge_color = set.edge_color.map(|color| {
                        crate::scene::cache::block_cache::fade_toward_bg(color, self.current_bg())
                    });
                }
                Some(set)
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
            live_section.as_ref(),
            None,
        ));
        all
    }

    /// The planes a live section keeps the positive side of, in model space,
    /// as `[nx, ny, nz, d]` with `n·p + d >= 0` kept: what point clouds are
    /// clipped by, as solids are cut.
    pub(crate) fn live_section_clip_planes(&self) -> Vec<[f64; 4]> {
        let Some(section) = self.active_live_section(self.model_space_block_handle()) else {
            return Vec::new();
        };
        Self::section_planes(&section.data, section.slice_depth, None)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|plane| {
                let normal = glam::DVec3::from(plane.normal()?).normalize_or_zero();
                (normal != glam::DVec3::ZERO).then(|| {
                    let d = -normal.dot(glam::DVec3::from(plane.origin));
                    [normal.x, normal.y, normal.z, d]
                })
            })
            .collect()
    }

    fn active_live_section(&self, target_block: Handle) -> Option<LiveSection> {
        if !self.has_section_objects() {
            return None;
        }
        self.document
            .entities()
            .filter_map(|entity| {
                let EntityType::Extended(extended) = entity else {
                    return None;
                };
                let document::entities::ExtendedEntityData::SectionObject(data) = &extended.data
                else {
                    return None;
                };
                let owner = extended.common.owner_handle;
                (data.flags & 1 != 0
                    && !extended.common.invisible
                    && (owner.is_null() || owner == target_block))
                    .then(|| LiveSection {
                        data: data.clone(),
                        slice_depth: crate::entities::extended::section_is_slice(extended).then(
                            || {
                                crate::entities::extended::section_slice_depth(extended)
                                    .unwrap_or(0.0)
                            },
                        ),
                    })
            })
            .last()
    }

    fn sectioned_body(
        &self,
        handle: Handle,
        section: &LiveSection,
    ) -> Result<Option<ocs_solid::model::Brep>, ()> {
        let body = self.section_source_body(handle).ok_or(())?;
        Self::section_body(&body, &section.data, section.slice_depth, None)
    }

    /// Build a temporary entity whose wire data matches the live-section body.
    /// The document entity stays unchanged so disabling the section restores
    /// the complete source geometry and saving never persists the clipped body.
    fn sectioned_wire_entity(
        &self,
        handle: Handle,
        section: &LiveSection,
    ) -> Result<Option<EntityType>, ()> {
        let Some(body) = self.sectioned_body(handle, section)? else {
            return Ok(None);
        };
        let (_, wires, center) = self.prepare_solid_model_display(handle, &body).ok_or(())?;
        let mut entity = self.document.get_entity(handle).cloned().ok_or(())?;
        let reference = document::types::Vector3::new(center[0], center[1], center[2]);
        match &mut entity {
            EntityType::Solid3D(solid) => {
                solid.point_of_reference = reference;
                solid.wires = wires;
                solid.silhouettes.clear();
            }
            EntityType::Surface(surface) => {
                surface.point_of_reference = reference;
                surface.wires = wires;
                surface.silhouettes.clear();
            }
            EntityType::Region(region) => {
                region.point_of_reference = reference;
                region.wires = wires;
                region.silhouettes.clear();
            }
            EntityType::Body(body) => {
                body.point_of_reference = reference;
                body.wires = wires;
                body.silhouettes.clear();
            }
            // The cut mesh is drawn as the solid it bounds.
            EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_) => {
                let mut solid = document::entities::Solid3D::new();
                solid.common = entity.common().clone();
                solid.point_of_reference = reference;
                solid.wires = wires;
                entity = EntityType::Solid3D(solid);
            }
            _ => return Err(()),
        }
        Ok(Some(entity))
    }

    fn section_source_body(
        &self,
        handle: Handle,
    ) -> Option<std::borrow::Cow<'_, ocs_solid::model::Brep>> {
        if let Some(body) = self.solid_models.get(&handle) {
            return Some(std::borrow::Cow::Borrowed(body));
        }
        let body = match self.document.get_entity(handle) {
            Some(EntityType::Solid3D(solid)) => {
                crate::scene::convert::solid3d_tess::solid_record(solid)
            }
            Some(EntityType::Region(region)) => {
                crate::scene::convert::solid3d_tess::region_record(region)
            }
            Some(EntityType::Body(body)) => {
                crate::scene::convert::solid3d_tess::acis_record(&body.acis_data)
            }
            Some(EntityType::Surface(surface)) => {
                crate::scene::convert::solid3d_tess::surface_record(surface)
            }
            // A mesh that closes a volume is cut as the solid it bounds.
            Some(
                mesh @ (EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)),
            ) => crate::entities::mesh::closed_mesh_body(mesh),
            _ => None,
        }?;
        Some(std::borrow::Cow::Owned(body))
    }

    fn section_body(
        body: &ocs_solid::model::Brep,
        data: &document::entities::SectionObjectData,
        slice_depth: Option<f64>,
        body_to_world: Option<&document::types::Transform>,
    ) -> Result<Option<ocs_solid::model::Brep>, ()> {
        let planes = Self::section_planes(data, slice_depth, body_to_world).ok_or(())?;
        let mut current = body.clone();
        for plane in planes {
            match keep_section_positive(&current, plane)? {
                Some(next) => current = next,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// The planes bounding what section `data` keeps, each facing inward:
    /// the section plane, then the back of a slice, the ends of a boundary
    /// and the top and bottom of a volume.
    fn section_planes(
        data: &document::entities::SectionObjectData,
        slice_depth: Option<f64>,
        body_to_world: Option<&document::types::Transform>,
    ) -> Option<Vec<ocs_geom::space::Plane>> {
        let (origin, tangent, vertical, viewing) = section_frame(data)?;
        let mut planes = vec![section_plane(origin, tangent, viewing, body_to_world)?];
        if data.state == 1 && slice_depth.is_none() {
            return Some(planes);
        }

        let depth = slice_depth.unwrap_or_else(|| {
            data.vertices
                .first()
                .zip(data.back_line_vertices.first())
                .map_or(0.0, |(front, back)| {
                    (glam::DVec3::new(back.x, back.y, back.z)
                        - glam::DVec3::new(front.x, front.y, front.z))
                    .dot(viewing)
                    .abs()
                })
        });
        if depth <= 1e-12 {
            return Some(planes);
        }
        planes.push(section_plane(
            origin + viewing * depth,
            tangent,
            -viewing,
            body_to_world,
        )?);
        if data.state == 1 {
            return Some(planes);
        }

        let first = data.vertices.first()?;
        let last = data.vertices.last()?;
        let first = glam::DVec3::new(first.x, first.y, first.z);
        let last = glam::DVec3::new(last.x, last.y, last.z);
        if (last - first).length() <= 1e-12 {
            return Some(planes);
        }
        planes.push(section_plane(first, vertical, tangent, body_to_world)?);
        planes.push(section_plane(last, vertical, -tangent, body_to_world)?);
        if data.state != 4 {
            return Some(planes);
        }

        planes.push(section_plane(
            origin - vertical * data.bottom_height,
            tangent,
            vertical,
            body_to_world,
        )?);
        planes.push(section_plane(
            origin + vertical * data.top_height,
            tangent,
            -vertical,
            body_to_world,
        )?);
        Some(planes)
    }

    /// Whether a top-level object in the current space can be picked right
    /// now. Mirrors the visibility predicate the resident wire build uses, so
    /// a selection command can never add an object the canvas does not draw.
    pub fn entity_pickable(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        let c = entity.common();
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            return false;
        }
        if self.display_entity_invisible(c) || self.entity_temporarily_hidden(handle) {
            return false;
        }
        let layer = self.document.layers.get(&c.layer);
        if layer
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
            || self.layer_walk_hidden(layer)
        {
            return false;
        }
        if let Some(frozen) = self.interaction_viewport_frozen_layers() {
            if frozen.contains(&c.handle) || layer.is_some_and(|l| frozen.contains(&l.handle)) {
                return false;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        !crate::scene::annotative::annotative_offscale_for(
            &self.document,
            c,
            scale,
            self.annotation_all_visible(),
        )
    }

    /// True when `layer` is turned off or frozen — entities on it never render.
    /// Also true when the current space's LAYWALK set omits it: every hatch /
    /// fill / mesh consumer funnels through here, so the walk hides them too.
    fn layer_hidden(&self, layer: &str) -> bool {
        let record = self.document.layers.get(layer);
        record
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
            || self.layer_walk_hidden(record)
    }

    /// Handles hidden inside content viewport `vp`: its VP-frozen layers plus
    /// model-space references bound to another viewport (Inventor drawing
    /// views overlap in model space; each is drawn only in its own viewport).
    /// Entity and layer handles never collide, so both ride the same set and
    /// its cache signature.
    pub(super) fn viewport_hidden_handles(&self, vp: Handle) -> HashSet<Handle> {
        let mut hidden: HashSet<Handle> = match self.document.get_entity(vp) {
            Some(EntityType::Viewport(v)) => v.frozen_layers.iter().copied().collect(),
            _ => HashSet::default(),
        };
        if self
            .document
            .classes
            .get_by_name("ACIDBLOCKREFERENCE")
            .is_some()
        {
            hidden.extend(
                self.view_rep_references()
                    .iter()
                    .filter(|(_, bound)| *bound != vp)
                    .map(|&(handle, _)| handle),
            );
        }
        hidden
    }

    /// Model-space INSERTs carrying a view-representation binding, cached for
    /// the current geometry epoch. Any entity add/remove/modify bumps the
    /// epoch, so a new or rebound reference is picked up on the next call; the
    /// model-space block handle is part of the key because a structure-only
    /// undo can swap it without touching geometry.
    fn view_rep_references(&self) -> Arc<Vec<(Handle, Handle)>> {
        let model = self.model_space_block_handle();
        if let Some((epoch, cached_model, references)) =
            self.view_rep_reference_cache.borrow().as_ref()
        {
            if *epoch == self.geometry_epoch && *cached_model == model {
                return Arc::clone(references);
            }
        }
        let references: Vec<(Handle, Handle)> = self
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Insert(insert) if insert.common.owner_handle == model => insert
                    .view_rep_handle
                    .map(|bound| (insert.common.handle, bound)),
                _ => None,
            })
            .collect();
        let references = Arc::new(references);
        *self.view_rep_reference_cache.borrow_mut() =
            Some((self.geometry_epoch, model, Arc::clone(&references)));
        references
    }

    /// True when `layer`'s handle is in a content viewport's per-viewport
    /// frozen-layer set (VP freeze). Mirrors the wire path's test in
    /// [`Scene::resident_entity_visible`] so fills / images / meshes hide on the
    /// same layers wires do. `None` / empty set → never frozen.
    fn layer_frozen_in(&self, layer: &str, frozen: Option<&HashSet<Handle>>) -> bool {
        // LAYWALK rides the same gate: consumers of VP freeze (fills, plot
        // fills, meshes) hide walked-out layers as if the viewport froze them.
        if self.layer_walk_hidden(self.document.layers.get(layer)) {
            return true;
        }
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

    fn viewport_style_key(&self, viewport: Option<Handle>) -> u64 {
        let Some(viewport) = viewport.filter(|handle| handle.is_valid()) else {
            return 0;
        };
        let stale = self
            .viewport_style_override_cache
            .borrow()
            .as_ref()
            .map(|(epoch, _)| *epoch != self.geometry_epoch)
            .unwrap_or(true);
        if stale {
            use document::objects::KnownXRecordKind;
            let kinds = [
                KnownXRecordKind::LayerViewportAlphaOverride,
                KnownXRecordKind::LayerViewportColorOverride,
                KnownXRecordKind::LayerViewportLinetypeOverride,
                KnownXRecordKind::LayerViewportLineweightOverride,
            ];
            let mut overridden = HashSet::default();
            for layer in self.document.layers.iter() {
                for kind in kinds {
                    overridden.extend(
                        self.document
                            .layer_viewport_overrides(layer.handle, kind)
                            .into_iter()
                            .map(|(handle, _)| handle),
                    );
                }
            }
            *self.viewport_style_override_cache.borrow_mut() =
                Some((self.geometry_epoch, overridden));
        }
        self.viewport_style_override_cache
            .borrow()
            .as_ref()
            .is_some_and(|(_, overridden)| overridden.contains(&viewport))
            .then_some(viewport.value())
            .unwrap_or(0)
    }

    /// Hatch / 2-D-solid fills for a content viewport, with its frozen layers
    /// removed. Cached per frozen-set signature (viewports sharing a frozen set
    /// share the build). No frozen layers → the shared unfiltered set.
    pub(super) fn hatch_models_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
        tint_selected: bool,
    ) -> Arc<Vec<HatchModel>> {
        let tint_selected = tint_selected && self.current_layout != "Model";
        if viewport.is_null() && frozen.is_empty() {
            return self.hatch_models_arc_for_view(tint_selected);
        }
        let target_block = self.content_render_block_handle();
        let scale = self.viewport_scale_handle(viewport);
        let all_visible = self.annotation_all_visible();
        let context_sig = scale.map_or(0, |handle| handle.value())
            ^ u64::from(all_visible).rotate_left(61)
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, self.current_layout.clone(), sig);
        let sel = if tint_selected {
            self.selected_hatch_sig()
        } else {
            0
        };
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
            tint_selected,
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
        let context_sig =
            scale.map_or(0, |handle| handle.value()) ^ u64::from(all_visible).rotate_left(61);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
        let key = (target_block, self.current_layout.clone(), sig);
        {
            let reuse = match self.frozen_wipeout_cache.borrow().get(&key) {
                Some((cached_epoch, arc))
                    if self.wipeout_cache_current(*cached_epoch, arc.is_empty()) =>
                {
                    Some(Arc::clone(arc))
                }
                _ => None,
            };
            if let Some(arc) = reuse {
                // Carry the entry forward, as the model-space path does: left
                // at its original epoch it would eventually fall below
                // `geometry_journal_floor`, where no delta can be replayed and
                // every later edit rebuilds it again.
                if let Some((e, _)) = self.frozen_wipeout_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let arc = Arc::new(self.wipeout_models(
            target_block,
            Some(frozen),
            scale,
            all_visible,
            (!viewport.is_null()).then_some(viewport),
        ));
        self.frozen_wipeout_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Point clouds of the model content, with a content viewport's frozen
    /// layers removed. The paper sheet shows none. The set is rebuilt per
    /// geometry epoch; unchanged clouds keep their placed points (and GPU
    /// buffers) by key.
    pub(super) fn point_clouds_for_viewport(
        &self,
        viewport: Handle,
        frozen: &HashSet<Handle>,
        paper_sheet: bool,
    ) -> Arc<crate::scene::model::point_cloud::PointCloudSet> {
        use crate::scene::model::point_cloud;
        static EMPTY: std::sync::OnceLock<Arc<point_cloud::PointCloudSet>> =
            std::sync::OnceLock::new();
        let empty = || Arc::clone(EMPTY.get_or_init(Default::default));
        if paper_sheet {
            return empty();
        }
        let target_block = self.content_render_block_handle();
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        // Viewports sharing a frozen set only share the build when the walk
        // matches too: the active (walked) viewport must not leak its set.
        let key = (
            target_block,
            Self::frozen_layers_sig(frozen) ^ Self::layer_walk_sig(layer_walk),
        );
        if let Some((epoch, arc)) = self.point_cloud_cache.borrow().get(&key) {
            if *epoch == self.geometry_epoch {
                return Arc::clone(arc);
            }
        }
        let arc = if point_cloud::any_definition(&self.document) {
            let depths = rustc_hash::FxHashMap::default();
            let visibility_reveal = self.visibility_reveal_handles();
            let graph = render_graph::RenderSceneGraph::new(
                &self.document,
                (!frozen.is_empty()).then_some(frozen),
                None,
                true,
                &depths,
            )
            .with_visibility_reveal(&visibility_reveal)
            .with_layer_walk(layer_walk);
            let mut clouds = Vec::new();
            graph.walk_root(
                self.render_scene_root(target_block),
                |entity, context| {
                    context.is_instanced()
                        || !self.entity_temporarily_hidden(entity.common().handle)
                },
                |entity, context| {
                    let EntityType::Extended(extended) = entity else {
                        return;
                    };
                    let document::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data
                    else {
                        return;
                    };
                    let transform = context.is_instanced().then_some(&context.transform);
                    // Object Color stylization draws the cloud in its own colour.
                    let common = entity.common();
                    let color = if matches!(common.color, document::types::Color::ByLayer) {
                        self.document
                            .layers
                            .get(&common.layer)
                            .map(|layer| layer.color)
                            .unwrap_or(common.color)
                    } else {
                        common.color
                    };
                    let object_color = color.rgb().map_or([255; 3], |(r, g, b)| [r, g, b]);
                    clouds.extend(point_cloud::placed(
                        &self.document,
                        data,
                        object_color,
                        transform,
                        &point_cloud::hidden(data),
                    ));
                },
            );
            if clouds.is_empty() {
                empty()
            } else {
                Arc::new(point_cloud::PointCloudSet {
                    point_size: point_cloud::point_size(&self.document),
                    clouds,
                })
            }
        } else {
            empty()
        };
        self.point_cloud_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Wireframe 2D draws no cloud points: each shown cloud whose file is
    /// found draws its extents box and a message instead, unless
    /// POINTCLOUD2DVSDISPLAY is 1.
    // ponytail: model-space clouds only, rebuilt per frame; cache by epoch if
    // drawings with many clouds make it show.
    pub(super) fn point_cloud_2d_wires(&self) -> Vec<WireModel> {
        use crate::scene::model::point_cloud::{resolve_source, setting, setting_value};
        if !crate::scene::model::point_cloud::any_definition(&self.document)
            || setting("POINTCLOUD2DVSDISPLAY")
                .is_some_and(|s| setting_value(&self.document, s) == 1)
        {
            return Vec::new();
        }
        let mut wires = Vec::new();
        for entity in self.document.entities() {
            let EntityType::Extended(extended) = entity else {
                continue;
            };
            let document::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data else {
                continue;
            };
            let common = entity.common();
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(common.handle)
                || self
                    .document
                    .layers
                    .get(&common.layer)
                    .is_some_and(|layer| layer.is_off() || layer.is_frozen())
                || self.layer_walk_hidden(self.document.layers.get(&common.layer))
                || resolve_source(&self.document, data).is_none()
            {
                continue;
            }
            let color = if matches!(common.color, document::types::Color::ByLayer) {
                self.document
                    .layers
                    .get(&common.layer)
                    .map(|layer| layer.color)
                    .unwrap_or(common.color)
            } else {
                common.color
            };
            let (r, g, b) = color.rgb().unwrap_or((255, 255, 255));
            wires.push(WireModel::solid_f64(
                common.handle.value().to_string(),
                crate::entities::extended::point_cloud_2d_style_lines(data),
                [
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                ],
                false,
            ));
        }
        wires
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
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        // The walked (interaction) viewport must not share its build with a
        // frozen-set twin that renders un-walked.
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
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
            ^ self.viewport_style_key(Some(viewport)).rotate_left(23);
        let layer_walk =
            self.render_space_layer_walk(target_block, (!viewport.is_null()).then_some(viewport));
        let sig = Self::frozen_layers_sig(frozen) ^ context_sig ^ Self::layer_walk_sig(layer_walk);
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

    /// True when `handle`'s entity is a Civil / AEC object
    /// (`ExtendedEntityData::Arch`). The codec cannot rewrite its class data
    /// yet, so a move would shift only its cached picture and leave the
    /// object where it was (#3394); transform paths skip it.
    pub fn is_fixed_in_place(&self, handle: Handle) -> bool {
        matches!(
            self.document.get_entity(handle),
            Some(EntityType::Extended(x))
                if matches!(x.data, document::entities::ExtendedEntityData::Arch(_))
        )
    }

    /// True when `handle`'s entity sits on a locked layer, or is a point cloud
    /// locked on its own. Locked objects stay visible and snappable, and
    /// selectable unless PICKLOCKED is off; mutation paths must skip them.
    pub fn is_layer_locked(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        is_locked_point_cloud(entity)
            || self
                .document
                .layers
                .get(&entity.common().layer)
                .is_some_and(|l| l.is_locked())
            // REFEDIT with "Lock objects not in working set": only the
            // working set can be edited.
            || (self.refedit_lock
                && self
                    .refedit_keep
                    .as_ref()
                    .is_some_and(|keep| !keep.contains(&handle)))
    }

    /// True when `handle` addresses a reference-only entity (API v9):
    /// plugin-owned display geometry the built-in edit paths treat like a
    /// locked-layer entity while its owning plugin keeps write access through
    /// the plugin host API. Absent entities are never reference-only.
    pub fn is_reference_only(&self, handle: Handle) -> bool {
        self.reference_only.is_marked(handle) && self.document.get_entity(handle).is_some()
    }

    /// API v18 selectable-custom owner-stamp flag. Unlike reference-only,
    /// this never affects hit-testing or ordinary selection.
    pub fn native_edit_disabled(&self, handle: Handle) -> bool {
        self.document
            .get_entity(handle)
            .is_some_and(crate::plugin::entity_labels::native_edit_disabled)
    }

    /// Insert or remove `handle` in the scene-scoped designation set,
    /// returning whether membership changed. Pure storage: no epoch bump, no
    /// undo bookkeeping — those belong to the callers (the plugin host API
    /// mutation and the history channels).
    pub(crate) fn set_reference_only_membership(&mut self, handle: Handle, on: bool) -> bool {
        self.reference_only.set(handle, on)
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
        if self.display_entity_invisible(c) {
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
        live_section: Option<&LiveSection>,
        // `Some`: only the placements of these INSERTs, as when patching a
        // cached list after they changed.
        targets: Option<&HashSet<Handle>>,
    ) -> Vec<MeshLodSet> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let depth_map = self.draw_depth_map();
        let visibility_reveal = self.visibility_reveal_handles();
        let layer_walk = self.render_space_layer_walk(layout_block, viewport);
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut out = Vec::new();
        // Skip block subtrees that hold no mesh at all, the way the instanced
        // hatch walk skips hatch-free ones. Without it every INSERT's whole
        // definition tree is enumerated to ask, which on a drawing whose
        // blocks carry no solids is ~795 ms spent producing nothing. The
        // presence test is memoised across instances by block name.
        let mut mesh_block_memo = std::collections::HashMap::new();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                if context.is_instanced() {
                    return true;
                }
                let handle = entity.common().handle;
                if self.entity_temporarily_hidden(handle) {
                    return false;
                }
                if !targets.is_none_or(|targets| {
                    matches!(entity, EntityType::Insert(_)) && targets.contains(&handle)
                }) {
                    return false;
                }
                // The graph walks at `self.annotation_scale`, so the presence
                // test has to resolve representations at that same scale: at
                // a different one an annotative INSERT can resolve to another
                // block, and skipping on that answer would hide real meshes.
                let block_uses: Vec<_> = render_graph::entity_render_block_uses(
                    &self.document,
                    entity,
                    self.annotation_scale,
                )
                .into_iter()
                .filter(|block_use| block_use.active)
                .collect();
                block_uses.is_empty()
                    || block_uses.into_iter().any(|block_use| {
                        render_graph::block_contains_mesh(
                            &self.document,
                            &block_use.insert.block_name,
                            &mut mesh_block_memo,
                        )
                    })
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let handle = entity.common().handle;
                let Some(original) = self.block_meshes.get(&handle) else {
                    return;
                };
                let sectioned;
                let set = if let Some((section, body)) = live_section.and_then(|section| {
                    self.section_source_body(handle).map(|body| (section, body))
                }) {
                    match Self::section_body(
                        &body,
                        &section.data,
                        section.slice_depth,
                        Some(&context.transform),
                    ) {
                        Ok(Some(body)) => {
                            if let Some((mut mesh, _, _)) =
                                self.prepare_solid_model_display(handle, &body)
                            {
                                mesh.prepare_instance_source(handle);
                                sectioned = mesh;
                                &sectioned
                            } else {
                                original
                            }
                        }
                        Ok(None) => return,
                        Err(()) => original,
                    }
                } else {
                    original
                };
                let inherit = self.mesh_inherit_for_path(&context.insert_path);
                let own_alpha = set.display_color().map_or(1.0, |color| color[3]);
                let mut transformed = transform_block_mesh_lod_set(set, &context.transform);
                if let Some(color) =
                    self.block_mesh_override_color(entity, handle, inherit.as_ref(), own_alpha)
                {
                    transformed.instance_color = Some(color);
                    if let Some(material) = transformed.material.as_mut() {
                        if material.handle.is_none() {
                            material.diffuse = color;
                        }
                    }
                }
                if let Some(mut material) =
                    self.block_mesh_override_material(entity, inherit.as_ref())
                {
                    // A material that takes the object's colour takes this
                    // child's, not the insert's it was resolved with.
                    if material.diffuse_by_object {
                        if let Some(own) = transformed.instance_color.or(set.display_color()) {
                            material.diffuse = [own[0], own[1], own[2], material.diffuse[3]];
                            material.ambient = [own[0], own[1], own[2]];
                        }
                    }
                    material.apply_to_with_face_overrides(
                        &mut transformed,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                }
                if self.count_display.is_some() || self.visibility_revealed(context.root_handle) {
                    let own = transformed
                        .instance_color
                        .or(set.display_color())
                        .unwrap_or([1.0; 4]);
                    let color = self.count_color(context.root_handle, own, self.current_bg());
                    transformed.instance_color = Some(color);
                    // A material pinned the edges to the object's colour.
                    if transformed.edge_color.is_some() {
                        transformed.edge_color = Some(color);
                    }
                    if let Some(material) = transformed.material.as_mut() {
                        material.diffuse = color;
                    }
                }
                transformed.instance_handle = Some(context.root_handle);
                transformed.effective_layer = Some(std::sync::Arc::from(
                    context.effective_layer(&entity.common().layer),
                ));
                out.push(transformed);
            },
        );
        out
    }

    fn mesh_inherit_for_path(&self, path: &[DxfInsert]) -> Option<BlockMeshInherit> {
        let first = path.first()?;
        let entity = EntityType::Insert(first.clone());
        let bg = self.current_bg();
        let insert_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::render_style_for(&self.document, &entity).0,
            bg,
        );
        let layer0_color = crate::scene::view::render::adapt_to_bg(
            crate::scene::view::render::layer_render_style(&self.document, &first.common.layer)
                .color,
            bg,
        );
        let mut inherit = BlockMeshInherit {
            insert_color,
            layer0_color,
            insert_material: crate::scene::model::material_model::resolve_material_with_base(
                &self.document,
                &entity,
                insert_color,
                None,
                self.material_base_dir.as_deref(),
            ),
            layer0_material: crate::scene::model::material_model::resolve_layer_material_with_base(
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
    pub(crate) fn current_bg(&self) -> [f32; 4] {
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
        ins: &document::entities::Insert,
        parent: &BlockMeshInherit,
    ) -> BlockMeshInherit {
        use document::types::Color;
        let bg = self.current_bg();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&ins.common.layer);
        let insert_entity = EntityType::Insert(ins.clone());
        let has_book_color =
            crate::scene::view::render::has_resolved_book_color(&self.document, &insert_entity);
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
        use document::types::Color;
        let common = e.common();
        let on_l0 = crate::scene::view::render::is_effective_layer_zero(&common.layer);
        let has_book_color = crate::scene::view::render::has_resolved_book_color(&self.document, e);
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
                c = crate::scene::cache::block_cache::refedit_fade(c, self.current_bg());
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

    /// Each object drawn in the current space that `keep` accepts, once, as a
    /// `(handle, entity)` clone for a modify command's working copy. Only the
    /// resident wire names are read and block copies stay unexpanded: one
    /// object can own many wires, and cloning per expanded wire copied a large
    /// drawing many times over when the command started (#1978).
    pub fn visible_entities_for_edit(
        &self,
        keep: impl Fn(&EntityType) -> bool,
    ) -> Vec<(Handle, EntityType)> {
        let wires = self.entity_wires_arc();
        let mut seen: HashSet<Handle> = HashSet::default();
        wires
            .iter()
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .filter(|handle| seen.insert(*handle))
            .filter_map(|handle| {
                let entity = self.document.get_entity(handle).filter(|e| keep(e))?;
                Some((handle, entity.clone()))
            })
            .collect()
    }

    /// Only the handles of editable, visible HATCH entities. Modification
    /// commands need these in addition to the wire-based entity list: hatch
    /// fills are rendered separately and otherwise never reach TRIM.
    /// Unlike visible_hatches_for_click this never clones GPU hatch models.
    pub fn visible_hatch_handles_for_edit(&self) -> Vec<Handle> {
        let interactive = self.hatch_interaction_filter();
        self.hatches
            .keys()
            .copied()
            .filter(|handle| {
                interactive(*handle)
                    && matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Hatch(_))
                    )
                    && !self.is_reference_only(*handle)
            })
            .collect()
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
        let interactive = self.hatch_interaction_filter();
        let visible = |h: Handle, m: &HatchModel| interactive(h).then(|| (h, m.clone()));
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

    /// Whether a fill takes part in picking. The layout block and annotation
    /// state each search the document's objects, so they are resolved once
    /// per filter, not once per fill (#2023).
    fn hatch_interaction_filter(&self) -> impl Fn(Handle) -> bool + '_ {
        let scale = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        let block = self.interaction_block_handle();
        move |handle| {
            let Some(entity) = self.document.get_entity(handle) else {
                return false;
            };
            // Use the SOLID's WCS-aware wire fill for interaction as well as
            // drawing. Its cached HatchModel is flattened to XY for plotting
            // and must not leave an invisible selection target at z=0 (#617).
            if matches!(entity, EntityType::Solid(_)) {
                return false;
            }
            let common = entity.common();
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(handle)
                || self.layer_hidden(&common.layer)
                || self.interaction_layer_frozen(&common.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    common,
                    scale,
                    all_visible,
                )
            {
                return false;
            }
            self.belongs_to_visible_block(handle, common.owner_handle, block)
        }
    }

    /// Instanced hatch models keyed by their block-backed host handle.
    pub fn insert_hatches_for_click(&self) -> Arc<HashMap<Handle, Vec<HatchModel>>> {
        let interaction_block = self.interaction_block_handle();
        let space_key = self.interaction_space_key();
        {
            let c = self.insert_hatch_cache.borrow();
            if let Some((epoch, cached_space, ref arc)) = *c {
                if cached_space == space_key
                    && self.category_cache_valid(epoch, CACHE_CATEGORY_INSERT_HATCH, |handle| {
                        self.document.get_entity(handle).is_some_and(|entity| {
                            render_graph::entity_render_block_uses(
                                &self.document,
                                entity,
                                self.annotation_scale,
                            )
                            .into_iter()
                            .any(|block_use| block_use.active)
                        })
                    })
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
        // Only some entities changed since the cached map was built (no
        // structural delta in between): rebuild the entries of those
        // entities alone. A paste of many INSERTs otherwise paid a walk of
        // every block reference in the drawing on the first click after it.
        let stale = self.insert_hatch_cache.borrow_mut().take();
        if let Some((epoch, cached_space, mut map)) = stale {
            if cached_space == space_key {
                if let Some(changes) = self.replay_since(epoch) {
                    let changed: Vec<Handle> = changes.iter().map(|(handle, _)| *handle).collect();
                    let fresh = self.collect_insert_hatches(
                        interaction_block,
                        changed
                            .iter()
                            .filter_map(|handle| self.document.get_entity(*handle)),
                    );
                    let entries = Arc::make_mut(&mut map);
                    for handle in &changed {
                        entries.remove(handle);
                    }
                    entries.extend(fresh);
                    *self.insert_hatch_cache.borrow_mut() =
                        Some((self.geometry_epoch, space_key, Arc::clone(&map)));
                    return map;
                }
            }
        }
        let arc =
            Arc::new(self.collect_insert_hatches(interaction_block, self.document.entities()));
        *self.insert_hatch_cache.borrow_mut() =
            Some((self.geometry_epoch, space_key, Arc::clone(&arc)));
        arc
    }

    /// Hatches inside the block references among `entities`, placed in
    /// world space, keyed by the reference that draws them.
    fn collect_insert_hatches<'a>(
        &'a self,
        interaction_block: Handle,
        entities: impl Iterator<Item = &'a EntityType>,
    ) -> HashMap<Handle, Vec<HatchModel>> {
        // No block definition holds a hatch, so no reference places one.
        if !render_graph::any_block_definition_holds(&self.document, |entity| {
            matches!(entity, EntityType::Hatch(_))
        }) {
            return HashMap::default();
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
        let visibility_reveal = self.visibility_reveal_handles();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            (!frozen.is_empty()).then_some(&frozen),
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        // The cache is keyed on `interaction_space_key`, already walk-folded,
        // so this build always serves the walked interaction space.
        .with_layer_walk(self.interaction_layer_walk())
        .with_annotation_scale(self.annotation_scale);
        for entity in entities {
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                entity,
                annotation_scale_handle,
            );
            let host = contextual.as_ref();
            let common = host.common();
            // Cheap, memoised filter first: most entities use no block that
            // holds a hatch, and the visibility tests below are far costlier
            // per entity than this lookup.
            let block_uses: Vec<_> =
                render_graph::entity_render_block_uses(&self.document, host, self.annotation_scale)
                    .into_iter()
                    .filter(|block_use| {
                        block_use.active
                            && render_graph::block_contains_hatch(
                                &self.document,
                                &block_use.insert.block_name,
                                &mut hatch_memo,
                            )
                    })
                    .collect();
            if block_uses.is_empty() {
                continue;
            }
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(common.handle)
                || layer_hidden(&common.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    common,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                continue;
            }
            if !self.belongs_to_visible_block(common.handle, common.owner_handle, interaction_block)
            {
                continue;
            }
            for block_use in block_uses {
                graph.walk_block_use(
                    &block_use,
                    common.handle,
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
                        let Some(mut model) = Self::hatch_model_from_dxf(&hatch, color) else {
                            return;
                        };
                        for clip in &context.clips {
                            let clipped = pick::xclip::clip_hatch_boundary(
                                &model.boundary,
                                model.world_origin,
                                clip,
                            );
                            if clipped.is_empty() {
                                return;
                            }
                            model.boundary = Arc::new(clipped);
                        }
                        out.entry(common.handle).or_default().push(model);
                    },
                );
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

    /// Wires of the space being edited that may cross a world-XY box: the
    /// indexed broad phase in model space, every wire elsewhere.
    pub fn interaction_candidates_xy(
        &self,
        aabb: [f64; 4],
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        let wires = self.hit_test_wires();
        if self.current_layout != "Model" || !self.interaction_index_worthwhile(&wires) {
            return self.materialize_stubs_cached(
                crate::scene::pick::interaction_index::InteractionCandidates::all(wires),
            );
        }
        self.indexed_interaction_candidates_xy(wires, aabb, false, false, false, None)
    }

    /// Small simple sets are cheaper to scan. Either many wires or substantial
    /// sub-geometry (one giant polyline/text batch/mesh) enables the index.
    const INTERACTION_INDEX_MIN_WIRES: usize = 4_000;
    const INTERACTION_INDEX_MIN_WORK: usize = 20_000;

    /// `InteractionCandidates::materialize_stubs`, but an unrestricted
    /// expansion of a wire set already expanded under this `geometry_epoch`
    /// is reused instead of recomputed. A restricted (XY- or index-limited)
    /// query depends on the query too, so it is passed straight through.
    fn materialize_stubs_cached(
        &self,
        candidates: crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        if !candidates.expansion_is_unrestricted() {
            return candidates.materialize_stubs();
        }
        let Some(source) = candidates.snapshot_arc() else {
            return candidates.materialize_stubs();
        };
        if !candidates.has_stubs() {
            return candidates;
        }
        // Which guard rejected the entry, so the cost of a miss can be
        // attributed: `geometry` moves on any entity edit, `block` only when a
        // definition changes, and the expansion depends on the latter.
        let mut miss = "no-entry";
        if let Some((weak, epoch, block_epoch, wires)) = self.stub_expansion_cache.borrow().as_ref()
        {
            let same_source = weak
                .upgrade()
                .is_some_and(|alive| Arc::ptr_eq(&alive, &source));
            let same_geometry = *epoch == self.geometry_epoch;
            let same_block = *block_epoch == self.block_epoch;
            if same_geometry && same_source {
                if crate::perf::enabled() {
                    crate::perf_record!(
                        "[perf] stub-cache hit wires={} source={}",
                        wires.len(),
                        source.len(),
                    );
                }
                return candidates.with_materialized(Arc::clone(wires));
            }
            miss = match (same_source, same_geometry, same_block) {
                (false, _, _) => "source",
                (true, false, true) => "geometry-only",
                (true, false, false) => "geometry-and-block",
                (true, true, _) => "unreachable",
            };
        }
        if crate::perf::enabled() {
            crate::perf_record!("[perf] stub-cache miss reason={}", miss);
        }
        let expanded = candidates.materialize_stubs();
        if let Some(wires) = expanded.snapshot_arc() {
            let budget = if Self::low_memory_viewing() { 16 } else { 64 } << 20;
            let mut cache = self.stub_expansion_cache.borrow_mut();
            if wires.len() * std::mem::size_of::<WireModel>() > budget
                || Self::resident_wire_bytes(&wires) > budget
            {
                *cache = None;
                return expanded;
            }
            *cache = Some((
                Arc::downgrade(&source),
                self.geometry_epoch,
                self.block_epoch,
                wires,
            ));
        }
        expanded
    }

    fn interaction_index_worthwhile(&self, wires: &[WireModel]) -> bool {
        wires.len() >= Self::INTERACTION_INDEX_MIN_WIRES
            || crate::scene::pick::interaction_index::InteractionIndex::estimated_work(wires)
                >= Self::INTERACTION_INDEX_MIN_WORK
            // A slot can stand for a whole block. Keep aperture queries
            // indexed even when the number of resident slots is small.
            || wires.iter().any(|wire| wire.stub.is_some())
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

    pub(crate) fn view_needs_projected_interaction(&self) -> bool {
        let camera = self.navigation_camera();
        camera.projection == view::camera::Projection::Perspective
            || (camera.rotation * glam::Vec3::Z).truncate().length_squared() > 1.0e-12
    }

    /// Navigation can start building a projected index before the first pick.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prepare_projected_interaction(&self) {
        if !self.view_needs_projected_interaction() {
            return;
        }
        let cache = self.interaction_index_cache.borrow();
        if let Some((_, _, _, index)) = cache.iter().rev().find(|(epoch, _, source, _)| {
            *epoch == self.geometry_epoch && source.strong_count() > 0
        }) {
            index.prepare_screen_in_background();
        }
    }

    fn cached_interaction_index(
        &self,
        wires: &Arc<Vec<WireModel>>,
    ) -> Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>> {
        let source = Arc::as_ptr(wires) as usize;
        let mut cache = self.interaction_index_cache.borrow_mut();
        cache
            .retain(|(epoch, _, weak, _)| *epoch == self.geometry_epoch && weak.strong_count() > 0);
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
        cache.push((epoch, source, Arc::downgrade(&wires), Arc::clone(&index)));
        if cache.len() > MAX_CACHED_SOURCES {
            cache.remove(0);
        }
        if epoch == self.geometry_epoch && self.current_layout == "Model" {
            *self.interaction_base_index_cache.borrow_mut() =
                Some((epoch, self.interaction_space_key(), index));
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

    fn interaction_overlay_can_replay(&self, base_epoch: u64, changes: usize) -> bool {
        changes <= Self::INTERACTION_OVERLAY_MAX_HANDLES
            || self
                .geometry_deltas
                .borrow()
                .iter()
                .filter(|delta| delta.epoch > base_epoch && !delta.layer_visibility_only)
                .map(|delta| delta.changes.len())
                .sum::<usize>()
                <= Self::INTERACTION_OVERLAY_MAX_HANDLES
    }

    fn interaction_overlay_base(&self) -> Option<OverlayBase> {
        let space_key = self.interaction_space_key();
        let (epoch, index) = {
            let cache = self.interaction_base_index_cache.borrow();
            let (epoch, cached_space, index) = cache.as_ref()?;
            if *epoch == self.geometry_epoch || *cached_space != space_key {
                return None;
            }
            (*epoch, Arc::clone(index))
        };
        {
            let cached = self.interaction_overlay_changes.borrow();
            if let Some((base_epoch, geometry_epoch, cached_space, changes)) = cached.as_ref() {
                if *base_epoch == epoch
                    && *geometry_epoch == self.geometry_epoch
                    && *cached_space == space_key
                {
                    return self
                        .interaction_overlay_can_replay(epoch, changes.len())
                        .then_some((epoch, index, Arc::clone(changes)));
                }
            }
        }
        let changes = Arc::new(self.replay_since(epoch)?);
        *self.interaction_overlay_changes.borrow_mut() =
            Some((epoch, self.geometry_epoch, space_key, Arc::clone(&changes)));
        self.interaction_overlay_can_replay(epoch, changes.len())
            .then_some((epoch, index, changes))
    }

    /// Recover the wire snapshot that was indexed by an immutable overlay
    /// base. The weak source is kept in `interaction_index_cache` so using
    /// the base index never pairs its wire ordinals with a newer snapshot.
    fn interaction_source_for_index(
        &self,
        epoch: u64,
        index: &Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) -> Option<Arc<Vec<WireModel>>> {
        self.interaction_index_cache.borrow().iter().find_map(
            |(cached_epoch, _, wires, cached_index)| {
                (*cached_epoch == epoch && Arc::ptr_eq(cached_index, index))
                    .then(|| wires.upgrade())
                    .flatten()
            },
        )
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
        let block = self.interaction_block_handle();
        let frozen: Option<HashSet<Handle>> = self
            .interaction_viewport_frozen_layers()
            .map(|layers| layers.iter().copied().collect());
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();

        let changed_live: Vec<Handle> = changes
            .iter()
            .filter_map(|(handle, kind)| {
                if matches!(kind, ChangeKind::Removed) {
                    return None;
                }

                let entity = self.document.get_entity(*handle)?;
                self.resident_entity_visible(
                    entity,
                    block,
                    frozen.as_ref(),
                    annotation_scale_handle,
                    all_visible,
                )
                .then_some(*handle)
            })
            .collect();
        let wires = Arc::new(self.wire_models_for(&changed_live));
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
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
        changed_keys: impl IntoIterator<Item = (u64, u32)>,
        changes: &[(Handle, ChangeKind)],
        changed_wires: &Arc<Vec<WireModel>>,
    ) -> OverlayGather {
        enum MemoSliceRef<'a> {
            Owned(&'a Arc<Vec<WireModel>>),
            Slice(&'a Arc<Vec<WireModel>>, usize, usize),
        }
        impl<'a> MemoSliceRef<'a> {
            #[inline]
            fn shared_at(&self, ordinal: usize) -> Option<(&'a Arc<Vec<WireModel>>, u32)> {
                match *self {
                    Self::Owned(wires) => {
                        (ordinal < wires.len()).then_some((wires, ordinal as u32))
                    }
                    Self::Slice(set, start, len) => {
                        (ordinal < len).then_some((set, (start + ordinal) as u32))
                    }
                }
            }
        }

        let changed: HashSet<Handle> = changes.iter().map(|(handle, _)| *handle).collect();
        let memo = self.resident_tess_memo.borrow();
        let backing_ref = self.resident_memo_backing.borrow();
        let backing_set = backing_ref.set.upgrade();
        let base_iter = base_keys.into_iter();
        let base_hint = base_iter.size_hint().0;
        let mut wires =
            crate::scene::pick::interaction_index::SharedWireRuns::with_capacity(base_hint);
        let mut base_slots = HashMap::with_capacity_and_hasher(base_hint, Default::default());
        let mut misses: HashMap<Handle, Vec<u32>> = HashMap::default();
        let mut prev_handle = Handle::NULL;
        let mut prev_hit: Option<MemoSliceRef<'_>> = None;
        let mut prev_valid = false;
        for (handle_value, ordinal) in base_iter {
            let handle = Handle::new(handle_value);
            if handle != prev_handle {
                prev_handle = handle;
                if changed.contains(&handle) || self.document.get_entity(handle).is_none() {
                    prev_valid = false;
                    prev_hit = None;
                } else {
                    prev_valid = true;
                    prev_hit = match memo.get(&handle) {
                        Some(MemoRun::Owned(w)) => Some(MemoSliceRef::Owned(w)),
                        Some(MemoRun::InSet) => {
                            if let (Some(set), Some(&(start, len))) =
                                (backing_set.as_ref(), backing_ref.ranges.get(&handle))
                            {
                                (start + len <= set.len())
                                    .then_some(MemoSliceRef::Slice(set, start, len))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                }
            }
            if !prev_valid {
                continue;
            }
            if let Some((run, index)) = prev_hit
                .as_ref()
                .and_then(|hit| hit.shared_at(ordinal as usize))
            {
                base_slots.insert((handle_value, ordinal), wires.len() as u32);
                wires.push(run, index);
            } else {
                misses.entry(handle).or_default().push(ordinal);
            }
        }
        drop(backing_ref);
        drop(memo);
        // A memo miss is uncommon (guard change / legacy source). Preserve
        // correctness without expanding every nearby entity: regenerate that
        // handle, then retain only the exact wire ordinals the base index
        // reported.
        for (handle, ordinals) in misses {
            let entity_wires = Arc::new(self.wire_models_for(&[handle]));
            for ordinal in ordinals {
                if entity_wires.get(ordinal as usize).is_some() {
                    base_slots.insert((handle.value(), ordinal), wires.len() as u32);
                    wires.push(&entity_wires, ordinal);
                }
            }
        }
        let changed_keys: HashSet<(u64, u32)> = changed_keys.into_iter().collect();
        let mut changed_slots = HashMap::default();
        let mut next_ordinal: HashMap<u64, u32> = HashMap::default();
        for (position, wire) in changed_wires.iter().enumerate() {
            if let Some(handle) = pipeline::wire_gpu::fast_parse_u64(&wire.name) {
                let ordinal = next_ordinal.entry(handle).or_default();
                let key = (handle, *ordinal);
                if changed_keys.contains(&key) {
                    changed_slots.insert(key, wires.len() as u32);
                    wires.push(changed_wires, position as u32);
                }
                *ordinal += 1;
            }
        }
        (Arc::new(wires), base_slots, changed_slots)
    }

    fn indexed_interaction_candidates_xy(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        allow_pending_empty: bool,
        area_only: bool,
        pick_only: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        // The stubbed query falls back to the whole unindexed set when no
        // index is cached yet, and that fallback is unrestricted — so it goes
        // through the cache, which passes a genuinely restricted result
        // straight on.
        self.materialize_stubs_cached(self.indexed_interaction_candidates_xy_stubbed(
            wires,
            aabb,
            allow_pending_empty,
            area_only,
            pick_only,
            snap,
        ))
    }

    /// `snap` narrows the query to the object-snap feature indexes (and,
    /// for the live preview, to discrete features only).
    fn indexed_interaction_candidates_xy_stubbed(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        allow_pending_empty: bool,
        area_only: bool,
        pick_only: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let direct = |index: &InteractionIndex, wires| match snap {
            Some(category) => index.query_snap_xy_with_category(wires, aabb, category),
            None if pick_only => index.query_xy_selection_for_materialize(wires, aabb, true),
            None if area_only => index.query_xy_selection_for_materialize(wires, aabb, false),
            None => index.query_xy_for_materialize(wires, aabb),
        };
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            // Nothing changed since the base was indexed: query its own
            // snapshot instead of gathering an overlay copy.
            if changes.is_empty() {
                if let Some(source) = self.interaction_source_for_index(base_epoch, &base) {
                    return direct(&base, source);
                }
            }
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let base_parts = snap.map(|category| base.snap_candidate_parts_xy(aabb, category));
            let keys = match &base_parts {
                Some(parts) => base.snap_candidate_keys(parts),
                None if pick_only => base.query_pick_wire_keys_xy(aabb),
                None => base.query_wire_keys_xy(aabb),
            };
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let changed_parts =
                snap.map(|category| changed_index.snap_candidate_parts_xy(aabb, category));
            let changed_keys = match &changed_parts {
                Some(parts) => changed_index.snap_candidate_keys(parts),
                None if pick_only => changed_index.query_pick_wire_keys_xy(aabb),
                None => changed_index.query_wire_keys_xy(aabb),
            };
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                changed_keys,
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            // `indexed_interaction_candidates_xy` runs `materialize_stubs` next,
            // which discards all primitive ref lists whenever any candidate
            // wire is a stub, so skip remapping them in that case.
            let result = if local.has_any_stub() {
                InteractionCandidates::shared_indexed_all(local, Some(aabb))
            } else {
                match (snap, base_parts, changed_parts) {
                    (Some(category), Some(base_parts), Some(changed_parts)) => {
                        let mut result = base.query_remapped_snap_xy_from_parts_with_category(
                            Arc::clone(&local),
                            &base_slots,
                            base_parts,
                            aabb,
                            category,
                        );
                        result.extend_indexed(
                            changed_index.query_remapped_snap_xy_from_parts_with_category(
                                local,
                                &changed_slots,
                                changed_parts,
                                aabb,
                                category,
                            ),
                        );
                        result
                    }
                    _ if area_only || pick_only => {
                        let mut result = base.query_remapped_xy_area(
                            Arc::clone(&local),
                            &base_slots,
                            aabb,
                            pick_only,
                        );
                        let replacements = changed_index.query_remapped_xy_area(
                            local,
                            &changed_slots,
                            aabb,
                            pick_only,
                        );
                        if pick_only {
                            result.extend_indexed(replacements);
                        } else {
                            result.extend_indexed_area(replacements);
                        }
                        result
                    }
                    _ => {
                        let mut result =
                            base.query_remapped_xy(Arc::clone(&local), &base_slots, aabb);
                        result.extend_indexed(changed_index.query_remapped_xy(
                            local,
                            &changed_slots,
                            aabb,
                        ));
                        result
                    }
                }
            };
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
            direct(&index, wires)
        } else {
            self.unindexed_interaction_candidates(wires, allow_pending_empty, snap, Some(aabb))
        }
    }

    /// No index for `wires` yet. A pending-empty caller gets an empty result
    /// while the index builds; the snap preview gets one for any large set,
    /// so it never scans the whole drawing on a pointer move.
    ///
    /// `near` is the region the caller is about to hit-test, when it has one.
    /// Passing it keeps the stub expansion local — the whole-set expansion of
    /// a stub-heavy drawing costs ~190 ms, against 7-13 ms for one aperture.
    fn unindexed_interaction_candidates(
        &self,
        wires: Arc<Vec<WireModel>>,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
        near: Option<[f64; 4]>,
    ) -> InteractionCandidates {
        let pending = allow_pending_empty
            && (snap.is_some() && wires.len() > 512
                || self.interaction_index_pending_key.get()
                    == Some((self.geometry_epoch, Arc::as_ptr(&wires) as usize)));
        if pending {
            InteractionCandidates::pending(wires)
        } else {
            match near {
                Some(aabb) => InteractionCandidates::all_near(wires, aabb),
                None => InteractionCandidates::all(wires),
            }
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
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        self.materialize_stubs_cached(self.indexed_interaction_candidates_screen_stubbed(
            wires,
            screen_rect,
            view_rot,
            eye,
            bounds,
            allow_pending_empty,
            snap,
        ))
    }

    fn indexed_interaction_candidates_screen_stubbed(
        &self,
        wires: Arc<Vec<WireModel>>,
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let direct = |index: &InteractionIndex, wires| match snap {
            Some(category) => index.query_snap_screen_with_category(
                wires,
                screen_rect,
                view_rot,
                eye,
                bounds,
                category,
            ),
            None => index.query_screen(wires, screen_rect, view_rot, eye, bounds),
        };
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            if changes.is_empty() {
                if let Some(source) = self.interaction_source_for_index(base_epoch, &base) {
                    return direct(&base, source);
                }
            }
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let base_parts = snap.map(|category| {
                base.snap_candidate_parts_screen(screen_rect, view_rot, eye, bounds, category)
            });
            let keys = match &base_parts {
                Some(parts) => base.snap_candidate_keys(parts),
                None => base.query_wire_keys_screen(screen_rect, view_rot, eye, bounds),
            };
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let changed_parts = snap.map(|category| {
                changed_index.snap_candidate_parts_screen(
                    screen_rect,
                    view_rot,
                    eye,
                    bounds,
                    category,
                )
            });
            let changed_keys = match &changed_parts {
                Some(parts) => changed_index.snap_candidate_keys(parts),
                None => changed_index.query_wire_keys_screen(screen_rect, view_rot, eye, bounds),
            };
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                changed_keys,
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            let result = match (snap, base_parts, changed_parts) {
                (Some(category), Some(base_parts), Some(changed_parts)) => {
                    let mut result = base.query_remapped_snap_screen_from_parts_with_category(
                        Arc::clone(&local),
                        &base_slots,
                        base_parts,
                        screen_rect,
                        view_rot,
                        eye,
                        bounds,
                        category,
                    );
                    result.extend_indexed(
                        changed_index.query_remapped_snap_screen_from_parts_with_category(
                            local,
                            &changed_slots,
                            changed_parts,
                            screen_rect,
                            view_rot,
                            eye,
                            bounds,
                            category,
                        ),
                    );
                    result
                }
                _ => {
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
                    result
                }
            };
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
            direct(&index, wires)
        } else {
            self.unindexed_interaction_candidates(wires, allow_pending_empty, snap, None)
        }
    }

    fn indexed_interaction_pick_radius(
        &self,
        wires: &Arc<Vec<WireModel>>,
        base_radius_px: f32,
        viewport_height_px: f32,
        include_line_weight: bool,
    ) -> f32 {
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            let (_, changed) = self.interaction_overlay_changed_index(base_epoch, &changes);
            base.pick_radius_px(
                changed.pick_radius_px(base_radius_px, viewport_height_px, include_line_weight),
                viewport_height_px,
                include_line_weight,
            )
        } else {
            self.cached_interaction_index(wires)
                .map_or(base_radius_px, |index| {
                    index.pick_radius_px(base_radius_px, viewport_height_px, include_line_weight)
                })
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

    /// Prepare hover indexes after derived geometry is installed during open.
    pub(crate) fn warm_interaction_caches(&self) {
        let perf = crate::perf::enabled();
        let t_start = perf.then(iced::time::Instant::now);
        let t_ei = perf.then(iced::time::Instant::now);
        let _ = self.entity_index();
        let ei_ms = crate::perf::elapsed_ms(t_ei);

        let t_ihi = perf.then(iced::time::Instant::now);
        let _ = self.interaction_handle_index();
        let ihi_ms = crate::perf::elapsed_ms(t_ihi);
        let total_ms = crate::perf::elapsed_ms(t_start);

        if perf {
            crate::perf_record!(
                "[perf] warm_interaction_caches total={:.1}ms entity_index={:.1}ms interaction_handle_index={:.1}ms",
                total_ms,
                ei_ms,
                ihi_ms,
            );
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
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let mut entries: Vec<(u64, [f64; 6])> = Vec::new();
        let interactive = self.hatch_interaction_filter();
        for (&handle, hatch) in &self.hatches {
            if !interactive(handle) {
                continue;
            }
            if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
            }
        }
        let direct_ms = crate::perf::elapsed_ms(rebuild_started);
        for (&handle, hatches) in self.insert_hatches_for_click().iter() {
            for hatch in hatches {
                if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                    entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
                }
            }
        }
        let inserts_ms = crate::perf::elapsed_ms(rebuild_started);
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionHandleIndex::build(entries));
        if rebuild_started.is_some() {
            crate::perf_record!(
                "[perf] handle-index rebuild direct={:.1}ms through-inserts={:.1}ms total={:.1}ms",
                direct_ms,
                inserts_ms,
                crate::perf::elapsed_ms(rebuild_started),
            );
        }
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
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            false,
            false,
            Some(SnapQueryCategory::Full),
        )
    }

    /// Fast cursor preview query. If the resident index is still being built,
    /// return an empty indexed result instead of scanning the entire drawing;
    /// the settled/click snap path continues to use the complete fallback.
    pub fn interaction_snap_preview_candidates_near(
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
            false,
            true,
            Some(SnapQueryCategory::DiscretePreview),
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
            None,
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
            None,
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
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let resident = self.interaction_source_is_resident(&wires, bounds.height);
        // Short-circuits as before: only asked when the source is resident.
        let worthwhile = resident && self.interaction_index_worthwhile(&wires);
        if !resident || !worthwhile {
            if crate::perf::enabled() {
                crate::perf_record!(
                    "[perf] pick-unindexed resident={} worthwhile={} wires={}",
                    resident,
                    worthwhile,
                    wires.len(),
                );
            }
            return self.materialize_stubs_cached(InteractionCandidates::all(wires));
        }
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        // Flat picks bring marker wires in separately. One large PDSIZE
        // marker should never expand the search for every ordinary edge.
        let radius_px = if snap.is_some() {
            radius_px
        } else {
            self.indexed_interaction_pick_radius(
                &wires,
                radius_px,
                if flat_ortho { 0.0 } else { bounds.height },
                include_line_weight,
            )
        };
        if !flat_ortho {
            let clip = view_rot * (cursor - eye).as_vec3().extend(1.0);
            if !clip.is_finite() || clip.w <= 1e-6 {
                return self.materialize_stubs_cached(self.unindexed_interaction_candidates(
                    wires,
                    allow_pending_empty,
                    snap,
                    None,
                ));
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
                snap,
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
            return self.materialize_stubs_cached(self.unindexed_interaction_candidates(
                wires,
                allow_pending_empty,
                snap,
                None,
            ));
        }
        let radius = radius_px.max(0.0) as f64 / s as f64;
        let query = [
            cursor.x - radius,
            cursor.y - radius,
            cursor.x + radius,
            cursor.y + radius,
        ];
        self.indexed_interaction_candidates_xy(
            wires,
            query,
            allow_pending_empty,
            false,
            snap.is_none(),
            snap,
        )
    }

    /// Shared rectangular broad phase for box/lasso/fence and command windows.
    /// Flat orthographic views query world XY; tilted/perspective views query
    /// projected 3D bounds using the supplied screen rectangle.
    /// Candidates for an area selection: box, crossing, fence and lasso.
    ///
    /// Every caller feeds the result to the box/fence hit tests and to
    /// `interaction_candidate_handles`, none of which snap, so the flat-ortho
    /// path asks for the reduced category set. A caller that needs snapping
    /// belongs on `interaction_candidates_near` instead.
    pub fn interaction_box_candidates(
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
        if flat_ortho && self.interaction_source_is_resident(&wires, bounds.height) {
            if let Some(index) = self.cached_interaction_index(&wires) {
                let pad_px = self.indexed_interaction_pick_radius(&wires, 0.0, bounds.height, true);
                let x = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
                    + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
                .sqrt();
                let y = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
                    + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
                .sqrt();
                let scale = x.min(y);
                if scale > 1e-6 {
                    let pad = pad_px as f64 / scale as f64;
                    let padded = [aabb[0] - pad, aabb[1] - pad, aabb[2] + pad, aabb[3] + pad];
                    return self.materialize_stubs_cached(index.query_xy_box_wires(wires, padded));
                }
            }
        }
        self.interaction_candidates_in_aabb(wires, aabb, screen_rect, view_rot, eye, bounds)
    }

    /// An already prepared resident index is required by incremental box
    /// previews. Its identity also invalidates a preview when an index is
    /// replaced asynchronously during the gesture.
    pub(crate) fn box_preview_interaction_index(
        &self,
        wires: &Arc<Vec<WireModel>>,
        height: f32,
    ) -> Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>> {
        self.interaction_source_is_resident(wires, height)
            .then(|| self.cached_interaction_index(wires))
            .flatten()
    }

    /// Fills and solid meshes have selection paths beyond the wire index.
    /// Recheck them conservatively instead of assuming their wire bounds
    /// describe every filled/instanced part of an entity.
    pub(crate) fn box_preview_non_wire_handles(&self) -> HashSet<Handle> {
        let mut handles: HashSet<Handle> = self.hatches.keys().copied().collect();
        handles.extend(self.insert_hatches_for_click().keys().copied());
        handles.extend(
            self.interaction_meshes_arc()
                .iter()
                .filter_map(|set| set.entity_handle()),
        );
        handles
    }

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
            return self.materialize_stubs_cached(
                crate::scene::pick::interaction_index::InteractionCandidates::all(wires),
            );
        }
        let pad_px = self.indexed_interaction_pick_radius(&wires, 0.0, bounds.height, true);
        if flat_ortho {
            let world_x_px = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
                + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
            .sqrt();
            let world_y_px = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
                + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
            .sqrt();
            let scale = world_x_px.min(world_y_px);
            let pad = if scale > 1e-6 {
                pad_px as f64 / scale as f64
            } else {
                0.0
            };
            self.indexed_interaction_candidates_xy(
                wires,
                [aabb[0] - pad, aabb[1] - pad, aabb[2] + pad, aabb[3] + pad],
                false,
                true,
                false,
                None,
            )
        } else {
            self.indexed_interaction_candidates_screen(
                wires,
                [
                    screen_rect[0] - pad_px,
                    screen_rect[1] - pad_px,
                    screen_rect[2] + pad_px,
                    screen_rect[3] + pad_px,
                ],
                view_rot,
                eye,
                bounds,
                false,
                None,
            )
        }
    }

    pub fn interaction_candidate_handles(
        &self,
        candidates: &crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> Option<HashSet<Handle>> {
        let handles = self.interaction_candidate_handles_all(candidates);
        // Reference-only entities (API v9) are plugin-owned display geometry:
        // their fills, solids and instanced meshes are not pickable, so the
        // click/hover fallback paths that consume this set skip them and a
        // click passes through to the editable entity underneath.
        handles.map(|mut handles| {
            handles.retain(|h| !self.is_reference_only(*h));
            handles
        })
    }

    fn interaction_candidate_handles_all(
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

    /// Handles whose geometry meets a world-XY box. The wire candidates are a
    /// broad phase: a wire without a finite box (an INSERT's snap carrier, a
    /// dimension part) comes back for any box, and an unindexed set returns
    /// every wire. STRETCH took all of them, so every block in the drawing
    /// joined it and vanished from its preview (#1891).
    pub fn interaction_handles_in_world_aabb(&self, aabb: [f64; 4]) -> HashSet<Handle> {
        let wires = self.hit_test_wires();
        let candidates =
            self.indexed_interaction_candidates_xy(wires, aabb, false, false, false, None);
        let mut handles: HashSet<Handle> = candidates
            .iter()
            .filter(|wire| wire_meets_xy_box(wire, aabb))
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .collect();
        let insert_hatches = self.insert_hatches_for_click();
        let meshes = self.interaction_meshes_arc();
        let interactive = self.hatch_interaction_filter();
        handles.extend(
            self.interaction_handle_index()
                .query_xy(aabb)
                .into_iter()
                .filter_map(|value| {
                    let handle = Handle::new(value);
                    (interactive(handle) || insert_hatches.contains_key(&handle)).then_some(handle)
                }),
        );
        handles.extend(meshes.iter().filter_map(|set| {
            let handle = set.entity_handle()?;
            let bounds = mesh_interaction_aabb(set)?;
            (bounds[3] >= aabb[0]
                && bounds[0] <= aabb[2]
                && bounds[4] >= aabb[1]
                && bounds[1] <= aabb[3])
                .then_some(handle)
        }));
        handles
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
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear inside or clear outside the box is told
                    // from the box around its vertices.
                    let told = boxes
                        .get(&(std::ptr::from_ref(mesh) as usize))
                        .and_then(|local| {
                            pick::hit_test::box_hit_from_bounds(
                                a,
                                b,
                                local,
                                set.instance_transform,
                                view_rot,
                                eye,
                                bounds,
                            )
                        });
                    told.unwrap_or_else(|| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Top-level solid handles caught by a lasso polygon.
    /// Everything a picked path takes, gathered from every kind of geometry the
    /// selection can reach: wires, hatches, hatches inside blocks, meshes and
    /// block meshes, narrowed first to what the path's own bounds can touch.
    ///
    /// `open_fence` distinguishes the two gestures that arrive here. A fence is
    /// a line drawn through the drawing and takes only what it actually cuts; a
    /// polygon closes back on itself and can also take what it encloses, or
    /// merely touches when `crossing`. The lasso and the Fence / WPolygon /
    /// CPolygon keywords both come through this, so they cannot drift apart on
    /// which geometry they remember to look at. (#596)
    #[allow(clippy::too_many_arguments)]
    pub fn path_hit_handles(
        &self,
        poly_pts: &[iced::Point],
        crossing: bool,
        open_fence: bool,
        all_wires: std::sync::Arc<Vec<WireModel>>,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        to_world: impl Fn(iced::Point) -> glam::DVec3,
    ) -> Vec<Handle> {
        let world_aabb = poly_pts.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |mut aabb, &point| {
                let world = to_world(point);
                aabb[0] = aabb[0].min(world.x);
                aabb[1] = aabb[1].min(world.y);
                aabb[2] = aabb[2].max(world.x);
                aabb[3] = aabb[3].max(world.y);
                aabb
            },
        );
        let screen_rect = poly_pts.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |mut rect, point| {
                rect[0] = rect[0].min(point.x);
                rect[1] = rect[1].min(point.y);
                rect[2] = rect[2].max(point.x);
                rect[3] = rect[3].max(point.y);
                rect
            },
        );
        let area_candidates = self.interaction_candidates_in_aabb(
            all_wires,
            world_aabb,
            screen_rect,
            view_rot,
            eye,
            bounds,
        );
        let candidate_handles = self.interaction_candidate_handles(&area_candidates);
        let wire_hits = if open_fence {
            crate::scene::pick::hit_test::poly_fence_hit(
                poly_pts,
                &area_candidates,
                view_rot,
                eye,
                bounds,
            )
        } else {
            crate::scene::pick::hit_test::poly_hit(
                poly_pts,
                crossing,
                &area_candidates,
                view_rot,
                eye,
                bounds,
            )
        };
        let mut handles: Vec<Handle> = wire_hits
            .into_iter()
            .filter_map(Self::handle_from_wire_name)
            .collect();
        // An open fence has no interior, so the area-based geometry can only be
        // reached by the polygon gestures.
        if !open_fence {
            handles.extend(crate::scene::pick::hit_test::poly_hit_hatch(
                poly_pts,
                crossing,
                &self.visible_hatches_for_click(candidate_handles.as_ref()),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(crate::scene::pick::hit_test::poly_hit_insert_hatch(
                poly_pts,
                crossing,
                self.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(self.mesh_poly_hit(
                poly_pts,
                crossing,
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(self.block_mesh_poly_hit(
                poly_pts,
                crossing,
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
        }
        handles.retain(|&h| self.passes_selection_filter(h) && !self.is_reference_only(h));
        handles
    }

    pub fn mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear of the rectangle around the lasso is told
                    // from the box around its vertices.
                    let missed =
                        boxes
                            .get(&(std::ptr::from_ref(mesh) as usize))
                            .is_some_and(|local| {
                                pick::hit_test::poly_misses_bounds(
                                    poly,
                                    local,
                                    set.instance_transform,
                                    view_rot,
                                    eye,
                                    bounds,
                                )
                            });
                    !missed
                        && !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
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
        self.solid_hit(cursor, view_rot, eye, bounds, candidate_handles, false)
    }

    /// Normal object selection for shaded 3D bodies is edge based. This keeps
    /// the broad face interior available to modelling commands without making
    /// it part of ordinary object picking.
    pub fn solid_edge_click_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
        tolerance_px: f32,
    ) -> Option<Handle> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_edge_click_hit(
            cursor,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                if candidate_handles.is_some_and(|handles| !handles.contains(&handle)) {
                    return None;
                }
                let (edges, edges_low) = set.geometry_edges();
                (!edges.is_empty()).then_some((handle, edges, edges_low, set.instance_transform))
            }),
            view_rot,
            eye,
            bounds,
            tolerance_px,
        )
    }

    pub fn solid_edge_hover_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
        tolerance_px: f32,
    ) -> Option<Handle> {
        self.solid_edge_click_hit(
            cursor,
            view_rot,
            eye,
            bounds,
            candidate_handles,
            tolerance_px,
        )
    }

    pub fn solid_click_point_for(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        target: Handle,
    ) -> Option<glam::DVec3> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_click_point(
            cursor,
            meshes.iter().filter_map(|set| {
                (set.entity_handle()? == target).then_some(())?;
                let mesh = set.geometry_lods().first()?;
                Some((
                    target,
                    mesh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            view_rot,
            eye,
            bounds,
        )
    }

    /// The solid under the cursor and where the pick ray meets it.
    pub fn solid_click_hit_point(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<(Handle, glam::DVec3)> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_click_handle_point(
            cursor,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                let mesh = set.geometry_lods().first()?;
                Some((
                    handle,
                    mesh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            view_rot,
            eye,
            bounds,
        )
    }

    pub fn solid_planar_face_normal_at(
        &mut self,
        handle: Handle,
        pick: glam::DVec3,
    ) -> Option<glam::DVec3> {
        self.restore_solid_models(&[handle]);
        let body = self.solid_models.get(&handle)?;
        let face = model::solid_model::nearest_planar_face(body, pick.to_array())?;
        model::solid_model::planar_face_normal(body, face).map(glam::DVec3::from_array)
    }

    pub fn view_center_surface_pivot(&self, bounds: iced::Rectangle) -> Option<glam::DVec3> {
        self.surface_pivot_at(
            bounds,
            iced::Point::new(bounds.width * 0.5, bounds.height * 0.5),
        )
    }

    /// The UCS of the planar solid face under `local` (relative to `bounds`),
    /// Z along the face normal, and the picked point on the face.
    pub fn planar_face_ucs_at(
        &mut self,
        bounds: iced::Rectangle,
        local: iced::Point,
    ) -> Option<(document::tables::Ucs, glam::DVec3)> {
        let camera = self.camera.borrow();
        let view_rot = camera.view_proj_rte(bounds);
        let eye = camera.eye();
        drop(camera);
        let (handle, pick) = self.solid_click_hit_point(local, view_rot, eye, bounds)?;
        self.restore_solid_models(&[handle]);
        let body = self.solid_models.get(&handle)?;
        let face = crate::scene::model::solid_model::nearest_planar_face(body, pick.to_array())?;
        crate::scene::model::solid_model::planar_face_ucs(body, face, pick.to_array())
            .map(|ucs| (ucs, pick))
    }

    /// Outline the planar solid face under `local` (relative to `bounds`), the
    /// face a Dynamic UCS would align to; `None` clears the outline.
    pub fn set_ducs_face_at(&mut self, at: Option<(iced::Rectangle, iced::Point)>) {
        let face = at.and_then(|(bounds, local)| {
            let camera = self.camera.borrow();
            let view_rot = camera.view_proj_rte(bounds);
            let eye = camera.eye();
            drop(camera);
            let (handle, pick) = self.solid_click_hit_point(local, view_rot, eye, bounds)?;
            self.restore_solid_models(&[handle]);
            let body = self.solid_models.get(&handle)?;
            let face =
                crate::scene::model::solid_model::nearest_planar_face(body, pick.to_array())?;
            Some((handle, face))
        });
        if face == self.ducs_face {
            return;
        }
        self.ducs_face = face;
        self.ducs_face_wires = face
            .and_then(|(handle, face)| {
                let body = self.solid_models.get(&handle)?;
                Some(
                    crate::scene::model::solid_model::face_outline(body, face)
                        .into_iter()
                        .map(|points| {
                            let mut wire = WireModel::solid_f64(
                                format!("ducs-face:{}", handle.value()),
                                points,
                                WireModel::HOVER,
                                false,
                            );
                            wire.set_fixed_screen_width(2.0);
                            wire
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        self.bump_hover();
    }

    /// The nearest drawn surface under `local` (relative to `bounds`): a solid
    /// or mesh face, or a wire's shaded triangles (3DFACE, filled SOLID,
    /// thickness walls) — drawings made only of faces must lock too.
    pub fn surface_pivot_at(
        &self,
        bounds: iced::Rectangle,
        local: iced::Point,
    ) -> Option<glam::DVec3> {
        let camera = self.camera.borrow();
        let view_rot = camera.view_proj_rte(bounds);
        let eye = camera.eye();
        drop(camera);
        let (near, _) = pick::hit_test::pick_ray(local, view_rot, eye, bounds)?;
        let wires = self.interaction_pick_candidates_near(
            self.hit_test_wires(),
            near,
            view_rot,
            eye,
            bounds,
            1.0,
        );
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::surface_click_point(
            local,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                let mesh = set.geometry_lods().first()?;
                Some((
                    handle,
                    mesh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            &wires,
            view_rot,
            eye,
            bounds,
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
        self.solid_hit(cursor, view_rot, eye, bounds, candidate_handles, true)
    }

    fn solid_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
        coarse_lod: bool,
    ) -> Option<Handle> {
        // Reuse the renderer's cached top-level and block-instance mesh set.
        let meshes = self.interaction_meshes_arc();
        let sets: Vec<(Handle, &MeshLodSet)> = meshes
            .iter()
            .filter_map(|set| {
                let handle = set.entity_handle()?;
                Some((handle, set))
            })
            .collect();
        pick::hit_test::mesh_click_hit(
            cursor,
            sets.iter().filter_map(|(handle, set)| {
                let lods = set.geometry_lods();
                let mesh = if coarse_lod {
                    lods.last()?
                } else {
                    lods.first()?
                };
                Some((
                    *handle,
                    mesh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
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
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear inside or clear outside the box is told
                    // from the box around its vertices.
                    let told = boxes
                        .get(&(std::ptr::from_ref(mesh) as usize))
                        .and_then(|local| {
                            pick::hit_test::box_hit_from_bounds(
                                a,
                                b,
                                local,
                                set.instance_transform,
                                view_rot,
                                eye,
                                bounds,
                            )
                        });
                    told.unwrap_or_else(|| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
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
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear of the rectangle around the lasso is told
                    // from the box around its vertices.
                    let missed =
                        boxes
                            .get(&(std::ptr::from_ref(mesh) as usize))
                            .is_some_and(|local| {
                                pick::hit_test::poly_misses_bounds(
                                    poly,
                                    local,
                                    set.instance_transform,
                                    view_rot,
                                    eye,
                                    bounds,
                                )
                            });
                    !missed
                        && !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
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
        let layer = self.document.layers.get(&e.common().layer);
        self.resident_entity_visible_with_layer(
            e,
            block_handle,
            frozen_layers,
            annotation_scale_handle,
            all_visible,
            layer,
        )
    }

    /// Visibility predicate with the layer resolved by the caller.
    fn resident_entity_visible_with_layer(
        &self,
        e: &EntityType,
        block_handle: Handle,
        frozen_layers: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        layer: Option<&document::tables::layer::Layer>,
    ) -> bool {
        let c = e.common();
        if self.display_entity_invisible(c) {
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
        if layer
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
        {
            return false;
        }
        // Session LAYWALK: in a walked space, layers outside the visible
        // set hide exactly like frozen ones — in memory, for this space only.
        if self.layer_walk_hidden(layer) {
            return false;
        }
        if let Some(frozen) = frozen_layers {
            if !frozen.is_empty() {
                if frozen.contains(&c.handle) {
                    return false;
                }
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

    fn wires_for_block_culled<'doc>(
        &'doc self,
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
        // Build only these entities — the same wires a full build would give
        // them, though not in document order — instead of the whole block.
        // View culling is not applied.
        only: Option<&HashSet<Handle>>,
    ) -> Vec<WireModel> {
        use document::objects::ObjectType;

        // Each INSERT tessellated below resolves its block's base point.
        let _owners = block_authoring::DictionaryOwners::publish(&self.document);
        // Skip diagnostic clock reads when PERF is disabled.
        let perf = crate::perf::enabled();
        let t_fn = perf.then(iced::time::Instant::now);

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
        // Resolve each distinct layer name once for this candidate pass.
        type LayerRef<'a> = Option<&'a document::tables::layer::Layer>;
        let layer_of: RefCell<rustc_hash::FxHashMap<&'doc str, LayerRef<'doc>>> =
            RefCell::new(rustc_hash::FxHashMap::default());
        let layer_for =
            |e: &'doc EntityType| match layer_of.borrow_mut().entry(e.common().layer.as_str()) {
                std::collections::hash_map::Entry::Occupied(hit) => *hit.get(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    *slot.insert(self.document.layers.get(e.common().layer.as_str()))
                }
            };
        let has_temporary_hidden = self.object_isolation.is_active()
            || !self.preview_hidden.is_empty()
            || !self.command_preview_hidden.is_empty()
            || !self.hidden_dynamic_dimensions.is_empty();
        let visibility_ok = |e: &'doc EntityType| {
            let c = e.common();
            if c.invisible && self.display_entity_invisible(c) {
                return false;
            }
            if has_temporary_hidden && self.entity_temporarily_hidden(c.handle) {
                return false;
            }
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                return false;
            }
            let layer = layer_for(e);
            if layer
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
            {
                return false;
            }
            if self.layer_walk_hidden(layer) {
                return false;
            }
            if let Some(frozen) = frozen_layers {
                if !frozen.is_empty() {
                    if frozen.contains(&c.handle) {
                        return false;
                    }
                    if let Some(lh) = layer.map(|l| l.handle) {
                        if frozen.contains(&lh) {
                            return false;
                        }
                    }
                }
            }
            if !all_visible
                && crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    c,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                return false;
            }
            self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
        };

        let sort_cache_ms = crate::perf::elapsed_ms(t_fn);
        let t_visible = perf.then(iced::time::Instant::now);
        // Which path the selection took, and how many candidates it looked at.
        // Left uninitialised on purpose: every branch below must set it, and
        // the compiler enforces that. A default here is what made the first
        // version of this diagnostic report a constant.
        let visible_path: &str;
        let mut visible_candidates: usize;
        // PERF-only breakdown for index maintenance and handle resolution.
        let mut visible_index_ms = 0.0f64;
        let mut visible_probe_ms = 0.0f64;
        // PERF-only ablation of the visibility predicate (see the block that
        // fills them). Each is a full pass, so their sum is also how much the
        // diagnostic itself adds to `visible` on a PERF run.
        let mut vis_a_layer_ms = 0.0f64;
        let mut vis_a_hidden_ms = 0.0f64;
        let mut vis_a_invis_ms = 0.0f64;
        let mut vis_a_walk_ms = 0.0f64;
        let mut vis_a_block_ms = 0.0f64;
        let mut vis_a_full_ms = 0.0f64;

        // Phase 2.1 — quadtree-driven candidate selection. When a view
        // AABB exists (Model layout with a settled camera), only iterate
        // entities whose stored WCS bbox intersects the view; unindexable
        // entities (Insert/Viewport) are appended via a small linear scan.
        // Paper space and the first-frame "settle" path fall back to the
        // full doc scan — preserving prior behaviour.
        let visible: Vec<&EntityType> = if let Some(only) = only {
            debug_assert!(view_aabb.is_none(), "a restricted build is never culled");
            visible_path = "only";
            visible_candidates = only.len();
            // Same membership as the full-scan / block-index paths below: any
            // entity when no block ownership is known, else direct content of
            // this block by owner handle or, without one, by the block map.
            let members: Vec<&EntityType> = {
                let block_map = self.entity_block_map();
                let any_block = block_handle.is_null() || block_map.is_empty();
                only.iter()
                    .filter_map(|&handle| self.document.get_entity(handle))
                    .filter(|entity| {
                        let common = entity.common();
                        any_block
                            || if common.owner_handle.is_null() {
                                block_map.get(&common.handle) == Some(&block_handle)
                            } else {
                                common.owner_handle == block_handle
                            }
                    })
                    .collect()
            };
            members
                .into_iter()
                .filter(|entity| visibility_ok(entity))
                .collect()
        } else if let Some(local_view) = view_aabb {
            visible_path = "quadtree";
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
            visible_candidates = candidates.len() + unbounded.len();
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
            // (block expansion, viewport rendering); always candidates. Taken
            // from the per-epoch list rather than by walking the document,
            // which preserves document order because the list is built that
            // way.
            {
                let unindexable = self.unindexable_handles();
                visible_candidates += unindexable.1.len();
                for &h in &unindexable.1 {
                    if let Some(e) = self.document.get_entity(h) {
                        if visibility_ok(e) {
                            out.push(e);
                        }
                    }
                }
            }
            out
        } else if block_handle.is_null() || self.entity_block_map().is_empty() {
            // Either every entity qualifies, or the drawing declares no block
            // contents anywhere and `belongs_to_visible_block` turns permissive
            // — the legacy-DXF case. Both need the full walk, and both must
            // keep document order, which is draw order for the wire pass.
            visible_path = "full-scan";
            visible_candidates = self.document.entity_count();
            self.document
                .entities()
                .filter(|e| visibility_ok(e))
                .collect()
        } else {
            // The per-epoch index preserves document order for each block.
            visible_path = "block-index";
            // Time index maintenance separately from the candidate loop.
            let t_members = perf.then(iced::time::Instant::now);
            let members = self.block_members();
            visible_index_ms = crate::perf::elapsed_ms(t_members);
            let (_, by_block) = &*members;
            let claimed = by_block
                .get(&block_handle)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            visible_candidates = claimed.len();
            let mut out: Vec<&EntityType> = Vec::with_capacity(claimed.len());
            for &handle in claimed {
                if let Some(entity) = self.document.get_entity(handle) {
                    if visibility_ok(entity) {
                        out.push(entity);
                    }
                }
            }
            if perf {
                // PERF-only lower bound for warm handle resolution.
                let t_probe = iced::time::Instant::now();
                let mut resolved = 0usize;
                for &handle in claimed {
                    if self.document.get_entity(handle).is_some() {
                        resolved += 1;
                    }
                }
                std::hint::black_box(resolved);
                visible_probe_ms = t_probe.elapsed().as_secs_f64() * 1000.0;

                // Which part of the predicate owns the rest of `visible`.
                //
                // Behind its own variable, not plain `PERF=1`: these are six
                // extra passes over every candidate, and they run INSIDE the
                // `visible` region, so leaving them on inflates `visible` and
                // `wires-build` (measured: 2406 ms → 4389 ms) and makes a
                // PERF run incomparable with any other.
                //
                // Timing each call individually cannot answer this either: at
                // ~1.3 µs per entity the two `Instant::now()` reads cost more
                // than the work and report themselves (tried; the two halves
                // came back equal to 3 digits and `visible` went 350 ms →
                // 4650 ms). So each component gets ONE timer over the whole
                // list, and its cost is that run minus `visible_probe`.
                //
                // Read them as WARM figures. They run after the main loop has
                // already touched every entity, so they exclude the cold-miss
                // cost that dominates the real pass: `full` here is ~149 ms
                // against ~650 ms for the same work done first. Do not
                // conclude from a small `full` that the predicate is cheap.
                let ablate = |f: &dyn Fn(&'doc EntityType) -> bool| {
                    let t = iced::time::Instant::now();
                    let mut n = 0usize;
                    for &handle in claimed {
                        if let Some(e) = self.document.get_entity(handle) {
                            n += f(e) as usize;
                        }
                    }
                    std::hint::black_box(n);
                    t.elapsed().as_secs_f64() * 1000.0
                };
                if std::env::var_os("OCS_PERF_ABLATE").is_some() {
                    vis_a_layer_ms = ablate(&|e| layer_for(e).is_some());
                    vis_a_hidden_ms =
                        ablate(&|e| self.entity_temporarily_hidden(e.common().handle));
                    vis_a_invis_ms = ablate(&|e| self.display_entity_invisible(e.common()));
                    // Note this resolves the layer itself, so its figure
                    // includes `layer`; subtract to get the walk test alone.
                    vis_a_walk_ms = ablate(&|e| self.layer_walk_hidden(layer_for(e)));
                    vis_a_block_ms = ablate(&|e| {
                        let c = e.common();
                        self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
                    });
                    vis_a_full_ms = ablate(&|e| visibility_ok(e));
                }
            }
            out
        };

        let visible_ms = crate::perf::elapsed_ms(t_visible);
        let visible_count = visible.len();

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
        // The reference-only designation set drives the wire stamp; captured
        // as a shared ref like `doc`/`sel` so the rayon workers can read it.
        let reference_only_set = self.reference_only.handles();
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
        // Content shown inside a paper-space viewport carries a scale override
        // or the PSLTSCALE factor; only there do complex linetypes ignore the
        // annotation scale.
        let paper = self.viewport_lt_for(anno_scale_override);
        let t_blk = perf.then(iced::time::Instant::now);
        let blk_cache =
            self.block_cache_arc_for(annotation_scale_handle, all_visible, style_viewport);
        let blk_ms = crate::perf::elapsed_ms(t_blk);
        let blk_ref: &cache::block_cache::BlockCache = &blk_cache;
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
        let resident = base_ok && view_aabb.is_none() && resident_memo_enabled();
        let memo_active = base_ok && (view_aabb.is_some() || resident);
        // Reported in one line below. `tess_ms` / `memo_misses` are assigned by
        // both branches; the rest exist only on the memo path and stay zero.
        let mut classify_ms = 0.0f64;
        let mut hits_ms = 0.0f64;
        let tess_ms;
        let mut materialize_ms = 0.0f64;
        let mut memo_hits = 0usize;
        let memo_misses;
        // Two consecutive identical resident builds both missed every entry,
        // which a stable guard cannot explain — the memo is being cleared
        // between them. Report the guard and the inputs that can move it.
        let mut guard_dbg = 0u64;
        let mut guard_was_stale = false;
        let t_build = perf.then(iced::time::Instant::now);
        let mut wires: Vec<WireModel> = if memo_active {
            // Invalidate when any baked display input changes.
            let guard = tess_memo_guard(
                view_aabb,
                anno,
                annotation_scale_handle,
                all_visible,
                bg,
                avp,
            ) ^ if self.constraint_shading_active() {
                0xC057_A705_0000_0001
            } else {
                0
            };
            let (memo_cell, guard_cell, atlas_cell, complex_lt_cell) = if resident {
                (
                    &self.resident_tess_memo,
                    &self.resident_tess_guard,
                    &self.resident_tess_atlas,
                    &self.resident_tess_complex_lt,
                )
            } else {
                (
                    &self.tess_memo,
                    &self.tess_memo_guard,
                    &self.tess_memo_atlas,
                    &self.tess_memo_complex_lt,
                )
            };
            guard_dbg = guard;
            guard_was_stale = guard_cell.get() != guard;
            if guard_was_stale {
                memo_cell.borrow_mut().clear();
                guard_cell.set(guard);
            }
            drop_stale_text(self, &mut memo_cell.borrow_mut(), atlas_cell);
            drop_stale_complex_lt(self, &mut memo_cell.borrow_mut(), complex_lt_cell);
            // Classify (serial, cheap): reuse memoized Arcs, collect misses.
            let t_classify = perf.then(iced::time::Instant::now);
            let mut hit_arcs: Vec<MemoHit> = Vec::new();
            let mut misses: Vec<&EntityType> = Vec::new();
            {
                let memo = memo_cell.borrow();
                for e in &visible {
                    let h = e.common().handle;
                    match memo.get(&h).and_then(|run| self.resolve_memo_run(h, run)) {
                        Some(hit) => hit_arcs.push(hit),
                        None => misses.push(*e),
                    }
                }
            }
            classify_ms = crate::perf::elapsed_ms(t_classify);
            memo_hits = hit_arcs.len();
            memo_misses = misses.len();
            // Materialize hits + tessellate misses, both in parallel.
            let t_hits = perf.then(iced::time::Instant::now);
            let hit_wires: Vec<WireModel> = hit_arcs
                .par_iter()
                .flat_map_iter(|hit| hit.wires().iter().cloned())
                .collect();
            hits_ms = crate::perf::elapsed_ms(t_hits);
            let is_building_resident = resident && self.building_resident_set.get();
            let mut out = hit_wires;
            if is_building_resident {
                const MISS_CHUNK_SIZE: usize = 256;
                struct MissChunk {
                    wires: Vec<WireModel>,
                    handles: Vec<Handle>,
                    solid_ranges: Vec<(Handle, usize, usize)>,
                }
                let t_tess_miss = perf.then(iced::time::Instant::now);
                let chunks: Vec<MissChunk> = misses
                    .par_chunks(MISS_CHUNK_SIZE)
                    .map(|chunk| {
                        let mut wires = Vec::with_capacity(chunk.len());
                        let mut handles = Vec::with_capacity(chunk.len());
                        let mut solid_ranges = Vec::new();
                        for &e in chunk {
                            let h = e.common().handle;
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
                                reference_only_set,
                            );
                            if Self::is_solid_shell_entity(e) {
                                let start = wires.len();
                                wires.extend(w);
                                let end = wires.len();
                                solid_ranges.push((h, start, end));
                            } else {
                                wires.extend(w);
                            }
                            handles.push(h);
                        }
                        MissChunk {
                            wires,
                            handles,
                            solid_ranges,
                        }
                    })
                    .collect();
                tess_ms = crate::perf::elapsed_ms(t_tess_miss);
                let t_materialize = perf.then(iced::time::Instant::now);
                let total_new_wires: usize = chunks.iter().map(|c| c.wires.len()).sum();
                out.reserve(total_new_wires);
                {
                    let mut memo = memo_cell.borrow_mut();
                    memo.reserve(misses.len());
                    for chunk in chunks {
                        let base = out.len();
                        out.extend(chunk.wires);
                        for (h, start, end) in chunk.solid_ranges {
                            self.attach_solid_snaps(h, &mut out[base + start..base + end]);
                        }
                        for h in chunk.handles {
                            // The resident set built from `out` becomes the
                            // memo's backing: no second copy (#1542, #1698).
                            memo.insert(h, MemoRun::Pending);
                        }
                    }
                }
                materialize_ms = crate::perf::elapsed_ms(t_materialize);
            } else {
                let t_tess_miss = perf.then(iced::time::Instant::now);
                let miss_pairs: Vec<(Handle, bool, Vec<WireModel>)> = misses
                    .par_iter()
                    .map(|&e| {
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
                            reference_only_set,
                        );
                        (e.common().handle, Self::is_solid_shell_entity(e), w)
                    })
                    .collect();
                tess_ms = crate::perf::elapsed_ms(t_tess_miss);
                // Materialize new wires while the memo retains their shared source.
                let t_materialize = perf.then(iced::time::Instant::now);
                let total_new_wires: usize = miss_pairs.iter().map(|(_, _, w)| w.len()).sum();
                out.reserve(total_new_wires);
                {
                    let mut memo = memo_cell.borrow_mut();
                    memo.reserve(miss_pairs.len());
                    for (h, is_solid, mut wires) in miss_pairs {
                        // Enrich solid shells with B-rep vertex snaps before the memo/assembly split.
                        if is_solid {
                            self.attach_solid_snaps(h, &mut wires);
                        }
                        let a = Arc::new(wires);
                        out.extend(a.iter().cloned());
                        memo.insert(h, MemoRun::Owned(a));
                    }
                }
                materialize_ms = crate::perf::elapsed_ms(t_materialize);
            }
            out
        } else {
            let t_tess_all = perf.then(iced::time::Instant::now);
            let out: Vec<WireModel> = visible
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
                        reference_only_set,
                    )
                })
                .collect();
            tess_ms = crate::perf::elapsed_ms(t_tess_all);
            memo_misses = visible_count;
            out
        };

        // Normal Wireframe renders the persisted entity wires, whereas shaded
        // and 3D Wireframe modes render the temporary sectioned mesh. Replace
        // only the displayed wire run with one tessellated from that same
        // temporary body so every visual style shows the identical live cut.
        if let Some(section) = self.active_live_section(block_handle) {
            let handles: Vec<Handle> = self.meshes.keys().copied().collect();
            for handle in handles {
                if only.is_some_and(|only| !only.contains(&handle)) {
                    continue;
                }
                let Some(source) = self.document.get_entity(handle) else {
                    continue;
                };
                if !visibility_ok(source) {
                    continue;
                }
                let replacement = match self.sectioned_wire_entity(handle, &section) {
                    Ok(Some(entity)) => Some(tessellate_entity(
                        doc,
                        sel,
                        avp,
                        bg,
                        anno,
                        annotation_scale_handle,
                        &entity,
                        Some(blk_ref),
                        view_aabb,
                        wpp,
                        paper,
                        reference_only_set,
                    )),
                    Ok(None) => Some(Vec::new()),
                    Err(()) => None,
                };
                let Some(replacement) = replacement else {
                    continue;
                };
                let name = handle.value().to_string();
                wires.retain(|wire| wire.name != name);
                wires.extend(replacement);
            }
        }
        let build_ms = crate::perf::elapsed_ms(t_build);

        // Resolve display colours for the live background. Memo hits were
        // built under whatever background was current then, misses under this
        // one; the pass recomputes both from their recorded raw colour, so a
        // mixed set lands in the same place either way.
        let t_colors = perf.then(iced::time::Instant::now);
        crate::scene::view::render::resolve_colors_for_bg(&mut wires, bg);
        self.shade_constraint_wires(block_handle, &mut wires);
        let colors_ms = crate::perf::elapsed_ms(t_colors);

        // Apply draw order via the cached index (O(1) block lookup).
        let t_sort = perf.then(iced::time::Instant::now);
        let mut sorted = false;
        {
            let cache = self.sort_cache.borrow();
            if let Some((_, ref idx)) = *cache {
                if let Some(sort_map) = idx.get(&block_handle) {
                    sorted = true;
                    // Parse each handle once while preserving stable draw order.
                    wires.sort_by_cached_key(|w| {
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
        if perf {
            crate::perf_record!(
                "[perf] wires-build total={:.1}ms sort_cache={:.1} visible={:.1} blk_cache={:.1} colors={:.1} \
build={:.1} [classify={:.1} hits={:.1} tess={:.1} materialize={:.1}] sort={:.1}({}) \
entities={} memo_hit={} memo_miss={} wires={} memo={} visible_path={} candidates={} \
guard={:016x} guard_stale={} avp={} anno={:.4} anno_h={} all_vis={} sdf_gen={} \
vis_index={:.1} visible_probe={:.1} \
vis_a[layer={:.1} hidden={:.1} invis={:.1} walk={:.1} block={:.1} full={:.1}]",
                crate::perf::elapsed_ms(t_fn),
                sort_cache_ms,
                visible_ms,
                blk_ms,
                colors_ms,
                build_ms,
                classify_ms,
                hits_ms,
                tess_ms,
                materialize_ms,
                crate::perf::elapsed_ms(t_sort),
                if sorted { "applied" } else { "skipped" },
                visible_count,
                memo_hits,
                memo_misses,
                wires.len(),
                if memo_active { "on" } else { "off" },
                visible_path,
                visible_candidates,
                guard_dbg,
                guard_was_stale,
                avp.map(|h| h.value()).unwrap_or(0),
                anno,
                annotation_scale_handle.map(|h| h.value()).unwrap_or(0),
                all_visible,
                crate::scene::text::sdf_atlas::generation(),
                visible_index_ms,
                visible_probe_ms,
                vis_a_layer_ms,
                vis_a_hidden_ms,
                vis_a_invis_ms,
                vis_a_walk_ms,
                vis_a_block_ms,
                vis_a_full_ms,
            );
        }
        wires
    }

    /// Unindexable entity handles in document order, cached per geometry epoch.
    fn unindexable_handles(&self) -> std::cell::Ref<'_, (u64, Vec<Handle>)> {
        {
            let cache = self.unindexable_cache.borrow();
            if cache
                .as_ref()
                .is_some_and(|(epoch, _)| *epoch == self.geometry_epoch)
            {
                drop(cache);
                return std::cell::Ref::map(self.unindexable_cache.borrow(), |c| {
                    c.as_ref().unwrap()
                });
            }
        }
        let handles: Vec<Handle> = self
            .document
            .entities()
            .filter(|entity| is_unindexable_entity(entity))
            .map(|entity| entity.common().handle)
            .collect();
        *self.unindexable_cache.borrow_mut() = Some((self.geometry_epoch, handles));
        std::cell::Ref::map(self.unindexable_cache.borrow(), |c| c.as_ref().unwrap())
    }

    /// Resolve block membership once per geometry epoch. A single addition is
    /// appended safely; any other delta rebuilds the document-ordered index.
    fn block_members(&self) -> std::cell::Ref<'_, BlockMembers> {
        let mut cached_epoch = None;
        {
            let cache = self.block_members_cache.borrow();
            if let Some((epoch, _)) = cache.as_ref() {
                if *epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.block_members_cache.borrow(), |c| {
                        c.as_ref().unwrap()
                    });
                }
                cached_epoch = Some(*epoch);
            }
        }

        if let Some(since) = cached_epoch {
            let lone_add = self
                .replay_since(since)
                .filter(|deltas| deltas.len() == 1 && deltas[0].1 == ChangeKind::Added);
            if let Some(deltas) = lone_add {
                let handle = deltas[0].0;
                // Resolve the owner before touching the cache: both lookups
                // borrow, and `entity_block_map` is its own `RefCell`.
                let owner = self.document.get_entity(handle).and_then(|entity| {
                    let owner = entity.common().owner_handle;
                    if !owner.is_null() {
                        Some(owner)
                    } else {
                        self.entity_block_map().get(&handle).copied()
                    }
                });
                let mut cache = self.block_members_cache.borrow_mut();
                if let Some((epoch, members)) = cache.as_mut() {
                    if let Some(owner) = owner {
                        members.entry(owner).or_default().push(handle);
                    }
                    *epoch = self.geometry_epoch;
                    drop(cache);
                    return std::cell::Ref::map(self.block_members_cache.borrow(), |c| {
                        c.as_ref().unwrap()
                    });
                }
            }
        }
        let mut members: HashMap<Handle, Vec<Handle>> = HashMap::default();
        {
            let map = self.entity_block_map();
            for entity in self.document.entities() {
                let common = entity.common();
                let owner = common.owner_handle;
                if !owner.is_null() {
                    members.entry(owner).or_default().push(common.handle);
                } else if let Some(&block) = map.get(&common.handle) {
                    members.entry(block).or_default().push(common.handle);
                }
            }
        }
        *self.block_members_cache.borrow_mut() = Some((self.geometry_epoch, members));
        std::cell::Ref::map(self.block_members_cache.borrow(), |c| c.as_ref().unwrap())
    }

    /// Whether the entity is direct content of this block.
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

    fn entity_dependency_signature(&self, entity: &EntityType) -> u64 {
        self.entity_dependency_signature_with(
            entity,
            &render_graph::entity_block_uses(&self.document, entity, 1.0),
            &normalize_name(&entity.common().layer),
        )
    }

    /// `block_uses` must be exactly what `entity_block_uses` returns for this
    /// entity, and `normalized_layer` the uppercased common layer. The rebuild
    /// passes what its roots loop already collected instead of recomputing
    /// them for every entity a second time. The hash feeds only in-memory
    /// cache-invalidation comparisons, so the hasher choice is free.
    fn entity_dependency_signature_with(
        &self,
        entity: &EntityType,
        block_uses: &[render_graph::BlockUse],
        normalized_layer: &str,
    ) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        let common = entity.common();
        common.handle.hash(&mut hasher);
        common.owner_handle.hash(&mut hasher);
        normalized_layer.hash(&mut hasher);
        normalize_name(&common.linetype).hash(&mut hasher);

        let category = match entity {
            EntityType::Point(_) => 1_u8,
            EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::AttributeEntity(_)
            | EntityType::Dimension(_)
            | EntityType::Leader(_)
            | EntityType::Tolerance(_)
            | EntityType::MultiLeader(_)
            | EntityType::Table(_)
            | EntityType::Shape(_) => 2,
            EntityType::Hatch(_) => 3,
            EntityType::Insert(insert) if !insert.attributes.is_empty() => 4,
            EntityType::Wipeout(_) => 5,
            _ => 0,
        };
        category.hash(&mut hasher);
        crate::scene::annotative::is_annotative(&self.document, entity).hash(&mut hasher);

        for block_use in block_uses {
            block_use.block.hash(&mut hasher);
            normalize_name(&block_use.insert.block_name).hash(&mut hasher);
            block_use.role.hash(&mut hasher);
            block_use.scale_policy.hash(&mut hasher);
            block_use.active.hash(&mut hasher);
        }

        match entity {
            EntityType::Text(text) => normalize_name(&text.style).hash(&mut hasher),
            EntityType::MText(text) => normalize_name(&text.style).hash(&mut hasher),
            EntityType::AttributeDefinition(attribute) => {
                normalize_name(&attribute.text_style).hash(&mut hasher)
            }
            EntityType::AttributeEntity(attribute) => {
                normalize_name(&attribute.text_style).hash(&mut hasher)
            }
            EntityType::Insert(insert) => {
                for attribute in &insert.attributes {
                    normalize_name(&attribute.common.layer).hash(&mut hasher);
                    normalize_name(&attribute.text_style).hash(&mut hasher);
                }
            }
            EntityType::Dimension(dimension) => {
                normalize_name(&dimension.base().style_name).hash(&mut hasher)
            }
            EntityType::Leader(leader) => normalize_name(&leader.dimension_style).hash(&mut hasher),
            EntityType::Tolerance(tolerance) => {
                normalize_name(&tolerance.dimension_style_name).hash(&mut hasher)
            }
            EntityType::Table(table) => {
                table.table_style_handle.hash(&mut hasher);
                for row in &table.rows {
                    row.style
                        .as_ref()
                        .and_then(|style| style.text_style_handle)
                        .hash(&mut hasher);
                    for cell in &row.cells {
                        cell.style
                            .as_ref()
                            .and_then(|style| style.text_style_handle)
                            .hash(&mut hasher);
                        for content in &cell.contents {
                            content.text_style_handle.hash(&mut hasher);
                        }
                    }
                }
            }
            EntityType::MultiLeader(leader) => {
                leader.style_handle.hash(&mut hasher);
                leader.text_style_handle.hash(&mut hasher);
                leader.context.text_style_handle.hash(&mut hasher);
            }
            EntityType::MLine(line) => line.style_handle.hash(&mut hasher),
            _ => {}
        }
        hasher.finish()
    }

    fn refresh_dependency_index_for_changes(&self, changes: &[(Handle, ChangeKind)]) {
        let mut cache = self.dependency_index_cache.borrow_mut();
        let Some(index) = cache.as_ref() else {
            return;
        };
        let unchanged = changes.iter().all(|(handle, kind)| {
            if !matches!(kind, ChangeKind::Modified) {
                return false;
            }
            let Some(entity) = self.document.get_entity(*handle) else {
                return false;
            };
            index.signatures.get(handle).copied() == Some(self.entity_dependency_signature(entity))
        });
        if !unchanged {
            cache.take();
        }
    }

    /// doc(hidden) public only so `benches/dependency_index_rebuild.rs` can
    /// time the real production rebuild that follows an edit-dropped cache.
    #[doc(hidden)]
    pub fn dependency_index_rebuild_for_bench(&self) -> (usize, usize, usize) {
        let index = self.rebuild_dependency_index();
        (
            index.layers.len(),
            index.text_styles.len(),
            index.block_roots.values().map(|roots| roots.len()).sum(),
        )
    }

    fn rebuild_dependency_index(&self) -> SceneDependencyIndex {
        let mut layout_blocks: HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                document::objects::ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        // DWG imports may have model/paper block records without matching
        // Layout objects. They still own directly rendered entities, so their
        // layer dependencies must resolve to those entity handles.
        layout_blocks.extend(self.document.block_records.iter().filter_map(|record| {
            let name = normalize_name(&record.name);
            (name.starts_with("*MODEL_SPACE") || name.starts_with("*PAPER_SPACE"))
                .then_some(record.handle)
        }));
        let block_names: HashMap<Handle, String> = self
            .document
            .block_records
            .iter()
            .map(|record| (record.handle, normalize_name(&record.name)))
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
        let linetype_names: HashMap<Handle, String> = self
            .document
            .line_types
            .iter()
            .map(|lt| (lt.handle, lt.name.clone()))
            .collect();

        let mut roots: HashMap<String, HashSet<Handle>> = HashMap::default();
        let mut parents: HashMap<String, HashSet<String>> = HashMap::default();
        let mut block_uses_of: HashMap<Handle, Vec<render_graph::BlockUse>> = HashMap::default();
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
            let block_uses = render_graph::entity_block_uses(&self.document, entity, 1.0);
            for block_use in &block_uses {
                let target = normalize_name(&block_use.insert.block_name);
                if target.is_empty() {
                    continue;
                }
                if owner.is_null() || layout_blocks.contains(&owner) {
                    roots.entry(target).or_default().insert(common.handle);
                } else if let Some(parent) = block_names.get(&owner) {
                    parents.entry(target).or_default().insert(parent.clone());
                }
            }
            if !block_uses.is_empty() {
                block_uses_of.insert(common.handle, block_uses);
            }
        }
        // Propagate root users through nested block references.
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
        index.signatures.reserve(self.document.entity_count());
        let mut normalized_layers: HashMap<&str, String> = HashMap::default();
        let mut complex_linetypes: HashMap<&str, bool> = HashMap::default();
        let defines_complex = crate::io::linetypes::defines_complex_lt(&self.document);
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
            let render_handles = if inside_block {
                RenderHandles::Block(block_names.get(&owner).and_then(|name| roots.get(name)))
            } else {
                RenderHandles::SelfOnly(common.handle)
            };
            if matches!(entity, EntityType::Wipeout(_)) {
                match &render_handles {
                    RenderHandles::SelfOnly(handle) => {
                        index.wipeout_hosts.insert(*handle);
                    }
                    RenderHandles::Block(Some(roots)) => {
                        index.wipeout_hosts.extend(roots.iter().copied());
                    }
                    RenderHandles::Block(None) => {}
                }
            }
            let block_uses = block_uses_of
                .get(&common.handle)
                .map(|uses| uses.as_slice())
                .unwrap_or(&[]);
            // Layers repeat across the document; memoize the uppercased key
            // instead of allocating it once per entity.
            let normalized_layer = normalized_layers
                .entry(common.layer.as_str())
                .or_insert_with(|| normalize_name(&common.layer));
            index.signatures.insert(
                common.handle,
                self.entity_dependency_signature_with(entity, block_uses, normalized_layer),
            );
            if matches!(entity, EntityType::Point(_)) {
                render_handles.extend_into(&mut index.points, common.handle);
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
                render_handles.extend_into(&mut index.text_geometry, common.handle);
            }
            // A complex linetype bakes the annotation scale into its wires, so
            // changing the scale has to re-tessellate it like annotative content.
            let mut is_complex = |name| {
                *complex_linetypes.entry(name).or_insert_with(|| {
                    crate::io::linetypes::resolve_complex_lt(&self.document, name).is_some()
                })
            };
            let lt_name = view::render::linetype_name_for_viewport(&self.document, entity, None);
            let complex_linetype = is_complex(lt_name);
            // An MLINE draws its elements' linetypes, read off its style; a style
            // without elements draws two ByLayer lines.
            let complex_mline = defines_complex
                && match entity {
                    EntityType::MLine(mline) => {
                        let layer = mline.common.layer.as_str();
                        match crate::entities::mline::resolved_mline_style(mline, &self.document)
                            .filter(|style| !style.elements.is_empty())
                        {
                            Some(style) => style.elements.iter().any(|element| {
                                is_complex(crate::entities::mline::element_linetype_name(
                                    &self.document,
                                    layer,
                                    &element.linetype,
                                    None,
                                ))
                            }),
                            None => is_complex(crate::entities::mline::element_linetype_name(
                                &self.document,
                                layer,
                                "ByLayer",
                                None,
                            )),
                        }
                    }
                    _ => false,
                };
            if complex_linetype
                || complex_mline
                || matches!(entity, EntityType::Hatch(_))
                || crate::scene::annotative::is_annotative(&self.document, entity)
            {
                render_handles.extend_into(&mut index.annotation_geometry, common.handle);
            }
            // The raw-name probe avoids a normalized-name allocation per
            // entity; a miss still inserts under the normalized key, so
            // case-variant names keep resolving to the same entry.
            let add = |map: &mut HashMap<String, DependencyTargets>, name: &str| {
                let target = match map.get_mut(name) {
                    Some(target) => target,
                    None => map.entry(normalize_name(name)).or_default(),
                };
                render_handles.extend_into(target, common.handle);
            };
            let add_handle = |map: &mut HashMap<Handle, DependencyTargets>,
                              handle: Option<Handle>| {
                let Some(handle) = handle.filter(|handle| !handle.is_null()) else {
                    return;
                };
                let target = map.entry(handle).or_default();
                render_handles.extend_into(target, common.handle);
            };
            let is_direct_linetype = |name: &str| {
                !name.is_empty()
                    && !name.eq_ignore_ascii_case("bylayer")
                    && !name.eq_ignore_ascii_case("byblock")
            };
            add(&mut index.layers, &common.layer);
            if is_direct_linetype(&common.linetype) {
                add(&mut index.linetypes, &common.linetype);
            }
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
                        add(&mut index.layers, &attribute.common.layer);
                        if is_direct_linetype(&attribute.common.linetype) {
                            add(&mut index.linetypes, &attribute.common.linetype);
                        }
                        add(&mut index.text_styles, &attribute.text_style);
                    }
                }
                EntityType::Dimension(dimension) => {
                    add(&mut index.dim_styles, &dimension.base().style_name);
                    if let Some(style) = self.document.dim_styles.get(&dimension.base().style_name)
                    {
                        add(&mut index.text_styles, &style.dimtxsty);
                        for lt_handle in [
                            style.dimltex_handle,
                            style.dimltex1_handle,
                            style.dimltex2_handle,
                        ] {
                            if let Some(name) = linetype_names.get(&lt_handle) {
                                add(&mut index.linetypes, name);
                            }
                        }
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
                            for border in [
                                &style.top_border,
                                &style.right_border,
                                &style.bottom_border,
                                &style.left_border,
                            ]
                            .into_iter()
                            .chain(style.additional_borders.iter().map(|(_, border)| border))
                            {
                                if let Some(name) = border
                                    .line_type_handle
                                    .and_then(|handle| linetype_names.get(&handle))
                                {
                                    add(&mut index.linetypes, name);
                                }
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
                                for border in [
                                    &style.top_border,
                                    &style.right_border,
                                    &style.bottom_border,
                                    &style.left_border,
                                ]
                                .into_iter()
                                .chain(style.additional_borders.iter().map(|(_, border)| border))
                                {
                                    if let Some(name) = border
                                        .line_type_handle
                                        .and_then(|handle| linetype_names.get(&handle))
                                    {
                                        add(&mut index.linetypes, name);
                                    }
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
                        if let Some(name) = handle.and_then(|handle| text_style_names.get(&handle))
                        {
                            add(&mut index.text_styles, name);
                        }
                    }
                    for handle in
                        [leader.line_type_handle].into_iter().chain(
                            leader.context.leader_roots.iter().flat_map(|root| {
                                root.lines.iter().map(|line| line.line_type_handle)
                            }),
                        )
                    {
                        if let Some(name) = handle.and_then(|handle| linetype_names.get(&handle)) {
                            add(&mut index.linetypes, name);
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
                        if let Some(name) = style
                            .line_type_handle
                            .and_then(|handle| linetype_names.get(&handle))
                        {
                            add(&mut index.linetypes, name);
                        }
                    }
                }
                EntityType::MLine(line) => {
                    add_handle(&mut index.object_styles, line.style_handle);
                    if let Some(style) =
                        crate::entities::mline::resolved_mline_style(line, &self.document)
                    {
                        for element in &style.elements {
                            if is_direct_linetype(&element.linetype) {
                                add(&mut index.linetypes, &element.linetype);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        index.block_roots = roots;
        index
    }

    fn dependency_targets(&self, kind: DependencyKind, names: &[String]) -> DependencyTargets {
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        let cache = self.dependency_index_cache.borrow();
        let index = cache.as_ref().unwrap();
        let map = match kind {
            DependencyKind::Layer => &index.layers,
            DependencyKind::Linetype => &index.linetypes,
            DependencyKind::TextStyle => &index.text_styles,
            DependencyKind::DimStyle => &index.dim_styles,
        };
        let mut combined = DependencyTargets::default();
        for name in names {
            let Some(target) = map.get(&normalize_name(name)) else {
                continue;
            };
            combined.extend_from(target);
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
        self.bump_entities_restyled(&changes);
    }

    /// Off/freeze toggle on `names`, nothing else changed. Visibility gates an
    /// entity before it is tessellated, so entities carrying these layers keep
    /// their tessellation memo and only leave or re-enter the resident set;
    /// block users re-expand, since their definitions bake the children's
    /// visibility. Nothing is recoloured.
    pub fn invalidate_layer_visibility(&mut self, names: &[String]) {
        // Turning a layer off or freezing it removes its entities from the
        // interactive scene as well as from rendering. Drop any entities that
        // were already selected before the visibility change, otherwise a
        // subsequent MOVE/ERASE/etc. could still modify invisible geometry.
        let block = self.interaction_block_handle();
        let frozen: Option<HashSet<Handle>> = self
            .interaction_viewport_frozen_layers()
            .map(|layers| layers.iter().copied().collect());
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();

        let visible_selection: Vec<Handle> = self
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| {
                self.document.get_entity(*handle).is_some_and(|entity| {
                    self.resident_entity_visible(
                        entity,
                        block,
                        frozen.as_ref(),
                        annotation_scale_handle,
                        all_visible,
                    )
                })
            })
            .collect();
        self.replace_selection_exact(&visible_selection);

        let targets = self.dependency_targets(DependencyKind::Layer, names);
        if targets.render_handles.is_empty() {
            return;
        }
        if targets.touches_block_definition {
            let records: HashMap<Handle, &str> = self
                .document
                .block_records
                .iter()
                .map(|record| (record.handle, record.name.as_str()))
                .collect();
            let touched: Vec<String> = targets
                .source_handles
                .iter()
                .filter_map(|handle| self.document.get_entity(*handle))
                .filter_map(|entity| records.get(&entity.common().owner_handle))
                .map(|name| name.to_string())
                .collect();
            self.mark_block_definitions_dirty(touched);
        }
        let kept: Vec<_> = {
            let mut tm = self.tess_memo.borrow_mut();
            let mut rm = self.resident_tess_memo.borrow_mut();
            targets
                .render_handles
                .iter()
                .filter(|handle| !targets.block_users.contains(handle))
                .map(|&handle| (handle, tm.remove(&handle), rm.remove(&handle)))
                .collect()
        };
        let changes: Vec<(Handle, ChangeKind)> = targets
            .render_handles
            .iter()
            .map(|&handle| (handle, ChangeKind::Modified))
            .collect();
        self.bump_entities_restyled(&changes);
        // A series of layer toggles can name thousands of handles, but most
        // are hidden and never enter the overlay's replacement index.
        // Keep using the base instead of rescanning all resident geometry.
        if let Some(delta) = self.geometry_deltas.get_mut().back_mut() {
            debug_assert_eq!(delta.epoch, self.geometry_epoch);
            delta.layer_visibility_only = true;
        }
        let mut tm = self.tess_memo.borrow_mut();
        let mut rm = self.resident_tess_memo.borrow_mut();
        for (handle, culled, resident) in kept {
            if let Some(wires) = culled {
                tm.insert(handle, wires);
            }
            if let Some(wires) = resident {
                rm.insert(handle, wires);
            }
        }
    }

    pub fn invalidate_layer_dependencies(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::Layer, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_linetype_dependencies(&mut self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let wanted: HashSet<String> = names.iter().map(|name| normalize_name(name)).collect();
        let wanted_handles: HashSet<Handle> = self
            .document
            .line_types
            .iter()
            .filter(|lt| wanted.contains(&normalize_name(&lt.name)))
            .map(|lt| lt.handle)
            .collect();
        let affected_layers: Vec<String> = self
            .document
            .layers
            .iter()
            .filter(|layer| {
                wanted.contains(&normalize_name(&layer.line_type))
                    || (!wanted_handles.is_empty()
                        && self
                            .document
                            .layer_viewport_overrides(
                                layer.handle,
                                document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
                            )
                            .into_iter()
                            .any(|(_, value)| {
                                value
                                    .as_handle()
                                    .is_some_and(|handle| wanted_handles.contains(&handle))
                            }))
            })
            .map(|layer| layer.name.clone())
            .collect();
        let mut targets = self.dependency_targets(DependencyKind::Linetype, names);
        if !affected_layers.is_empty() {
            let layer_targets = self.dependency_targets(DependencyKind::Layer, &affected_layers);
            targets.extend_from(&layer_targets);
        }
        self.invalidate_dependency_targets(targets);
        self.invalidate_dependency_index();
    }

    pub fn invalidate_text_style_dependencies(&mut self, name: &str) {
        self.invalidate_text_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_text_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::TextStyle, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_dim_style_dependencies(&mut self, name: &str) {
        self.invalidate_dim_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_dim_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::DimStyle, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_object_style_dependencies(&mut self, handles: &[Handle]) {
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
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
            *self.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        let targets = self
            .dependency_index_cache
            .borrow()
            .as_deref()
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
        // Objects (and so DIMASSOC associations) may have appeared or gone;
        // the direction that matters is undo/redo and plugin edits that
        // bypass `store_dimension_association_full`.
        self.has_dimension_associations.set(None);
        // Entity additions, erasures, in-place restores (undo/redo replays
        // entity images) and plugin edits can add, drop or re-point a centre
        // association without naming a line as modified.
        self.associative_centerline_source_cache.borrow_mut().take();
        self.associative_centermark_source_cache.borrow_mut().take();
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
                    // replay_since coalesces to one entry per handle, so each
                    // touched handle appears in exactly one op: sweep the
                    // unbounded list once for the whole batch instead of one
                    // retain per op (O(ops x handles) on point-heavy drawings).
                    let mut touched: HashSet<Handle> = HashSet::default();
                    let mut unbounded_pushes: Vec<Handle> = Vec::new();
                    for op in &ops {
                        match *op {
                            IdxOp::Remove(h) => {
                                idx.tree.remove(h);
                                touched.insert(h);
                            }
                            IdxOp::SetBounded(h, ab) => {
                                idx.tree.update(h, ab);
                                touched.insert(h);
                            }
                            IdxOp::SetUnbounded(h) => {
                                idx.tree.remove(h);
                                touched.insert(h);
                                unbounded_pushes.push(h);
                            }
                        }
                    }
                    idx.unbounded_handles.retain(|x| !touched.contains(x));
                    idx.unbounded_handles.extend(unbounded_pushes);
                    *epoch = self.geometry_epoch;
                    drop(cache);
                    return std::cell::Ref::map(self.entity_index_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }

        let perf = crate::perf::enabled();
        let t_aabbs_start = perf.then(iced::time::Instant::now);
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
        let aabbs_ms = crate::perf::elapsed_ms(t_aabbs_start);
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
        let t_tree_start = perf.then(iced::time::Instant::now);
        let mut tree = pick::quadtree::QuadTree::new(root);
        for (h, ab) in items {
            tree.insert(h, ab);
        }
        let tree_ms = crate::perf::elapsed_ms(t_tree_start);

        if perf {
            crate::perf_record!(
                "[perf] entity_index-rebuild aabbs={:.1}ms tree={:.1}ms entities={} root={:?}",
                aabbs_ms,
                tree_ms,
                tree.len(),
                root,
            );
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

    /// Resolve the active layout and annotation context for tessellation.
    ///
    /// This scans the drawing's objects for the annotation scale and for the
    /// current layout. A pass over many entities takes it one time, at its
    /// first entity, and gives it to [`Self::tessellate_one_in`].
    fn tessellation_context(&self) -> TessellationContext {
        #[cfg(test)]
        TESSELLATION_CONTEXTS.with(|count| count.set(count.get() + 1));
        let is_model = self.current_layout == "Model";
        let bg = if is_model {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if is_model {
            self.annotation_scale
        } else if let Some(viewport) = self.active_viewport {
            self.viewport_annotation_multiplier(viewport)
        } else {
            1.0
        };
        let annotation_scale_handle = if is_model {
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
        (bg, anno, annotation_scale_handle, blk_cache, !is_model)
    }

    fn tessellate_one(&self, e: &EntityType) -> Vec<WireModel> {
        self.tessellate_one_in(&std::cell::OnceCell::new(), e)
    }

    /// [`Self::tessellate_one`] for a pass over many entities: the first call
    /// resolves the context into `context` and the later calls use it. The
    /// scene cannot change during the pass, so the context stays correct. A
    /// pass that tessellates nothing resolves nothing, as before.
    fn tessellate_one_in(
        &self,
        context: &std::cell::OnceCell<TessellationContext>,
        e: &EntityType,
    ) -> Vec<WireModel> {
        let (bg, anno, annotation_scale_handle, blk_cache, paper) =
            context.get_or_init(|| self.tessellation_context());
        let (bg, anno, annotation_scale_handle, paper) =
            (*bg, *anno, *annotation_scale_handle, *paper);
        // tessellate_one is used for one-off lookups (hit test, properties).
        // Skip culling here so the caller always gets the full geometry.
        let mut wires = tessellate_entity(
            &self.document,
            &self.selected,
            self.active_viewport,
            bg,
            anno,
            annotation_scale_handle,
            e,
            Some(blk_cache),
            None,
            None,
            paper.then_some(1.0),
            self.reference_only.handles(),
        );
        // Same solid-corner snaps as the resident paths, so one-off consumers
        // (overlay changed-wires, edit previews) agree with hit-test/snap.
        self.attach_solid_snaps(e.common().handle, &mut wires);
        self.shade_constraint_wires(e.common().owner_handle, &mut wires);
        wires
    }

    /// `doc(hidden)` public only so `benches/model_space_handle_lookup.rs`
    /// can time the real production lookup.
    #[doc(hidden)]
    pub fn model_space_block_handle(&self) -> Handle {
        let key = (
            self.geometry_epoch,
            self.layout_epoch,
            self.document.objects.len(),
        );
        if let Some((geometry, layout, objects, handle)) = self.model_space_block_cache.get() {
            if (geometry, layout, objects) == key {
                return handle;
            }
        }
        let handle = self.resolve_model_space_block_handle();
        self.model_space_block_cache
            .set(Some((key.0, key.1, key.2, handle)));
        handle
    }

    fn resolve_model_space_block_handle(&self) -> Handle {
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
    /// when something is selected. `None` keeps the current camera target as
    /// the orbit centre. (#229)
    pub fn orbit_pivot(&self) -> Option<glam::DVec3> {
        self.navigation_selection_bounds()
            .map(|(min, max)| (min + max) * 0.5)
    }

    pub(crate) fn navigation_selection_bounds(&self) -> Option<(glam::DVec3, glam::DVec3)> {
        if self.selected.is_empty() {
            return None;
        }
        let model_view = self.current_layout == "Model" || self.active_viewport.is_some();
        let block = if self.active_viewport.is_some() {
            self.model_space_block_handle()
        } else {
            self.current_layout_block_handle()
        };
        let scale = if model_view {
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
            Some(&self.selected),
        );
        let mut min = glam::DVec3::splat(f64::INFINITY);
        let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
        let mut any = false;
        let mut include = |point: glam::DVec3| {
            if point.is_finite() {
                min = min.min(point);
                max = max.max(point);
                any = true;
            }
        };
        for wire in &wires {
            let Some(h) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            if !self.selected.contains(&h) {
                continue;
            }
            for (index, &[x, y, z]) in wire.points.iter().enumerate() {
                let [lx, ly, lz] = wire.points_low.get(index).unwrap_or([0.0; 3]);
                include(glam::DVec3::new(
                    x as f64 + lx as f64,
                    y as f64 + ly as f64,
                    z as f64 + lz as f64,
                ));
            }
        }
        for set in self.interaction_meshes_arc().iter() {
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            if !self.selected.contains(&handle) {
                continue;
            }
            if let Some(aabb) = mesh_interaction_aabb(set) {
                include(glam::DVec3::new(aabb[0], aabb[1], aabb[2]));
                include(glam::DVec3::new(aabb[3], aabb[4], aabb[5]));
            }
        }
        if any {
            Some((min, max))
        } else {
            None
        }
    }

    fn compute_model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let model_block = self.model_space_block_handle();
        // Shared with ZOOM EXTENTS so the two never disagree about what the
        // drawing covers. The previous walk here read only `key_vertices`,
        // which is empty for every tessellated curve (see `WireModel`) — so
        // circles, arcs, ellipses and splines contributed nothing, and
        // neither did hatches or images. Everything that reads drawing
        // extents (plot Extents, sheet-set framing, viewport auto-fit,
        // SpaceMouse model bounds) inherited that blind spot.
        if let Some((min, max)) = self.visible_block_bounds(model_block) {
            return Some((min.as_vec3(), max.as_vec3()));
        }
        // Last resort: saved EXTMIN/EXTMAX before the wire cache is built.
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
        self.fit_viewport_to_model(vp_handle, 1.1);
    }

    /// Fit a viewport's view to the model-space content, with `margin` (1 is
    /// tight) around it.
    pub fn fit_viewport_to_model(&mut self, vp_handle: Handle, margin: f64) {
        let extents = self.model_space_extents();
        let (min, max) = match extents {
            Some(e) => e,
            None => return,
        };
        let (min, max) = (min.as_dvec3(), max.as_dvec3());
        let center = (min + max) * 0.5;

        let vp = match self.document.get_entity_mut(vp_handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
            _ => return,
        };
        let direction = glam::DVec3::new(
            vp.view_direction.x,
            vp.view_direction.y,
            vp.view_direction.z,
        )
        .normalize_or(glam::DVec3::Z);
        // The extents box as seen along the view direction: its span across
        // the view's right and up axes. Looking down Z that is its X/Y size.
        let right = glam::DVec3::Z.cross(direction).normalize_or(glam::DVec3::X);
        let up = direction.cross(right);
        let (mut span_right, mut span_up) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
        for corner in 0..8 {
            let point = glam::DVec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let (along_right, along_up) = (point.dot(right), point.dot(up));
            span_right = (span_right.0.min(along_right), span_right.1.max(along_right));
            span_up = (span_up.0.min(along_up), span_up.1.max(along_up));
        }
        let content_w = (span_right.1 - span_right.0).max(1e-3);
        let content_h = (span_up.1 - span_up.0).max(1e-3);

        // Aim at the model's centre; a plan view keeps it on the XY plane.
        vp.view_target.x = center.x;
        vp.view_target.y = center.y;
        vp.view_target.z = if direction == glam::DVec3::Z {
            0.0
        } else {
            center.z
        };

        // Choose the scale that fits both dimensions with the margin.
        let scale_w = vp.width / (content_w * margin);
        let scale_h = vp.height / (content_h * margin);
        let fit_scale = scale_w.min(scale_h).min(1000.0).max(1e-6);

        vp.custom_scale = fit_scale;
        vp.view_height = vp.height / fit_scale;
    }
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
fn fit_sheet_viewport(vp: &mut document::entities::Viewport, min: (f64, f64), max: (f64, f64)) {
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
static PICK_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn pick_locked() -> bool {
    PICK_LOCKED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_pick_locked(on: bool) {
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
