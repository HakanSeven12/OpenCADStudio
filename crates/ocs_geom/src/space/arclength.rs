//! Arc-length traversal of spatial polylines and rational spline curves.
use super::{NurbsCurve3, Vec3};

#[derive(Clone)]
enum SpatialCurve { Polyline(Vec<[f64; 3]>), Nurbs(NurbsCurve3) }

/// Reusable length stations, built once and queried for each marker.
/// Polylines are exact; spline lengths integrate analytic speed inside each
/// nonempty knot span, refining quadrature until the error target is met.
#[derive(Clone)]
pub struct ArcLengthCurve3 {
    curve: SpatialCurve,
    stations: Vec<(f64, f64)>,
    closed: bool,
}

impl ArcLengthCurve3 {
    pub fn from_polyline(points: &[[f64; 3]], closed: bool) -> Option<Self> {
        if points.iter().flatten().any(|value| !value.is_finite()) { return None; }
        let mut controls = Vec::new();
        for point in points {
            if controls.last() != Some(point) { controls.push(*point); }
        }
        if closed && controls.len() > 1 && controls.first() != controls.last() {
            controls.push(controls[0]);
        }
        if controls.len() < 2 { return None; }
        let segments = controls.len() - 1;
        let mut stations = vec![(0.0, 0.0)];
        let mut length = 0.0;
        for (index, pair) in controls.windows(2).enumerate() {
            length += Vec3::from(pair[0]).distance(pair[1].into());
            stations.push(((index + 1) as f64 / segments as f64, length));
        }
        if !length.is_finite() || length <= 0.0 { return None; }
        Some(Self { curve: SpatialCurve::Polyline(controls), stations, closed })
    }

    pub fn from_nurbs(curve: NurbsCurve3) -> Option<Self> {
        let scale = curve.weights().iter().copied().fold(0.0_f64, f64::max);
        if !scale.is_finite() || scale <= 0.0 { return None; }
        let weights = curve.weights().iter().map(|weight| weight / scale).collect();
        let curve = NurbsCurve3::new_strict(curve.degree(), curve.control_points().to_vec(),
            curve.knots().to_vec(), weights)?.with_periodicity(curve.periodicity());
        let (start, end) = curve.domain();
        let span = end - start;
        if !span.is_finite() || span <= 0.0 { return None; }
        let scale = super::polygon::chain_length(curve.control_points());
        if !scale.is_finite() { return None; }
        let tolerance = (scale * 1e-10).max(1e-12);
        let mut parameters = vec![0.0];
        for knot in curve.knots() {
            if *knot > start && *knot < end {
                let t = (*knot - start) / span;
                if parameters.last() != Some(&t) { parameters.push(t); }
            }
        }
        parameters.push(1.0);
        let mut stations = vec![(0.0, 0.0)];
        for pair in parameters.windows(2) {
            append_stations(&curve, pair[0], pair[1], tolerance * (pair[1] - pair[0]), 0, &mut stations)?;
        }
        if stations.last()?.1 <= 0.0 { return None; }
        let closed = curve.is_closed();
        Some(Self { curve: SpatialCurve::Nurbs(curve), stations, closed })
    }

    pub fn length(&self) -> f64 { self.stations.last().map_or(0.0, |station| station.1) }
    pub fn is_closed(&self) -> bool { self.closed }

    pub fn point_at(&self, parameter: f64) -> [f64; 3] {
        match &self.curve {
            SpatialCurve::Nurbs(curve) => curve.point_at(parameter),
            SpatialCurve::Polyline(points) => {
                let scaled = parameter.clamp(0.0, 1.0) * (points.len() - 1) as f64;
                let index = (scaled.floor() as usize).min(points.len() - 2);
                Vec3::from(points[index]).lerp(points[index + 1].into(), scaled - index as f64).to_array()
            }
        }
    }

    pub fn tangent_at(&self, parameter: f64) -> [f64; 3] {
        match &self.curve {
            SpatialCurve::Nurbs(curve) => {
                let (start, end) = curve.domain();
                curve.derivative_at_knot(start + (end - start) * parameter.clamp(0.0, 1.0))
            }
            SpatialCurve::Polyline(points) => {
                let index = ((parameter.clamp(0.0, 1.0) * (points.len() - 1) as f64).floor() as usize).min(points.len() - 2);
                (Vec3::from(points[index + 1]) - Vec3::from(points[index])).to_array()
            }
        }
    }

    pub fn parameter_at_distance(&self, distance: f64) -> f64 {
        if distance.is_nan() || distance <= 0.0 { return 0.0; }
        if distance >= self.length() { return 1.0; }
        let upper = self.stations.partition_point(|station| station.1 < distance);
        let (from, before) = self.stations[upper - 1];
        let (to, after) = self.stations[upper];
        if let SpatialCurve::Nurbs(curve) = &self.curve {
            let mut low = from; let mut high = to;
            for _ in 0..44 {
                let middle = (low + high) * 0.5;
                if before + integrate_speed(curve, from, middle) < distance { low = middle; }
                else { high = middle; }
            }
            (low + high) * 0.5
        } else {
            from + (to - from) * (distance - before) / (after - before)
        }
    }

    pub fn point_at_distance(&self, distance: f64) -> [f64; 3] {
        self.point_at(self.parameter_at_distance(distance))
    }
}

fn integrate_speed(curve: &NurbsCurve3, from: f64, to: f64) -> f64 {
    // Five-point Gauss-Legendre quadrature, avoiding derivative ambiguity at knots.
    const NODES: [(f64, f64); 5] = [(0.0, 0.5688888888888889),
        (-0.5384693101056831, 0.4786286704993665), (0.5384693101056831, 0.4786286704993665),
        (-0.9061798459386640, 0.2369268850561891), (0.9061798459386640, 0.2369268850561891)];
    let (start, end) = curve.domain();
    let half = (to - from) * 0.5; let middle = (from + to) * 0.5;
    half * (end - start) * NODES.iter().map(|(node, weight)| {
        let parameter = start + (end - start) * (middle + half * node);
        Vec3::from(curve.derivative_at_knot(parameter)).length() * weight
    }).sum::<f64>()
}

fn append_stations(curve: &NurbsCurve3, from: f64, to: f64, tolerance: f64,
    depth: usize, stations: &mut Vec<(f64, f64)>) -> Option<()> {
    let middle = (from + to) * 0.5;
    let coarse = integrate_speed(curve, from, to);
    let left = integrate_speed(curve, from, middle);
    let right = integrate_speed(curve, middle, to);
    if !coarse.is_finite() || !left.is_finite() || !right.is_finite() { return None; }
    if (left + right - coarse).abs() <= tolerance {
        let before = stations.last()?.1;
        stations.push((middle, before + left));
        stations.push((to, before + left + right));
    } else {
        if depth >= 20 { return None; }
        append_stations(curve, from, middle, tolerance * 0.5, depth + 1, stations)?;
        append_stations(curve, middle, to, tolerance * 0.5, depth + 1, stations)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polylines_and_splines_are_walked_by_distance() {
        let polyline = ArcLengthCurve3::from_polyline(
            &[[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 4.0, 0.0]], false).unwrap();
        assert_eq!(polyline.length(), 7.0);
        assert_eq!(polyline.point_at_distance(5.0), [3.0, 2.0, 0.0]);

        let line = NurbsCurve3::new_strict(1, vec![[0.0, 0.0, 0.0], [0.0, 0.0, 5.0]],
            vec![0.0, 0.0, 1.0, 1.0], vec![1.0, 1.0]).unwrap();
        let spline = ArcLengthCurve3::from_nurbs(line).unwrap();
        assert!((spline.length() - 5.0).abs() < 1e-12);
        assert!((spline.point_at_distance(2.0)[2] - 2.0).abs() < 1e-9);
    }
}
