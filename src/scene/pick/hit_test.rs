//! CPU-side hit-testing for wire geometry.
//!
//! All tests are performed in **screen space** — wire vertices are projected
//! to 2-D pixel coordinates, then compared against the cursor or selection box.
//! This matches the visual result the user sees.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use acadrust::Handle;
use glam::Mat4;
use iced::{Point, Rectangle};

use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::mesh_model::MeshModel;
use crate::scene::model::wire_model::WireModel;
use crate::scene::pick::interaction_index::WireSource;

/// Pick radius for one wire, in screen pixels.
///
/// A wire renders as a band `line_weight_px` wide, so testing every wire at the
/// configured base radius would leave the outer part of a heavy line
/// unselectable — the cursor would sit on solid ink and miss. Widening to the
/// rendered half-width keeps "looks like I'm on it" and "picks it" the same
/// thing at any zoom: both quantities are screen-space, so the relation holds
/// however far in the view is.
///
/// `lw_display` mirrors the wire shader's `select(0.5, half_width, ...)`
/// (`wire.wgsl`) — with lineweight display off the line collapses to 1 px, so
/// the pick band must collapse with it rather than stay secretly fat.
///
/// The standard weights all land under the threshold today (the widest, 2.11 mm,
/// renders 7.97 px half-width), so this only bites for out-of-range weights —
/// and it keeps the two sides from silently drifting apart if the display boost
/// in `view::render::lineweight_to_px` ever changes.
pub fn pick_tolerance_px(wire: &WireModel, lw_display: bool, base_radius_px: f32) -> f32 {
    let half_width = if lw_display {
        wire.line_weight_px * 0.5
    } else {
        0.5
    };
    base_radius_px.max(1.0).max(half_width)
}

/// Is `aabb` — a wire's world-space XY box — further than `tol` pixels from
/// `cursor` once projected, so the wire can be skipped without touching its
/// geometry?
///
/// Only sound in a flat (untilted) view, where a point's screen x/y depends on
/// its world x/y alone and the box therefore projects exactly. Callers must
/// check that themselves, and must skip the unbounded sentinel.
fn aabb_rejects(
    aabb: [f32; 4],
    cursor: Point,
    tol: f32,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> bool {
    let [minx, miny, maxx, maxy] = aabb;
    // Project all four corners — a plan view can be rotated about Z, so the
    // screen footprint isn't axis-aligned and the two diagonal corners alone
    // wouldn't bound it.
    let (mut sx0, mut sy0, mut sx1, mut sy1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (cx, cy) in [(minx, miny), (maxx, miny), (maxx, maxy), (minx, maxy)] {
        let s = world_to_screen(
            glam::DVec3::new(cx as f64, cy as f64, 0.0),
            view_rot,
            eye,
            bounds,
        );
        sx0 = sx0.min(s.x);
        sx1 = sx1.max(s.x);
        sy0 = sy0.min(s.y);
        sy1 = sy1.max(s.y);
    }
    cursor.x < sx0 - tol || cursor.x > sx1 + tol || cursor.y < sy0 - tol || cursor.y > sy1 + tol
}

/// Depth of the first triangle in `tris` whose screen projection contains
/// `cursor`, as the mean NDC z of its corners; `None` when none do.
///
/// `tris` is a flat vertex list, 3 per triangle, and `tris_low` its
/// double-single residual — empty meaning an all-zero low half, per the
/// [`WireModel`] contract.
fn tris_hit_depth(
    cursor: Point,
    tris: &[[f32; 3]],
    tris_low: &[[f32; 3]],
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<f32> {
    let mut t = 0;
    while t + 2 < tris.len() {
        let mut sp = [Point::ORIGIN; 3];
        let mut depth = 0.0f32;
        for j in 0..3 {
            let k = t + j;
            let hi = tris[k];
            let lo = tris_low.get(k).copied().unwrap_or([0.0; 3]);
            let world = glam::DVec3::new(
                hi[0] as f64 + lo[0] as f64,
                hi[1] as f64 + lo[1] as f64,
                hi[2] as f64 + lo[2] as f64,
            );
            let ndc = view_rot.project_point3((world - eye).as_vec3());
            sp[j] = Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            );
            depth += ndc.z;
        }
        t += 3;
        if point_in_polygon(cursor, &sp) {
            return Some(depth / 3.0);
        }
    }
    None
}

fn triangle_ref_hit_depth(
    cursor: Point,
    wire: &WireModel,
    start: usize,
    pick_only: bool,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<f32> {
    let (points, low) = if pick_only {
        (&wire.pick_tris, &wire.pick_tris_low)
    } else {
        (&wire.fill_tris, &wire.fill_tris_low)
    };
    let points = points.get(start..start + 3)?;
    let mut tri = [[0.0; 3]; 3];
    let mut tri_low = [[0.0; 3]; 3];
    tri.copy_from_slice(points);
    for (dst, index) in tri_low.iter_mut().zip(start..start + 3) {
        *dst = low.get(index).copied().unwrap_or([0.0; 3]);
    }
    tris_hit_depth(cursor, &tri, &tri_low, view_rot, eye, bounds)
}

/// Screen-space area of the smallest SDF glyph quad containing `cursor`.
///
/// Block expansion batches same-style text runs into one [`WireModel`], so the
/// wire AABB is the union of every run and may span large empty gaps (notably
/// for XREFs). Glyph vertices retain the exact six-vertex quad boundaries after
/// batching, letting picking stay tight without splitting the GPU batch.
fn text_quad_hit_area(
    cursor: Point,
    verts: &[crate::scene::pipeline::text_gpu::TextVertex],
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<f32> {
    let mut best = f32::MAX;
    for quad in verts.chunks_exact(6) {
        // push_glyph_vertices emits BL, BR, TR, BL, TR, TL.
        let mut screen = [Point::ORIGIN; 4];
        for (dst, src) in screen.iter_mut().zip([0usize, 1, 2, 5]) {
            let v = quad[src];
            let world = glam::DVec3::new(
                v.pos[0] as f64 + v.pos_low[0] as f64,
                v.pos[1] as f64 + v.pos_low[1] as f64,
                v.pos[2] as f64 + v.pos_low[2] as f64,
            );
            *dst = world_to_screen(world, view_rot, eye, bounds);
        }
        if point_in_polygon(cursor, &screen) {
            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in screen {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
            best = best.min((max_x - min_x) * (max_y - min_y));
        }
    }
    (best < f32::MAX).then_some(best)
}

// ── Single-click hit test ─────────────────────────────────────────────────

/// Return the `name` of the closest wire whose screen-space segments pass
/// within that wire's [`pick_tolerance_px`] of `cursor`.
///
/// Returns `None` when no wire is close enough.
pub fn click_hit<'a, W: WireSource + ?Sized>(
    cursor: Point,
    wires: &'a W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    lw_display: bool,
    base_radius_px: f32,
) -> Option<&'a str> {
    // A click outside the pane rectangle (e.g. on the paper around a floating
    // viewport) must not reach geometry scissored out of the viewport.
    if cursor.x < 0.0 || cursor.x > bounds.width || cursor.y < 0.0 || cursor.y > bounds.height {
        return None;
    }
    // Each wire brings its own threshold now (a heavy line catches over its full
    // rendered width), so the running best can't double as the cut-off.
    let mut best_dist = f32::MAX;
    let mut best: Option<&str> = None;

    for wire in wires.iter().filter(|wire| wire.point_marker.is_some()) {
        let tolerance = pick_tolerance_px(wire, lw_display, base_radius_px);
        let mut previous = None;
        for (index, point) in wire.points.iter().enumerate() {
            if !point[0].is_finite() {
                previous = None;
                continue;
            }
            let screen = world_to_screen(
                wire_point_world(wire, index, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            if let Some(start) = previous {
                let distance = dist_point_to_segment(cursor, start, screen);
                if distance < tolerance && distance < best_dist {
                    best_dist = distance;
                    best = Some(wire.name.as_str());
                }
            }
            previous = Some(screen);
        }
    }

    // World z only shifts the *screen* x/y when the view is tilted (orbit /
    // perspective). In the flat top-down ortho view — the case where hover lag
    // on large drawings actually bites — a wire's screen position depends only
    // on its world x/y, so its world-space AABB projects exactly and we can
    // reject wires nowhere near the cursor without projecting any of their
    // points (the dominant per-move cost on 100 k-wire drawings).
    let z_flat = view_rot.z_axis.x.abs() < 1e-9 && view_rot.z_axis.y.abs() < 1e-9;

    if let Some(segments) = wires.segments() {
        // The interaction index already narrowed long/batched wires to segments
        // touching the cursor aperture. Hover and click therefore project only
        // those local edges, not every point of every candidate wire.
        for segment in segments {
            let Some(wire) = wires.source_wire(segment.wire) else {
                continue;
            };
            if wire.point_marker.is_some() {
                continue;
            }
            let start = segment.start as usize;
            if start + 1 >= wire.points.len() {
                continue;
            }
            let p0 = world_to_screen(
                wire_point_world(wire, start, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let p1 = world_to_screen(
                wire_point_world(wire, start + 1, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let d = dist_point_to_segment(cursor, p0, p1);
            if d < pick_tolerance_px(wire, lw_display, base_radius_px) && d < best_dist {
                best_dist = d;
                best = Some(&wire.name);
            }
        }
    } else {
        // Q: lazy projection — no Vec allocation per wire; NaN resets the segment chain.
        for wire in wires.iter() {
            if wire.point_marker.is_some() {
                continue;
            }
            let tol = pick_tolerance_px(wire, lw_display, base_radius_px);
            // Cheap AABB pre-reject (flat view only; never for the unbounded
            // sentinel used by previews / greeked text).
            if z_flat
                && wire.point_marker.is_none()
                && wire.aabb != WireModel::UNBOUNDED_AABB
                && aabb_rejects(wire.aabb, cursor, tol, view_rot, eye, bounds)
            {
                continue;
            }
            let mut prev: Option<Point> = None;
            for (i, &[px, _, _]) in wire.points.iter().enumerate() {
                if px.is_nan() {
                    prev = None;
                    continue;
                }
                let cur = world_to_screen(
                    wire_point_world(wire, i, view_rot, eye),
                    view_rot,
                    eye,
                    bounds,
                );
                if let Some(p0) = prev {
                    let d = dist_point_to_segment(cursor, p0, cur);
                    if d < tol && d < best_dist {
                        best_dist = d;
                        best = Some(&wire.name);
                    }
                }
                prev = Some(cur);
            }
        }
    }

    if best.is_some() {
        return best;
    }

    // No edge close enough. Mesh entities (PolyfaceMesh / PolygonMesh / SubD
    // Mesh) carry their shaded faces as `fill_tris`; test those so a mesh is
    // selectable by clicking its surface — not only its thin edges — the way a
    // 3D solid is. Same projected-triangle containment as `mesh_click_hit`,
    // front-most wins.
    let mut best_fill: Option<(f32, &str)> = None;
    if let Some(triangles) = wires.fill_triangles() {
        for triangle in triangles {
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if let Some(depth) = triangle_ref_hit_depth(
                cursor,
                wire,
                triangle.start as usize,
                false,
                view_rot,
                eye,
                bounds,
            ) {
                if best_fill.is_none_or(|(best, _)| depth < best) {
                    best_fill = Some((depth, wire.name.as_str()));
                }
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.fill_tris.is_empty() {
                continue;
            }
            if let Some(depth) = tris_hit_depth(
                cursor,
                &wire.fill_tris,
                &wire.fill_tris_low,
                view_rot,
                eye,
                bounds,
            ) {
                if best_fill.is_none_or(|(best, _)| depth < best) {
                    best_fill = Some((depth, wire.name.as_str()));
                }
            }
        }
    }
    if let Some((_, n)) = best_fill {
        return Some(n);
    }

    // No fill of this wire's own either. `pick_tris` closes the surfaces that
    // `points` only bounds: a thickness wall (drawn as four edges with nothing
    // between them) and a wide polyline's band (drawn, but by the hatch
    // pipeline, so no fill hangs off this wire). Without them the cursor falls
    // through what plainly reads as solid. Front-most wins.
    //
    // Ranked below `fill_tris` because that geometry is this wire's own drawn
    // surface — where the two overlap, the nearer thing to the eye is decided
    // by depth, but a wire that has a real fill should win on it first.
    let mut best_wall: Option<(f32, &str)> = None;
    if let Some(triangles) = wires.pick_triangles() {
        for triangle in triangles {
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if let Some(depth) = triangle_ref_hit_depth(
                cursor,
                wire,
                triangle.start as usize,
                true,
                view_rot,
                eye,
                bounds,
            ) {
                if best_wall.is_none_or(|(best, _)| depth < best) {
                    best_wall = Some((depth, wire.name.as_str()));
                }
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.pick_tris.is_empty() {
                continue;
            }
            // This runs on every hover that misses everything else — the common case
            // over empty space — and a wall is two triangles per base segment, so an
            // extruded circle alone is ~128 of them. Reject on the box first.
            if z_flat
                && wire.aabb != WireModel::UNBOUNDED_AABB
                && aabb_rejects(wire.aabb, cursor, 0.0, view_rot, eye, bounds)
            {
                continue;
            }
            if let Some(depth) = tris_hit_depth(
                cursor,
                &wire.pick_tris,
                &wire.pick_tris_low,
                view_rot,
                eye,
                bounds,
            ) {
                if best_wall.is_none_or(|(best, _)| depth < best) {
                    best_wall = Some((depth, wire.name.as_str()));
                }
            }
        }
    }
    if let Some((_, n)) = best_wall {
        return Some(n);
    }

    // SDF text renders as glyph quads, not strokes. Test those exact quads:
    // block expansion may batch distant same-style text runs into one wire,
    // whose union AABB includes empty space between them (#438). Lowest
    // priority — real edges and fills above always win.
    let mut best_area = f32::MAX;
    let mut best_box: Option<&str> = None;
    if let Some(glyphs) = wires.glyphs() {
        for glyph in glyphs {
            let Some(wire) = wires.source_wire(glyph.wire) else {
                continue;
            };
            let start = glyph.start as usize;
            let Some(vertices) = wire.text_verts.get(start..start + 6) else {
                continue;
            };
            if let Some(area) = text_quad_hit_area(cursor, vertices, view_rot, eye, bounds) {
                if area < best_area {
                    best_area = area;
                    best_box = Some(wire.name.as_str());
                }
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.text_verts.is_empty() {
                continue;
            }
            if let Some(area) = text_quad_hit_area(cursor, &wire.text_verts, view_rot, eye, bounds)
            {
                if area < best_area {
                    best_area = area;
                    best_box = Some(wire.name.as_str());
                }
            }
        }
    }
    best_box
}

/// Like `click_hit` but returns every wire within the click threshold,
/// nearest first. Used by selection cycling to step through overlapping
/// objects under the cursor.
pub fn click_hits_all<'a, W: WireSource + ?Sized>(
    cursor: Point,
    wires: &'a W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    lw_display: bool,
    base_radius_px: f32,
) -> Vec<&'a str> {
    if cursor.x < 0.0 || cursor.x > bounds.width || cursor.y < 0.0 || cursor.y > bounds.height {
        return Vec::new();
    }
    let mut hits: Vec<(f32, &str)> = Vec::new();
    let mut marker_hits: HashMap<&str, f32> = HashMap::default();
    for wire in wires.iter().filter(|wire| wire.point_marker.is_some()) {
        let tolerance = pick_tolerance_px(wire, lw_display, base_radius_px);
        let mut previous = None;
        for (index, point) in wire.points.iter().enumerate() {
            if !point[0].is_finite() {
                previous = None;
                continue;
            }
            let screen = world_to_screen(
                wire_point_world(wire, index, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            if let Some(start) = previous {
                let distance = dist_point_to_segment(cursor, start, screen);
                if distance < tolerance {
                    marker_hits
                        .entry(wire.name.as_str())
                        .and_modify(|best| *best = best.min(distance))
                        .or_insert(distance);
                }
            }
            previous = Some(screen);
        }
    }
    hits.extend(marker_hits.into_iter().map(|(name, distance)| (distance, name)));
    if let Some(segments) = wires.segments() {
        let mut best_by_wire: HashMap<u32, f32> = HashMap::default();
        for segment in segments {
            let Some(wire) = wires.source_wire(segment.wire) else {
                continue;
            };
            if wire.point_marker.is_some() {
                continue;
            }
            let start = segment.start as usize;
            if start + 1 >= wire.points.len() {
                continue;
            }
            let p0 = world_to_screen(
                wire_point_world(wire, start, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let p1 = world_to_screen(
                wire_point_world(wire, start + 1, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let d = dist_point_to_segment(cursor, p0, p1);
            if d < pick_tolerance_px(wire, lw_display, base_radius_px) {
                best_by_wire
                    .entry(segment.wire)
                    .and_modify(|best| *best = best.min(d))
                    .or_insert(d);
            }
        }
        hits.extend(best_by_wire.into_iter().filter_map(|(wire_idx, distance)| {
            wires
                .source_wire(wire_idx)
                .map(|wire| (distance, wire.name.as_str()))
        }));
    } else {
        for wire in wires.iter() {
            if wire.point_marker.is_some() {
                continue;
            }
            let tol = pick_tolerance_px(wire, lw_display, base_radius_px);
            let mut prev: Option<Point> = None;
            let mut best_for_wire = tol;
            let mut hit = false;
            for (i, &[px, _, _]) in wire.points.iter().enumerate() {
                if px.is_nan() {
                    prev = None;
                    continue;
                }
                let cur = world_to_screen(
                    wire_point_world(wire, i, view_rot, eye),
                    view_rot,
                    eye,
                    bounds,
                );
                if let Some(p0) = prev {
                    let d = dist_point_to_segment(cursor, p0, cur);
                    if d < best_for_wire {
                        best_for_wire = d;
                        hit = true;
                    }
                }
                prev = Some(cur);
            }
            if hit {
                hits.push((best_for_wire, &wire.name));
            }
        }
    }
    // Filled mesh faces join the cycle too, matching `click_hit`. Ranked at
    // threshold distance because face depth is not comparable to edge distance.
    if let Some(triangles) = wires.fill_triangles() {
        let mut matched: HashSet<u32> = HashSet::default();
        for triangle in triangles {
            if matched.contains(&triangle.wire) {
                continue;
            }
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if hits.iter().any(|&(_, name)| name == wire.name) {
                matched.insert(triangle.wire);
                continue;
            }
            if triangle_ref_hit_depth(
                cursor,
                wire,
                triangle.start as usize,
                false,
                view_rot,
                eye,
                bounds,
            )
            .is_some()
            {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
                matched.insert(triangle.wire);
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.fill_tris.is_empty() || hits.iter().any(|&(_, name)| name == wire.name) {
                continue;
            }
            if tris_hit_depth(
                cursor,
                &wire.fill_tris,
                &wire.fill_tris_low,
                view_rot,
                eye,
                bounds,
            )
            .is_some()
            {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
            }
        }
    }

    // Thickness walls join the cycle so an extruded entity picked on its wall
    // can be stepped past to whatever sits behind it. Ranked at the threshold,
    // below every proximity hit — same convention the text boxes below use.
    //
    // A wall's own edges live on the same wire, so skip any wire the loop above
    // already caught: cycling must not offer one entity twice.
    if let Some(triangles) = wires.pick_triangles() {
        let mut matched: HashSet<u32> = HashSet::default();
        for triangle in triangles {
            if matched.contains(&triangle.wire) {
                continue;
            }
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if hits.iter().any(|&(_, name)| name == wire.name) {
                matched.insert(triangle.wire);
                continue;
            }
            if triangle_ref_hit_depth(
                cursor,
                wire,
                triangle.start as usize,
                true,
                view_rot,
                eye,
                bounds,
            )
            .is_some()
            {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
                matched.insert(triangle.wire);
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.pick_tris.is_empty() || hits.iter().any(|&(_, name)| name == wire.name) {
                continue;
            }
            if tris_hit_depth(
                cursor,
                &wire.pick_tris,
                &wire.pick_tris_low,
                view_rot,
                eye,
                bounds,
            )
            .is_some()
            {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
            }
        }
    }
    // SDF text: use the same exact glyph-quad test as `click_hit`; a batched
    // text wire's union AABB may cover empty space between distant runs (#438).
    // Ranked after real geometry (distance = the click threshold).
    if let Some(glyphs) = wires.glyphs() {
        let mut matched: HashSet<u32> = HashSet::default();
        for glyph in glyphs {
            if matched.contains(&glyph.wire) {
                continue;
            }
            let Some(wire) = wires.source_wire(glyph.wire) else {
                continue;
            };
            if hits.iter().any(|&(_, name)| name == wire.name) {
                matched.insert(glyph.wire);
                continue;
            }
            let start = glyph.start as usize;
            let Some(vertices) = wire.text_verts.get(start..start + 6) else {
                continue;
            };
            if text_quad_hit_area(cursor, vertices, view_rot, eye, bounds).is_some() {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
                matched.insert(glyph.wire);
            }
        }
    } else {
        for wire in wires.iter() {
            if wire.text_verts.is_empty() || hits.iter().any(|&(_, name)| name == wire.name) {
                continue;
            }
            if text_quad_hit_area(cursor, &wire.text_verts, view_rot, eye, bounds).is_some() {
                hits.push((base_radius_px.max(1.0), wire.name.as_str()));
            }
        }
    }
    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.into_iter().map(|(_, name)| name).collect()
}

type MeshPickItem<'a> = (
    Handle,
    &'a MeshModel,
    Option<acadrust::types::Transform>,
    [f64; 6],
);

pub(crate) fn mesh_click_hit<'a>(
    cursor: Point,
    meshes: impl Iterator<Item = MeshPickItem<'a>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<Handle> {
    mesh_click_result(cursor, meshes, view_rot, eye, bounds).map(|(_, handle, _)| handle)
}

pub(crate) fn mesh_click_point<'a>(
    cursor: Point,
    meshes: impl Iterator<Item = MeshPickItem<'a>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<glam::DVec3> {
    mesh_click_result(cursor, meshes, view_rot, eye, bounds).map(|(_, _, point)| point)
}

fn mesh_click_result<'a>(
    cursor: Point,
    meshes: impl Iterator<Item = MeshPickItem<'a>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<(f64, Handle, glam::DVec3)> {
    let profile = crate::perf::enabled().then(iced::time::Instant::now);
    let mut set_count = 0usize;
    let mut bound_hits = 0usize;
    let mut exact_triangles = 0usize;
    let ndc = glam::Vec3::new(
        cursor.x / bounds.width * 2.0 - 1.0,
        1.0 - cursor.y / bounds.height * 2.0,
        0.0,
    );
    let inverse_view = view_rot.inverse();
    let near = eye + inverse_view.project_point3(ndc).as_dvec3();
    let far = eye
        + inverse_view
            .project_point3(glam::Vec3::new(ndc.x, ndc.y, 1.0))
            .as_dvec3();
    let world_direction = (far - near).normalize_or_zero();
    if !near.is_finite() || !world_direction.is_finite() || world_direction.length_squared() < 1e-18
    {
        return None;
    }
    let mut best: Option<(f64, Handle, glam::DVec3)> = None;
    for (handle, mesh, transform, aabb) in meshes {
        set_count += 1;
        let Some((near_t, _)) = ray_aabb(near, world_direction, aabb) else {
            continue;
        };
        bound_hits += 1;
        exact_triangles += mesh.indices.len() / 3;
        if best.is_some_and(|(distance, _, _)| near_t > distance) {
            continue;
        }
        let model = transform.map(codec_transform_matrix);
        let (origin, direction) = if let Some(model) = model {
            if !model.is_finite() || model.determinant().abs() <= 1e-18 {
                continue;
            }
            let inverse = model.inverse();
            let origin = inverse.transform_point3(near);
            let direction = inverse.transform_vector3(world_direction).normalize_or_zero();
            (origin, direction)
        } else {
            (near, world_direction)
        };
        if direction.length_squared() < 1e-18 {
            continue;
        }
        let local_t = mesh
            .indices
            .chunks_exact(3)
            .filter_map(|triangle| {
                ray_triangle(
                    origin,
                    direction,
                    mesh_vert(
                        mesh.verts[triangle[0] as usize],
                        &mesh.verts_low,
                        triangle[0] as usize,
                    ),
                    mesh_vert(
                        mesh.verts[triangle[1] as usize],
                        &mesh.verts_low,
                        triangle[1] as usize,
                    ),
                    mesh_vert(
                        mesh.verts[triangle[2] as usize],
                        &mesh.verts_low,
                        triangle[2] as usize,
                    ),
                )
            })
            .min_by(f64::total_cmp);
        if let Some(local_t) = local_t {
            let local_hit = origin + direction * local_t;
            let world_hit = model.map_or(local_hit, |model| model.transform_point3(local_hit));
            let distance = (world_hit - near).dot(world_direction);
            if distance >= 0.0 && best.is_none_or(|(current, _, _)| distance < current) {
                best = Some((distance, handle, world_hit));
            }
        }
    }
    if let Some(started) = profile {
        crate::perf_record!(
            "[perf] mesh-ray {:>7.1}ms sets={} bounds={} source_triangles={}",
            started.elapsed().as_secs_f64() * 1000.0,
            set_count,
            bound_hits,
            exact_triangles,
        );
    }
    best
}

fn codec_transform_matrix(transform: acadrust::types::Transform) -> glam::DMat4 {
    let matrix = transform.matrix.m;
    glam::DMat4::from_cols_array(&[
        matrix[0][0], matrix[1][0], matrix[2][0], matrix[3][0],
        matrix[0][1], matrix[1][1], matrix[2][1], matrix[3][1],
        matrix[0][2], matrix[1][2], matrix[2][2], matrix[3][2],
        matrix[0][3], matrix[1][3], matrix[2][3], matrix[3][3],
    ])
}

fn ray_aabb(origin: glam::DVec3, direction: glam::DVec3, aabb: [f64; 6]) -> Option<(f64, f64)> {
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        let origin = origin[axis];
        let direction = direction[axis];
        if direction.abs() <= 1e-18 {
            if origin < aabb[axis] || origin > aabb[axis + 3] {
                return None;
            }
            continue;
        }
        let first = (aabb[axis] - origin) / direction;
        let second = (aabb[axis + 3] - origin) / direction;
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if far < near {
            return None;
        }
    }
    Some((near, far))
}

fn ray_triangle(
    origin: glam::DVec3,
    direction: glam::DVec3,
    a: glam::DVec3,
    b: glam::DVec3,
    c: glam::DVec3,
) -> Option<f64> {
    let edge1 = b - a;
    let edge2 = c - a;
    let cross = direction.cross(edge2);
    let determinant = edge1.dot(cross);
    if determinant.abs() <= 1e-12 {
        return None;
    }
    let inverse = determinant.recip();
    let offset = origin - a;
    let u = offset.dot(cross) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = offset.cross(edge1);
    let v = direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge2.dot(q) * inverse;
    (distance >= 0.0).then_some(distance)
}

/// Reconstruct a mesh vertex's absolute f64 position from its high/low pair —
/// without the low residual the f32 high alone is ~0.5 m off at UTM scale and
/// box / lasso / face selection lands on the wrong place.
#[inline]
fn mesh_vert(
    hi: [f32; 3],
    low: &[[f32; 3]],
    i: usize,
) -> glam::DVec3 {
    let l = low.get(i).copied().unwrap_or([0.0; 3]);
    glam::DVec3::new(
        hi[0] as f64 + l[0] as f64,
        hi[1] as f64 + l[1] as f64,
        hi[2] as f64 + l[2] as f64,
    )
}

/// Project a mesh's vertices to screen space.
fn project_mesh_verts(
    mesh: &MeshModel,
    transform: Option<acadrust::types::Transform>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<Point> {
    mesh.verts
        .iter()
        .enumerate()
        .map(|(i, &w)| {
            let point = mesh_vert(w, &mesh.verts_low, i);
            let point = transform.map_or(point, |transform| {
                let point = transform.apply(acadrust::types::Vector3::new(point.x, point.y, point.z));
                glam::DVec3::new(point.x, point.y, point.z)
            });
            let ndc = view_rot.project_point3((point - eye).as_vec3());
            Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            )
        })
        .collect()
}

/// True when any of `mesh`'s projected triangles contains one of `pts`
/// (used so a crossing box / lasso entirely inside a solid still selects it).
fn mesh_covers_any(proj: &[Point], indices: &[u32], pts: &[Point]) -> bool {
    let mut t = 0;
    while t + 2 < indices.len() {
        let tri = [
            proj[indices[t] as usize],
            proj[indices[t + 1] as usize],
            proj[indices[t + 2] as usize],
        ];
        t += 3;
        if pts.iter().any(|p| point_in_polygon(*p, &tri)) {
            return true;
        }
    }
    false
}

/// Solid (mesh) handles caught by a rectangular selection box. Window mode
/// (`crossing == false`) needs every projected vertex inside the box;
/// crossing mode needs any vertex inside, or the box to sit inside the solid.
pub fn mesh_box_hit<'a>(
    a: Point,
    b: Point,
    crossing: bool,
    meshes: impl Iterator<
        Item = (
            Handle,
            &'a MeshModel,
            Option<acadrust::types::Transform>,
        ),
    >,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<Handle> {
    let (min_x, max_x) = (a.x.min(b.x), a.x.max(b.x));
    let (min_y, max_y) = (a.y.min(b.y), a.y.max(b.y));
    let in_box = |p: &Point| p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y;
    let corners = [
        Point::new(min_x, min_y),
        Point::new(max_x, min_y),
        Point::new(max_x, max_y),
        Point::new(min_x, max_y),
    ];
    let mut out = Vec::new();
    for (h, mesh, transform) in meshes {
        let proj = project_mesh_verts(mesh, transform, view_rot, eye, bounds);
        if proj.is_empty() {
            continue;
        }
        let hit = if crossing {
            proj.iter().any(in_box) || mesh_covers_any(&proj, &mesh.indices, &corners)
        } else {
            proj.iter().all(in_box)
        };
        if hit {
            out.push(h);
        }
    }
    out
}

/// Solid (mesh) handles caught by a lasso polygon. Window mode needs every
/// projected vertex inside the lasso; crossing mode needs any vertex inside,
/// or the lasso to sit inside the solid.
pub fn mesh_poly_hit<'a>(
    poly: &[Point],
    crossing: bool,
    meshes: impl Iterator<
        Item = (
            Handle,
            &'a MeshModel,
            Option<acadrust::types::Transform>,
        ),
    >,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<Handle> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (h, mesh, transform) in meshes {
        let proj = project_mesh_verts(mesh, transform, view_rot, eye, bounds);
        if proj.is_empty() {
            continue;
        }
        let hit = if crossing {
            proj.iter().any(|p| point_in_polygon(*p, poly))
                || mesh_covers_any(&proj, &mesh.indices, poly)
        } else {
            proj.iter().all(|p| point_in_polygon(*p, poly))
        };
        if hit {
            out.push(h);
        }
    }
    out
}

// ── Box / window selection ────────────────────────────────────────────────

fn projected_wire_triangle(
    wire: &WireModel,
    start: usize,
    pick_only: bool,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<[Point; 3]> {
    let (points, low) = if pick_only {
        (&wire.pick_tris, &wire.pick_tris_low)
    } else {
        (&wire.fill_tris, &wire.fill_tris_low)
    };
    let points = points.get(start..start + 3)?;
    Some(std::array::from_fn(|offset| {
        world_to_screen(
            wp64(points[offset], low, start + offset),
            view_rot,
            eye,
            bounds,
        )
    }))
}

fn projected_text_quad(
    wire: &WireModel,
    start: usize,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Option<[Point; 6]> {
    let vertices = wire.text_verts.get(start..start + 6)?;
    Some(std::array::from_fn(|offset| {
        let vertex = vertices[offset];
        world_to_screen(
            glam::DVec3::new(
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
            ),
            view_rot,
            eye,
            bounds,
        )
    }))
}

fn triangle_crosses_box(triangle: [Point; 3], corners: [Point; 4]) -> bool {
    let min_x = corners[0].x;
    let min_y = corners[0].y;
    let max_x = corners[2].x;
    let max_y = corners[2].y;
    let inside_box =
        |point: Point| point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y;
    triangle.iter().copied().any(inside_box)
        || corners
            .iter()
            .copied()
            .any(|corner| point_in_polygon(corner, &triangle))
        || (0..3).any(|tri_edge| {
            let a = triangle[tri_edge];
            let b = triangle[(tri_edge + 1) % 3];
            (0..4).any(|box_edge| {
                segments_intersect(a, b, corners[box_edge], corners[(box_edge + 1) % 4])
            })
        })
}

fn triangle_crosses_polygon(triangle: [Point; 3], poly: &[Point]) -> bool {
    triangle
        .iter()
        .copied()
        .any(|point| point_in_polygon(point, poly))
        || poly
            .iter()
            .copied()
            .any(|point| point_in_polygon(point, &triangle))
        || (0..3)
            .any(|edge| segment_crosses_polygon(triangle[edge], triangle[(edge + 1) % 3], poly))
}

fn indexed_box_crossing_hits<'a, W: WireSource + ?Sized>(
    wires: &'a W,
    corners: [Point; 4],
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::default();
    let inside = |point: Point| {
        point.x >= corners[0].x
            && point.x <= corners[2].x
            && point.y >= corners[0].y
            && point.y <= corners[2].y
    };
    let segment_hits = |a: Point, b: Point| {
        inside(a)
            || inside(b)
            || (0..4).any(|edge| segments_intersect(a, b, corners[edge], corners[(edge + 1) % 4]))
    };

    for segment in wires.segments().unwrap_or_default() {
        let Some(wire) = wires.source_wire(segment.wire) else {
            continue;
        };
        let start = segment.start as usize;
        if start + 1 >= wire.points.len() {
            continue;
        }
        let a = world_to_screen(
            wire_point_world(wire, start, view_rot, eye),
            view_rot,
            eye,
            bounds,
        );
        let b = world_to_screen(
            wire_point_world(wire, start + 1, view_rot, eye),
            view_rot,
            eye,
            bounds,
        );
        if segment_hits(a, b) && seen.insert(wire.name.as_str()) {
            out.push(wire.name.as_str());
        }
    }
    for wire in wires.iter().filter(|wire| wire.point_marker.is_some()) {
        if !seen.contains(wire.name.as_str())
            && marker_segment_hit(wire, view_rot, eye, bounds, |a, b| segment_hits(a, b))
            && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    for (triangles, pick_only) in [
        (wires.fill_triangles().unwrap_or_default(), false),
        (wires.pick_triangles().unwrap_or_default(), true),
    ] {
        for triangle in triangles {
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if seen.contains(wire.name.as_str()) {
                continue;
            }
            if projected_wire_triangle(
                wire,
                triangle.start as usize,
                pick_only,
                view_rot,
                eye,
                bounds,
            )
            .is_some_and(|triangle| triangle_crosses_box(triangle, corners))
                && seen.insert(wire.name.as_str())
            {
                out.push(wire.name.as_str());
            }
        }
    }
    for glyph in wires.glyphs().unwrap_or_default() {
        let Some(wire) = wires.source_wire(glyph.wire) else {
            continue;
        };
        if seen.contains(wire.name.as_str()) {
            continue;
        }
        let Some(screen) =
            projected_text_quad(wire, glyph.start as usize, view_rot, eye, bounds)
        else {
            continue;
        };
        if [0usize, 3].into_iter().any(|offset| {
            triangle_crosses_box(
                [screen[offset], screen[offset + 1], screen[offset + 2]],
                corners,
            )
        }) && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }

    // Degenerate point-only wires have no indexed segment or surface primitive.
    for wire in wires.iter() {
        if seen.contains(wire.name.as_str())
            || wire.points.len() >= 2
            || !wire.fill_tris.is_empty()
            || !wire.pick_tris.is_empty()
            || !wire.text_verts.is_empty()
        {
            continue;
        }
        if wire.points.iter().enumerate().any(|(index, &point)| {
            point[0].is_finite()
                && inside(world_to_screen(
                    wire_point_world(wire, index, view_rot, eye),
                    view_rot,
                    eye,
                    bounds,
                ))
        }) && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    out
}

/// Wires the open polyline `fence` actually crosses.
///
/// The Fence selection mode draws a line through a drawing and takes whatever
/// it cuts. That is not the polygon test with `crossing` set: the polygon one
/// closes the point list back to its start and counts anything lying inside the
/// area that closure encloses, so a fence drawn past a group of objects would
/// sweep up everything behind it as well. Here the chain stays open and only a
/// real intersection counts. (#596)
pub fn poly_fence_hit<'a, W: WireSource + ?Sized>(
    fence: &[Point],
    wires: &'a W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<&'a str> {
    if fence.len() < 2 {
        return vec![];
    }
    let cuts = |a: Point, b: Point| {
        fence
            .windows(2)
            .any(|leg| segments_intersect(a, b, leg[0], leg[1]))
    };
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::default();
    // Indexed segments when the source has them (the same fast path the
    // polygon test uses), otherwise walk each wire's own points.
    if let Some(segments) = wires.segments() {
        for segment in segments {
            let Some(wire) = wires.source_wire(segment.wire) else {
                continue;
            };
            let start = segment.start as usize;
            if start + 1 >= wire.points.len() || seen.contains(wire.name.as_str()) {
                continue;
            }
            let a = world_to_screen(
                wire_point_world(wire, start, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let b = world_to_screen(
                wire_point_world(wire, start + 1, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            if cuts(a, b) && seen.insert(wire.name.as_str()) {
                out.push(wire.name.as_str());
            }
        }
        for wire in wires.iter().filter(|wire| wire.point_marker.is_some()) {
            if !seen.contains(wire.name.as_str())
                && marker_segment_hit(wire, view_rot, eye, bounds, |a, b| cuts(a, b))
                && seen.insert(wire.name.as_str())
            {
                out.push(wire.name.as_str());
            }
        }
        return out;
    }
    for wire in wires.iter() {
        if wire.points.len() < 2 {
            continue;
        }
        let hit = (0..wire.points.len() - 1).any(|k| {
            let a = world_to_screen(
                wire_point_world(wire, k, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            let b = world_to_screen(
                wire_point_world(wire, k + 1, view_rot, eye),
                view_rot,
                eye,
                bounds,
            );
            cuts(a, b)
        });
        if hit && seen.insert(wire.name.as_str()) {
            out.push(wire.name.as_str());
        }
    }
    out
}

fn indexed_polygon_crossing_hits<'a, W: WireSource + ?Sized>(
    wires: &'a W,
    poly: &[Point],
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::default();
    for segment in wires.segments().unwrap_or_default() {
        let Some(wire) = wires.source_wire(segment.wire) else {
            continue;
        };
        let start = segment.start as usize;
        if start + 1 >= wire.points.len() {
            continue;
        }
        let a = world_to_screen(
            wire_point_world(wire, start, view_rot, eye),
            view_rot,
            eye,
            bounds,
        );
        let b = world_to_screen(
            wire_point_world(wire, start + 1, view_rot, eye),
            view_rot,
            eye,
            bounds,
        );
        if (point_in_polygon(a, poly)
            || point_in_polygon(b, poly)
            || segment_crosses_polygon(a, b, poly))
            && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    for wire in wires.iter().filter(|wire| wire.point_marker.is_some()) {
        if !seen.contains(wire.name.as_str())
            && marker_segment_hit(wire, view_rot, eye, bounds, |a, b| {
                point_in_polygon(a, poly)
                    || point_in_polygon(b, poly)
                    || segment_crosses_polygon(a, b, poly)
            })
            && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    for (triangles, pick_only) in [
        (wires.fill_triangles().unwrap_or_default(), false),
        (wires.pick_triangles().unwrap_or_default(), true),
    ] {
        for triangle in triangles {
            let Some(wire) = wires.source_wire(triangle.wire) else {
                continue;
            };
            if seen.contains(wire.name.as_str()) {
                continue;
            }
            if projected_wire_triangle(
                wire,
                triangle.start as usize,
                pick_only,
                view_rot,
                eye,
                bounds,
            )
            .is_some_and(|triangle| triangle_crosses_polygon(triangle, poly))
                && seen.insert(wire.name.as_str())
            {
                out.push(wire.name.as_str());
            }
        }
    }
    for glyph in wires.glyphs().unwrap_or_default() {
        let Some(wire) = wires.source_wire(glyph.wire) else {
            continue;
        };
        if seen.contains(wire.name.as_str()) {
            continue;
        }
        let Some(screen) =
            projected_text_quad(wire, glyph.start as usize, view_rot, eye, bounds)
        else {
            continue;
        };
        if [0usize, 3].into_iter().any(|offset| {
            triangle_crosses_polygon(
                [screen[offset], screen[offset + 1], screen[offset + 2]],
                poly,
            )
        }) && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    for wire in wires.iter() {
        if seen.contains(wire.name.as_str())
            || wire.points.len() >= 2
            || !wire.fill_tris.is_empty()
            || !wire.pick_tris.is_empty()
            || !wire.text_verts.is_empty()
        {
            continue;
        }
        if wire.points.iter().enumerate().any(|(index, &point)| {
            point[0].is_finite()
                && point_in_polygon(
                    world_to_screen(
                        wire_point_world(wire, index, view_rot, eye),
                        view_rot,
                        eye,
                        bounds,
                    ),
                    poly,
                )
        }) && seen.insert(wire.name.as_str())
        {
            out.push(wire.name.as_str());
        }
    }
    out
}

/// Return the names of wires selected by a completed rectangular selection box.
///
/// - **Window mode** (`crossing = false`, left→right drag):
///   ALL projected points must lie inside the box.
/// - **Crossing mode** (`crossing = true`, right→left drag):
///   ANY projected point inside the box, OR any wire segment crosses the box
///   boundary (so large entities like viewport frames are caught even when
///   no corner falls inside the selection rectangle).
pub fn box_hit<'a, W: WireSource + ?Sized>(
    corner_a: Point,
    corner_b: Point,
    crossing: bool,
    wires: &'a W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<&'a str> {
    // Clamp the selection box to the pane rectangle so it can't reach geometry
    // the GPU scissored out of a floating viewport (the hit-test wire set runs
    // past the visible rect). No-op in model space, where bounds is the canvas.
    let min_x = corner_a.x.min(corner_b.x).max(0.0);
    let max_x = corner_a.x.max(corner_b.x).min(bounds.width);
    let min_y = corner_a.y.min(corner_b.y).max(0.0);
    let max_y = corner_a.y.max(corner_b.y).min(bounds.height);

    // Ignore zero-area boxes (including a box clamped entirely off-pane).
    if (max_x - min_x) < 1.0 || (max_y - min_y) < 1.0 {
        return vec![];
    }

    let inside = |sp: Point| sp.x >= min_x && sp.x <= max_x && sp.y >= min_y && sp.y <= max_y;

    // Box corners for segment-intersection tests (crossing mode only).
    let box_tl = Point { x: min_x, y: min_y };
    let box_tr = Point { x: max_x, y: min_y };
    let box_bl = Point { x: min_x, y: max_y };
    let box_br = Point { x: max_x, y: max_y };
    let box_corners = [box_tl, box_tr, box_br, box_bl];
    if crossing && wires.segments().is_some() {
        return indexed_box_crossing_hits(
            wires,
            box_corners,
            view_rot,
            eye,
            bounds,
        );
    }

    // Q: lazy projection — accumulate screen points without allocating per-wire Vec.
    wires
        .iter()
        .filter_map(|wire| {
            // Fallback: when wire has no line geometry (e.g. greek text emits
            // only fill_tris) treat the AABB rectangle as the hit-test shape
            // so low-LOD text stays selectable. See #19.
            let aabb_pts: Vec<[f32; 3]>;
            let empty_pts: [[f32; 3]; 0] = [];
            let pts: &[[f32; 3]] = if !wire.points.is_empty() {
                &wire.points
            } else if !wire.text_verts.is_empty() {
                &empty_pts
            } else if wire.aabb != WireModel::UNBOUNDED_AABB {
                let [ax, ay, bx, by] = wire.aabb;
                aabb_pts = vec![
                    [ax, ay, 0.0],
                    [bx, ay, 0.0],
                    [bx, by, 0.0],
                    [ax, by, 0.0],
                    [ax, ay, 0.0],
                ];
                &aabb_pts
            } else {
                return None;
            };

            // Low residual parallel to `pts` (empty for the AABB fallback,
            // whose coarse f32 box doesn't carry one).
            let low: &[[f32; 3]] = if !wire.points.is_empty() {
                &wire.points_low
            } else {
                &[]
            };
            let mut hit = false;
            let mut all_inside = true;
            let mut prev: Option<Point> = None;

            for (i, &[px, py, pz]) in pts.iter().enumerate() {
                if px.is_nan() {
                    prev = None;
                    continue;
                }
                let world = if !wire.points.is_empty() {
                    wire_point_world(wire, i, view_rot, eye)
                } else {
                    wp64([px, py, pz], low, i)
                };
                let sp = world_to_screen(world, view_rot, eye, bounds);
                if crossing {
                    if inside(sp) {
                        hit = true;
                    }
                    if let Some(p0) = prev {
                        if !hit {
                            hit = segments_intersect(p0, sp, box_tl, box_tr)
                                || segments_intersect(p0, sp, box_tr, box_br)
                                || segments_intersect(p0, sp, box_br, box_bl)
                                || segments_intersect(p0, sp, box_bl, box_tl);
                        }
                    }
                } else {
                    if !inside(sp) {
                        all_inside = false;
                    }
                }
                prev = Some(sp);
            }

            let glyphs_present = !wire.text_verts.is_empty();
            let mut glyph_crosses = false;
            let mut glyphs_inside = true;
            for start in (0..wire.text_verts.len()).step_by(6) {
                let Some(screen) = projected_text_quad(wire, start, view_rot, eye, bounds) else {
                    continue;
                };
                if crossing {
                    if [0usize, 3].into_iter().any(|offset| {
                        triangle_crosses_box(
                            [screen[offset], screen[offset + 1], screen[offset + 2]],
                            box_corners,
                        )
                    }) {
                        glyph_crosses = true;
                        break;
                    }
                } else if !screen.iter().copied().all(inside) {
                    glyphs_inside = false;
                    break;
                }
            }

            let result = if crossing {
                hit || glyph_crosses
            } else {
                all_inside && glyphs_inside && (prev.is_some() || glyphs_present)
            };
            if result {
                Some(wire.name.as_str())
            } else {
                None
            }
        })
        .collect()
}

// ── Polygon / lasso selection ─────────────────────────────────────────────

/// Return the names of wires selected by a freehand polygon lasso.
///
/// - **Window mode** (`crossing = false`): ALL projected points inside polygon.
/// - **Crossing mode** (`crossing = true`): ANY point inside OR any wire
///   segment crosses a polygon edge.
pub fn poly_hit<'a, W: WireSource + ?Sized>(
    poly: &[Point],
    crossing: bool,
    wires: &'a W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Vec<&'a str> {
    if poly.len() < 3 {
        return vec![];
    }
    if crossing && wires.segments().is_some() {
        return indexed_polygon_crossing_hits(wires, poly, view_rot, eye, bounds);
    }

    // Q: lazy projection — no Vec allocation per wire.
    wires
        .iter()
        .filter_map(|wire| {
            // Same AABB fallback as `box_hit`: when a wire has no line
            // geometry (e.g. greek-LOD text emits only fill_tris) treat the
            // AABB rectangle as the hit-test shape so low-LOD text stays
            // selectable. See #19.
            let aabb_pts: Vec<[f32; 3]>;
            let empty_pts: [[f32; 3]; 0] = [];
            let pts: &[[f32; 3]] = if !wire.points.is_empty() {
                &wire.points
            } else if !wire.text_verts.is_empty() {
                &empty_pts
            } else if wire.aabb != WireModel::UNBOUNDED_AABB {
                let [ax, ay, bx, by] = wire.aabb;
                aabb_pts = vec![
                    [ax, ay, 0.0],
                    [bx, ay, 0.0],
                    [bx, by, 0.0],
                    [ax, by, 0.0],
                    [ax, ay, 0.0],
                ];
                &aabb_pts
            } else {
                return None;
            };

            let low: &[[f32; 3]] = if !wire.points.is_empty() {
                &wire.points_low
            } else {
                &[]
            };
            let mut hit = false;
            let mut all_inside = true;
            let mut prev: Option<Point> = None;

            for (i, &[px, py, pz]) in pts.iter().enumerate() {
                if px.is_nan() {
                    prev = None;
                    continue;
                }
                let world = if !wire.points.is_empty() {
                    wire_point_world(wire, i, view_rot, eye)
                } else {
                    wp64([px, py, pz], low, i)
                };
                let sp = world_to_screen(world, view_rot, eye, bounds);
                // Reject points the GPU scissored out of a floating viewport so
                // the lasso can't reach clipped geometry. No-op in model space.
                if sp.x < 0.0 || sp.x > bounds.width || sp.y < 0.0 || sp.y > bounds.height {
                    all_inside = false;
                    prev = None;
                    continue;
                }
                if crossing {
                    if point_in_polygon(sp, poly) {
                        hit = true;
                    }
                    if !hit {
                        if let Some(p0) = prev {
                            if segment_crosses_polygon(p0, sp, poly) {
                                hit = true;
                            }
                        }
                    }
                } else {
                    if !point_in_polygon(sp, poly) {
                        all_inside = false;
                    }
                }
                prev = Some(sp);
            }

            let glyphs_present = !wire.text_verts.is_empty();
            let mut glyph_crosses = false;
            let mut glyphs_inside = true;
            for start in (0..wire.text_verts.len()).step_by(6) {
                let Some(screen) = projected_text_quad(wire, start, view_rot, eye, bounds) else {
                    continue;
                };
                if crossing {
                    if [0usize, 3].into_iter().any(|offset| {
                        triangle_crosses_polygon(
                            [screen[offset], screen[offset + 1], screen[offset + 2]],
                            poly,
                        )
                    }) {
                        glyph_crosses = true;
                        break;
                    }
                } else if !screen.iter().copied().all(|point| {
                    point.x >= 0.0
                        && point.x <= bounds.width
                        && point.y >= 0.0
                        && point.y <= bounds.height
                        && point_in_polygon(point, poly)
                }) {
                    glyphs_inside = false;
                    break;
                }
            }

            let result = if crossing {
                hit || glyph_crosses
            } else {
                all_inside && glyphs_inside && (prev.is_some() || glyphs_present)
            };
            if result {
                Some(wire.name.as_str())
            } else {
                None
            }
        })
        .collect()
}

// ── Helpers ───────────────────────────────────────────────────────────────

pub(crate) fn world_to_screen(
    world: glam::DVec3,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Point {
    let ndc = view_rot.project_point3((world - eye).as_vec3());
    Point::new(
        (ndc.x + 1.0) * 0.5 * bounds.width,
        (1.0 - ndc.y) * 0.5 * bounds.height,
    )
}

/// Reconstruct the absolute-f64 world position of wire vertex `i` from its
/// double-single high (`points`) + low (`points_low`) pair. At UTM scale the
/// high f32 alone is ~0.5 m off, which throws box / lasso / click selection
/// edges off by metres; adding the low residual restores f64 precision.
#[inline]
fn wp64(hi: [f32; 3], low: &[[f32; 3]], i: usize) -> glam::DVec3 {
    let l = low.get(i).copied().unwrap_or([0.0; 3]);
    glam::DVec3::new(
        hi[0] as f64 + l[0] as f64,
        hi[1] as f64 + l[1] as f64,
        hi[2] as f64 + l[2] as f64,
    )
}

fn wire_point_world(
    wire: &WireModel,
    index: usize,
    view_rot: Mat4,
    eye: glam::DVec3,
) -> glam::DVec3 {
    let Some(marker) = wire.point_marker else {
        return wire.point_world(index, 0.0);
    };
    let row_y = glam::DVec3::new(
        view_rot.x_axis.y as f64,
        view_rot.y_axis.y as f64,
        view_rot.z_axis.y as f64,
    );
    let projection_scale = row_y.length().max(1.0e-12);
    let relative = (marker.origin - eye).as_vec3().extend(1.0);
    let clip_w = (view_rot * relative).w.abs().max(1.0e-6);
    wire.point_world(index, 2.0 * clip_w as f64 / projection_scale)
}

fn marker_segment_hit(
    wire: &WireModel,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    mut hit: impl FnMut(Point, Point) -> bool,
) -> bool {
    let mut previous = None;
    for (index, point) in wire.points.iter().enumerate() {
        if !point[0].is_finite() {
            previous = None;
            continue;
        }
        let screen = world_to_screen(
            wire_point_world(wire, index, view_rot, eye),
            view_rot,
            eye,
            bounds,
        );
        if previous.is_some_and(|start| hit(start, screen)) {
            return true;
        }
        previous = Some(screen);
    }
    false
}

/// Even-odd ray-casting test: is `p` inside the polygon?
///
/// Handles multi-path boundaries: NaN points (used as path separators by
/// hatches with islands / holes) reset the previous-vertex tracking so
/// that the ray-cast doesn't draw a spurious closing edge between the
/// end of one sub-path and the start of the next. Each sub-path with at
/// least 2 finite vertices contributes its segments to the parity flip.
fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    // Ray-cast crossing test for a single edge a→b.
    fn cross(p: Point, a: Point, b: Point, inside: &mut bool) {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            *inside = !*inside;
        }
    }

    let mut inside = false;
    let mut prev: Option<Point> = None;
    let mut path_start: Option<Point> = None;
    // Vertices in the current sub-path. A boundary can be encoded either as a
    // ring (`[v0,v1,v2,v3]`, needs an implicit closing edge) or as an explicit
    // edge list (`[v0,v1, NaN, v1,v2, NaN, …]`, already closed). Only close a
    // sub-path that is a real ring (≥3 verts); closing a 2-point explicit edge
    // would add a degenerate back-edge that cancels its own crossing.
    let mut count = 0usize;
    let close =
        |prev: Option<Point>, path_start: Option<Point>, count: usize, inside: &mut bool| {
            if count >= 3 {
                if let (Some(pv), Some(sv)) = (prev, path_start) {
                    cross(p, pv, sv, inside);
                }
            }
        };
    for &pt in poly {
        if !pt.x.is_finite() || !pt.y.is_finite() {
            close(prev, path_start, count, &mut inside);
            prev = None;
            path_start = None;
            count = 0;
            continue;
        }
        if let Some(prev_v) = prev {
            cross(p, prev_v, pt, &mut inside);
        } else {
            path_start = Some(pt);
        }
        prev = Some(pt);
        count += 1;
    }
    close(prev, path_start, count, &mut inside);
    inside
}

/// Does segment `[a, b]` cross any edge of the polygon?
fn segment_crosses_polygon(a: Point, b: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    for i in 0..n {
        let c = poly[i];
        let d = poly[(i + 1) % n];
        if segments_intersect(a, b, c, d) {
            return true;
        }
    }
    false
}

/// Do segments `[a,b]` and `[c,d]` intersect?
fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let cross = |o: Point, p: Point, q: Point| -> f32 {
        (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x)
    };
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    false
}

// ── Hatch hit-testing ─────────────────────────────────────────────────────

/// Return the Handle of the first hatch whose screen-space boundary polygon
/// contains `cursor`.
pub fn click_hit_hatch(
    cursor: Point,
    hatches: &HashMap<Handle, HatchModel>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Option<Handle> {
    for (&handle, hatch) in hatches {
        if candidate_handles.is_some_and(|handles| !handles.contains(&handle)) {
            continue;
        }
        if hatch_contains_screen_point(hatch, cursor, view_rot, eye, bounds) {
            return Some(handle);
        }
    }
    None
}

/// Same as `click_hit_hatch` but tests block-internal hatches grouped by
/// their parent Insert handle. The first matching model returns its Insert so
/// clicking a sub-hatch of a block selects the Insert, matching
/// AutoCAD's behaviour for block sub-entities.
pub fn click_hit_insert_hatch(
    cursor: Point,
    insert_hatches: &HashMap<Handle, Vec<HatchModel>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Option<Handle> {
    if let Some(handles) = candidate_handles {
        for handle in handles {
            let Some(hatches) = insert_hatches.get(handle) else {
                continue;
            };
            if hatches
                .iter()
                .any(|hatch| hatch_contains_screen_point(hatch, cursor, view_rot, eye, bounds))
            {
                return Some(*handle);
            }
        }
        return None;
    }
    for (handle, hatches) in insert_hatches {
        if hatches
            .iter()
            .any(|hatch| hatch_contains_screen_point(hatch, cursor, view_rot, eye, bounds))
        {
            return Some(*handle);
        }
    }
    None
}

fn hatch_contains_screen_point(
    hatch: &HatchModel,
    cursor: Point,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> bool {
    // A cursor outside the pane rectangle can't pick a hatch scissored out of a
    // floating viewport. No-op in model space (bounds is the canvas).
    if cursor.x < 0.0 || cursor.x > bounds.width || cursor.y < 0.0 || cursor.y > bounds.height {
        return false;
    }
    // boundary verts are stored as small f32 offsets from
    // `world_origin` (f64). Reconstruct offset-rel WCS before
    // projecting to screen.
    let (ox, oy) = (hatch.world_origin[0], hatch.world_origin[1]);
    let screen: Vec<Point> = hatch
        .boundary
        .iter()
        .map(|&[x, y]| {
            if x.is_finite() && y.is_finite() {
                world_to_screen(
                    glam::DVec3::new(x as f64 + ox, y as f64 + oy, 0.0),
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                // Preserve path separators for the NaN-aware
                // point_in_polygon ray-cast.
                Point::new(f32::NAN, f32::NAN)
            }
        })
        .collect();
    screen.len() >= 3 && point_in_polygon(cursor, &screen)
}

/// Return Handles of hatches selected by a completed rectangular selection box.
fn hatch_box_hit(
    corner_a: Point,
    corner_b: Point,
    crossing: bool,
    hatch: &HatchModel,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> bool {
    let min_x = corner_a.x.min(corner_b.x);
    let max_x = corner_a.x.max(corner_b.x);
    let min_y = corner_a.y.min(corner_b.y);
    let max_y = corner_a.y.max(corner_b.y);
    if (max_x - min_x) < 1.0 || (max_y - min_y) < 1.0 || hatch.boundary.is_empty() {
        return false;
    }
    let inside =
        |point: Point| point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y;
    let (ox, oy) = (hatch.world_origin[0], hatch.world_origin[1]);
    let screen: Vec<Point> = hatch
        .boundary
        .iter()
        .map(|&[x, y]| {
            if x.is_finite() && y.is_finite() {
                world_to_screen(
                    glam::DVec3::new(x as f64 + ox, y as f64 + oy, 0.0),
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                Point::new(f32::NAN, f32::NAN)
            }
        })
        .collect();
    if !screen
        .iter()
        .any(|point| point.x.is_finite() && point.y.is_finite())
    {
        return false;
    }
    if !crossing {
        return screen
            .iter()
            .filter(|point| point.x.is_finite() && point.y.is_finite())
            .copied()
            .all(inside);
    }
    let corners = [
        Point::new(min_x, min_y),
        Point::new(max_x, min_y),
        Point::new(max_x, max_y),
        Point::new(min_x, max_y),
    ];
    screen.iter().copied().any(inside)
        || screen
            .windows(2)
            .filter(|edge| {
                edge[0].x.is_finite()
                    && edge[0].y.is_finite()
                    && edge[1].x.is_finite()
                    && edge[1].y.is_finite()
            })
            .any(|edge| {
                (0..4).any(|side| {
                    segments_intersect(edge[0], edge[1], corners[side], corners[(side + 1) % 4])
                })
            })
        || corners
            .iter()
            .copied()
            .any(|corner| point_in_polygon(corner, &screen))
}

pub fn box_hit_hatch(
    corner_a: Point,
    corner_b: Point,
    crossing: bool,
    hatches: &HashMap<Handle, HatchModel>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Vec<Handle> {
    hatches
        .iter()
        .filter_map(|(&handle, hatch)| {
            if candidate_handles.is_some_and(|handles| !handles.contains(&handle)) {
                return None;
            }
            hatch_box_hit(corner_a, corner_b, crossing, hatch, view_rot, eye, bounds)
                .then_some(handle)
        })
        .collect()
}

pub fn box_hit_insert_hatch(
    corner_a: Point,
    corner_b: Point,
    crossing: bool,
    insert_hatches: &HashMap<Handle, Vec<HatchModel>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(handles) = candidate_handles {
        for handle in handles {
            if insert_hatches.get(handle).is_some_and(|hatches| {
                let test = |hatch: &HatchModel| {
                    hatch_box_hit(corner_a, corner_b, crossing, hatch, view_rot, eye, bounds)
                };
                if crossing {
                    hatches.iter().any(test)
                } else {
                    !hatches.is_empty() && hatches.iter().all(test)
                }
            }) {
                out.push(*handle);
            }
        }
    } else {
        for (handle, hatches) in insert_hatches {
            let test = |hatch: &HatchModel| {
                hatch_box_hit(corner_a, corner_b, crossing, hatch, view_rot, eye, bounds)
            };
            let hit = if crossing {
                hatches.iter().any(test)
            } else {
                !hatches.is_empty() && hatches.iter().all(test)
            };
            if hit {
                out.push(*handle);
            }
        }
    }
    out
}

/// Return Handles of hatches selected by a freehand polygon lasso.
fn hatch_polygon_hit(
    poly: &[Point],
    crossing: bool,
    hatch: &HatchModel,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> bool {
    if poly.len() < 3 || hatch.boundary.is_empty() {
        return false;
    }
    let (ox, oy) = (hatch.world_origin[0], hatch.world_origin[1]);
    let screen: Vec<Point> = hatch
        .boundary
        .iter()
        .map(|&[x, y]| {
            if x.is_finite() && y.is_finite() {
                world_to_screen(
                    glam::DVec3::new(x as f64 + ox, y as f64 + oy, 0.0),
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                Point::new(f32::NAN, f32::NAN)
            }
        })
        .collect();
    if !screen
        .iter()
        .any(|point| point.x.is_finite() && point.y.is_finite())
    {
        return false;
    }
    if crossing {
        screen
            .iter()
            .copied()
            .any(|point| point_in_polygon(point, poly))
            || screen
                .windows(2)
                .filter(|edge| {
                    edge[0].x.is_finite()
                        && edge[0].y.is_finite()
                        && edge[1].x.is_finite()
                        && edge[1].y.is_finite()
                })
                .any(|edge| segment_crosses_polygon(edge[0], edge[1], poly))
            || poly
                .iter()
                .copied()
                .any(|point| point_in_polygon(point, &screen))
    } else {
        screen
            .iter()
            .filter(|point| point.x.is_finite() && point.y.is_finite())
            .all(|point| point_in_polygon(*point, poly))
    }
}

pub fn poly_hit_hatch(
    poly: &[Point],
    crossing: bool,
    hatches: &HashMap<Handle, HatchModel>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Vec<Handle> {
    hatches
        .iter()
        .filter_map(|(&handle, hatch)| {
            if candidate_handles.is_some_and(|handles| !handles.contains(&handle)) {
                return None;
            }
            hatch_polygon_hit(poly, crossing, hatch, view_rot, eye, bounds).then_some(handle)
        })
        .collect()
}

pub fn poly_hit_insert_hatch(
    poly: &[Point],
    crossing: bool,
    insert_hatches: &HashMap<Handle, Vec<HatchModel>>,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    candidate_handles: Option<&HashSet<Handle>>,
) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(handles) = candidate_handles {
        for handle in handles {
            if insert_hatches.get(handle).is_some_and(|hatches| {
                let test = |hatch: &HatchModel| {
                    hatch_polygon_hit(poly, crossing, hatch, view_rot, eye, bounds)
                };
                if crossing {
                    hatches.iter().any(test)
                } else {
                    !hatches.is_empty() && hatches.iter().all(test)
                }
            }) {
                out.push(*handle);
            }
        }
    } else {
        for (handle, hatches) in insert_hatches {
            let test = |hatch: &HatchModel| {
                hatch_polygon_hit(poly, crossing, hatch, view_rot, eye, bounds)
            };
            let hit = if crossing {
                hatches.iter().any(test)
            } else {
                !hatches.is_empty() && hatches.iter().all(test)
            };
            if hit {
                out.push(*handle);
            }
        }
    }
    out
}

/// Minimum distance from point `p` to line segment `[a, b]` in 2-D.
fn dist_point_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    let t = if len2 < 1e-6 {
        0.0
    } else {
        let apx = p.x - a.x;
        let apy = p.y - a.y;
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    };
    let cx = a.x + t * abx;
    let cy = a.y + t * aby;
    let dx = p.x - cx;
    let dy = p.y - cy;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod aabb_reject_tests {
    use super::*;

    fn wire(name: &str, pts: Vec<[f32; 3]>, aabb: [f32; 4]) -> WireModel {
        let mut w = WireModel::solid(name.to_string(), pts, [1.0; 4], false);
        w.aabb = aabb;
        w
    }

    // Identity ortho view: world (x,y) → screen ((x+1)*100, (1-y)*100) for a
    // 200×200 viewport. The view is flat (z_axis.xy == 0) so the AABB pre-reject
    // is active — these tests guard it against false negatives.
    #[test]
    fn aabb_reject_keeps_near_wire_drops_far() {
        let vp = Mat4::IDENTITY;
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let cursor = Point::new(100.0, 100.0); // world origin

        let near = wire(
            "5",
            vec![[-0.02, 0.0, 0.0], [0.02, 0.0, 0.0]],
            [-0.02, 0.0, 0.02, 0.0],
        );
        let far = wire(
            "9",
            vec![[0.9, 0.9, 0.0], [0.95, 0.9, 0.0]],
            [0.9, 0.9, 0.95, 0.9],
        );

        let eye = glam::DVec3::ZERO;
        assert_eq!(
            click_hit(cursor, std::slice::from_ref(&near), vp, eye, bounds, true, 8.0),
            Some("5")
        );
        assert_eq!(
            click_hit(cursor, std::slice::from_ref(&far), vp, eye, bounds, true, 8.0),
            None
        );
        // The far wire must be rejected without hiding the near one.
        assert_eq!(
            click_hit(cursor, &[far, near], vp, eye, bounds, true, 8.0),
            Some("5")
        );
    }

    #[test]
    fn crossing_hits_glyphs_batched_with_distant_block_geometry() {
        use crate::scene::pipeline::text_gpu::TextVertex;

        let vertex = |x, y| TextVertex {
            pos: [x, y, 0.0],
            pos_low: [0.0; 3],
            uv: [0.0; 2],
            color: [1.0; 4],
            draw_depth: 0.0,
        };
        let mut block = wire(
            "479",
            vec![[0.75, 0.75, 0.0], [0.9, 0.75, 0.0]],
            [-0.1, -0.1, 0.9, 0.75],
        );
        block.text_verts = vec![
            vertex(-0.1, -0.1),
            vertex(0.1, -0.1),
            vertex(0.1, 0.1),
            vertex(-0.1, -0.1),
            vertex(0.1, 0.1),
            vertex(-0.1, 0.1),
        ];

        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let wires = std::slice::from_ref(&block);
        assert_eq!(
            box_hit(
                Point::new(85.0, 85.0),
                Point::new(115.0, 115.0),
                true,
                wires,
                Mat4::IDENTITY,
                glam::DVec3::ZERO,
                bounds,
            ),
            vec!["479"],
        );
        assert_eq!(
            poly_hit(
                &[
                    Point::new(85.0, 85.0),
                    Point::new(115.0, 85.0),
                    Point::new(115.0, 115.0),
                    Point::new(85.0, 115.0),
                ],
                true,
                wires,
                Mat4::IDENTITY,
                glam::DVec3::ZERO,
                bounds,
            ),
            vec!["479"],
        );
    }
}
