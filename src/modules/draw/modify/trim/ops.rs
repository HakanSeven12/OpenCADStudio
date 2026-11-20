use super::*;

pub(crate) fn trim_ellipse(orig: &EllipseEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 {
        t1 += TAU;
    }
    let span = t1 - t0;
    let angle_at = |t: f64| t0 + span * t;

    // A closed ellipse has no ends for `trim_intervals` to anchor on. Handing
    // it one makes it invent cuts at the parameter seam, so the survivor comes
    // back as two arcs either side of the seam instead of one joined across
    // it — and if the click lands in the wrapping piece, the wrong side goes.
    // Circles avoid this by treating the cuts cyclically; see `trim_circle`.
    if (span - TAU).abs() < 1e-9 {
        if ts.len() < 2 {
            return vec![];
        }
        let click = t_click.rem_euclid(1.0);
        // The gap holding the click, with the last one wrapping past 1.0 back
        // to the first cut.
        let gap = (0..ts.len()).find_map(|i| {
            let from = ts[i];
            let to = if i + 1 < ts.len() {
                ts[i + 1]
            } else {
                ts[0] + 1.0
            };
            let holds = |t: f64| t >= from && t < to;
            (holds(click) || holds(click + 1.0)).then_some((from, to))
        });
        let Some((from, to)) = gap else {
            return vec![];
        };
        // The survivor runs from the far edge of the removed gap all the way
        // round to its near edge. Leaving the end below the start is what
        // signals the wrap to everything downstream.
        let mut e = orig.clone();
        e.common.handle = Handle::NULL;
        e.start_parameter = angle_at(to % 1.0);
        e.end_parameter = angle_at(from % 1.0);
        return vec![EntityType::Ellipse(e)];
    }

    let major_radius = orig.major_axis.x.hypot(orig.major_axis.y);
    if major_radius < 1e-9 {
        return vec![];
    }
    let curve = Curve::Ellipse(KernelEllipseArc {
        ellipse: KernelEllipse {
            centre: [orig.center.x, orig.center.y],
            major_radius,
            minor_radius: major_radius * orig.minor_axis_ratio,
            major_axis: [
                orig.major_axis.x / major_radius,
                orig.major_axis.y / major_radius,
            ],
        },
        start_parameter: t0,
        end_parameter: t1,
    });
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 {
                return None;
            }
            let mut e = orig.clone();
            e.common.handle = Handle::NULL;
            e.start_parameter = angle_at(ta);
            e.end_parameter = angle_at(tb);
            Some(EntityType::Ellipse(e))
        })
        .collect()
}

/// Extend an Ellipse arc to the nearest boundary (along the arc direction).

pub(crate) fn extend_ellipse(orig: &EllipseEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 {
        t1 += TAU;
    }
    let span = t1 - t0;
    let a = (orig.major_axis.x.powi(2) + orig.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 {
        return None;
    }
    let b = a * orig.minor_axis_ratio;
    let (nx, ny) = (orig.major_axis.x / a, orig.major_axis.y / a);
    let cx = orig.center.x;
    let cy = orig.center.y;
    let ts = ellipse_seg_ts(cx, cy, a, b, nx, ny, t0, t1, orig.common.handle, geos);
    let extend_end = t_click >= 0.5;

    let best = if extend_end {
        ts.into_iter()
            .filter(|&t| t > 1.0 + 1e-6)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    } else {
        ts.into_iter()
            .filter(|&t| t < -1e-6)
            .max_by(|x, y| x.partial_cmp(y).unwrap())
    };

    let best_t = best?;
    let new_param = t0 + span * best_t;
    let mut e = orig.clone();
    e.common.handle = Handle::NULL;
    if extend_end {
        e.end_parameter = new_param;
    } else {
        e.start_parameter = new_param;
    }
    Some(EntityType::Ellipse(e))
}

// ── Spline trim / extend ──────────────────────────────────────────────────

/// Trim a Spline entity. Returns surviving spline pieces (one or two).

pub(crate) fn trim_spline(spl: &SplineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let Some(curve) = spline_to_nurbs(spl) else {
        return vec![];
    };
    let (t0, t1) = curve.domain();
    let curve = Curve::Nurbs(curve);

    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let t_lo = t0 + ta * (t1 - t0);
            let t_hi = t0 + tb * (t1 - t0);
            if t_hi - t_lo < 1e-9 {
                return None;
            }
            // Cut away everything before the interval, then everything after
            // it, leaving [t_lo, t_hi]. Either cut is a no-op when the
            // interval already reaches that end of the curve — dropping the
            // piece there would delete the half the user meant to keep.
            let after_low = match spline_cut(spl, t_lo) {
                Some((_, right)) => right,
                None => spl.clone(),
            };
            let kept = match spline_cut(&after_low, t_hi) {
                Some((middle, _)) => middle,
                // The interval reaches the far end, so nothing is left to cut.
                None => after_low,
            };
            Some(EntityType::Spline(kept))
        })
        .collect()
}

/// Extend a Spline toward the nearest boundary (nearest endpoint to pick).

pub(crate) fn extend_spline(spl: &SplineEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    // Sample spline and treat it like a polyline; look for intersections beyond
    // the current start (t<0 virtual) or end (t>1 virtual).
    // For splines we simply find whether the start (t=0) or end (t=1) is closer
    // to the click, then walk along that tangent direction to the nearest boundary.
    let curve = spline_to_nurbs(spl)?;
    let (t0, t1) = curve.domain();
    let extend_end = t_click >= 0.5;

    // Tangent at the endpoint (numerical, Δ = 1e-4 of range)
    let delta = (t1 - t0) * 1e-4;
    let (ep_t, tang_dir) = if extend_end {
        let p0 = curve.point_at_knot(t1 - delta);
        let p1 = curve.point_at_knot(t1);
        (t1, [p1[0] - p0[0], p1[1] - p0[1]])
    } else {
        let p0 = curve.point_at_knot(t0);
        let p1 = curve.point_at_knot(t0 + delta);
        (t0, [p0[0] - p1[0], p0[1] - p1[1]]) // reverse for "before start"
    };
    let ep = curve.point_at_knot(ep_t);
    let (dx, dy) = (tang_dir[0], tang_dir[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return None;
    }
    let (dx, dy) = (dx / len, dy / len);

    // Shoot a real ray from the endpoint along the tangent. The direction is a
    // unit vector, so the parameter that comes back is the distance to the
    // boundary — no stand-in length to pick, and nothing beyond it to miss.
    let shot = Curve::Ray(KernelRay {
        origin: ep,
        direction: [dx, dy],
    });
    let best_t = cut_params(&shot, spl.common.handle, geos)
        .into_iter()
        .filter(|&t| t > 1e-6)
        .reduce(f64::min)?;

    let hit_x = ep[0] + best_t * dx;
    let hit_y = ep[1] + best_t * dy;

    // Add a new control point at the hit location by appending/prepending.
    let z = spl.control_points.first().map(|v| v.z).unwrap_or(0.0);
    let mut new_spl = spl.clone();
    new_spl.common.handle = Handle::NULL;
    new_spl.fit_points.clear();
    if extend_end {
        new_spl
            .control_points
            .push(document::types::Vector3::new(hit_x, hit_y, z));
    } else {
        new_spl
            .control_points
            .insert(0, document::types::Vector3::new(hit_x, hit_y, z));
    }
    // Rebuild knots (uniform) for the extended control polygon.
    let degree = new_spl.degree as usize;
    let n = new_spl.control_points.len();
    new_spl.knots = clamped_uniform_knots(degree, n);
    Some(EntityType::Spline(new_spl))
}

// ── Trim helpers ──────────────────────────────────────────────────────────

pub(crate) fn trim_intervals(curve: &Curve, ts: &[f64], t_click: f64) -> Vec<(f64, f64)> {
    kernel_trim_spans(curve, ts, t_click, KernelTolerance::new(CUT_TOLERANCE))
        .into_iter()
        .map(|span| (span[0], span[1]))
        .collect()
}

/// Trim a Line entity. Returns the surviving line segments.

pub(crate) fn trim_line(orig: &LineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let p1 = [orig.start.x, orig.start.y];
    let p2 = [orig.end.x, orig.end.y];
    let z = orig.start.z;
    let curve = Curve::Line(KernelLine { start: p1, end: p2 });
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let a = lerp2(p1, p2, ta);
            let b = lerp2(p1, p2, tb);
            if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-6 {
                return None;
            }
            let mut l = orig.clone();
            l.common.handle = Handle::NULL;
            l.start = Vector3::new(a[0], a[1], z);
            l.end = Vector3::new(b[0], b[1], z);
            Some(EntityType::Line(l))
        })
        .collect()
}

/// Return the part removed by this trim pick, found as the gap containing the
/// cursor parameter between the surviving line segments.

pub(crate) fn extend_arc(orig: &ArcEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let cx = orig.center.x;
    let cy = orig.center.y;
    let r = orig.radius;
    if r < 1e-9 {
        return None;
    }
    let a0 = orig.start_angle;
    let a1 = orig.end_angle;
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    if span >= TAU - 1e-9 {
        return None; // already a full circle
    }
    // Boundary crossings around the FULL circle, then re-expressed relative
    // to the arc's own span: t ∈ [0,1] lies on the arc, t > 1 walks CCW past
    // the end and wraps around toward the start.
    let ts: Vec<f64> = arc_seg_ts(cx, cy, r, a0, a0 + TAU, orig.common.handle, geos)
        .into_iter()
        .map(|t| t * TAU / span)
        .collect();
    let extend_end = t_click >= 0.5;
    let best = if extend_end {
        // First crossing CCW past the end.
        ts.iter()
            .copied()
            .filter(|&t| t > 1.0 + 1e-6)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    } else {
        // First crossing CW before the start — in wrapped span-relative
        // coordinates that is the LARGEST t outside the arc.
        ts.iter()
            .copied()
            .filter(|&t| t > 1.0 + 1e-6)
            .max_by(|x, y| x.partial_cmp(y).unwrap())
    }?;
    let ang = norm(a0) + span * best;
    let mut a = orig.clone();
    a.common.handle = Handle::NULL;
    if extend_end {
        a.end_angle = ang;
    } else {
        a.start_angle = ang;
    }
    Some(EntityType::Arc(a))
}

/// Trim an Arc entity. Returns the surviving arc segments.

pub(crate) fn trim_arc(orig: &ArcEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let a0 = orig.start_angle;
    let a1 = orig.end_angle;
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    let angle_at = |t: f64| norm(a0) + span * t;
    let curve = Curve::Arc(KernelArc {
        centre: [orig.center.x, orig.center.y],
        radius: orig.radius,
        start_angle: a0,
        end_angle: a1,
    });

    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 {
                return None;
            }
            let mut a = orig.clone();
            a.common.handle = Handle::NULL;
            a.start_angle = angle_at(ta);
            a.end_angle = angle_at(tb);
            Some(EntityType::Arc(a))
        })
        .collect()
}

/// Trim a clicked Circle. A full circle has no endpoints, so it needs ≥2
/// boundary crossings: the arc segment containing the click is removed and the
/// circle becomes a single Arc spanning the rest. With fewer than two crossings
/// there is nothing to cut, so an empty result leaves the circle unchanged.
///
/// `ts` are the cut parameters in [0,1) around the circle (angle / TAU), sorted.

pub(crate) fn trim_circle(orig: &CircleEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    if ts.len() < 2 {
        return vec![];
    }
    let tc = t_click.rem_euclid(1.0);
    // Find the cyclic gap (ta, tb) between adjacent cuts that holds the click;
    // the last gap wraps past 1.0 back to the first cut.
    let n = ts.len();
    let mut removed: Option<(f64, f64)> = None;
    for i in 0..n {
        let ta = ts[i];
        let tb = if i + 1 < n { ts[i + 1] } else { ts[0] + 1.0 };
        if (tc >= ta && tc < tb) || (tc + 1.0 >= ta && tc + 1.0 < tb) {
            removed = Some((ta, tb));
            break;
        }
    }
    let (ta, tb) = match removed {
        Some(g) => g,
        None => return vec![],
    };

    // Surviving arc runs CCW from the far edge of the removed gap all the way
    // around to its near edge.
    let mut arc = ArcEnt::new();
    arc.common = orig.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = orig.center;
    arc.radius = orig.radius;
    arc.thickness = orig.thickness;
    arc.normal = orig.normal;
    arc.start_angle = (tb % 1.0) * TAU;
    arc.end_angle = (ta % 1.0) * TAU;
    vec![EntityType::Arc(arc)]
}

/// Extract a sub-polyline between parameter `s0` and `s1` (`s0 < s1`).
/// Preserves and proportionally scales arc bulges on partial segments.

pub(crate) fn extract_sub_polyline(poly: &LwPolyline, s0: f64, s1: f64) -> Option<LwPolyline> {
    if s1 <= s0 + 1e-9 {
        return None;
    }
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    if seg_count == 0 {
        return None;
    }

    let vx = |i: usize| -> [f64; 2] {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    let mut raw_verts: Vec<(f64, f64, f64)> = Vec::new();
    let mut curr = s0;
    while curr < s1 - 1e-9 {
        let seg_idx_unwrapped = curr.floor() as usize;
        let next_int = (seg_idx_unwrapped + 1) as f64;
        let end_curr = s1.min(next_int);
        let seg = seg_idx_unwrapped % seg_count;
        let u_a = (curr - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        let u_b = (end_curr - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        if u_b > u_a + 1e-9 {
            let p0 = vx(seg);
            let p1 = vx(seg + 1);
            let b = seg_bulge(seg);
            let (start_pt, sub_bulge) = if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
                let sp = ba.sample(u_a);
                let sub_b = if (u_a - 0.0).abs() < 1e-9 && (u_b - 1.0).abs() < 1e-9 {
                    b
                } else {
                    let sweep = ba.sweep * (u_b - u_a);
                    (sweep * 0.25).tan()
                };
                (sp, sub_b)
            } else {
                let sp = [p0[0] + u_a * (p1[0] - p0[0]), p0[1] + u_a * (p1[1] - p0[1])];
                (sp, 0.0)
            };
            raw_verts.push((start_pt[0], start_pt[1], sub_bulge));
        }
        curr = end_curr;
    }

    // End point of the last sub-segment
    let end_pt = {
        let seg_idx_unwrapped = (s1 - 1e-9).floor() as usize;
        let seg = seg_idx_unwrapped % seg_count;
        let u = (s1 - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        let p0 = vx(seg);
        let p1 = vx(seg + 1);
        let b = seg_bulge(seg);
        if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
            ba.sample(u)
        } else {
            [p0[0] + u * (p1[0] - p0[0]), p0[1] + u * (p1[1] - p0[1])]
        }
    };
    raw_verts.push((end_pt[0], end_pt[1], 0.0));

    // Filter out coincident consecutive points to prevent degenerate zero-length segments
    let mut clean_verts: Vec<(f64, f64, f64)> = Vec::new();
    for (x, y, b) in raw_verts {
        if let Some(last) = clean_verts.last_mut() {
            if (last.0 - x).hypot(last.1 - y) < 1e-6 {
                if last.2 == 0.0 {
                    last.2 = b;
                }
                continue;
            }
        }
        clean_verts.push((x, y, b));
    }

    if let Some(last) = clean_verts.last_mut() {
        last.2 = 0.0;
    }

    if clean_verts.len() < 2 {
        return None;
    }

    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    new_poly.is_closed = false;
    new_poly.vertices = clean_verts
        .into_iter()
        .map(|(x, y, b)| {
            let mut v = LwVertex::from_coords(x, y);
            v.bulge = b;
            v
        })
        .collect();
    Some(new_poly)
}

/// Kernel curve for one LwPolyline segment.
///
/// The boolean tells whether the kernel curve runs opposite to the polyline's
/// own segment direction. Negative-bulge arcs are represented by the kernel as
/// a positive-sweep arc, so their intersection parameter has to be reversed
/// back to the polyline's local 0..1 parameter.

pub(crate) fn lwpoly_segment_curve(poly: &LwPolyline, segment: usize) -> Option<(Curve, bool)> {
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }

    let a = &poly.vertices[segment % n];
    let b = &poly.vertices[(segment + 1) % n];

    let p0 = [a.location.x, a.location.y];
    let p1 = [b.location.x, b.location.y];

    if (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-12 {
        return None;
    }

    if let Some(arc) = BulgeArc::from_bulge(p0, p1, a.bulge) {
        if arc.sweep >= 0.0 {
            Some((
                Curve::Arc(KernelArc {
                    centre: arc.center,
                    radius: arc.radius,
                    start_angle: arc.start_angle,
                    end_angle: arc.start_angle + arc.sweep,
                }),
                false,
            ))
        } else {
            Some((
                Curve::Arc(KernelArc {
                    centre: arc.center,
                    radius: arc.radius,
                    start_angle: arc.end_angle,
                    end_angle: arc.end_angle - arc.sweep,
                }),
                true,
            ))
        }
    } else {
        Some((Curve::Line(KernelLine { start: p0, end: p1 }), false))
    }
}

/// Global polyline parameters at which non-adjacent segments of the same
/// polyline intersect.
///
/// Segment neighbours are deliberately excluded: their common vertex is a
/// normal polyline vertex, not a self-intersection. A polyline cuts itself
/// only when it is one of the cutting edges.

pub(crate) fn lwpoly_self_cut_params(poly: &LwPolyline, geos: &[Geo]) -> Vec<f64> {
    let n = poly.vertices.len();
    if n < 3 || !geos.iter().any(|geo| geo.handle() == poly.common.handle) {
        return Vec::new();
    }

    let closed = poly.is_closed;
    let segment_count = if closed { n } else { n - 1 };
    let total = segment_count as f64;

    let mut cuts = Vec::new();
    // Self-cuts previously reconstructed the arc/line for segment j inside
    // the nested O(N²) loop. Precompute every curve and its conservative
    // envelope once; index the envelopes so only spatially overlapping
    // non-neighbour pairs reach the exact intersection kernel.
    let curves: Vec<_> = (0..segment_count)
        .map(|i| lwpoly_segment_curve(poly, i))
        .collect();
    let spatial = TrimAabbIndex::new(curves.iter().map(|entry| {
        entry
            .as_ref()
            .and_then(|(curve, _)| trim_target_bounds(curve))
    }));
    for i in 0..segment_count {
        let Some((curve_a, reverse_a)) = curves[i].as_ref() else {
            continue;
        };
        let candidate_segments = trim_target_bounds(curve_a)
            .map(|bounds| spatial.overlapping(bounds))
            .unwrap_or_else(|| (0..segment_count).collect());

        for j in candidate_segments {
            if j <= i {
                continue;
            }
            // Ordinary neighbouring segments meet at their common vertex.
            let adjacent = j == i + 1 || (closed && i == 0 && j + 1 == segment_count);
            if adjacent {
                continue;
            }
            let Some((curve_b, reverse_b)) = curves[j].as_ref() else {
                continue;
            };
            let hits = kernel_intersect(curve_a, curve_b, KernelTolerance::new(CUT_TOLERANCE));

            for hit in hits {
                let mut ua = hit.t_a;
                let mut ub = hit.t_b;

                if *reverse_a {
                    ua = 1.0 - ua;
                }
                if *reverse_b {
                    ub = 1.0 - ub;
                }

                if !ua.is_finite()
                    || !ub.is_finite()
                    || ua < -1e-7
                    || ua > 1.0 + 1e-7
                    || ub < -1e-7
                    || ub > 1.0 + 1e-7
                {
                    continue;
                }

                ua = ua.clamp(0.0, 1.0);
                ub = ub.clamp(0.0, 1.0);

                // If both sides only touch at existing vertices, those
                // vertices already act as trim limits below.
                let a_endpoint = ua <= 1e-7 || ua >= 1.0 - 1e-7;
                let b_endpoint = ub <= 1e-7 || ub >= 1.0 - 1e-7;

                if a_endpoint && b_endpoint {
                    continue;
                }

                let pa = i as f64 + ua;
                let pb = j as f64 + ub;

                cuts.push(if closed { pa.rem_euclid(total) } else { pa });

                cuts.push(if closed { pb.rem_euclid(total) } else { pb });
            }
        }
    }

    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    cuts
}

/// Trim a clicked LwPolyline: remove the portion containing the click, bounded
/// by the nearest boundary intersections on each side. A closed polyline needs
/// ≥2 cuts and becomes an open polyline (the surviving arc); an open one yields
/// the surviving piece(s). Bulges on surviving arc segments are preserved
/// with proportionally scaled curvature.

pub(crate) fn lwpoly_cut_params(poly: &LwPolyline, geos: &[Geo]) -> Vec<f64> {
    let started = crate::perf::enabled().then(iced::time::Instant::now);
    let handle = poly.common.handle;
    let n = poly.vertices.len();
    if n < 2 {
        return Vec::new();
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;
    let vx = |i: usize| -> [f64; 2] {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    // External boundary cuts as global params (segment index + local u).
    let mut cuts: Vec<f64> = Vec::new();
    // On a large low-poly drawing the same tens of thousands of boundaries
    // were scanned again for *every* target vertex. Index once per target
    // and narrow each segment's exact kernel pass to overlapping boundaries.
    let cutter_index = (seg_count >= 8 && geos.len() >= 256)
        .then(|| TrimAabbIndex::new(geos.iter().map(trim_geo_aabb)));
    let mut local_geos = Vec::new();

    for i in 0..seg_count {
        let p0 = vx(i);
        let p1 = vx(i + 1);
        let b = seg_bulge(i);
        let segment_geos = if let Some(index) = cutter_index.as_ref() {
            // Use the full supporting arc envelope for bulged segments. The
            // same conservative bounds are applied by cut_params itself.
            let bounds = lwpoly_segment_curve(poly, i)
                .as_ref()
                .and_then(|(curve, _)| trim_target_bounds(curve));
            if let Some(bounds) = bounds {
                local_geos.clear();
                local_geos.extend(
                    index
                        .overlapping(bounds)
                        .into_iter()
                        .map(|idx| geos[idx].clone()),
                );
                local_geos.as_slice()
            } else {
                geos
            }
        } else {
            geos
        };
        for u in polyline_seg_ts(p0, p1, b, handle, segment_geos) {
            let param = i as f64 + u.clamp(0.0, 1.0);

            cuts.push(if closed {
                param.rem_euclid(total)
            } else {
                param
            });
        }
    }

    let external_ms = crate::perf::elapsed_ms(started);

    // A polyline can also cut itself. These are intersections between
    // non-adjacent segments belonging to this same LwPolyline. Its vertices
    // are not cuts: the trimmed piece runs on past corners to the next
    // cutting edge, and a polyline no edge crosses is left alone.
    cuts.extend(lwpoly_self_cut_params(poly, geos));

    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let total_ms = crate::perf::elapsed_ms(started);
    if total_ms >= 8.0 {
        crate::perf_record!(
            "[perf] trim-lwpoly-cuts {total_ms:.2}ms segments={seg_count} boundaries={} external={external_ms:.2}ms self={:.2}ms cuts={}",
            geos.len(),
            total_ms - external_ms,
            cuts.len(),
        );
    }
    cuts
}

/// Given fixed cutting points, only locate the clicked segment and construct
/// its surviving pieces. Reusing these points on hover avoids repeating the
/// expensive multi-segment kernel intersection pass on every cursor move.

pub(crate) fn trim_lwpolyline_with_cuts(
    poly: &LwPolyline,
    cx: f64,
    cy: f64,
    cuts: &[f64],
) -> Option<Vec<EntityType>> {
    let n = poly.vertices.len();
    if n < 2 || cuts.is_empty() {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;
    let vx = |i: usize| -> [f64; 2] {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    // Click param: nearest point on the polyline.
    let mut best = (f64::INFINITY, 0.0_f64);
    for i in 0..seg_count {
        let p0 = vx(i);
        let p1 = vx(i + 1);
        let b = seg_bulge(i);
        let (d, u) = if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
            let angle = (cy - ba.center[1]).atan2(cx - ba.center[0]);
            let (travelled, sweep) = if ba.sweep >= 0.0 {
                ((angle - ba.start_angle).rem_euclid(TAU), ba.sweep)
            } else {
                ((ba.start_angle - angle).rem_euclid(TAU), -ba.sweep)
            };
            let u = if sweep < 1e-12 {
                0.0
            } else if travelled <= sweep {
                (travelled / sweep).clamp(0.0, 1.0)
            } else {
                let gap = TAU - sweep;
                let past_end = travelled - sweep;
                if past_end <= gap * 0.5 {
                    1.0
                } else {
                    0.0
                }
            };
            let pt = ba.sample(u);
            let dist2 = (pt[0] - cx).powi(2) + (pt[1] - cy).powi(2);
            (dist2, u)
        } else {
            let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
            let len2 = dx * dx + dy * dy;
            let u = if len2 > 1e-12 {
                (((cx - p0[0]) * dx + (cy - p0[1]) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let px = p0[0] + u * dx;
            let py = p0[1] + u * dy;
            let dist2 = (px - cx).powi(2) + (py - cy).powi(2);
            (dist2, u)
        };
        if d < best.0 {
            best = (d, i as f64 + u);
        }
    }
    let t_click = best.1;

    let mut out: Vec<EntityType> = Vec::new();
    if closed {
        if cuts.len() < 2 {
            return None;
        }
        let n = cuts.len();
        let mut removed = None;
        for i in 0..n {
            let a = cuts[i];
            let b = if i + 1 < n {
                cuts[i + 1]
            } else {
                cuts[0] + total
            };
            if (t_click >= a && t_click < b) || (t_click + total >= a && t_click + total < b) {
                removed = Some((a, b));
                break;
            }
        }
        let Some((lo, hi)) = removed else {
            return None;
        };
        let mut s1 = lo;
        while s1 <= hi {
            s1 += total;
        }
        if let Some(piece) = extract_sub_polyline(poly, hi, s1) {
            out.push(EntityType::LwPolyline(piece));
        }
    } else {
        let mut bounds = vec![0.0];
        bounds.extend(cuts.iter().copied());
        bounds.push(total);
        bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let n = bounds.len();
        let mut removed = None;
        for i in 0..n - 1 {
            let a = bounds[i];
            let b = bounds[i + 1];
            if (t_click >= a && t_click < b) || (i + 2 == n && t_click <= b) {
                removed = Some((a, b));
                break;
            }
        }
        let Some((lo, hi)) = removed else {
            return None;
        };
        if lo > 1e-6 {
            if let Some(piece) = extract_sub_polyline(poly, 0.0, lo) {
                out.push(EntityType::LwPolyline(piece));
            }
        }
        if hi < total - 1e-6 {
            if let Some(piece) = extract_sub_polyline(poly, hi, total) {
                out.push(EntityType::LwPolyline(piece));
            }
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub(crate) fn trim_lwpolyline(
    poly: &LwPolyline,
    cx: f64,
    cy: f64,
    geos: &[Geo],
) -> Option<Vec<EntityType>> {
    let cuts = lwpoly_cut_params(poly, geos);
    trim_lwpolyline_with_cuts(poly, cx, cy, &cuts)
}

// ── Extend helpers ────────────────────────────────────────────────────────

/// Extend the first or last segment of an LwPolyline to the nearest boundary.
/// Click point (DXF XY) determines which end to extend.
/// The nearest boundary crossing past one end of a segment.
///
/// `t` runs on the infinite line through `from` → `to`: zero at `from`, one
/// at `to`. Extending the far end wants the smallest `t` above one; extending
/// the near end wants the largest `t` below zero. `None` when nothing lies
/// that way.
///
/// The crossings come from `cut_params`, which is the same kernel call every
/// other cut in this file uses. What was here instead was a match over every
/// `Geo` variant with its own line-line, line-circle and line-ellipse test —
/// twice, once for a line and once for a polyline's terminal segment — and a
/// boundary spline was walked as sixty-four chords rather than intersected.

pub(crate) fn extension_hit(
    from: [f64; 2],
    to: [f64; 2],
    handle: Handle,
    geos: &[Geo],
    beyond_end: bool,
) -> Option<f64> {
    let direction = [to[0] - from[0], to[1] - from[1]];
    if direction[0].hypot(direction[1]) < 1e-9 {
        return None;
    }
    // An infinite line rather than the segment: the crossing being looked for
    // is by definition off the end of the drawn part.
    let ray = Curve::XLine(KernelXLine {
        base: from,
        direction,
    });
    let hits = cut_params(&ray, handle, geos);
    if beyond_end {
        hits.into_iter()
            .filter(|t| *t > 1.0 + 1e-6)
            .min_by(f64::total_cmp)
    } else {
        hits.into_iter()
            .filter(|t| *t < -1e-6)
            .max_by(f64::total_cmp)
    }
}

pub(crate) fn extend_lwpoly(
    poly: &LwPolyline,
    click_x: f64,
    click_y: f64,
    geos: &[Geo],
) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 2 || poly.is_closed {
        return None;
    }

    let first = &poly.vertices[0];
    let second = &poly.vertices[1];
    let last = &poly.vertices[n - 1];
    let prev = &poly.vertices[n - 2];

    let d_first = (first.location.x - click_x).hypot(first.location.y - click_y);
    let d_last = (last.location.x - click_x).hypot(last.location.y - click_y);
    let extend_end = d_last <= d_first;

    // A bulged terminal segment is a circular arc, not its chord. Extend it
    // on the same circle and update the bulge to represent the longer sweep.
    let segment = if extend_end { n - 2 } else { 0 };
    let start = &poly.vertices[segment];
    let end = &poly.vertices[segment + 1];
    if let Some(arc) = BulgeArc::from_bulge(
        [start.location.x, start.location.y],
        [end.location.x, end.location.y],
        start.bulge,
    ) {
        let sign = arc.sweep.signum();
        if sign == 0.0 {
            return None;
        }
        let terminal_angle = if extend_end {
            arc.start_angle + arc.sweep
        } else {
            arc.start_angle
        };
        // Crossings around the full circle from the terminal point, as CCW
        // fractions of a turn; a CW walk measures them from the far side.
        let ccw = extend_end == (sign > 0.0);
        let [cx, cy] = arc.center;
        let ts = arc_seg_ts(
            cx,
            cy,
            arc.radius,
            terminal_angle,
            terminal_angle + TAU,
            poly.common.handle,
            geos,
        );
        let travel = ts
            .into_iter()
            .map(|t| (if ccw { t } else { 1.0 - t }) * TAU)
            .filter(|&travel| travel > 1e-6)
            .min_by(f64::total_cmp)?;
        let end_angle = if ccw {
            terminal_angle + travel
        } else {
            terminal_angle - travel
        };
        let point = [
            cx + arc.radius * end_angle.cos(),
            cy + arc.radius * end_angle.sin(),
        ];
        // A bulge encodes a sweep shorter than one full revolution. Do not
        // manufacture an invalid/infinite bulge when the next crossing would
        // require a full circle or more.
        let new_sweep = arc.sweep.abs() + travel;
        if new_sweep >= TAU - 1e-6 {
            return None;
        }
        let mut new_poly = poly.clone();
        new_poly.common.handle = Handle::NULL;
        let new_bulge = (new_sweep * 0.25).tan() * sign;
        if extend_end {
            let vertex = new_poly.vertices.last_mut()?;
            vertex.location.x = point[0];
            vertex.location.y = point[1];
            new_poly.vertices[n - 2].bulge = new_bulge;
        } else {
            let vertex = new_poly.vertices.first_mut()?;
            vertex.location.x = point[0];
            vertex.location.y = point[1];
            new_poly.vertices[0].bulge = new_bulge;
        }
        return Some(EntityType::LwPolyline(new_poly));
    }

    // Extract the terminal straight segment as a virtual line.
    let (ax, ay, bx, by) = if extend_end {
        (
            prev.location.x,
            prev.location.y,
            last.location.x,
            last.location.y,
        )
    } else {
        (
            second.location.x,
            second.location.y,
            first.location.x,
            first.location.y,
        )
    };

    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return None;
    }

    let best_t = extension_hit([ax, ay], [bx, by], poly.common.handle, geos, true)?;

    let new_x = ax + best_t * dx;
    let new_y = ay + best_t * dy;
    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    if extend_end {
        let last_v = new_poly.vertices.last_mut()?;
        last_v.location.x = new_x;
        last_v.location.y = new_y;
    } else {
        let first_v = new_poly.vertices.first_mut()?;
        first_v.location.x = new_x;
        first_v.location.y = new_y;
    }
    Some(EntityType::LwPolyline(new_poly))
}

/// Extend a Line to the nearest boundary on the extended side.
/// t_click < 0.5 → extend start (look for t < 0); t_click ≥ 0.5 → extend end (t > 1).

pub(crate) fn extend_line(orig: &LineEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let ax = orig.start.x;
    let ay = orig.start.y;
    let bx = orig.end.x;
    let by = orig.end.y;
    let (dx, dy) = (bx - ax, by - ay);
    let extend_end = t_click >= 0.5;
    let best_t = extension_hit([ax, ay], [bx, by], orig.common.handle, geos, extend_end)?;

    let mut line = orig.clone();
    line.common.handle = Handle::NULL;
    let new_x = ax + best_t * dx;
    let new_y = ay + best_t * dy;
    if extend_end {
        line.end = Vector3::new(new_x, new_y, orig.end.z);
    } else {
        line.start = Vector3::new(new_x, new_y, orig.start.z);
    }
    Some(EntityType::Line(line))
}

/// Trim a Ray entity.
///
/// The parameter is the ray's own: `t = 0` at the base and advancing by one
/// `direction` per unit, unbounded above. A surviving piece that reaches
/// infinity stays a Ray; one with two ends becomes a Line.

pub(crate) fn trim_ray(orig: &RayEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    let pt = |t: f64| [bx + t * dx, by + t * dy, bz + t * dz];

    let curve = ray_curve(orig);
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            if tb.is_finite() && (pb[0] - pa[0]).hypot(pb[1] - pa[1]) < 1e-6 {
                return None;
            }

            if tb.is_infinite() {
                // Still extends to infinity → remains a Ray with new base
                let r = RayEnt::new(Vector3::new(pa[0], pa[1], pa[2]), Vector3::new(dx, dy, dz));
                let mut r = r;
                r.common = orig.common.clone();
                r.common.handle = Handle::NULL;
                Some(EntityType::Ray(r))
            } else {
                // Finite segment → Line
                let mut l = LineEnt {
                    common: orig.common.clone(),
                    ..LineEnt::new()
                };
                l.common.handle = Handle::NULL;
                l.start = Vector3::new(pa[0], pa[1], pa[2]);
                l.end = Vector3::new(pb[0], pb[1], pb[2]);
                Some(EntityType::Line(l))
            }
        })
        .collect()
}

/// Trim an XLine entity.
///
/// The parameter is the line's own: `t = 0` at the base point, advancing by
/// one `direction` per unit and running both ways without limit. A surviving
/// piece keeps whichever ends reach infinity — both, and it is still an XLine;
/// one, and it is a Ray; neither, and it is a Line.

pub(crate) fn trim_xline(orig: &XLineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    let pt = |t: f64| [bx + t * dx, by + t * dy, bz + t * dz];

    let curve = xline_curve(orig);
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            let ext_neg = ta.is_infinite();
            let ext_pos = tb.is_infinite();

            match (ext_neg, ext_pos) {
                (true, true) => {
                    // Whole XLine survived (shouldn't happen after a real trim)
                    let mut x = orig.clone();
                    x.common.handle = Handle::NULL;
                    Some(EntityType::XLine(x))
                }
                (true, false) => {
                    // Extends toward -infinity: Ray at pb pointing in -dir
                    let r = RayEnt::new(
                        Vector3::new(pb[0], pb[1], pb[2]),
                        Vector3::new(-dx, -dy, -dz),
                    );
                    let mut r = r;
                    r.common = orig.common.clone();
                    r.common.handle = Handle::NULL;
                    Some(EntityType::Ray(r))
                }
                (false, true) => {
                    // Extends toward +infinity: Ray at pa pointing in +dir
                    let r =
                        RayEnt::new(Vector3::new(pa[0], pa[1], pa[2]), Vector3::new(dx, dy, dz));
                    let mut r = r;
                    r.common = orig.common.clone();
                    r.common.handle = Handle::NULL;
                    Some(EntityType::Ray(r))
                }
                (false, false) => {
                    // Finite segment
                    let mut l = LineEnt {
                        common: orig.common.clone(),
                        ..LineEnt::new()
                    };
                    l.common.handle = Handle::NULL;
                    l.start = Vector3::new(pa[0], pa[1], pa[2]);
                    l.end = Vector3::new(pb[0], pb[1], pb[2]);
                    Some(EntityType::Line(l))
                }
            }
        })
        .collect()
}

// ── Point-generation helpers ──────────────────────────────────────────────

pub(crate) fn pick_trim_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<Vec<EntityType>> {
    let entity = all.iter().find(|e| e.common().handle == handle);
    let result: Option<Vec<EntityType>> = match entity {
        Some(EntityType::Line(l)) => {
            let ax = l.start.x;
            let ay = l.start.y;
            let bx = l.end.x;
            let by = l.end.y;
            let ts = line_seg_ts(ax, ay, bx, by, handle, geos);
            if ts.is_empty() {
                return None;
            }
            let dx = bx - ax;
            let dy = by - ay;
            let len2 = dx * dx + dy * dy;
            let t_click = if len2 > 1e-12 {
                ((px - ax) * dx + (py - ay) * dy) / len2
            } else {
                0.5
            };
            Some(trim_line(l, &ts, t_click))
        }
        Some(EntityType::Arc(a)) => {
            let cx = a.center.x;
            let cy = a.center.y;
            let a0 = a.start_angle;
            let a1 = a.end_angle;
            let ts = arc_seg_ts(cx, cy, a.radius, a0, a1, handle, geos);
            if ts.is_empty() {
                return None;
            }
            let click_angle = (py - cy).atan2(px - cx);
            let t_click = arc_t(click_angle, a0, a1);
            Some(trim_arc(a, &ts, t_click))
        }
        Some(EntityType::Circle(c)) => {
            let cx = c.center.x;
            let cy = c.center.y;
            let ts = arc_seg_ts(cx, cy, c.radius, 0.0, TAU, handle, geos);
            if ts.len() < 2 {
                return None;
            }
            let click_angle = (py - cy).atan2(px - cx);
            let t_click = arc_t(click_angle, 0.0, TAU);
            let survivors = trim_circle(c, &ts, t_click);
            if survivors.is_empty() {
                return None;
            }
            Some(survivors)
        }
        Some(EntityType::Ray(r)) => {
            let curve = ray_curve(r);
            let ts = cut_params(&curve, handle, geos);
            if ts.is_empty() {
                return None;
            }
            let t_click = curve.parameter_at([px, py]);
            Some(trim_ray(r, &ts, t_click))
        }
        Some(EntityType::XLine(x)) => {
            let curve = xline_curve(x);
            let ts = cut_params(&curve, handle, geos);
            if ts.is_empty() {
                return None;
            }
            let t_click = curve.parameter_at([px, py]);
            Some(trim_xline(x, &ts, t_click))
        }
        Some(EntityType::Ellipse(e)) => {
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 {
                return None;
            }
            let b = a * e.minor_axis_ratio;
            if b < 1e-9 {
                return None;
            }
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            let ts = ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos);
            if ts.is_empty() {
                return None;
            }
            // t_click: project mouse onto ellipse local param
            let rx = px - e.center.x;
            let ry = py - e.center.y;
            let xl = rx * nx + ry * ny;
            let yl = -rx * ny + ry * nx;
            let t_ell = ellipse_closest_parameter(a, b, xl, yl);
            let t_click = arc_t(t_ell, t0, t1);
            Some(trim_ellipse(e, &ts, t_click))
        }
        Some(EntityType::Spline(s)) => {
            let ts = spline_seg_ts(s, handle, geos);
            if ts.is_empty() {
                return None;
            }
            let t_click = spline_nearest_t(s, px, py)
                .and_then(|t_actual| {
                    let (t0, t1) = spline_range(s)?;
                    Some(t_to_rel(t_actual, t0, t1))
                })
                .unwrap_or(0.5);
            Some(trim_spline(s, &ts, t_click))
        }
        Some(EntityType::LwPolyline(p)) => trim_lwpolyline(p, px, py, geos),
        Some(EntityType::MLine(m)) => {
            super::super::mledit::trim_mline_at(m, DVec3::new(px, py, 0.0), |a, b| {
                line_seg_ts(a.x, a.y, b.x, b.y, handle, geos)
            })
            .map(|trimmed| vec![EntityType::MLine(trimmed)])
        }
        Some(EntityType::Hatch(hatch)) => trim_hatch_at(hatch, geos, px, py)
            .map(|result| vec![EntityType::Hatch(Box::new(result))]),
        _ => None,
    };
    result
}

/// Quick-mode trim also removes a valid line when it has no boundary
/// intersections. Standard mode continues to treat that pick as a no-op.

/// Quick-mode trim at a click/crossing point: the surviving pieces, or `None`
/// when the pick doesn't intersect any boundary (or the type is unsupported).
pub(crate) fn quick_trim_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<Vec<EntityType>> {
    if let Some(EntityType::Line(line)) = all.iter().find(|e| e.common().handle == handle) {
        let dx = line.end.x - line.start.x;
        let dy = line.end.y - line.start.y;
        if dx.hypot(dy) > 1e-9
            && line_seg_ts(
                line.start.x,
                line.start.y,
                line.end.x,
                line.end.y,
                handle,
                geos,
            )
            .is_empty()
        {
            return Some(Vec::new());
        }
    }
    pick_trim_at(all, geos, handle, px, py)
}

/// Quick-mode extend at a click/crossing point: the lengthened entity, or
/// `None` when no boundary lies beyond that end (or the type is unsupported).

pub(crate) fn pick_extend_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<EntityType> {
    let entity = all.iter().find(|e| e.common().handle == handle);
    let result: Option<EntityType> = match entity {
        Some(EntityType::Line(l)) => {
            let ax = l.start.x;
            let ay = l.start.y;
            let bx = l.end.x;
            let by = l.end.y;
            let dx = bx - ax;
            let dy = by - ay;
            let len2 = dx * dx + dy * dy;
            let t_click = if len2 > 1e-12 {
                ((px - ax) * dx + (py - ay) * dy) / len2
            } else {
                0.5
            };
            extend_line(l, t_click, geos)
        }
        Some(EntityType::Arc(a)) => {
            let ang = (py - a.center.y).atan2(px - a.center.x);
            let t_click = arc_t(ang, a.start_angle, a.end_angle);
            extend_arc(a, t_click, geos)
        }
        Some(EntityType::Ellipse(e)) => {
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            let span = t1 - t0;
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 {
                return None;
            }
            let b = a * e.minor_axis_ratio;
            if b < 1e-9 {
                return None;
            }
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let rx = px - e.center.x;
            let ry = py - e.center.y;
            let xl = rx * nx + ry * ny;
            let yl = -rx * ny + ry * nx;
            let t_ell = ellipse_closest_parameter(a, b, xl, yl);
            let t_click = arc_t(t_ell, t0, t1);
            let _ = span;
            extend_ellipse(e, t_click, geos)
        }
        Some(EntityType::LwPolyline(p)) => extend_lwpoly(p, px, py, geos),
        Some(EntityType::Spline(s)) => {
            let t_click = spline_nearest_t(s, px, py)
                .and_then(|t_actual| {
                    let (t0, t1) = spline_range(s)?;
                    Some(t_to_rel(t_actual, t0, t1))
                })
                .unwrap_or(0.5);
            extend_spline(s, t_click, geos)
        }
        _ => None,
    };
    result
}
