//! Parametric helical curves in three-dimensional space.

use super::{NurbsCurve3, Vec3};

/// Direction in which the curve winds when viewed along its positive axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelixDirection {
    Clockwise,
    CounterClockwise,
}

/// Parameters needed to build and measure a circular or conical helix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixCurve {
    pub base_center: [f64; 3],
    pub axis_direction: [f64; 3],
    pub start_direction: [f64; 3],
    pub base_radius: f64,
    pub top_radius: f64,
    pub height: f64,
    pub turns: f64,
    pub direction: HelixDirection,
}

impl HelixCurve {
    /// Recover the axial frame and base radius from stored generating points.
    /// At a zero-radius base the projected curve tangent defines radial phase.
    pub fn frame_from_points(base: [f64; 3], axis: [f64; 3], start: [f64; 3], tangent: [f64; 3]) -> Option<([f64; 3], [f64; 3], f64)> {
        if base.iter().chain(axis.iter()).chain(start.iter()).chain(tangent.iter()).any(|v| !v.is_finite()) { return None; }
        let axis = Vec3::from(axis).normalize()?;
        let delta = Vec3::from(start) - Vec3::from(base);
        let radial = delta - axis * delta.dot(axis);
        let radius = radial.length();
        if !radius.is_finite() { return None; }
        let direction = if radius > 1e-12 { radial.normalize()? } else {
            let tangent = Vec3::from(tangent);
            (tangent - axis * tangent.dot(axis)).normalize()?
        };
        Some((axis.to_array(), direction.to_array(), radius))
    }

    /// Reverse traversal while preserving the complete helical locus.
    /// Axis and endpoint radii exchange roles; handedness and turn count stay fixed.
    pub fn reversed(&self) -> Option<Self> {
        if !self.is_valid() { return None; }
        if self.turns == 0.0 { return Some(*self); }
        let (center, x, y, axis) = self.frame()?;
        let angle = self.total_angle()?;
        let winding = match self.direction { HelixDirection::Clockwise => -1.0, HelixDirection::CounterClockwise => 1.0 };
        let reversed = Self {
            base_center: (center + axis * self.height).to_array(),
            axis_direction: (-axis).to_array(),
            start_direction: (x * angle.cos() + y * (winding * angle.sin())).to_array(),
            base_radius: self.top_radius,
            top_radius: self.base_radius,
            height: self.height,
            turns: self.turns,
            direction: self.direction,
        };
        (reversed.is_valid() && reversed.frame().is_some()).then_some(reversed)
    }

    const SEGMENTS_PER_TURN: f64 = 6.0;
    const MAX_SEGMENTS: usize = 100_000;

    fn frame(&self) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let center = Vec3::from(self.base_center);
        let axis = Vec3::from(self.axis_direction).normalize()?;
        let radial = Vec3::from(self.start_direction);
        let x_axis = (radial - axis * radial.dot(axis)).normalize()?;
        let y_axis = axis.cross(x_axis).normalize()?;
        Some((center, x_axis, y_axis, axis))
    }

    fn is_valid(&self) -> bool {
        self.base_center.iter().all(|value| value.is_finite())
            && self.axis_direction.iter().all(|value| value.is_finite())
            && self.start_direction.iter().all(|value| value.is_finite())
            && self.base_radius.is_finite()
            && self.top_radius.is_finite()
            && self.height.is_finite()
            && self.turns.is_finite()
            && self.base_radius >= 0.0
            && self.top_radius >= 0.0
            && (self.base_radius > 0.0 || self.top_radius > 0.0)
            && self.turns >= 0.0
            && (self.turns > 0.0 || self.height == 0.0)
    }

    fn total_angle(&self) -> Option<f64> {
        let angle = self.turns * std::f64::consts::TAU;
        angle.is_finite().then_some(angle)
    }

    fn segment_count(&self) -> Option<usize> {
        let count = (self.turns * Self::SEGMENTS_PER_TURN).ceil();
        (count.is_finite() && count >= 1.0 && count <= Self::MAX_SEGMENTS as f64)
            .then_some(count as usize)
    }

    fn point_and_derivative(
        &self,
        theta: f64,
        total_angle: f64,
        frame: (Vec3, Vec3, Vec3, Vec3),
    ) -> (Vec3, Vec3) {
        let (center, x_axis, y_axis, axis) = frame;
        let fraction = theta / total_angle;
        let radius = self.base_radius + (self.top_radius - self.base_radius) * fraction;
        let radial_rate = (self.top_radius - self.base_radius) / total_angle;
        let height_rate = self.height / total_angle;
        let winding = match self.direction {
            HelixDirection::Clockwise => -1.0,
            HelixDirection::CounterClockwise => 1.0,
        };
        let cosine = theta.cos();
        let sine = theta.sin();
        let point = center
            + x_axis * (radius * cosine)
            + y_axis * (winding * radius * sine)
            + axis * (self.height * fraction);
        let derivative = x_axis * (radial_rate * cosine - radius * sine)
            + y_axis * (winding * (radial_rate * sine + radius * cosine))
            + axis * height_rate;
        (point, derivative)
    }

    /// Builds a smooth cubic NURBS approximation from analytic tangents.
    pub fn nurbs(&self) -> Option<NurbsCurve3> {
        if !self.is_valid() {
            return None;
        }
        let frame = self.frame()?;
        if self.turns == 0.0 {
            let point = (frame.0 + frame.1 * self.base_radius).to_array();
            return NurbsCurve3::new_strict(1, vec![point, point], vec![0.0, 0.0, 1.0, 1.0], vec![1.0, 1.0]);
        }
        let total_angle = self.total_angle()?;
        let segments = self.segment_count()?;
        let angle_step = total_angle / segments as f64;
        let control_count = segments.checked_mul(3)?.checked_add(1)?;
        let mut controls = Vec::new();
        controls.try_reserve_exact(control_count).ok()?;

        for segment in 0..segments {
            let from = segment as f64 * angle_step;
            let to = (segment + 1) as f64 * angle_step;
            let (start, start_tangent) = self.point_and_derivative(from, total_angle, frame);
            let (end, end_tangent) = self.point_and_derivative(to, total_angle, frame);
            if segment == 0 {
                controls.push(start.to_array());
            }
            controls.push((start + start_tangent * (angle_step / 3.0)).to_array());
            controls.push((end - end_tangent * (angle_step / 3.0)).to_array());
            controls.push(end.to_array());
        }

        let mut knots = Vec::with_capacity(controls.len() + 4);
        knots.extend(std::iter::repeat_n(0.0, 4));
        for span in 1..segments {
            knots.extend(std::iter::repeat_n(span as f64, 3));
        }
        knots.extend(std::iter::repeat_n(segments as f64, 4));
        let weights = vec![1.0; controls.len()];
        NurbsCurve3::new_strict(3, controls, knots, weights)
    }

    /// Exact length of the mathematical circular or conical helix.
    pub fn length(&self) -> Option<f64> {
        if !self.is_valid() || self.frame().is_none() {
            return None;
        }
        if self.turns == 0.0 { return Some(0.0); }
        let total_angle = self.total_angle()?;
        let radius_delta = self.top_radius - self.base_radius;
        let radial_rate = radius_delta / total_angle;
        let height_rate = self.height / total_angle;
        let constant = radial_rate.hypot(height_rate);
        if radius_delta == 0.0 {
            let length = total_angle * self.base_radius.hypot(constant);
            return length.is_finite().then_some(length);
        }
        let radius_scale = self
            .base_radius
            .max(self.top_radius)
            .max(constant)
            .max(1.0);
        if radius_delta.abs() <= f64::EPSILON.sqrt() * radius_scale {
            let midpoint = 0.5 * (self.base_radius + self.top_radius);
            let norm = midpoint.hypot(constant);
            let correction = constant * constant * radius_delta * radius_delta
                / (24.0 * norm * norm * norm);
            let length = total_angle * (norm + correction);
            return length.is_finite().then_some(length);
        }
        let primitive = |radius: f64| {
            if constant <= 1.0e-15 {
                0.5 * radius * radius.abs()
            } else {
                0.5
                    * (radius * radius.hypot(constant)
                        + constant * constant * (radius / constant).asinh())
            }
        };
        let length =
            ((primitive(self.top_radius) - primitive(self.base_radius)) / radial_rate).abs();
        length.is_finite().then_some(length)
    }

    /// Taper angle between the helix envelope and its axis.
    pub fn turn_slope(&self) -> Option<f64> {
        (self.is_valid() && self.frame().is_some())
            .then(|| (self.top_radius - self.base_radius).atan2(self.height.abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_base_radius_reverses_between_the_same_endpoints() {
        let helix = HelixCurve { base_center: [1.0, 2.0, 3.0], axis_direction: [0.0, 0.0, 1.0],
            start_direction: [1.0, 0.0, 0.0], base_radius: 0.0, top_radius: 2.0, height: 5.0,
            turns: 1.25, direction: HelixDirection::CounterClockwise };
        let original = helix.nurbs().unwrap();
        let reversed = helix.reversed().unwrap().nurbs().unwrap();
        assert!(Vec3::from(original.point_at(0.0)).distance(Vec3::from(reversed.point_at(1.0))) < 1e-9);
        assert!(Vec3::from(original.point_at(1.0)).distance(Vec3::from(reversed.point_at(0.0))) < 1e-9);
    }

    #[test]
    fn a_zero_turn_zero_height_helix_is_one_exact_point() {
        let helix = HelixCurve {
            base_center: [1.0, 2.0, 3.0],
            axis_direction: [0.0, 0.0, 1.0],
            start_direction: [1.0, 0.0, 0.0],
            base_radius: 2.0,
            top_radius: 4.0,
            height: 0.0,
            turns: 0.0,
            direction: HelixDirection::CounterClockwise,
        };
        let curve = helix.nurbs().unwrap();
        assert_eq!(helix.length(), Some(0.0));
        assert_eq!(curve.point_at(0.0), [3.0, 2.0, 3.0]);
        assert_eq!(curve.point_at(1.0), [3.0, 2.0, 3.0]);
        assert_eq!(helix.reversed(), Some(helix));
    }
}
