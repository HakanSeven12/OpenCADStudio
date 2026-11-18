//! Bounded straight-segment approximation of positive-weight clamped splines.
use super::{NurbsCurve3, Vec3};

const MAX_POINTS: usize = 1_000_000;

/// Approximation coordinates and the maximum control-hull chord deviation.
pub struct SplinePolyline {
    pub points: Vec<[f64; 3]>,
    pub tolerance: f64,
}

impl NurbsCurve3 {
    /// Approximate with monotonically increasing precision in 0..=99.
    ///
    /// This API defines its own scale-relative accuracy policy, not an external
    /// application's vertex-count policy: control-box diagonal / (32*(p+1)^2).
    /// Positive rational Bezier convex hulls bound every accepted segment's
    /// distance from the curve. Endpoints and knot boundaries are retained.
    /// Unsupported unclamped/discontinuous curves and exhausted resource limits
    /// return None instead of relaxing the requested accuracy. Degree is limited
    /// to 64, control/output nets to one million points, recursion to 32 levels
    /// and cumulative extraction/subdivision work to sixteen million units.
    pub fn to_polyline_precision(&self, precision: u8) -> Option<SplinePolyline> {
        if precision > 99 || self.control_points().len() > MAX_POINTS { return None; }
        let degree = self.degree();
        if self.knots().len() > 2 * MAX_POINTS || degree >= self.control_points().len() { return None; }
        let mut budget = 16_000_000usize;
        let (start, end) = self.domain();
        if degree == 0 || degree > 64 || !self.knots()[..=degree].iter().all(|k| *k == start)
            || !self.knots()[self.knots().len()-degree-1..].iter().all(|k| *k == end) { return None; }
        let mut low = self.control_points()[0]; let mut high = low;
        for point in self.control_points() {
            for axis in 0..3 { low[axis] = low[axis].min(point[axis]); high[axis] = high[axis].max(point[axis]); }
        }
        let scale = Vec3::from(high).distance(Vec3::from(low));
        let tolerance = scale / (32.0 * (f64::from(precision) + 1.0).powi(2));
        if !tolerance.is_finite() || tolerance <= 0.0 { return None; }
        let weight_scale = self.weights().iter().copied().fold(0.0_f64, f64::max);
        let mut controls: Vec<[f64;4]> = self.control_points().iter().zip(self.weights()).map(|(p,w)| {
            let w = w / weight_scale; [p[0]*w,p[1]*w,p[2]*w,w]
        }).collect();
        if controls.iter().any(|p| p[3] <= 0.0 || p.iter().any(|v| !v.is_finite())) { return None; }
        let mut knots = self.knots().to_vec();
        let mut interior: Vec<(f64, usize)> = Vec::new();
        for knot in knots.iter().copied().filter(|k| *k > start && *k < end) {
            if let Some((previous, multiplicity)) = interior.last_mut() {
                if *previous == knot { *multiplicity += 1; continue; }
            }
            interior.push((knot, 1));
        }
        for (knot, mut multiplicity) in interior {
            if multiplicity > degree { return None; }
            while multiplicity < degree {
                if controls.len() >= MAX_POINTS { return None; }
                budget = budget.checked_sub(controls.len().checked_add(degree)?)?;
                let span = super::spline::span_of(degree, &knots, controls.len()-1, knot);
                let mut next = Vec::with_capacity(controls.len()+1);
                next.extend_from_slice(&controls[..=span-degree]);
                for i in span-degree+1..=span-multiplicity {
                    let width = knots[i+degree]-knots[i];
                    if width <= 0.0 { return None; }
                    let alpha = (knot-knots[i])/width;
                    next.push(std::array::from_fn(|axis| (1.0-alpha)*controls[i-1][axis]+alpha*controls[i][axis]));
                }
                next.extend_from_slice(&controls[span-multiplicity..]);
                controls = next; knots.insert(span+1,knot); multiplicity += 1;
            }
        }
        let mut points = vec![cartesian(controls[0])?];
        for piece in controls.windows(degree+1).step_by(degree) {
            subdivide(piece, tolerance, 0, &mut budget, &mut points)?;
        }
        Some(SplinePolyline { points, tolerance })
    }
}

fn cartesian(p: [f64;4]) -> Option<[f64;3]> {
    if p[3] <= 0.0 { return None; }
    let result = [p[0]/p[3],p[1]/p[3],p[2]/p[3]];
    result.iter().all(|x| x.is_finite()).then_some(result)
}

fn subdivide(net: &[[f64;4]], tolerance: f64, depth: usize, budget: &mut usize, out: &mut Vec<[f64;3]>) -> Option<()> {
    *budget = budget.checked_sub(net.len().checked_mul(net.len())?)?;
    let start = Vec3::from(cartesian(net[0])?);
    let end_array = cartesian(*net.last()?)?;
    let end = Vec3::from(end_array);
    let chord = end-start; let length = chord.length();
    if !length.is_finite() { return None; }
    let direction = if length > 0.0 { chord / length } else { Vec3::new(0.0,0.0,0.0) };
    let mut flat = true;
    for control in net {
        let point = Vec3::from(cartesian(*control)?);
        let nearest = start + direction * (point-start).dot(direction).clamp(0.0,length);
        let distance = point.distance(nearest);
        if !distance.is_finite() { return None; }
        if distance > tolerance { flat = false; }
    }
    if flat { if out.len() >= MAX_POINTS { return None; } out.push(end_array); return Some(()); }
    if depth >= 32 || out.len() >= MAX_POINTS { return None; }
    let mut work = net.to_vec(); let mut left = vec![work[0]]; let mut right = vec![*work.last()?];
    for remaining in (1..work.len()).rev() {
        for i in 0..remaining { work[i] = std::array::from_fn(|axis| work[i][axis]*0.5+work[i+1][axis]*0.5); }
        left.push(work[0]); right.push(work[remaining-1]);
    }
    right.reverse();
    subdivide(&left,tolerance,depth+1,budget,out)?;
    subdivide(&right,tolerance,depth+1,budget,out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance_to_polyline(point: [f64; 3], polyline: &[[f64; 3]]) -> f64 {
        let point = Vec3::from(point);
        polyline
            .windows(2)
            .map(|segment| {
                let start = Vec3::from(segment[0]);
                let delta = Vec3::from(segment[1]) - start;
                let parameter = if delta.length_squared() > 0.0 {
                    (point - start).dot(delta) / delta.length_squared()
                } else {
                    0.0
                };
                point.distance(start + delta * parameter.clamp(0.0, 1.0))
            })
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn precision_is_monotonic_and_bounds_the_sampled_curve() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 3.0, 1.0],
                [2.0, -1.0, 2.0],
                [4.0, 0.0, 3.0],
            ],
            vec![0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0],
            vec![1.0, 3.0, 0.5, 2.0],
        )
        .unwrap();
        let coarse = curve.to_polyline_precision(0).unwrap();
        let fine = curve.to_polyline_precision(20).unwrap();
        assert!(fine.tolerance < coarse.tolerance);
        assert!(fine.points.len() >= coarse.points.len());
        assert_eq!(fine.points.first(), Some(&curve.point_at(0.0)));
        assert_eq!(fine.points.last(), Some(&curve.point_at(1.0)));
        for step in 0..=512 {
            let point = curve.point_at(step as f64 / 512.0);
            assert!(distance_to_polyline(point, &fine.points) <= fine.tolerance + 1e-10);
        }
    }

    #[test]
    fn knot_boundaries_survive_and_discontinuous_curves_are_rejected() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [2.0, 0.0, 0.0],
                [3.0, 1.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0],
            vec![1.0; 4],
        )
        .unwrap();
        let approximation = curve.to_polyline_precision(10).unwrap();
        let boundary = curve.point_at_knot(0.4);
        assert!(approximation
            .points
            .iter()
            .any(|point| Vec3::from(*point).distance(Vec3::from(boundary)) < 1e-12));

        let discontinuous = NurbsCurve3::new_strict(
            2,
            vec![[0.0; 3], [1.0; 3], [2.0; 3], [4.0; 3], [5.0; 3], [6.0; 3]],
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![1.0; 6],
        )
        .unwrap();
        assert!(discontinuous.to_polyline_precision(10).is_none());
        assert!(curve.to_polyline_precision(100).is_none());
    }
}
