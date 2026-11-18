//! Curvature-continuous joins for editable NURBS endpoints.

use super::{NurbsCurve3, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineEnd {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveJet {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
    pub curvature: [f64; 3],
}

impl CurveJet {
    pub fn linear(point: [f64; 3], tangent: [f64; 3]) -> Option<Self> {
        Vec3::from(tangent).normalize().map(|tangent| Self {
            point,
            tangent: tangent.to_array(),
            curvature: [0.0; 3],
        })
    }

    pub fn circular(point: [f64; 3], tangent: [f64; 3], centre: [f64; 3]) -> Option<Self> {
        let tangent = Vec3::from(tangent).normalize()?;
        let inward = Vec3::from(centre) - Vec3::from(point);
        let radius_squared = inward.length_squared();
        (radius_squared > 1e-24).then_some(Self {
            point,
            tangent: tangent.to_array(),
            curvature: (inward / radius_squared).to_array(),
        })
    }

    pub fn from_nurbs(curve: &NurbsCurve3, endpoint: SplineEnd) -> Option<Self> {
        let parameter = match endpoint {
            SplineEnd::Start => curve.domain().0,
            SplineEnd::End => curve.domain().1,
        };
        let point = curve.point_at_knot(parameter);
        let derivative = Vec3::from(curve.derivative_at_knot(parameter));
        let speed = derivative.length();
        if speed <= 1e-12 {
            return None;
        }
        let tangent = derivative / speed;
        let acceleration = Vec3::from(curve.acceleration_at_knot(parameter));
        let normal_acceleration = acceleration - tangent * acceleration.dot(tangent);
        Some(Self {
            point,
            tangent: tangent.to_array(),
            curvature: (normal_acceleration / (speed * speed)).to_array(),
        })
    }
}

fn start_smoothed(curve: &NurbsCurve3, target: CurveJet) -> Option<NurbsCurve3> {
    let degree = curve.degree();
    if degree < 2 || curve.control_points().len() < 3 {
        return None;
    }
    let knots = curve.knots();
    let start = curve.domain().0;
    if knots
        .iter()
        .take(degree + 1)
        .any(|knot| (*knot - start).abs() > 1e-12)
    {
        return None;
    }
    let first_span = knots[degree + 1] - start;
    let second_span = knots[degree + 2] - knots[2];
    if first_span <= 1e-12 || second_span <= 1e-12 {
        return None;
    }

    let current_d1 = Vec3::from(curve.derivative_at_knot(start));
    let speed = current_d1.length();
    let mut tangent = Vec3::from(target.tangent).normalize()?;
    if tangent.dot(current_d1) < 0.0 {
        tangent = -tangent;
    }
    let current_d2 = Vec3::from(curve.acceleration_at_knot(start));
    let tangential_acceleration = current_d2.dot(tangent);
    let desired_d1 = tangent * speed;
    let desired_d2 =
        Vec3::from(target.curvature) * (speed * speed) + tangent * tangential_acceleration;

    let weights = curve.weights();
    let w0 = weights[0];
    let w1 = weights[1];
    let w2 = weights[2];
    let d0_scale = degree as f64 / first_span;
    let d1_scale = degree as f64 / second_span;
    let second_scale = (degree - 1) as f64 / first_span;
    let w_d0 = d0_scale * (w1 - w0);
    let w_d1 = d1_scale * (w2 - w1);
    let w_dd = second_scale * (w_d1 - w_d0);

    let point = Vec3::from(target.point);
    let a0 = point * w0;
    let a_d = desired_d1 * w0 + point * w_d0;
    let a_dd = desired_d2 * w0 + desired_d1 * (2.0 * w_d0) + point * w_dd;
    let h0 = [a0.x, a0.y, a0.z, w0];
    let hd0 = [a_d.x, a_d.y, a_d.z, w_d0];
    let hdd = [a_dd.x, a_dd.y, a_dd.z, w_dd];
    let h1: [f64; 4] = std::array::from_fn(|axis| h0[axis] + hd0[axis] / d0_scale);
    let hd1: [f64; 4] = std::array::from_fn(|axis| hd0[axis] + hdd[axis] / second_scale);
    let h2: [f64; 4] = std::array::from_fn(|axis| h1[axis] + hd1[axis] / d1_scale);

    let mut controls = curve.control_points().to_vec();
    controls[0] = [h0[0] / w0, h0[1] / w0, h0[2] / w0];
    controls[1] = [h1[0] / w1, h1[1] / w1, h1[2] / w1];
    controls[2] = [h2[0] / w2, h2[1] / w2, h2[2] / w2];
    NurbsCurve3::new_strict(degree, controls, knots.to_vec(), weights.to_vec())
}

/// Moves only the three controls nearest `endpoint`, preserving the rest of
/// the curve while matching the target point, tangent and curvature.
pub fn smooth_nurbs_endpoint(
    curve: &NurbsCurve3,
    endpoint: SplineEnd,
    target: CurveJet,
) -> Option<NurbsCurve3> {
    match endpoint {
        SplineEnd::Start => start_smoothed(curve, target),
        SplineEnd::End => start_smoothed(&curve.reversed()?, target)?.reversed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_point_tangent_and_zero_curvature() {
        let curve = NurbsCurve3::from_control_polygon(
            3,
            &[
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [2.0, 2.0, 0.0],
                [3.0, 1.0, 0.0],
            ],
            false,
        )
        .unwrap();
        let target = CurveJet::linear([5.0, 4.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        let result = smooth_nurbs_endpoint(&curve, SplineEnd::Start, target).unwrap();
        let jet = CurveJet::from_nurbs(&result, SplineEnd::Start).unwrap();
        assert!(Vec3::from(jet.point).distance(Vec3::from(target.point)) < 1e-9);
        assert!(
            Vec3::from(jet.tangent)
                .cross(Vec3::from(target.tangent))
                .length()
                < 1e-9
        );
        assert!(
            Vec3::from(jet.curvature).length() < 5e-3,
            "finite-difference endpoint jet was {:?}",
            jet.curvature
        );
        assert_eq!(&result.control_points()[3..], &curve.control_points()[3..]);
    }

    #[test]
    fn matches_circular_curvature_at_the_end() {
        let curve = NurbsCurve3::from_control_polygon(
            3,
            &[
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 1.0, 0.0],
                [3.0, 2.0, 0.0],
            ],
            false,
        )
        .unwrap();
        let target = CurveJet::circular([2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]).unwrap();
        let result = smooth_nurbs_endpoint(&curve, SplineEnd::End, target).unwrap();
        let jet = CurveJet::from_nurbs(&result, SplineEnd::End).unwrap();
        assert!(Vec3::from(jet.point).distance(Vec3::from(target.point)) < 1e-9);
        assert!(
            Vec3::from(jet.tangent)
                .cross(Vec3::from(target.tangent))
                .length()
                < 1e-9
        );
        assert!(
            Vec3::from(jet.curvature).distance(Vec3::from(target.curvature)) < 5e-3,
            "expected {:?}, got {:?}",
            target.curvature,
            jet.curvature
        );
    }
}
