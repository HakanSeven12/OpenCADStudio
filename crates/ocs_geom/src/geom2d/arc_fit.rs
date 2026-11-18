//! Tangent-continuous circular fitting of planar vertex chains.
use super::{arc_from_start_tangent, Curve, Vec2};

/// One fitted vertex, with its original span and distance fraction for attributes.
#[derive(Clone, Debug)]
pub struct ArcFitVertex {
    pub point: [f64; 2],
    pub bulge: f64,
    pub source: usize,
    pub fraction: f64,
    pub inserted: bool,
}

fn unit(direction: Vec2) -> Option<Vec2> {
    let length = direction.length();
    (length.is_finite() && length > 1e-12).then(|| direction / length)
}

fn arc_piece(start: Vec2, tangent: Vec2, end: Vec2) -> Option<(f64, f64)> {
    let chord = end - start;
    let length = chord.length();
    if !length.is_finite() || length <= 1e-12 { return None; }
    if tangent.cross(chord).abs() <= length * 1e-12 {
        return (tangent.dot(chord) > 0.0).then_some((0.0, length));
    }
    let arc = arc_from_start_tangent(start.to_array(), tangent.to_array(), end.to_array(), false)?;
    let sweep = arc.sweep();
    let sign = tangent.cross(chord).signum();
    let bulge = sign * (sweep * 0.25).tan();
    let length = arc.radius * sweep;
    (bulge.is_finite() && length.is_finite() && length > 0.0).then_some((bulge, length))
}

/// Fit two tangent arcs per span. Unspecified interior tangents bisect the
/// neighboring chords; free end tangents reflect that automatic direction
/// across their end chord. Explicit directions override the automatic frame.
/// Invalid/degenerate input returns None without producing a partial chain.
pub fn fit_arc_chain(points: &[[f64; 2]], closed: bool, directions: &[Option<[f64; 2]>]) -> Option<Vec<ArcFitVertex>> {
    let n = points.len();
    if n < 2 || directions.len() != n || points.iter().flatten().any(|v| !v.is_finite()) { return None; }
    let points: Vec<Vec2> = points.iter().copied().map(Vec2::from).collect();
    let spans = if closed { n } else { n - 1 };
    let chords: Vec<Vec2> = (0..spans).map(|i| unit(points[(i + 1) % n] - points[i])).collect::<Option<_>>()?;
    let mut tangents = vec![Vec2::default(); n];
    for i in 0..n {
        if !closed && (i == 0 || i + 1 == n) { continue; }
        tangents[i] = (chords[(i + spans - 1) % spans] + chords[i % spans]).normalize().unwrap_or(chords[i % spans]);
    }
    if !closed && n == 2 { tangents[0] = chords[0]; tangents[1] = chords[0]; }
    else if !closed {
        tangents[0] = chords[0] * (2.0 * chords[0].dot(tangents[1])) - tangents[1];
        tangents[n - 1] = chords[n - 2] * (2.0 * chords[n - 2].dot(tangents[n - 2])) - tangents[n - 2];
    }
    for (tangent, override_direction) in tangents.iter_mut().zip(directions) {
        if let Some(direction) = override_direction {
            if direction.iter().any(|v| !v.is_finite()) { return None; }
            *tangent = unit(Vec2::from(*direction))?;
        }
    }
    let mut result = Vec::with_capacity(spans * 2 + 1);
    for i in 0..spans {
        let j = (i + 1) % n;
        let (p0, p1, t0, t1) = (points[i], points[j], tangents[i], tangents[j]);
        let d = p1 - p0;
        if (chords[i] - t0).length_squared() < 1e-24 && (chords[i] - t1).length_squared() < 1e-24 {
            result.push(ArcFitVertex { point: p0.to_array(), bulge: 0.0, source: i, fraction: 0.0, inserted: false });
            continue;
        }
        let s0 = chords[i].cross(t0);
        let s1 = chords[i].cross(t1);
        if s0.abs() <= 1e-12 || s1.abs() <= 1e-12 {
            result.push(ArcFitVertex { point: p0.to_array(), bulge: 0.0, source: i, fraction: 0.0, inserted: false });
            continue;
        }
        // Match the two circle-center projections on the chord, then solve
        // their tangency. Signed radii support inflection as well as convex arcs.
        let a = -2.0 * (1.0 - t0.dot(t1).clamp(-1.0, 1.0)) / (s0 * s1);
        let discriminant = 16.0 - 4.0 * a;
        let knee = if discriminant >= -1e-12 {
            let reach = d.length() * 2.0 / (4.0 + discriminant.max(0.0).sqrt());
            let r0 = -reach / s0;
            let r1 = reach / s1;
            let c0 = p0 + t0.perpendicular() * r0;
            let c1 = p1 + t1.perpendicular() * r1;
            if (r0 - r1).abs() <= 1e-12 * r0.abs().max(r1.abs()).max(1.0) {
                let arc = arc_from_start_tangent(p0.to_array(), t0.to_array(), p1.to_array(), false)?;
                Vec2::from(Curve::Arc(arc).point_at(0.5))
            } else { (c1 * r0 - c0 * r1) / (r0 - r1) }
        } else {
            // A convex pair can lack symmetric-center roots. Join its circles
            // with a common tangent parallel to the chord instead.
            let normal = chords[i].perpendicular();
            let from = t0.perpendicular() - normal;
            let to = normal - t1.perpendicular();
            let determinant = from.cross(to);
            if determinant.abs() <= 1e-12 { return None; }
            let radius = d.cross(to) / determinant;
            p0 + from * radius
        };
        if knee.to_array().iter().any(|v| !v.is_finite()) { return None; }
        let (first_bulge, first_length) = arc_piece(p0, t0, knee)?;
        let (reverse_bulge, second_length) = arc_piece(p1, -t1, knee)?;
        let total_length = first_length + second_length;
        if !total_length.is_finite() || total_length <= 0.0 { return None; }
        let fraction = first_length / total_length;
        result.push(ArcFitVertex { point: p0.to_array(), bulge: first_bulge, source: i, fraction: 0.0, inserted: false });
        result.push(ArcFitVertex { point: knee.to_array(), bulge: -reverse_bulge, source: i, fraction, inserted: true });
    }
    if !closed { result.push(ArcFitVertex { point: points[n - 1].to_array(), bulge: 0.0, source: n - 1, fraction: 0.0, inserted: false }); }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_points_remain_one_straight_span() {
        let fitted = fit_arc_chain(&[[0.0, 0.0], [3.0, 4.0]], false, &[None, None]).unwrap();
        assert_eq!(fitted.len(), 2);
        assert_eq!(fitted[0].point, [0.0, 0.0]);
        assert_eq!(fitted[0].bulge, 0.0);
        assert_eq!(fitted[1].point, [3.0, 4.0]);
    }

    #[test]
    fn curved_chain_keeps_endpoints_and_returns_finite_arcs() {
        let fitted = fit_arc_chain(
            &[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
            false,
            &[None, None, None],
        )
        .unwrap();
        assert_eq!(fitted.first().unwrap().point, [0.0, 0.0]);
        assert_eq!(fitted.last().unwrap().point, [2.0, 0.0]);
        assert!(fitted.iter().all(|vertex| {
            vertex.point.iter().all(|value| value.is_finite())
                && vertex.bulge.is_finite()
                && vertex.fraction.is_finite()
        }));
    }

    #[test]
    fn invalid_direction_rejects_the_entire_fit() {
        assert!(fit_arc_chain(
            &[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
            false,
            &[Some([f64::MAX, f64::MAX]), None, None],
        )
        .is_none());
    }
}
