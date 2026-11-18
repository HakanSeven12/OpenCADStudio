use super::triangulate::simple_rings;
use super::Curve;
use crate::tessellation::DEFAULT_ANGLE;

const MAX_RECOVERY_POINTS: usize = 512;
const MAX_EXTRA_POINTS: usize = 128;
const MAX_PASSES: usize = 16;
const MAX_INTERSECTION_CHECKS: usize = 1_000_000;

/// Resample a crossed spline boundary without changing ordinary boundaries.
/// `project` must use the same coordinate frame as the eventual fill mesh.
pub fn refine_spline_boundary(
    ring: &[[f64; 2]],
    curves: impl FnOnce() -> Vec<Curve>,
    project: impl Fn([f64; 2]) -> [f64; 2],
) -> Option<Vec<[f64; 2]>> {
    if ring.len() > MAX_RECOVERY_POINTS {
        return None;
    }
    let mut work_left = MAX_INTERSECTION_CHECKS;
    if !crossings(&[ring], &project, &mut work_left)?[0] {
        return None;
    }
    let projected = ring.iter().copied().map(&project).collect();
    if !simple_rings(&[projected]).1.is_empty() {
        return None;
    }
    let limit = (ring.len() + MAX_EXTRA_POINTS).min(MAX_RECOVERY_POINTS);
    let mut spans = Vec::new();
    for curve in curves() {
        if let Curve::Nurbs(spline) = curve {
            let (start, end) = spline.domain();
            if end <= start || spline.control_points().len() > limit {
                return None;
            }
            for pair in spline.knots().windows(2) {
                if pair[0] >= start && pair[1] <= end && pair[0] < pair[1] {
                    spans.push(Curve::Nurbs(spline.trimmed(
                        (pair[0] - start) / (end - start),
                        (pair[1] - start) / (end - start),
                    )?));
                }
            }
        } else {
            spans.push(curve);
        }
    }
    let mut remaining = limit;
    let mut points: Vec<_> = spans
        .iter()
        .map(|curve| sample(curve, &mut remaining))
        .collect::<Option<_>>()?;
    for _ in 0..MAX_PASSES {
        let marked = crossings(&points, &project, &mut work_left)?;
        if !marked.iter().any(|marked| *marked) {
            let ring = chain_samples(points)?;
            let projected = ring.iter().copied().map(&project).collect();
            return (!simple_rings(&[projected]).1.is_empty()).then_some(ring);
        }
        let mut next = Vec::new();
        let mut sampled = Vec::new();
        let mut changed = false;
        let mut remaining = limit;
        for ((curve, points), marked) in spans.into_iter().zip(points).zip(marked) {
            if marked {
                if let Curve::Nurbs(spline) = &curve {
                    let (a, b) = spline.split_at(0.5)?;
                    for part in [Curve::Nurbs(a), Curve::Nurbs(b)] {
                        sampled.push(sample(&part, &mut remaining)?);
                        next.push(part);
                    }
                    changed = true;
                    continue;
                }
            }
            remaining = remaining.checked_sub(points.len())?;
            next.push(curve);
            sampled.push(points);
        }
        if !changed {
            return None;
        }
        spans = next;
        points = sampled;
    }
    None
}

fn sample(curve: &Curve, remaining: &mut usize) -> Option<Vec<[f64; 2]>> {
    let Curve::Nurbs(spline) = curve else {
        let points = curve.tessellate_angle(DEFAULT_ANGLE);
        *remaining = remaining.checked_sub(points.len())?;
        return Some(points);
    };
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in spline.control_points() {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    let tolerance = (max[0] - min[0]).hypot(max[1] - min[1]) * (1.0 - (DEFAULT_ANGLE * 0.5).cos());
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return None;
    }
    let points = curve.tessellate_within(tolerance);
    *remaining = remaining.checked_sub(points.len())?;
    Some(points)
}

fn chain_samples(mut samples: Vec<Vec<[f64; 2]>>) -> Option<Vec<[f64; 2]>> {
    let distance_squared = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    samples.retain(|points| !points.is_empty());
    let first = samples.pop()?;
    let mut chain: std::collections::VecDeque<_> = first.into();
    while !samples.is_empty() {
        let head = *chain.front()?;
        let tail = *chain.back()?;
        let mut best = (f64::MAX, 0usize, false, false);
        for (index, points) in samples.iter().enumerate() {
            let start = points[0];
            let end = *points.last()?;
            for candidate in [
                (distance_squared(tail, start), index, false, false),
                (distance_squared(tail, end), index, true, false),
                (distance_squared(head, end), index, false, true),
                (distance_squared(head, start), index, true, true),
            ] {
                if candidate.0 < best.0 {
                    best = candidate;
                }
            }
        }
        let (_, index, reverse, at_front) = best;
        let mut points = samples.swap_remove(index);
        if reverse {
            points.reverse();
        }
        if at_front {
            if distance_squared(*points.last()?, head) < 1e-18 {
                points.pop();
            }
            for point in points.into_iter().rev() {
                chain.push_front(point);
            }
        } else {
            let mut points = points.into_iter();
            if let Some(first) = points.next() {
                if distance_squared(first, tail) >= 1e-18 {
                    chain.push_back(first);
                }
            }
            chain.extend(points);
        }
    }
    Some(chain.into())
}

fn crossings(
    edges: &[impl AsRef<[[f64; 2]]>],
    project: &impl Fn([f64; 2]) -> [f64; 2],
    work_left: &mut usize,
) -> Option<Vec<bool>> {
    let mut marked = vec![false; edges.len()];
    for projected in [false, true] {
        let mut segments = Vec::new();
        for (edge, points) in edges.iter().enumerate() {
            for pair in points.as_ref().windows(2) {
                let [a, b] = [pair[0], pair[1]].map(|p| if projected { project(p) } else { p });
                if !a.into_iter().chain(b).all(f64::is_finite) {
                    return None;
                }
                segments.push((a[0].min(b[0]), a[0].max(b[0]), edge, a, b));
            }
        }
        segments.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        for (index, &(_, right, edge, a, b)) in segments.iter().enumerate() {
            for &(left, _, other, c, d) in &segments[index + 1..] {
                if left > right {
                    break;
                }
                *work_left = work_left.checked_sub(1)?;
                if a[1].max(b[1]) < c[1].min(d[1]) || c[1].max(d[1]) < a[1].min(b[1]) {
                    continue;
                }
                let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
                    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
                };
                let opposite = |a: f64, b: f64| (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0);
                let inside = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
                    cross(a, b, p) == 0.0
                        && (0..2).any(|axis| {
                            p[axis] > a[axis].min(b[axis]) && p[axis] < a[axis].max(b[axis])
                        })
                };
                if opposite(cross(a, b, c), cross(a, b, d))
                    && opposite(cross(c, d, a), cross(c, d, b))
                    || inside(a, c, d)
                    || inside(b, c, d)
                    || inside(c, a, b)
                    || inside(d, a, b)
                    || a != b && (a == c && b == d || a == d && b == c)
                {
                    marked[edge] = true;
                    marked[other] = true;
                }
            }
        }
    }
    Some(marked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::NurbsCurve;

    fn narrow(clearance: f64) -> NurbsCurve {
        let line = |a: [f64; 2], b: [f64; 2]| {
            [
                a,
                std::array::from_fn(|i| a[i] + (b[i] - a[i]) / 3.0),
                std::array::from_fn(|i| a[i] + 2.0 * (b[i] - a[i]) / 3.0),
                b,
            ]
        };
        let [a, b, c, d, e, f] = [
            [-1.0, -1.0],
            [0.0, -1.0],
            [0.0, 1.0],
            [clearance, 0.999],
            [-0.5, 1.5],
            [-1.0, 1.5],
        ];
        let spans = [
            line(a, b),
            line(b, c),
            line(c, d),
            [d, [clearance, 1.1], [-0.3, 1.5], e],
            line(e, f),
            line(f, a),
        ];
        let mut controls = vec![a];
        for span in spans {
            controls.extend_from_slice(&span[1..]);
        }
        let mut knots = vec![0.0; 4];
        for index in 1..6 {
            knots.extend([index as f64; 3]);
        }
        knots.extend([6.0; 4]);
        NurbsCurve::new(3, controls, knots, None).unwrap()
    }

    #[test]
    fn crossed_spline_samples_are_refined_in_the_kernel() {
        let spline = narrow(0.00001);
        let curve = Curve::Nurbs(spline.clone());
        let original = curve.tessellate_angle(DEFAULT_ANGLE);
        assert!(simple_rings(std::slice::from_ref(&original)).1.is_empty());
        let refined = refine_spline_boundary(&original, || vec![curve], |point| point).unwrap();
        assert!(!simple_rings(std::slice::from_ref(&refined)).1.is_empty());
        for index in 0..=6 {
            assert!(refined.contains(&spline.point_at(index as f64 / 6.0)));
        }
    }

    #[test]
    fn valid_and_oversized_boundaries_are_not_changed() {
        let curve = Curve::Nurbs(narrow(0.01));
        assert!(refine_spline_boundary(
            &curve.tessellate_angle(DEFAULT_ANGLE),
            || panic!("valid path resampled"),
            |point| point,
        )
        .is_none());
        assert!(refine_spline_boundary(
            &vec![[0.0; 2]; MAX_RECOVERY_POINTS + 1],
            || panic!("size cap ignored"),
            |point| point,
        )
        .is_none());
    }
}
