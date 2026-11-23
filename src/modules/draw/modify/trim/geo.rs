use super::*;

/// If a trim interval endpoint is beyond this threshold it is treated as "infinite".

#[derive(Clone)]
pub(crate) enum Geo {
    Line {
        handle: Handle,
        p1: [f64; 2],
        p2: [f64; 2],
    },
    Arc {
        handle: Handle,
        cx: f64,
        cy: f64,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Circle {
        handle: Handle,
        cx: f64,
        cy: f64,
        r: f64,
    },
    /// Semi-infinite line from base in +direction.
    Ray {
        handle: Handle,
        bx: f64,
        by: f64,
        dx: f64,
        dy: f64,
    },
    /// Fully-infinite line through base along direction.
    InfLine {
        handle: Handle,
        bx: f64,
        by: f64,
        dx: f64,
        dy: f64,
    },
    /// Ellipse arc: center, semi-axes, unit major-axis direction, parameter range [t0,t1].
    Ellipse {
        handle: Handle,
        cx: f64,
        cy: f64,
        a: f64,  // semi-major
        b: f64,  // semi-minor
        nx: f64, // unit major-axis X
        ny: f64, // unit major-axis Y
        t0: f64, // start parameter
        t1: f64, // end parameter (may be > 2π if wrapped)
    },
    /// A NURBS boundary, carried exactly.
    Spline {
        handle: Handle,
        curve: Box<NurbsCurve>,
    },
}

impl Geo {
    pub(crate) fn handle(&self) -> Handle {
        match self {
            Self::Line { handle, .. }
            | Self::Arc { handle, .. }
            | Self::Circle { handle, .. }
            | Self::Ray { handle, .. }
            | Self::InfLine { handle, .. }
            | Self::Ellipse { handle, .. }
            | Self::Spline { handle, .. } => *handle,
        }
    }

    pub(crate) fn endpoints(&self) -> Vec<[f64; 2]> {
        match self {
            Self::Line { p1, p2, .. } => vec![*p1, *p2],
            Self::Arc {
                cx, cy, r, a0, a1, ..
            } => vec![
                [cx + r * a0.cos(), cy + r * a0.sin()],
                [cx + r * a1.cos(), cy + r * a1.sin()],
            ],
            Self::Ray { bx, by, .. } => vec![[*bx, *by]],
            Self::Ellipse {
                cx,
                cy,
                a,
                b,
                nx,
                ny,
                t0,
                t1,
                ..
            } => {
                if (t1 - t0).abs() < TAU - 1e-6 {
                    let pt_at = |t: f64| {
                        let ct = t.cos();
                        let st = t.sin();
                        [
                            cx + nx * a * ct - ny * b * st,
                            cy + ny * a * ct + nx * b * st,
                        ]
                    };
                    vec![pt_at(*t0), pt_at(*t1)]
                } else {
                    vec![]
                }
            }
            Self::Spline { curve, .. } => vec![curve.point_at(0.0), curve.point_at(1.0)],
            Self::Circle { .. } | Self::InfLine { .. } => vec![],
        }
    }
}

pub(crate) fn geo_overlaps_aabb(geo: &Geo, target: [f64; 4]) -> bool {
    let bounds = match geo {
        Geo::Spline { curve, .. } => {
            let samples = (0..=32).map(|i| curve.point_at(i as f64 / 32.0));
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for point in samples {
                bounds[0] = bounds[0].min(point[0]);
                bounds[1] = bounds[1].min(point[1]);
                bounds[2] = bounds[2].max(point[0]);
                bounds[3] = bounds[3].max(point[1]);
            }
            bounds
        }
        // Unbounded boundaries are always retained in the nearby set.
        _ => match trim_geo_aabb(geo) {
            Some(bounds) => bounds,
            None => return true,
        },
    };
    bounds[0] <= target[2]
        && bounds[2] >= target[0]
        && bounds[1] <= target[3]
        && bounds[3] >= target[1]
}

/// A conservative XY envelope for bounded cutter targets. Arc and ellipse
/// sweeps use their entire supporting circle, deliberately over-including
/// candidates so tangent and near-endpoint contacts are never lost.

pub(crate) fn trim_target_bounds(curve: &Curve) -> Option<[f64; 4]> {
    let bounds = match curve {
        Curve::Line(line) => [
            line.start[0].min(line.end[0]),
            line.start[1].min(line.end[1]),
            line.start[0].max(line.end[0]),
            line.start[1].max(line.end[1]),
        ],
        Curve::Arc(arc) => square_around(arc.centre, arc.radius.abs()),
        Curve::Circle(circle) => square_around(circle.centre, circle.radius.abs()),
        Curve::Ellipse(arc) => {
            let ellipse = &arc.ellipse;
            let r = ellipse.major_radius.abs().max(ellipse.minor_radius.abs());
            square_around(ellipse.centre, r)
        }
        // Rays/XLines must find distant intersections. NURBS do not have
        // a cheap guaranteed conservative envelope at this call site.
        _ => return None,
    };
    if !bounds.iter().all(|v| v.is_finite()) {
        return None;
    }
    let slack = CUT_TOLERANCE * 8.0;
    Some([
        bounds[0] - slack,
        bounds[1] - slack,
        bounds[2] + slack,
        bounds[3] + slack,
    ])
}

/// The XY box of a circle around `centre`.

pub(crate) fn square_around(centre: [f64; 2], r: f64) -> [f64; 4] {
    [centre[0] - r, centre[1] - r, centre[0] + r, centre[1] + r]
}

/// A static interval broad phase for the geometry of one TRIM operation.
/// Sort by minimum X once; the prefix maximum X permits binary-searching
/// *past* boundaries that finish before the target, not just those that start
/// after it. Unlike a per-segment full scan, typical queries cost O(log N + K)
/// when thousands of low-poly segments are spread across the drawing.
///
/// Entries without a safe finite box (splines, rays, infinite lines) are
/// unconditionally retained to preserve exact kernel semantics.

pub(crate) struct TrimAabbIndex {
    entries: Vec<(usize, [f64; 4])>,
    prefix_max_x: Vec<f64>,
    always: Vec<usize>,
}

impl TrimAabbIndex {
    pub(crate) fn new(bounds: impl IntoIterator<Item = Option<[f64; 4]>>) -> Self {
        let mut entries = Vec::new();
        let mut always = Vec::new();
        for (index, bounds) in bounds.into_iter().enumerate() {
            match bounds.filter(|box_| box_.iter().all(|value| value.is_finite())) {
                Some(bounds) => entries.push((index, bounds)),
                None => always.push(index),
            }
        }
        entries.sort_unstable_by(|a, b| a.1[0].total_cmp(&b.1[0]));
        let mut prefix_max_x = Vec::with_capacity(entries.len());
        let mut max_x = f64::NEG_INFINITY;
        for (_, bounds) in &entries {
            max_x = max_x.max(bounds[2]);
            prefix_max_x.push(max_x);
        }
        Self {
            entries,
            prefix_max_x,
            always,
        }
    }

    pub(crate) fn overlapping(&self, target: [f64; 4]) -> Vec<usize> {
        let end = self
            .entries
            .partition_point(|(_, aabb)| aabb[0] <= target[2]);
        let start = self.prefix_max_x[..end].partition_point(|x| *x < target[0]);
        let mut indices = self.always.clone();
        for &(index, aabb) in &self.entries[start..end] {
            if aabb[2] >= target[0] && aabb[1] <= target[3] && aabb[3] >= target[1] {
                indices.push(index);
            }
        }
        // The kernel/contact logic previously saw entities in document order.
        // Maintain that ordering for coincident cuts and endpoint priority.
        indices.sort_unstable();
        indices
    }
}

pub(crate) fn trim_geo_aabb(geo: &Geo) -> Option<[f64; 4]> {
    Some(match geo {
        Geo::Line { p1, p2, .. } => [
            p1[0].min(p2[0]),
            p1[1].min(p2[1]),
            p1[0].max(p2[0]),
            p1[1].max(p2[1]),
        ],
        Geo::Arc { cx, cy, r, .. } | Geo::Circle { cx, cy, r, .. } => {
            square_around([*cx, *cy], r.abs())
        }
        // Deliberately overestimate rotated ellipse bounds.
        Geo::Ellipse { cx, cy, a, b, .. } => square_around([*cx, *cy], a.abs() + b.abs()),
        Geo::Spline { .. } | Geo::Ray { .. } | Geo::InfLine { .. } => return None,
    })
}

pub fn is_trim_boundary_entity(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::Circle(_)
            | EntityType::Ellipse(_)
            | EntityType::Ray(_)
            | EntityType::XLine(_)
            | EntityType::Spline(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
    )
}

pub(crate) fn build_geos(entities: &[EntityType]) -> Vec<Geo> {
    build_geos_with_handle(entities, None)
}

/// Build boundary geometry while optionally attributing every segment to the
/// owning INSERT. Block contents are virtual TRIM boundaries; the INSERT stays
/// the real drawing entity and therefore remains untouched by the command.

pub(crate) fn build_geos_with_handle(entities: &[EntityType], owner: Option<Handle>) -> Vec<Geo> {
    let mut out = Vec::new();
    for e in entities {
        let h = owner.unwrap_or_else(|| e.common().handle);
        match e {
            // A polyline acts as a boundary through its constituent edges, so
            // a Line/Arc/… can be trimmed against it. Explode into Line + Arc
            // segments and tag each with the polyline's own handle (so trim
            // still excludes it as the click target).
            EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_) => {
                let before = out.len();
                for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
                    if let Some(g) = geo_from_entity(h, &seg) {
                        out.push(g);
                    }
                }
                // EXPLODE's LWPOLYLINE helper only materializes polylines
                // whose OCS plane is parallel to world XY. Imported files
                // can carry a tilted extrusion normal even when the entity is
                // intended to be used as a plan-view boundary. If that path
                // yields no usable edges, fall back to the shared curve
                // projection and use its tessellated segments as boundaries.
                if out.len() == before {
                    let points = projected_polyline_points_xy(e);
                    for pair in points.windows(2) {
                        if (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]) > 1e-12 {
                            out.push(Geo::Line {
                                handle: h,
                                p1: pair[0],
                                p2: pair[1],
                            });
                        }
                    }
                }
            }
            _ => {
                if let Some(g) = geo_from_entity(h, e) {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Convert a simple boundary entity (Line / Arc / Circle / Ray / XLine /
/// Ellipse / Spline) into a `Geo`, tagged with `h`. Returns `None` for types
/// that do not act as trim boundaries.

pub(crate) fn geo_from_entity(h: Handle, e: &EntityType) -> Option<Geo> {
    // The geometry itself comes from the one converter, in plan-view
    // coordinates. What is left here is only the shape of the boundary
    // record, which the edit options need to mutate — `imply_edge_geos` turns
    // a segment into an infinite line and opens an arc into a full circle,
    // neither of which a bare curve says.
    match entity_curve_xy(e)? {
        Curve::Line(line) => Some(Geo::Line {
            handle: h,
            p1: line.start,
            p2: line.end,
        }),
        Curve::Arc(arc) => Some(Geo::Arc {
            handle: h,
            cx: arc.centre[0],
            cy: arc.centre[1],
            r: arc.radius,
            a0: arc.start_angle,
            a1: arc.end_angle,
        }),
        Curve::Circle(circle) => Some(Geo::Circle {
            handle: h,
            cx: circle.centre[0],
            cy: circle.centre[1],
            r: circle.radius,
        }),
        Curve::Ray(ray) => Some(Geo::Ray {
            handle: h,
            bx: ray.origin[0],
            by: ray.origin[1],
            dx: ray.direction[0],
            dy: ray.direction[1],
        }),
        Curve::XLine(line) => Some(Geo::InfLine {
            handle: h,
            bx: line.base[0],
            by: line.base[1],
            dx: line.direction[0],
            dy: line.direction[1],
        }),
        Curve::Ellipse(arc) => {
            let ellipse = arc.ellipse;
            if ellipse.major_radius < 1e-9 {
                return None;
            }
            let t0 = arc.start_parameter;
            let mut t1 = arc.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            Some(Geo::Ellipse {
                handle: h,
                cx: ellipse.centre[0],
                cy: ellipse.centre[1],
                a: ellipse.major_radius,
                b: ellipse.minor_radius,
                nx: ellipse.major_axis[0],
                ny: ellipse.major_axis[1],
                t0,
                t1,
            })
        }
        Curve::Nurbs(curve) => Some(Geo::Spline {
            handle: h,
            curve: Box::new(curve),
        }),
        // A polyline reaches here already taken apart into its segments.
        Curve::Polyline(_) => None,
    }
}

// ── Intersection helpers ──────────────────────────────────────────────────

// ── Boundary crossings ────────────────────────────────────────────────────
//

// Every cut a trim or extend makes comes from one question: where does the
// entity being edited meet the boundaries picked as cutting edges. The kernel
// answers it for any pair of curves, so these four just describe the target as
// a `Curve` and hand it over.

/// The entity a boundary came from, so a curve is never cut by itself.
pub(crate) fn geo_handle(geo: &Geo) -> Handle {
    match geo {
        Geo::Line { handle, .. }
        | Geo::Arc { handle, .. }
        | Geo::Circle { handle, .. }
        | Geo::Ray { handle, .. }
        | Geo::InfLine { handle, .. }
        | Geo::Ellipse { handle, .. }
        | Geo::Spline { handle, .. } => *handle,
    }
}

/// A boundary as a kernel curve.

pub(crate) fn geo_to_curve(geo: &Geo) -> Option<Curve> {
    Some(match geo {
        Geo::Line { p1, p2, .. } => Curve::Line(KernelLine {
            start: *p1,
            end: *p2,
        }),
        Geo::Arc {
            cx, cy, r, a0, a1, ..
        } => Curve::Arc(KernelArc {
            centre: [*cx, *cy],
            radius: *r,
            start_angle: *a0,
            end_angle: *a1,
        }),
        Geo::Circle { cx, cy, r, .. } => Curve::Circle(KernelCircle {
            centre: [*cx, *cy],
            radius: *r,
        }),
        Geo::Ray { bx, by, dx, dy, .. } => Curve::Ray(KernelRay {
            origin: [*bx, *by],
            direction: [*dx, *dy],
        }),
        Geo::InfLine { bx, by, dx, dy, .. } => Curve::XLine(KernelXLine {
            base: [*bx, *by],
            direction: [*dx, *dy],
        }),
        Geo::Ellipse {
            cx,
            cy,
            a,
            b,
            nx,
            ny,
            t0,
            t1,
            ..
        } => Curve::Ellipse(KernelEllipseArc {
            ellipse: KernelEllipse {
                centre: [*cx, *cy],
                major_radius: *a,
                minor_radius: *b,
                major_axis: [*nx, *ny],
            },
            start_parameter: *t0,
            end_parameter: *t1,
        }),
        // Exactly, not through its sampling. The kernel intersects a NURBS by
        // subdivision against a bounding box, so a boundary spline cuts where
        // it actually runs rather than where a 64-chord approximation of it
        // does.
        Geo::Spline { curve, .. } => Curve::Nurbs((**curve).clone()),
    })
}

/// Where `target` is cut by every boundary except its own entity, as
/// parameters along it in `0..=1`.

pub(crate) fn cut_params(target: &Curve, handle: Handle, geos: &[Geo]) -> Vec<f64> {
    let tolerance = KernelTolerance::new(CUT_TOLERANCE);
    // A bounded target's parameters are pinned to its own span, as every
    // caller here expects. An unbounded one — a ray shot out to find the
    // nearest boundary — has to keep whatever it comes back with, since the
    // whole point is how far away the crossing is.
    let bounded = matches!(target.extent(), KernelExtent::Bounded);
    let target_bounds = bounded.then(|| trim_target_bounds(target)).flatten();
    // In imported CAD drawings several LINE entities can coincide exactly.
    // Intersecting every duplicate has no geometric benefit, and repeated
    // intersection allocations become especially costly during TRIM hover.
    // Exclude the picked entity before deduplicating: a coincident *other*
    // entity must still count as a valid cutting boundary.
    let mut seen_lines = std::collections::HashSet::new();
    let candidates: Vec<&Geo> = geos
        .iter()
        .filter(|geo| geo_handle(geo) != handle)
        .filter(|geo| {
            // The input may contain tens of thousands of low-poly INSERT
            // segments. Do the cheap conservative broad phase before kernel
            // intersection and endpoint-contact tests, for every LWPOLY
            // segment as well as LINE/ARC/CIRCLE hover paths.
            // Do not cull splines using their sampled AABB: a NURBS can
            // leave a sparse sample envelope between knot values.
            target_bounds.is_none_or(|bbox| {
                matches!(geo, Geo::Spline { .. }) || geo_overlaps_aabb(geo, bbox)
            })
        })
        .filter(|geo| match geo {
            Geo::Line { p1, p2, .. } => {
                let a = (p1[0].to_bits(), p1[1].to_bits());
                let b = (p2[0].to_bits(), p2[1].to_bits());
                seen_lines.insert(if a <= b { (a, b) } else { (b, a) })
            }
            _ => true,
        })
        .collect();
    let mut ts: Vec<f64> = candidates
        .iter()
        .copied()
        .filter_map(geo_to_curve)
        .flat_map(|boundary| {
            kernel_intersect(target, &boundary, tolerance)
                .into_iter()
                .map(|hit| {
                    if bounded {
                        hit.t_a.clamp(0.0, 1.0)
                    } else {
                        hit.t_a
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // ── Endpoint contact detection ──
    // When a boundary entity terminates on (or within snap tolerance of) the
    // target curve, register that contact point as a cut parameter.
    const ENDPOINT_TOLERANCE: f64 = CUT_TOLERANCE;
    let mut contacts = Vec::new();

    for &geo in &candidates {
        // 1. Boundary endpoints touching target
        for pt in geo.endpoints() {
            let t = target.parameter_at(pt);
            let valid_t = if bounded {
                let slack = CUT_TOLERANCE;
                if (-slack..=1.0 + slack).contains(&t) {
                    Some(t.clamp(0.0, 1.0))
                } else {
                    None
                }
            } else {
                Some(t)
            };

            if let Some(t_cand) = valid_t {
                let proj = target.point_at(t_cand);
                let dx = pt[0] - proj[0];
                let dy = pt[1] - proj[1];
                if (dx * dx + dy * dy) <= ENDPOINT_TOLERANCE * ENDPOINT_TOLERANCE {
                    contacts.push(t_cand);
                }
            }
        }

        // 2. Target endpoints touching boundary
        if bounded && !target.is_closed() {
            if let Some(boundary_curve) = geo_to_curve(geo) {
                let boundary_bounded = matches!(boundary_curve.extent(), KernelExtent::Bounded);
                for &t_end in &[0.0, 1.0] {
                    let pt = target.point_at(t_end);
                    let u = boundary_curve.parameter_at(pt);
                    let valid_u = if boundary_bounded {
                        let slack = CUT_TOLERANCE;
                        if (-slack..=1.0 + slack).contains(&u) {
                            Some(u.clamp(0.0, 1.0))
                        } else {
                            None
                        }
                    } else {
                        Some(u)
                    };
                    if let Some(u_cand) = valid_u {
                        let proj = boundary_curve.point_at(u_cand);
                        let dx = pt[0] - proj[0];
                        let dy = pt[1] - proj[1];
                        if (dx * dx + dy * dy) <= ENDPOINT_TOLERANCE * ENDPOINT_TOLERANCE {
                            contacts.push(t_end);
                        }
                    }
                }
            }
        }
    }

    // Prioritize exact endpoint contact parameters over raw ray intersections,
    // ensuring trimmed endpoints match boundary endpoints down to machine precision.
    let is_closed = target.is_closed();
    for t_contact in contacts {
        if let Some(pos) = ts.iter().position(|&t| {
            let diff = (t - t_contact).abs();
            let cyclic = if is_closed {
                diff.min(1.0 - diff)
            } else {
                diff
            };
            cyclic < 1e-6
        }) {
            ts[pos] = t_contact;
        } else {
            ts.push(t_contact);
        }
    }

    ts.sort_by(|a, b| a.total_cmp(b));
    ts.dedup_by(|a, b| {
        let diff = (*a - *b).abs();
        let cyclic = if is_closed {
            diff.min(1.0 - diff)
        } else {
            diff
        };
        cyclic < 1e-6
    });

    // For closed curves, ensure the first and last cuts don't duplicate across the 0/1 seam
    if is_closed && ts.len() >= 2 {
        let seam_diff = (ts[ts.len() - 1] - (ts[0] + 1.0)).abs();
        if seam_diff < 1e-6 {
            ts.pop();
        }
    }

    ts
}

/// A Ray entity as the kernel curve it is, so its parameter is the ray's own
/// rather than a fraction of some invented length.

pub(crate) fn ray_curve(r: &RayEnt) -> Curve {
    Curve::Ray(KernelRay {
        origin: [r.base_point.x, r.base_point.y],
        direction: [r.direction.x, r.direction.y],
    })
}

/// An XLine entity as the kernel curve it is.

pub(crate) fn xline_curve(x: &XLineEnt) -> Curve {
    Curve::XLine(KernelXLine {
        base: [x.base_point.x, x.base_point.y],
        direction: [x.direction.x, x.direction.y],
    })
}

/// How close two points have to be to count as a crossing.
///
/// Loose enough that a boundary drawn to meet an entity still registers when
/// its endpoint is a rounding step away, which is the usual case in a drawing
/// that was snapped together rather than computed.

pub(crate) const CUT_TOLERANCE: f64 = 1e-7;

pub(crate) fn line_seg_ts(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    cut_params(
        &Curve::Line(KernelLine {
            start: [ax, ay],
            end: [bx, by],
        }),
        target,
        geos,
    )
}

pub(crate) fn polyline_seg_ts(
    p0: [f64; 2],
    p1: [f64; 2],
    bulge: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    if let Some(ba) = BulgeArc::from_bulge(p0, p1, bulge) {
        let (from, to) = if ba.sweep >= 0.0 {
            (ba.start_angle, ba.start_angle + ba.sweep)
        } else {
            (ba.end_angle, ba.end_angle - ba.sweep)
        };
        let arc_curve = Curve::Arc(KernelArc {
            centre: ba.center,
            radius: ba.radius,
            start_angle: from,
            end_angle: to,
        });
        let ts = cut_params(&arc_curve, target, geos);
        let mut u_ts: Vec<f64> = if ba.sweep >= 0.0 {
            ts
        } else {
            ts.into_iter().map(|t| (1.0 - t).clamp(0.0, 1.0)).collect()
        };
        u_ts.sort_by(|a, b| a.total_cmp(b));
        u_ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        u_ts
    } else {
        line_seg_ts(p0[0], p0[1], p1[0], p1[1], target, geos)
    }
}

pub(crate) fn arc_seg_ts(
    cx: f64,
    cy: f64,
    r: f64,
    a0: f64,
    a1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    cut_params(
        &Curve::Arc(KernelArc {
            centre: [cx, cy],
            radius: r,
            start_angle: a0,
            end_angle: a1,
        }),
        target,
        geos,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn ellipse_seg_ts(
    cx: f64,
    cy: f64,
    a: f64,
    b: f64,
    nx: f64,
    ny: f64,
    t0: f64,
    t1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    cut_params(
        &Curve::Ellipse(KernelEllipseArc {
            ellipse: KernelEllipse {
                centre: [cx, cy],
                major_radius: a,
                minor_radius: b,
                major_axis: [nx, ny],
            },
            start_parameter: t0,
            end_parameter: t1,
        }),
        target,
        geos,
    )
}

/// Cuts on a spline, solved against the true curve rather than a sampling of
/// it. The kernel subdivides and converges, so a cut lands where the boundary
/// actually crosses instead of where a fixed number of samples said it did.

pub(crate) fn spline_seg_ts(spl: &SplineEnt, target: Handle, geos: &[Geo]) -> Vec<f64> {
    let Some(curve) = spline_to_nurbs(spl) else {
        return vec![];
    };
    cut_params(&Curve::Nurbs(curve), target, geos)
}

/// Trim an Ellipse entity. Returns the surviving ellipse-arc segments.

/// Edge option (#336): treat boundary edges as implied — straight edges run
/// on past their endpoints and arcs close to full circles, so objects trim /
/// extend to the boundary's extrapolation, not only its drawn extent.
pub(crate) fn imply_edge_geos(geos: &mut [Geo]) {
    for g in geos.iter_mut() {
        match g {
            Geo::Line { handle, p1, p2 } => {
                let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
                if dx.hypot(dy) > 1e-9 {
                    // An implied edge cuts wherever the boundary's line would
                    // reach, which is what an infinite line is. Stretching the
                    // segment to a stand-in length instead both invents a
                    // limit and, at survey coordinates, is no larger than the
                    // coordinates themselves.
                    *g = Geo::InfLine {
                        handle: *handle,
                        bx: p1[0],
                        by: p1[1],
                        dx,
                        dy,
                    };
                }
            }
            Geo::Arc { a0, a1, .. } => {
                *a0 = 0.0;
                *a1 = TAU;
            }
            _ => {}
        }
    }
}
