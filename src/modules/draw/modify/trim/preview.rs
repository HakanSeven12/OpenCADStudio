use super::*;

pub(crate) fn trimmed_line_removed_preview(
    orig: &LineEnt,
    survivors: &[EntityType],
    t_click: f64,
) -> Option<Vec<[f32; 3]>> {
    let dx = orig.end.x - orig.start.x;
    let dy = orig.end.y - orig.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return None;
    }
    let parameter = |x: f64, y: f64| {
        (((x - orig.start.x) * dx + (y - orig.start.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let mut spans: Vec<_> = survivors
        .iter()
        .filter_map(|entity| match entity {
            EntityType::Line(line) => {
                let a = parameter(line.start.x, line.start.y);
                let b = parameter(line.end.x, line.end.y);
                Some((a.min(b), a.max(b)))
            }
            _ => None,
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut gaps = Vec::new();
    let mut cursor = 0.0;
    for (start, end) in spans {
        if start > cursor + 1e-8 {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < 1.0 - 1e-8 {
        gaps.push((cursor, 1.0));
    }
    let t_click = t_click.clamp(0.0, 1.0);
    let (from, to) = gaps
        .into_iter()
        .find(|(from, to)| t_click >= from - 1e-8 && t_click <= to + 1e-8)?;
    let point = |t: f64| {
        [
            (orig.start.x + dx * t) as f32,
            (orig.start.y + dy * t) as f32,
            (orig.start.z + (orig.end.z - orig.start.z) * t) as f32,
        ]
    };
    Some(vec![point(from), point(to)])
}

/// Extend a clicked Arc end along its circle to the nearest boundary
/// crossing (#409). `t_click` ∈ [0,1] within the current span picks the end:
/// ≥ 0.5 extends the end angle CCW, < 0.5 extends the start angle CW.

pub(crate) const TRIM_REMOVE_GRAY: [f32; 4] = [0.40, 0.40, 0.40, 0.88];

pub(crate) fn line_pts(l: &LineEnt) -> Vec<[f32; 3]> {
    geom::line_points(
        [l.start.x, l.start.y, l.start.z],
        [l.end.x, l.end.y, l.end.z],
    )
}

pub(crate) fn entity_pts(e: &EntityType) -> Vec<[f32; 3]> {
    match e {
        EntityType::Line(l) => line_pts(l),
        EntityType::Arc(a) => arc_pts(
            a.center.x,
            a.center.y,
            a.radius,
            a.start_angle,
            a.end_angle,
            a.center.z,
        ),
        EntityType::Ellipse(e) => {
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 {
                return vec![];
            }
            let b = a * e.minor_axis_ratio;
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z)
        }
        EntityType::Spline(s) => spline_pts_wire(s),
        EntityType::LwPolyline(p) => {
            let elev = p.elevation as f32;
            let xy_pts = preview_sample_xy(e);
            xy_pts
                .into_iter()
                .map(|pt| [pt[0] as f32, pt[1] as f32, elev])
                .collect()
        }
        EntityType::Polyline(_) | EntityType::Polyline2D(_) | EntityType::Polyline3D(_) => {
            preview_sample_xy(e)
                .into_iter()
                .map(|pt| [pt[0] as f32, pt[1] as f32, 0.0])
                .collect()
        }
        // For preview, show a 20-unit section of semi-infinite results
        EntityType::Ray(r) => {
            let bx = r.base_point.x;
            let by = r.base_point.y;
            let bz = r.base_point.z;
            let far_x = bx + r.direction.x * 20.0;
            let far_y = by + r.direction.y * 20.0;
            let far_z = bz + r.direction.z * 20.0;
            vec![
                [bx as f32, bz as f32, by as f32],
                [far_x as f32, far_z as f32, far_y as f32],
            ]
        }
        _ => vec![],
    }
}

// ══════════════════════════════════════════════════════════════════════════

pub(crate) fn trim_edge_glow(entity: &EntityType, candidate: bool) -> Vec<WireModel> {
    let points = entity_pts(entity);
    if points.len() < 2 {
        return Vec::new();
    }
    let layers: [(f32, f32); 3] = if candidate {
        [(8.0, 0.035), (5.0, 0.065), (2.5, 0.11)]
    } else {
        [(8.0, 0.075), (5.0, 0.15), (2.5, 0.34)]
    };
    layers
        .into_iter()
        .enumerate()
        .map(|(index, (width, alpha))| {
            let mut wire = WireModel::solid(
                format!("trim_edge_glow_{index}"),
                points.clone(),
                [0.22, 0.60, 1.0, alpha],
                false,
            );
            wire.line_weight_px = wire.line_weight_px.max(width);
            wire
        })
        .collect()
}

/// Trim/extend one entity at EVERY fence crossing: the pick is re-applied to
/// the surviving pieces until no crossing bites (a fence across both
/// overhanging ends of one line clears both). Returns `None` when nothing
/// changed. Pieces carry NULL handles for the ReplaceMany placeholder flow.

pub(crate) fn fence_result_preview(
    all: &[EntityType],
    index: &ModifyEntityIndex,
    geos: &[Geo],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    extend: bool,
    implied_edges: bool,
) -> Vec<WireModel> {
    let candidates = trim_fence_candidates(index, fence, window);
    // Heavy overlapping geometry must not be re-trimmed on every mouse
    // movement.  Display the fence alone in crowded areas; the final
    // operation below remains exact and processes every matching object.
    const MAX_LIVE_FENCE_TARGETS: usize = 64;
    if candidates
        .as_ref()
        .is_some_and(|hits| hits.len() > MAX_LIVE_FENCE_TARGETS)
    {
        return Vec::new();
    }
    let fence_geos = build_fence_geos(fence);
    let mut out = Vec::new();
    for e in all {
        let handle = e.common().handle;
        if handle.is_null()
            || candidates
                .as_ref()
                .is_some_and(|hits| !hits.contains(&handle))
        {
            continue;
        }
        let cutters = cutters_near(e, geos, !extend && !implied_edges);
        if let Some(pieces) = fence_pieces(e, &cutters, &fence_geos, window, extend) {
            out.push(WireModel::solid(
                "trim_rm".into(),
                entity_pts(e),
                TRIM_REMOVE_GRAY,
                false,
            ));
            for (i, pe) in pieces.iter().enumerate() {
                out.push(WireModel::solid(
                    format!("trim_keep_{i}"),
                    entity_pts(pe),
                    WireModel::CYAN,
                    false,
                ));
                if extend {
                    if let Some(t) = extend_tail_preview(e, pe, "extend_tail") {
                        out.push(t);
                    }
                }
            }
            if implied_edges {
                let cuts = piece_cut_points(e, &pieces);
                out.extend(implied_cut_guides(all, e.common().handle, &cuts));
            }
        }
    }
    out
}

/// The ADDED tail of an extend result, dashed — the slice of the extended
/// curve beyond the original endpoint, so the preview shows the extension
/// itself rather than only a recolored whole (#336). Generic over entity
/// type via the sampled points.
pub(crate) fn extend_tail_preview(
    orig: &EntityType,
    ext: &EntityType,
    name: &str,
) -> Option<WireModel> {
    let op = entity_pts(orig);
    let ep = entity_pts(ext);
    if op.len() < 2 || ep.len() < 2 {
        return None;
    }
    let d2 = |a: &[f32; 3], b: &[f32; 3]| {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        dx * dx + dy * dy
    };
    let nearest_idx = |pts: &[[f32; 3]], q: &[f32; 3]| {
        let mut best = 0usize;
        let mut bd = f32::MAX;
        for (i, p) in pts.iter().enumerate() {
            let d = d2(p, q);
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    };
    let of = op.first().unwrap();
    let ol = op.last().unwrap();
    let ef = ep.first().unwrap();
    let el = ep.last().unwrap();
    let tail: Vec<[f32; 3]> = if d2(ol, el) > 1e-10 {
        let i = nearest_idx(&ep, ol);
        ep[i..].to_vec()
    } else if d2(of, ef) > 1e-10 {
        let i = nearest_idx(&ep, of);
        ep[..=i].to_vec()
    } else {
        return None;
    };
    if tail.len() < 2 {
        return None;
    }
    let mut w = WireModel::solid(name.into(), tail, [0.3, 1.0, 1.0, 1.0], false);
    w.pattern_length = 0.8;
    w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    w.line_weight_px = 1.6;
    Some(w)
}

/// Distance from `q` to the drawn (sampled) body of `e`.

pub(crate) fn dist_to_drawn(e: &EntityType, q: [f64; 2]) -> f64 {
    let pts = fence_sample_xy(e);
    let mut best = f64::MAX;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 1e-12 {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let px = a[0] + dx * t;
        let py = a[1] + dy * t;
        best = best.min((q[0] - px).hypot(q[1] - py));
    }
    best
}

/// Endpoints of the surviving pieces that are NOT endpoints of the original —
/// i.e. the actual cut points of a trim result.

pub(crate) fn piece_cut_points(orig: &EntityType, pieces: &[EntityType]) -> Vec<[f64; 2]> {
    let op = entity_pts(orig);
    if op.len() < 2 {
        return vec![];
    }
    let f = op.first().unwrap();
    let l = op.last().unwrap();
    let closed = (f[0] - l[0]).abs() < 1e-6 && (f[1] - l[1]).abs() < 1e-6;
    let ends: Vec<[f32; 3]> = if closed { vec![] } else { vec![*f, *l] };
    let mut out = Vec::new();
    for pe in pieces {
        let pp = entity_pts(pe);
        if pp.len() < 2 {
            continue;
        }
        for q in [pp.first().unwrap(), pp.last().unwrap()] {
            let is_orig_end = ends.iter().any(|e| {
                let dx = e[0] - q[0];
                let dy = e[1] - q[1];
                dx * dx + dy * dy < 1e-6
            });
            if !is_orig_end {
                out.push([q[0] as f64, q[1] as f64]);
            }
        }
    }
    out
}

/// Edge: Extend guides (#336): for every cut point that does NOT lie on a
/// boundary's drawn body, draw a dashed guide along the boundary's implied
/// extension — from its drawn end to the cut — so the user sees WHICH edge
/// causes the cut there. Lines get a straight guide, arcs follow the circle.

pub(crate) fn implied_cut_guides(
    all: &[EntityType],
    target: Handle,
    cuts: &[[f64; 2]],
) -> Vec<WireModel> {
    let mut out = Vec::new();
    'cuts: for (ci, cp) in cuts.iter().enumerate() {
        let tol = 1e-6 * (1.0 + cp[0].abs() + cp[1].abs());
        // Attributed to a drawn body → nothing implied to explain.
        for e in all {
            let h = e.common().handle;
            if h.is_null() || h == target {
                continue;
            }
            if dist_to_drawn(e, *cp) < tol {
                continue 'cuts;
            }
        }
        // Find the boundary whose extrapolation passes through the cut.
        for e in all {
            let h = e.common().handle;
            if h.is_null() || h == target {
                continue;
            }
            match e {
                EntityType::Line(l) => {
                    let (ax, ay) = (l.start.x, l.start.y);
                    let (dx, dy) = (l.end.x - ax, l.end.y - ay);
                    let len = dx.hypot(dy);
                    if len < 1e-9 {
                        continue;
                    }
                    // Perpendicular distance to the infinite line.
                    let d = ((cp[0] - ax) * dy - (cp[1] - ay) * dx).abs() / len;
                    if d < tol {
                        let d_start = (cp[0] - ax).hypot(cp[1] - ay);
                        let d_end = (cp[0] - l.end.x).hypot(cp[1] - l.end.y);
                        let from = if d_start < d_end {
                            [ax as f32, ay as f32, 0.0]
                        } else {
                            [l.end.x as f32, l.end.y as f32, 0.0]
                        };
                        let mut w = WireModel::solid(
                            format!("edge_guide_{ci}"),
                            vec![from, [cp[0] as f32, cp[1] as f32, 0.0]],
                            OPT_YELLOW,
                            false,
                        );
                        w.pattern_length = 0.8;
                        w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                        out.push(w);
                        continue 'cuts;
                    }
                }
                EntityType::Arc(a) => {
                    let r = (cp[0] - a.center.x).hypot(cp[1] - a.center.y);
                    if (r - a.radius).abs() < tol {
                        // Follow the circle from the nearer drawn end to the
                        // cut, going the short way outside the drawn span.
                        let theta = (cp[1] - a.center.y).atan2(cp[0] - a.center.x);
                        let a0 = norm(a.start_angle);
                        let a1 = norm(a.end_angle);
                        let ccw_from_end = (theta - a1).rem_euclid(TAU);
                        let cw_from_start = (a0 - theta).rem_euclid(TAU);
                        let (base, sweep) = if ccw_from_end <= cw_from_start {
                            (a1, ccw_from_end)
                        } else {
                            (a0, -cw_from_start)
                        };
                        let steps = ((sweep.abs() * 16.0).ceil() as usize).max(2);
                        let pts: Vec<[f32; 3]> = (0..=steps)
                            .map(|i| {
                                let ang = base + sweep * (i as f64 / steps as f64);
                                [
                                    (a.center.x + a.radius * ang.cos()) as f32,
                                    (a.center.y + a.radius * ang.sin()) as f32,
                                    0.0,
                                ]
                            })
                            .collect();
                        let mut w =
                            WireModel::solid(format!("edge_guide_{ci}"), pts, OPT_YELLOW, false);
                        w.pattern_length = 0.8;
                        w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                        out.push(w);
                        continue 'cuts;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Extend hover preview: the whole result in cyan, the added tail dashed and
/// — with Edge: Extend on — a guide along the implied boundary that the new
/// endpoint lands on.

pub(crate) fn extend_hover_wires(
    orig: &EntityType,
    ext: &EntityType,
    all: &[EntityType],
    target: Handle,
    implied_edges: bool,
) -> Vec<WireModel> {
    let mut out = vec![WireModel::solid(
        "extend_prev".into(),
        entity_pts(ext),
        WireModel::CYAN,
        false,
    )];
    if let Some(t) = extend_tail_preview(orig, ext, "extend_tail") {
        out.push(t);
    }
    if implied_edges {
        // The moved endpoint is the cut against the (implied) boundary.
        let cuts = piece_cut_points(orig, std::slice::from_ref(ext));
        out.extend(implied_cut_guides(all, target, &cuts));
    }
    out
}
