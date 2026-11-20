use super::*;

#[derive(Clone, Copy)]
pub(crate) struct CrossingWindow {
    pub(crate) min: [f64; 2],
    pub(crate) max: [f64; 2],
}

pub(crate) fn segment_window_range(
    p1: [f64; 2],
    p2: [f64; 2],
    window: CrossingWindow,
) -> Option<(f64, f64)> {
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for axis in 0..2 {
        let d = p2[axis] - p1[axis];
        if d.abs() < 1e-12 {
            if p1[axis] < window.min[axis] - 1e-9 || p1[axis] > window.max[axis] + 1e-9 {
                return None;
            }
            continue;
        }
        let mut a = (window.min[axis] - p1[axis]) / d;
        let mut b = (window.max[axis] - p1[axis]) / d;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        lo = lo.max(a);
        hi = hi.min(b);
        if lo > hi + 1e-9 {
            return None;
        }
    }
    Some((lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0)))
}

/// Crossing selection trims LwPolyline segments independently. A segment that
/// lies inside the selection rectangle but never meets a cutting edge must
/// survive; otherwise a rectangle loses its far vertical edge (#588).

pub(crate) fn crossing_trim_lwpolyline(
    poly: &LwPolyline,
    geos: &[Geo],
    window: CrossingWindow,
) -> Option<Vec<EntityType>> {
    let handle = poly.common.handle;
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;
    let vertex_xy = |i: usize| {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    // Cuts along the whole polyline as global parameters (segment index +
    // local parameter), as a single pick finds them: vertices are not cuts,
    // and an open polyline's own ends are not either (#2130).
    let mut cuts = lwpoly_self_cut_params(poly, geos);
    // The parts of the polyline inside the crossing box.
    let mut inside = Vec::<(f64, f64)>::new();
    for i in 0..seg_count {
        let a = vertex_xy(i);
        let b = vertex_xy(i + 1);
        let bulge = seg_bulge(i);
        cuts.extend(
            polyline_seg_ts(a, b, bulge, handle, geos)
                .into_iter()
                .map(|t| i as f64 + t),
        );
        let window_range = if let Some(ba) = BulgeArc::from_bulge(a, b, bulge) {
            let steps = 32usize;
            let mut min_t: Option<f64> = None;
            let mut max_t: Option<f64> = None;
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let pt = ba.sample(t);
                if pt[0] >= window.min[0] - 1e-9
                    && pt[0] <= window.max[0] + 1e-9
                    && pt[1] >= window.min[1] - 1e-9
                    && pt[1] <= window.max[1] + 1e-9
                {
                    min_t = Some(min_t.map_or(t, |m: f64| m.min(t)));
                    max_t = Some(max_t.map_or(t, |m: f64| m.max(t)));
                }
            }
            match (min_t, max_t) {
                (Some(lo), Some(hi)) => Some((lo, hi)),
                _ => None,
            }
        } else {
            segment_window_range(a, b, window)
        };
        if let Some((lo, hi)) = window_range {
            inside.push((i as f64 + lo, i as f64 + hi));
        }
    }
    if inside.is_empty() {
        return None;
    }
    if closed {
        for t in &mut cuts {
            *t = t.rem_euclid(total);
        }
    } else {
        cuts.retain(|t| *t > 1e-6 && *t < total - 1e-6);
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if closed && cuts.len() > 1 && cuts[0] + total - cuts[cuts.len() - 1] < 1e-6 {
        cuts.pop();
    }
    // A piece that no cutting edge crosses stays whole: nothing to trim.
    if cuts.is_empty() || (closed && cuts.len() < 2) {
        return None;
    }

    // The pieces between consecutive cuts; a closed polyline's last piece
    // runs past its start, split there.
    let mut pieces = Vec::<(f64, f64)>::new();
    if closed {
        for pair in cuts.windows(2) {
            pieces.push((pair[0], pair[1]));
        }
        let (last, first) = (cuts[cuts.len() - 1], cuts[0]);
        pieces.push((last, total));
        if first > 1e-6 {
            pieces.push((0.0, first));
        }
    } else {
        let mut bounds = vec![0.0];
        bounds.extend(cuts.iter().copied());
        bounds.push(total);
        for pair in bounds.windows(2) {
            pieces.push((pair[0], pair[1]));
        }
    }
    let overlaps = |(lo, hi): (f64, f64)| {
        inside
            .iter()
            .any(|&(a, b)| b >= lo - 1e-6 && a <= hi + 1e-6 && hi - lo > 1e-6)
    };
    let mut removed: Vec<(f64, f64)> = if closed {
        // The wrapped piece counts as one: both of its halves go together.
        let wrapped = (cuts[cuts.len() - 1], total);
        let head = (0.0, cuts[0]);
        let wrap_hit = overlaps(wrapped) || overlaps(head);
        pieces
            .into_iter()
            .filter(|&piece| {
                if piece == wrapped || piece == head {
                    wrap_hit
                } else {
                    overlaps(piece)
                }
            })
            .collect()
    } else {
        pieces
            .into_iter()
            .filter(|&piece| overlaps(piece))
            .collect()
    };
    if removed.is_empty() {
        return None;
    }

    removed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged = Vec::<(f64, f64)>::new();
    for span in removed {
        if let Some(last) = merged.last_mut() {
            if span.0 <= last.1 + 1e-6 {
                last.1 = last.1.max(span.1);
                continue;
            }
        }
        merged.push(span);
    }

    let mut kept = Vec::<(f64, f64)>::new();
    if closed {
        for i in 0..merged.len() {
            let start = merged[i].1;
            let mut end = merged[(i + 1) % merged.len()].0;
            if i + 1 == merged.len() {
                end += total;
            }
            if end - start > 1e-6 {
                kept.push((start, end));
            }
        }
    } else {
        let mut cursor = 0.0;
        for &(start, end) in &merged {
            if start - cursor > 1e-6 {
                kept.push((cursor, start));
            }
            cursor = cursor.max(end);
        }
        if total - cursor > 1e-6 {
            kept.push((cursor, total));
        }
    }

    Some(
        kept.into_iter()
            .filter_map(|(start, end)| {
                extract_sub_polyline(poly, start, end).map(EntityType::LwPolyline)
            })
            .collect(),
    )
}

/// A detached circular fill is itself one complete closed TRIM fragment.
/// Unlike a rectangular fill with a hole, it can be removed with a single
/// click without selecting another cutting edge. Do not delete arbitrary
/// multi-loop / polygonal hatches when a trim solver finds no intersection.

/// Dense XY sampling for the fence pass — covers the types the quick pick
/// supports (adds Circle / Ray / XLine over `sample_entity_xy`).
pub(crate) fn fence_sample_xy(e: &EntityType) -> Vec<[f64; 2]> {
    match e {
        // The two unbounded kinds, which `sample_entity_xy` declines to
        // guess a length for. Here there is one: as far as a trim reaches.
        EntityType::Ray(r) => vec![
            [r.base_point.x, r.base_point.y],
            [
                r.base_point.x + r.direction.x * TRIM_EXTENT,
                r.base_point.y + r.direction.y * TRIM_EXTENT,
            ],
        ],
        EntityType::XLine(x) => vec![
            [
                x.base_point.x - x.direction.x * TRIM_EXTENT,
                x.base_point.y - x.direction.y * TRIM_EXTENT,
            ],
            [
                x.base_point.x + x.direction.x * TRIM_EXTENT,
                x.base_point.y + x.direction.y * TRIM_EXTENT,
            ],
        ],
        _ => sample_entity_xy(e),
    }
}

/// Every crossing point of `e` with the fence polyline, in traversal order.
/// Each drives the same per-type pick as a direct click, so the actual cuts
/// happen at the exact boundary intersections.

pub(crate) fn fence_cross_points(e: &EntityType, fence_geos: &[Geo]) -> Vec<[f64; 2]> {
    let pts = fence_sample_xy(e);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        // NOTE: the exclusion handle must differ from the fence geos' own —
        // passing NULL against NULL-handle geos excluded the whole fence.
        let mut ts = line_seg_ts(
            w[0][0],
            w[0][1],
            w[1][0],
            w[1][1],
            Handle::new(FENCE_PROBE),
            fence_geos,
        );
        ts.sort_by(|a, b| a.total_cmp(b));
        for t in ts {
            out.push(lerp2(w[0], w[1], t));
        }
    }
    out
}

/// A representative point where an entity meets or lies inside a crossing
/// window. Checking sampled vertices alone misses a straight segment whose
/// endpoints are both outside the rectangle.

pub(crate) fn entity_point_in_window(e: &EntityType, window: CrossingWindow) -> Option<[f64; 2]> {
    let pts = fence_sample_xy(e);
    if let Some(point) = pts.iter().copied().find(|p| {
        p[0] >= window.min[0]
            && p[0] <= window.max[0]
            && p[1] >= window.min[1]
            && p[1] <= window.max[1]
    }) {
        return Some(point);
    }
    pts.windows(2).find_map(|segment| {
        segment_window_range(segment[0], segment[1], window)
            .map(|(lo, hi)| lerp2(segment[0], segment[1], (lo + hi) * 0.5))
    })
}

/// Find cutting edges touched by a drag rectangle. A left-to-right window
/// keeps only entities fully inside; a right-to-left crossing window also
/// keeps entities that intersect the rectangle boundary.

pub(crate) fn drag_selected_handles(
    entities: &[EntityType],
    index: &ModifyEntityIndex,
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    crossing: bool,
) -> Vec<Handle> {
    let candidates = trim_fence_candidates(index, fence, window);
    let fence_geos = build_fence_geos(fence);
    let lasso: Vec<Curve> = fence_geos.iter().filter_map(geo_to_curve).collect();
    entities
        .iter()
        .filter(|entity| {
            let handle = entity.common().handle;
            !handle.is_null()
                && candidates
                    .as_ref()
                    .is_none_or(|hits| hits.contains(&handle))
        })
        .filter(|entity| {
            let Some(window) = window else {
                // Lasso: edges crossing it or lying wholly inside it.
                return !fence_cross_points(entity, &fence_geos).is_empty()
                    || fence_sample_xy(entity).first().is_some_and(|point| {
                        ocs_geom::geom2d::contains(
                            &lasso,
                            *point,
                            KernelTolerance::new(CUT_TOLERANCE),
                        )
                    });
            };
            if crossing {
                !fence_cross_points(entity, &fence_geos).is_empty()
                    || entity_point_in_window(entity, window).is_some()
            } else {
                let points = fence_sample_xy(entity);
                !points.is_empty()
                    && points.iter().all(|point| {
                        point[0] >= window.min[0]
                            && point[0] <= window.max[0]
                            && point[1] >= window.min[1]
                            && point[1] <= window.max[1]
                    })
            }
        })
        .map(|entity| entity.common().handle)
        .collect()
}

/// Update cutting-edge membership in linear expected time.  An ordinary
/// Vec::contains for each selected handle made large drag selections quadratic.

pub(crate) fn update_trim_edge_set(edges: &mut Vec<Handle>, handles: Vec<Handle>, remove: bool) {
    if remove {
        let removing: rustc_hash::FxHashSet<_> = handles.into_iter().collect();
        edges.retain(|handle| !removing.contains(handle));
    } else {
        let mut existing: rustc_hash::FxHashSet<_> = edges.iter().copied().collect();
        for handle in handles {
            if existing.insert(handle) {
                edges.push(handle);
            }
        }
    }
}

pub(crate) fn fence_pieces(
    e: &EntityType,
    geos: &[Geo],
    fence_geos: &[Geo],
    window: Option<CrossingWindow>,
    extend: bool,
) -> Option<Vec<EntityType>> {
    if !extend {
        if let (EntityType::LwPolyline(poly), Some(window)) = (e, window) {
            return crossing_trim_lwpolyline(poly, geos, window);
        }
    }

    let target = e.common().handle;
    let mut pieces = vec![e.clone()];
    let mut changed = false;
    for _round in 0..8 {
        let mut next: Vec<EntityType> = Vec::new();
        let mut any = false;
        for piece in &pieces {
            let mut tmp = piece.clone();
            tmp.as_entity_mut().set_handle(target);
            let tmp_all = [tmp];
            let mut consumed = false;
            let mut cps = fence_cross_points(&tmp_all[0], fence_geos);
            // Crossing works like a crossing SELECTION: an object wholly
            // inside the window is picked too — synthesize the click at a
            // sampled point inside it.
            if cps.is_empty() {
                if let Some(window) = window {
                    if let Some(p) = entity_point_in_window(&tmp_all[0], window) {
                        cps.push(p);
                    }
                }
            }
            for cp in cps {
                let res = if extend {
                    pick_extend_at(&tmp_all, geos, target, cp[0], cp[1]).map(|x| vec![x])
                } else {
                    pick_trim_at(&tmp_all, geos, target, cp[0], cp[1])
                };
                if let Some(mut sub) = res {
                    for sp in &mut sub {
                        sp.as_entity_mut().set_handle(Handle::NULL);
                    }
                    next.extend(sub);
                    any = true;
                    changed = true;
                    consumed = true;
                    break;
                }
            }
            if !consumed {
                next.push(piece.clone());
            }
        }
        pieces = next;
        if !any || pieces.len() > 64 {
            break;
        }
    }
    if changed {
        Some(pieces)
    } else {
        None
    }
}

/// Fence / Crossing pass (#336): every object crossing the fence polyline is
/// trimmed (or extended) at each of its crossing points; the cuts land on the
/// boundary-edge intersections, the fence only selects.
/// Boundary/edge-selection highlight colour (the EXTRIM boundary yellow).

pub(crate) const OPT_YELLOW: [f32; 4] = [1.0, 0.90, 0.15, 1.0];

/// Sentinel handles for the fence pass: the fence's own geos and the probe
/// handle used against them must never collide with a real entity handle (or
/// with each other — `line_seg_ts` drops geos matching the probe handle).
pub(crate) const FENCE_GEO: u64 = u64::MAX - 8;
pub(crate) const FENCE_PROBE: u64 = u64::MAX - 9;

pub(crate) fn build_fence_geos(fence: &[[f64; 2]]) -> Vec<Geo> {
    fence
        .windows(2)
        .map(|w| Geo::Line {
            handle: Handle::new(FENCE_GEO),
            p1: w[0],
            p2: w[1],
        })
        .collect()
}

/// Conservative bounding rectangle for the entire fence/window.  A missing
/// or invalid bound falls back to the old all-entities path (never drop edits).

pub(crate) fn trim_selection_bounds(
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
) -> Option<[f64; 4]> {
    if let Some(w) = window {
        let aabb = [w.min[0], w.min[1], w.max[0], w.max[1]];
        return aabb.iter().all(|v| v.is_finite()).then_some(aabb);
    }
    let first = fence.first()?;
    if !first.iter().all(|v| v.is_finite()) {
        return None;
    }
    let mut aabb = [first[0], first[1], first[0], first[1]];
    for point in fence.iter().skip(1) {
        if !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        aabb[0] = aabb[0].min(point[0]);
        aabb[1] = aabb[1].min(point[1]);
        aabb[2] = aabb[2].max(point[0]);
        aabb[3] = aabb[3].max(point[1]);
    }
    Some(aabb)
}

pub(crate) fn trim_fence_candidates(
    index: &ModifyEntityIndex,
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
) -> Option<rustc_hash::FxHashSet<Handle>> {
    trim_selection_bounds(fence, window).map(|aabb| index.handles_in_aabb(aabb))
}

/// Live result preview of a fence / crossing pass: each affected original in
/// gray, its surviving pieces in cyan — the same convention as the pick hover.

pub(crate) fn fence_pass(
    all: &[EntityType],
    index: &ModifyEntityIndex,
    geos: &[Geo],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    extend: bool,
) -> Vec<(Handle, Vec<EntityType>)> {
    fence_pass_with(all, index, geos, fence, window, extend, false)
}

/// The cutters that can touch `target`'s pieces. A trimmed object's pieces
/// stay inside its own box, so a cutter outside the box cannot cut them:
/// with `local`, only the cutters reaching the box are kept, instead of
/// intersecting every object with every cutter (#2461). Splines keep their
/// place, as their sampled box can miss a bulge.

pub(crate) fn cutters_near<'a>(
    target: &EntityType,
    geos: &'a [Geo],
    local: bool,
) -> std::borrow::Cow<'a, [Geo]> {
    let Some(bounds) = local.then(|| modify_aabb(target)).flatten() else {
        return std::borrow::Cow::Borrowed(geos);
    };
    let pad = 1e-6
        * (bounds[2] - bounds[0])
            .abs()
            .max((bounds[3] - bounds[1]).abs())
            .max(1.0);
    let bounds = [
        bounds[0] - pad,
        bounds[1] - pad,
        bounds[2] + pad,
        bounds[3] + pad,
    ];
    std::borrow::Cow::Owned(
        geos.iter()
            .filter(|geo| matches!(geo, Geo::Spline { .. }) || geo_overlaps_aabb(geo, bounds))
            .cloned()
            .collect(),
    )
}

/// [`fence_pass`], cutting each object only with the cutters near it when
/// `local` (TRIM without extension, where nothing reaches past an object).

pub(crate) fn fence_pass_with(
    all: &[EntityType],
    index: &ModifyEntityIndex,
    geos: &[Geo],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    extend: bool,
    local: bool,
) -> Vec<(Handle, Vec<EntityType>)> {
    let candidates = trim_fence_candidates(index, fence, window);
    let fence_geos: Vec<Geo> = build_fence_geos(fence);
    let mut out = Vec::new();
    for e in all {
        let h = e.common().handle;
        if h.is_null() || candidates.as_ref().is_some_and(|hits| !hits.contains(&h)) {
            continue;
        }
        let cutters = cutters_near(e, geos, local && !extend);
        if let Some(pieces) = fence_pieces(e, &cutters, &fence_geos, window, extend) {
            out.push((h, pieces));
        }
    }
    out
}

/// EXTEND's normal box selection applies ONE extension per selected entity.
/// Unlike the multi-round Fence/Crossing operator, this cannot extend an
/// already-updated line again after it reaches its first boundary.
/// The box is only a selector; all targets still use the chosen common
/// boundary-edge geometry (`geos`).

pub(crate) fn extend_box_pass(
    all: &[EntityType],
    index: &ModifyEntityIndex,
    geos: &[Geo],
    boundary_handles: &[Handle],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    crossing: bool,
) -> Vec<(Handle, Vec<EntityType>)> {
    let selected = drag_selected_handles(all, index, fence, window, crossing);
    if selected.is_empty() {
        return Vec::new();
    }
    let selected: std::collections::HashSet<_> = selected.into_iter().collect();
    let boundaries: std::collections::HashSet<_> = boundary_handles.iter().copied().collect();
    let mut replacements = Vec::new();
    for entity in all {
        let handle = entity.common().handle;
        if !selected.contains(&handle) || boundaries.contains(&handle) {
            continue;
        }
        // Prefer the end visibly touched by the selection rectangle.
        // If both ends are within it, use the nearest reachable boundary;
        // if neither end is within it, follow the box crossing position.
        // This is independent of line direction in the file.
        let extended = if let EntityType::Line(line) = entity {
            let contains = |x: f64, y: f64| {
                window.is_some_and(|w| {
                    x >= w.min[0] && x <= w.max[0] && y >= w.min[1] && y <= w.max[1]
                })
            };
            let start_in = contains(line.start.x, line.start.y);
            let end_in = contains(line.end.x, line.end.y);
            if start_in && end_in {
                let start = extend_line(line, 0.0, geos);
                let end = extend_line(line, 1.0, geos);
                match (start, end) {
                    (Some(a), Some(b)) => {
                        let dist = |result: &EntityType| {
                            let EntityType::Line(extended) = result else {
                                return f64::INFINITY;
                            };
                            (extended.start.x - line.start.x).hypot(extended.start.y - line.start.y)
                                + (extended.end.x - line.end.x).hypot(extended.end.y - line.end.y)
                        };
                        Some(if dist(&a) <= dist(&b) { a } else { b })
                    }
                    (a @ Some(_), None) | (None, a @ Some(_)) => a,
                    (None, None) => None,
                }
            } else if start_in {
                extend_line(line, 0.0, geos)
            } else if end_in {
                extend_line(line, 1.0, geos)
            } else {
                let pick = window.and_then(|w| entity_point_in_window(entity, w));
                pick.and_then(|p| pick_extend_at(all, geos, handle, p[0], p[1]))
            }
        } else {
            // Keep the existing analytic extension for arcs, polylines,
            // ellipses and splines instead of introducing another solver.
            window
                .and_then(|w| entity_point_in_window(entity, w))
                .or_else(|| {
                    fence_cross_points(entity, &build_fence_geos(fence))
                        .first()
                        .copied()
                })
                .and_then(|p| pick_extend_at(all, geos, handle, p[0], p[1]))
        };
        if let Some(new_entity) = extended {
            replacements.push((handle, vec![new_entity]));
        }
    }
    replacements
}
