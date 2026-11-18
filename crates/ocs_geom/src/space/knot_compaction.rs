//! Remove redundant interior knots while retaining the represented curve.
use super::{NurbsCurve3, Vec3};

impl NurbsCurve3 {
    /// Compact repeated interior knots by inverse knot insertion. A removal
    /// that cannot reproduce its original homogeneous controls is skipped.
    /// Unequal rational weights are retained unchanged.
    pub fn compact_knots(&self, tolerance: f64) -> Option<Self> {
        if !tolerance.is_finite() || tolerance < 0.0 { return None; }
        if self.weights().windows(2).any(|pair| pair[0] != pair[1]) { return Some(self.clone()); }
        let mut curve = self.clone();
        let (start, end) = self.domain();
        let mut interior = self.knots().iter().copied().filter(|knot| *knot > start && *knot < end).collect::<Vec<_>>();
        interior.dedup();
        for knot in interior {
            while curve.knots().iter().filter(|value| **value == knot).count() > 1 {
                let Some(next) = remove_once(&curve, knot, tolerance) else { break; };
                curve = next;
            }
        }
        Some(curve)
    }
}

fn remove_once(curve: &NurbsCurve3, knot: f64, tolerance: f64) -> Option<NurbsCurve3> {
    let degree = curve.degree();
    let count = curve.control_points().len();
    if count <= degree + 1 { return None; }
    let mut knots = curve.knots().to_vec();
    let remove = knots.iter().rposition(|value| *value == knot)?;
    knots.remove(remove);
    let span = super::spline::span_of(degree, &knots, count - 2, knot);
    let multiplicity = knots.iter().filter(|value| **value == knot).count();
    let first = span.checked_sub(degree)?;
    let last = span.checked_sub(multiplicity)?;
    if last <= first || last + 1 >= count { return None; }
    let homogeneous = |index: usize| {
        let point = curve.control_points()[index]; let weight = curve.weights()[index];
        [point[0] * weight, point[1] * weight, point[2] * weight, weight]
    };
    let mut controls: Vec<[f64; 4]> = (0..=first).map(homogeneous).collect();
    for index in first + 1..=last {
        let width = knots[index + degree] - knots[index];
        if width <= 0.0 { return None; }
        let alpha = (knot - knots[index]) / width;
        if alpha <= 0.0 { return None; }
        let current = homogeneous(index); let previous = controls[index - 1];
        let mut recovered = std::array::from_fn(|axis| (current[axis] - (1.0 - alpha) * previous[axis]) / alpha);
        // Equal input weights define a polynomial curve. Preserve that exact
        // representation instead of introducing rational roundoff at each solve.
        recovered[3] = curve.weights()[0];
        controls.push(recovered);
    }
    let recovered = controls[last]; let expected = homogeneous(last + 1);
    if recovered[3] <= 0.0 || expected[3] <= 0.0 { return None; }
    let point = |value: [f64; 4]| Vec3::new(value[0] / value[3], value[1] / value[3], value[2] / value[3]);
    if point(recovered).distance(point(expected)) > tolerance
        || (recovered[3] - expected[3]).abs() > 1e-10 * recovered[3].abs().max(expected[3].abs()) { return None; }
    controls.extend((last + 2..count).map(homogeneous));
    let points = controls.iter().map(|point| [point[0] / point[3], point[1] / point[3], point[2] / point[3]]).collect();
    let weights = controls.iter().map(|point| point[3]).collect();
    NurbsCurve3::new_strict(degree, points, knots, weights).map(|result| result.with_periodicity(curve.periodicity()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redundant_inserted_knot_is_removed_without_changing_shape() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![
                [0.0, 0.0, 0.0],
                [0.5, 0.5, 0.0],
                [1.0, 0.5, 0.0],
                [1.5, 0.5, 0.0],
                [2.0, 0.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![0.1; 5],
        )
        .unwrap();
        let compact = curve.compact_knots(1e-12).unwrap();

        assert_eq!(compact.control_points().len(), 4);
        assert_eq!(compact.knots().iter().filter(|knot| **knot == 0.5).count(), 1);
        assert!(compact
            .weights()
            .iter()
            .all(|weight| *weight == curve.weights()[0]));
        for step in 0..=20 {
            let parameter = step as f64 / 20.0;
            assert!(Vec3::from(curve.point_at(parameter))
                .distance(Vec3::from(compact.point_at(parameter))) < 1e-12);
        }
    }

    #[test]
    fn unequal_rational_weights_are_left_untouched() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![
                [0.0, 0.0, 0.0],
                [0.5, 0.5, 0.0],
                [1.0, 0.5, 0.0],
                [1.5, 0.5, 0.0],
                [2.0, 0.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![1.0, 2.0, 1.0, 2.0, 1.0],
        )
        .unwrap();
        assert_eq!(curve.compact_knots(1e-12).unwrap(), curve);
    }
}
