//! NURBS in space: a curve that genuinely wanders, and a surface.
//!
//! [`geom2d`](crate::geom2d) has the plane's own NURBS, which is what a
//! drawing's SPLINE entity almost always is. This is the rest: a spline whose
//! points do not share a plane — a helix, a 3-D fit curve, an edge lifted out
//! of a file — and the tensor-product surface an ACIS `spline-surface` or a
//! DWG surface patch carries.
//!
//! # The same algorithm, one dimension up
//!
//! Both evaluate through [`space::spline::de_boor`](super::spline::de_boor),
//! which is written over however many coordinates a control point holds. A
//! surface is that applied twice: once along `u` for each row of the control
//! net, then once along `v` through the results. Writing the recursion again
//! per dimension is how the plane case and the space case drift apart.
//!
//! # Rational is polynomial, one dimension further up
//!
//! Weights are carried by multiplying each control point into homogeneous
//! coordinates and dividing at the very end. Dividing earlier — averaging
//! already-divided points — gives a curve that looks close and is not, and
//! the difference is exactly where the weights matter.

use super::spline::{clamped_uniform_knots, de_boor_by, interpolate_periodic, Parameterization};
use super::Vec3;

fn valid_knots(knots: &[f64]) -> bool {
    knots.iter().all(|value| value.is_finite())
        && knots.windows(2).all(|pair| pair[0] <= pair[1])
        && knots.first() < knots.last()
}

fn wrap_parameter(value: f64, start: f64, end: f64, periodic: bool) -> f64 {
    let span = end - start;
    if periodic && span.is_finite() && span > 0.0 && value != end {
        start + (value - start).rem_euclid(span)
    } else {
        value.clamp(start, end)
    }
}

/// A NURBS curve in space.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve3 {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<[f64; 3]>,
    weights: Vec<f64>,
    closed: bool,
}

impl NurbsCurve3 {
    /// A polynomial curve defined by its control polygon. Closed polygons
    /// produce a periodic curve with an exact clamped representation of one
    /// period; repeating the first point on an open curve is not equivalent.
    pub fn from_control_polygon(degree: usize, points: &[[f64; 3]], closed: bool) -> Option<Self> {
        Self::from_weighted_control_polygon(degree, points, &vec![1.0; points.len()], closed)
    }

    /// A rational curve defined by its weighted control polygon.
    pub fn from_weighted_control_polygon(
        degree: usize,
        points: &[[f64; 3]],
        weights: &[f64],
        closed: bool,
    ) -> Option<Self> {
        let count = points.len();
        if degree == 0
            || count <= degree
            || weights.len() != count
            || points.iter().flatten().any(|value| !value.is_finite())
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
        if !closed {
            return Self::new_strict(
                degree,
                points.to_vec(),
                clamped_uniform_knots(degree, count),
                weights.to_vec(),
            );
        }
        // Include a period on either side so both cuts are interior knots.
        // The seam follows the final degree control vertices, then wraps to
        // the first vertex of the supplied polygon.
        let mut controls: Vec<[f64; 4]> = (0..3 * count + degree)
            .map(|index| {
                let source = (index + count - degree) % count;
                let point = points[source];
                let weight = weights[source];
                [
                    point[0] * weight,
                    point[1] * weight,
                    point[2] * weight,
                    weight,
                ]
            })
            .collect();
        let mut knots: Vec<f64> = (0..controls.len() + degree + 1)
            .map(|index| index as f64)
            .collect();
        let from = (count + degree) as f64;
        let to = from + count as f64;
        for at in [from, to] {
            while knots.iter().filter(|&&knot| knot == at).count() < degree {
                let span = super::spline::span_of(degree, &knots, controls.len() - 1, at);
                let mut next = Vec::with_capacity(controls.len() + 1);
                next.extend_from_slice(&controls[..=span - degree]);
                for index in span - degree + 1..=span {
                    let width = knots[index + degree] - knots[index];
                    let weight = if width > 0.0 {
                        (at - knots[index]) / width
                    } else {
                        0.0
                    };
                    next.push(std::array::from_fn(|axis| {
                        controls[index - 1][axis] * (1.0 - weight) + controls[index][axis] * weight
                    }));
                }
                next.extend_from_slice(&controls[span..]);
                controls = next;
                knots.insert(span + 1, at);
            }
        }
        let first = knots.iter().rposition(|&knot| knot == from)? - degree;
        let last_knot = knots.iter().rposition(|&knot| knot == to)?;
        let last = last_knot - degree;
        let controls = &controls[first..=last];
        let points = controls
            .iter()
            .map(|point| {
                [
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                ]
            })
            .collect();
        let weights = controls.iter().map(|point| point[3]).collect();
        let knots = std::iter::once(from)
            .chain(knots[first + 1..=last_knot].iter().copied())
            .chain(std::iter::once(to))
            .map(|knot| knot - from)
            .collect();
        Self::new_strict(degree, points, knots, weights).map(|curve| curve.with_periodicity(true))
    }

    /// Reverse the parameter direction without changing the spatial curve.
    pub fn reversed(&self) -> Option<Self> {
        let (start, end) = self.domain();
        let mut reversed = Self::new_strict(
            self.degree,
            self.control_points.iter().copied().rev().collect(),
            self.knots
                .iter()
                .rev()
                .map(|value| start + end - value)
                .collect(),
            self.weights.iter().copied().rev().collect(),
        )?;
        reversed.closed = self.closed;
        Some(reversed)
    }

    /// The two pieces either side of knot parameter `at`, each clamped.
    /// `None` for a closed curve or a parameter outside the open domain.
    pub fn split_at(&self, at: f64) -> Option<(Self, Self)> {
        let (from, to) = self.domain();
        if self.closed || at <= from || at >= to {
            return None;
        }
        let degree = self.degree;
        let mut work = self.clone();
        let count = |knots: &[f64]| knots.iter().filter(|knot| (*knot - at).abs() < 1e-12).count();
        while count(&work.knots) < degree {
            work.insert_knot(at);
        }
        let first = work.knots.iter().position(|knot| (*knot - at).abs() < 1e-12)?;
        let mut left_knots = work.knots[..first].to_vec();
        left_knots.extend(std::iter::repeat_n(at, degree + 1));
        let mut right_knots = vec![at; degree + 1];
        right_knots.extend_from_slice(&work.knots[first + degree..]);
        let left = Self::new_strict(
            degree,
            work.control_points[..first].to_vec(),
            left_knots,
            work.weights[..first].to_vec(),
        )?;
        let right = Self::new_strict(
            degree,
            work.control_points[first - 1..].to_vec(),
            right_knots,
            work.weights[first - 1..].to_vec(),
        )?;
        Some((left, right))
    }

    /// This curve followed by `next`, which starts where this one ends: one
    /// clamped curve, continuous in position at the join. `None` for curves
    /// that are not open and clamped, or that do not meet.
    pub fn joined(&self, next: &Self) -> Option<Self> {
        self.joined_through(next, None)
    }

    /// [`joined`](Self::joined), the join put at `through` when given rather
    /// than halfway between the two ends.
    pub fn joined_through(&self, next: &Self, through: Option<[f64; 3]>) -> Option<Self> {
        let (mut first, mut second) = (self.clone(), next.clone());
        match first.degree.cmp(&second.degree) {
            std::cmp::Ordering::Less => first = first.elevated(second.degree - first.degree)?,
            std::cmp::Ordering::Greater => second = second.elevated(first.degree - second.degree)?,
            std::cmp::Ordering::Equal => {}
        }
        let degree = first.degree;
        let clamped = |curve: &Self| {
            let (from, to) = curve.domain();
            !curve.closed
                && curve.knots[..=degree].iter().all(|knot| *knot == from)
                && curve.knots[curve.knots.len() - degree - 1..].iter().all(|knot| *knot == to)
        };
        let (Some(&end), Some(&start)) =
            (first.control_points.last(), second.control_points.first())
        else {
            return None;
        };
        if !clamped(&first) || !clamped(&second) {
            return None;
        }
        let join =
            through.unwrap_or_else(|| std::array::from_fn(|axis| 0.5 * (end[axis] + start[axis])));
        let (_, at) = first.domain();
        let (from, _) = second.domain();
        // The second curve's weights are scaled to agree at the join; a
        // rational curve is unchanged by a common factor.
        let scale = first.weights.last()? / second.weights.first()?;
        let mut control_points = first.control_points.clone();
        *control_points.last_mut()? = join;
        control_points.extend(second.control_points.iter().skip(1).copied());
        let mut weights = first.weights.clone();
        weights.extend(second.weights.iter().skip(1).map(|weight| weight * scale));
        let mut knots = first.knots[..first.knots.len() - 1].to_vec();
        knots.extend(second.knots[degree + 1..].iter().map(|knot| knot - from + at));
        Self::new_strict(degree, control_points, knots, weights)
    }

    /// The same curve written at a higher degree without changing its shape.
    pub fn elevated(&self, by: usize) -> Option<Self> {
        if by == 0 {
            return None;
        }
        let mut curve = self.elevated_once()?;
        for _ in 1..by {
            curve = curve.elevated_once()?;
        }
        Some(curve)
    }

    /// The pair written on one degree and one knot vector over the same
    /// domain, each tracing the curve it was: what a ruled surface between
    /// them needs. `None` where their domains differ.
    pub fn made_compatible(&self, other: &Self) -> Option<(Self, Self)> {
        let degree = self.degree.max(other.degree);
        let raise = |curve: &Self| {
            if curve.degree == degree {
                Some(curve.clone())
            } else {
                curve.elevated(degree - curve.degree)
            }
        };
        let (mut first, mut second) = (raise(self)?, raise(other)?);
        let ((a0, a1), (b0, b1)) = (first.domain(), second.domain());
        let scale = 1e-10 * (a1 - a0).abs().max(1.0);
        if (a0 - b0).abs() > scale || (a1 - b1).abs() > scale {
            return None;
        }
        let count = |curve: &Self, at: f64| curve.knots.iter().filter(|knot| (**knot - at).abs() <= scale).count();
        for _ in 0..2 {
            let source = second.clone();
            let mut interior: Vec<f64> =
                source.knots.iter().copied().filter(|knot| *knot > a0 + scale && *knot < a1 - scale).collect();
            interior.dedup_by(|a, b| (*a - *b).abs() <= scale);
            for at in interior {
                for _ in count(&first, at)..count(&source, at) {
                    first.insert_knot(at);
                }
            }
            std::mem::swap(&mut first, &mut second);
        }
        if first.knots.len() != second.knots.len() {
            return None;
        }
        second.knots.clone_from(&first.knots);
        Some((first, second))
    }

    fn elevated_once(&self) -> Option<Self> {
        let degree = self.degree;
        let raised = degree + 1;
        let (segments, breaks) = self.bezier_segments();
        if segments.is_empty() {
            return None;
        }

        let mut control_points = Vec::new();
        let mut weights = Vec::new();
        for segment in &segments {
            let elevated: Vec<[f64; 4]> = (0..=raised)
                .map(|index| {
                    let alpha = index as f64 / raised as f64;
                    let before = index
                        .checked_sub(1)
                        .map_or([0.0; 4], |before| segment[before]);
                    let after = if index <= degree {
                        segment[index]
                    } else {
                        [0.0; 4]
                    };
                    std::array::from_fn(|axis| before[axis] * alpha + after[axis] * (1.0 - alpha))
                })
                .collect();
            for point in &elevated[usize::from(!control_points.is_empty())..] {
                control_points.push([
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                ]);
                weights.push(point[3]);
            }
        }

        let (start, end) = self.domain();
        let mut knots = vec![start; raised + 1];
        for at in breaks {
            knots.extend(std::iter::repeat_n(at, raised));
        }
        knots.extend(std::iter::repeat_n(end, raised + 1));
        Self::new_strict(raised, control_points, knots, weights)
            .map(|curve| curve.with_periodicity(self.closed))
    }

    fn bezier_segments(&self) -> (Vec<Vec<[f64; 4]>>, Vec<f64>) {
        let (start, end) = self.domain();
        let mut work = self.clone();
        loop {
            let mut multiplicities: Vec<(f64, usize)> = Vec::new();
            for &knot in &work.knots {
                if knot <= start + 1e-12 || knot >= end - 1e-12 {
                    continue;
                }
                match multiplicities
                    .iter_mut()
                    .find(|(at, _)| (*at - knot).abs() < 1e-12)
                {
                    Some((_, count)) => *count += 1,
                    None => multiplicities.push((knot, 1)),
                }
            }
            let missing: Vec<f64> = multiplicities
                .into_iter()
                .filter(|(_, count)| *count < work.degree)
                .map(|(at, _)| at)
                .collect();
            if missing.is_empty() {
                break;
            }
            for at in missing {
                work.insert_knot(at);
            }
        }

        let mut breaks: Vec<f64> = Vec::new();
        for &knot in &work.knots {
            if knot > start + 1e-12
                && knot < end - 1e-12
                && breaks
                    .last()
                    .is_none_or(|last| (*last - knot).abs() > 1e-12)
            {
                breaks.push(knot);
            }
        }
        let homogeneous = work.homogeneous();
        let segments = (0..(homogeneous.len() - 1) / work.degree)
            .map(|index| {
                let from = index * work.degree;
                homogeneous[from..=from + work.degree].to_vec()
            })
            .collect();
        (segments, breaks)
    }

    fn insert_knot(&mut self, knot: f64) {
        let (start, end) = self.domain();
        if knot <= start || knot >= end {
            return;
        }
        let span = super::spline::span_of(
            self.degree,
            &self.knots,
            self.control_points.len() - 1,
            knot,
        );
        let homogeneous = self.homogeneous();
        let mut updated = Vec::with_capacity(homogeneous.len() + 1);
        updated.extend_from_slice(&homogeneous[..=span - self.degree]);
        for index in span - self.degree + 1..=span {
            let width = self.knots[index + self.degree] - self.knots[index];
            let alpha = if width.abs() < 1e-15 {
                0.0
            } else {
                (knot - self.knots[index]) / width
            };
            updated.push(std::array::from_fn(|axis| {
                homogeneous[index - 1][axis] * (1.0 - alpha) + homogeneous[index][axis] * alpha
            }));
        }
        updated.extend_from_slice(&homogeneous[span..]);

        self.knots.insert(span + 1, knot);
        self.control_points = updated
            .iter()
            .map(|point| {
                [
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                ]
            })
            .collect();
        self.weights = updated.iter().map(|point| point[3]).collect();
    }

    fn homogeneous(&self) -> Vec<[f64; 4]> {
        self.control_points
            .iter()
            .zip(&self.weights)
            .map(|(point, &weight)| {
                [
                    point[0] * weight,
                    point[1] * weight,
                    point[2] * weight,
                    weight,
                ]
            })
            .collect()
    }

    /// Delete one control vertex from an open clamped curve, intentionally
    /// changing its shape. Remove the associated interior knot; if fewer
    /// vertices remain than the current order, lower the degree by one.
    pub fn without_control_vertex(&self, index: usize) -> Option<Self> {
        let count = self.control_points.len();
        let degree = self.degree;
        if self.closed || count <= 2 || index >= count {
            return None;
        }
        let (start, end) = self.domain();
        if !self.knots[..=degree].iter().all(|knot| *knot == start)
            || !self.knots[count..].iter().all(|knot| *knot == end)
        {
            return None;
        }
        let mut controls = self.control_points.clone();
        let mut weights = self.weights.clone();
        let mut knots = self.knots.clone();
        controls.remove(index);
        weights.remove(index);
        let degree = if controls.len() <= degree {
            knots.pop();
            knots.remove(0);
            degree - 1
        } else {
            knots.remove((index + degree.div_ceil(2)).clamp(degree + 1, count - 1));
            degree
        };
        Self::new_strict(degree, controls, knots, weights)
    }

    /// Open cubic interpolation in full spatial coordinates.
    pub fn interpolate_fit(
        points: &[[f64; 3]],
        start_tangent: Option<[f64; 3]>,
        end_tangent: Option<[f64; 3]>,
        parameterization: Parameterization,
    ) -> Option<Self> {
        let (controls, knots) =
            super::spline::interpolate_open(points, start_tangent, end_tangent, parameterization)?;
        let weights = vec![1.0; controls.len()];
        Self::new_strict(3, controls, knots, weights)
    }

    /// Analytic rational derivative with respect to the knot parameter.
    pub fn derivative_at_knot(&self, parameter: f64) -> [f64; 3] {
        let (start, end) = self.domain();
        let parameter = wrap_parameter(parameter, start, end, self.closed);
        let scale = self.weights.iter().copied().fold(0.0_f64, f64::max);
        let homogeneous = |index: usize| {
            let weight = self.weights[index] / scale;
            let point = self.control_points[index];
            [
                point[0] * weight,
                point[1] * weight,
                point[2] * weight,
                weight,
            ]
        };
        let value = de_boor_by(
            self.degree,
            &self.knots,
            self.control_points.len(),
            parameter,
            homogeneous,
        );
        let derivative = de_boor_by(
            self.degree - 1,
            &self.knots[1..self.knots.len() - 1],
            self.control_points.len() - 1,
            parameter,
            |index| {
                let width = self.knots[index + self.degree + 1] - self.knots[index + 1];
                if width == 0.0 {
                    return [0.0; 4];
                }
                let a = homogeneous(index);
                let b = homogeneous(index + 1);
                std::array::from_fn(|axis| (b[axis] - a[axis]) * self.degree as f64 / width)
            },
        );
        if value[3] <= 0.0 {
            return [f64::NAN; 3];
        }
        std::array::from_fn(|axis| {
            (derivative[axis] - value[axis] / value[3] * derivative[3]) / value[3]
        })
    }

    /// Builds a curve, filling in what the caller left out.
    ///
    /// A knot vector of the wrong length is replaced with a clamped uniform
    /// one — a drawing with a malformed spline should still draw something
    /// rather than nothing. Weights are optional; absent means all ones and
    /// the curve is polynomial.
    ///
    /// `None` when there are fewer control points than the degree needs,
    /// since there is then no curve to evaluate at all.
    pub fn new(
        degree: usize,
        control_points: Vec<[f64; 3]>,
        knots: Vec<f64>,
        weights: Option<Vec<f64>>,
    ) -> Option<Self> {
        if control_points.len() <= degree || degree == 0 {
            return None;
        }
        let wanted = control_points.len() + degree + 1;
        let knots = if knots.len() == wanted {
            knots
        } else {
            clamped_uniform_knots(degree, control_points.len())
        };
        let weights = match weights {
            Some(weights) if weights.len() == control_points.len() => weights,
            _ => vec![1.0; control_points.len()],
        };
        Some(Self {
            degree,
            knots,
            control_points,
            weights,
            closed: false,
        })
    }

    /// Builds a curve only when every supplied value is valid.
    pub fn new_strict(
        degree: usize,
        control_points: Vec<[f64; 3]>,
        knots: Vec<f64>,
        weights: Vec<f64>,
    ) -> Option<Self> {
        if degree == 0
            || control_points.len() <= degree
            || knots.len() != control_points.len() + degree + 1
            || weights.len() != control_points.len()
            || !valid_knots(&knots)
            || !control_points
                .iter()
                .flatten()
                .all(|value| value.is_finite())
            || !weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        {
            return None;
        }
        Some(Self {
            degree,
            knots,
            control_points,
            weights,
            closed: false,
        })
    }

    /// The closed C² cubic through every fit point.
    pub fn interpolate_periodic(
        points: &[[f64; 3]],
        parameterization: Parameterization,
    ) -> Option<Self> {
        let (control_points, knots) = interpolate_periodic(points, parameterization)?;
        let weights = vec![1.0; control_points.len()];
        Some(Self {
            degree: 3,
            knots,
            control_points,
            weights,
            closed: true,
        })
    }

    pub fn with_periodicity(mut self, closed: bool) -> Self {
        self.closed = closed;
        self
    }

    pub fn periodicity(&self) -> bool {
        self.closed
    }

    /// The degree of the curve.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Its control points, in order.
    pub fn control_points(&self) -> &[[f64; 3]] {
        &self.control_points
    }

    /// Its knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Its weights, all ones when the curve is polynomial.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// Whether any weight differs from the others. A curve whose weights are
    /// all equal is polynomial however large they are.
    pub fn is_rational(&self) -> bool {
        let first = self.weights.first().copied().unwrap_or(1.0);
        self.weights
            .iter()
            .any(|weight| (weight - first).abs() > 1e-12)
    }

    /// The knot range the curve is defined over.
    pub fn domain(&self) -> (f64, f64) {
        let last = self.control_points.len();
        (self.knots[self.degree], self.knots[last])
    }

    /// The point at a knot parameter.
    pub fn point_at_knot(&self, u: f64) -> [f64; 3] {
        let (from, to) = self.domain();
        let u = wrap_parameter(u, from, to, self.closed);
        let raw = de_boor_by(
            self.degree,
            &self.knots,
            self.control_points.len(),
            u,
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                [
                    point[0] * weight,
                    point[1] * weight,
                    point[2] * weight,
                    weight,
                ]
            },
        );
        if raw[3].abs() < 1e-15 {
            return [raw[0], raw[1], raw[2]];
        }
        [raw[0] / raw[3], raw[1] / raw[3], raw[2] / raw[3]]
    }

    /// The point at `t` in `0..=1`, which is the domain rescaled.
    ///
    /// Every other curve in this kernel runs zero to one, and a caller
    /// stepping along one should not have to ask which convention it is.
    pub fn point_at(&self, t: f64) -> [f64; 3] {
        let (from, to) = self.domain();
        self.point_at_knot(from + (to - from) * t.clamp(0.0, 1.0))
    }

    /// The knot parameter nearest `point`.
    pub fn parameter_at(&self, point: [f64; 3]) -> f64 {
        let (from, to) = self.domain();
        let target = Vec3::from(point);
        let distance = |parameter: f64| {
            Vec3::from(self.point_at_knot(parameter))
                .distance(target)
                .powi(2)
        };
        let steps = 64usize;
        let mut best = 0usize;
        let mut best_distance = f64::INFINITY;
        for step in 0..=steps {
            let parameter = from + (to - from) * step as f64 / steps as f64;
            let here = distance(parameter);
            if here < best_distance {
                best = step;
                best_distance = here;
            }
        }
        // A closed curve's nearest point may lie just round the seam from
        // the best sample, so its bracket runs past the domain's ends.
        let (below, above) = if self.closed {
            (best as f64 - 1.0, best as f64 + 1.0)
        } else {
            (best.saturating_sub(1) as f64, (best + 1).min(steps) as f64)
        };
        // Down to rounding: a corner placed on the curve is found on it again
        // within a modelling tolerance, not a sampling one.
        let narrow = |below: f64, above: f64| {
            let mut low = from + (to - from) * below / steps as f64;
            let mut high = from + (to - from) * above / steps as f64;
            if let Some(found) = self.newton_nearest(target, low, high) {
                if distance(found) <= distance(low).min(distance(high)) {
                    return wrap_parameter(found, from, to, self.closed);
                }
            }
            for _ in 0..64 {
                let third = (high - low) / 3.0;
                let one = low + third;
                let two = high - third;
                if distance(one) <= distance(two) {
                    high = two;
                } else {
                    low = one;
                }
            }
            wrap_parameter(0.5 * (low + high), from, to, self.closed)
        };
        let found = narrow(below, above);
        // Clamped but closed, its two ends are one sample and the start won
        // only the tie: a point just short of the end is narrowed there too.
        if !self.closed && (best == 0 || best == steps) && self.is_closed() {
            let other = if best == 0 {
                narrow(steps as f64 - 1.0, steps as f64)
            } else {
                narrow(0.0, 1.0)
            };
            if distance(other) < distance(found) {
                return other;
            }
        }
        found
    }

    /// The parameter nearest `target` between `low` and `high`, by Newton's
    /// method on the squared distance from the bracket's middle: a few
    /// evaluations where a search takes a hundred. `None` where it does not
    /// settle — the distance not convex there, or a step leaving the bracket
    /// twice — for the caller's search to take over.
    fn newton_nearest(&self, target: Vec3, low: f64, high: f64) -> Option<f64> {
        let step = (high - low) * 1e-3;
        let mut parameter = 0.5 * (low + high);
        let mut clamped = 0;
        for _ in 0..16 {
            let behind = Vec3::from(self.point_at_knot(parameter - step));
            let here = Vec3::from(self.point_at_knot(parameter));
            let ahead = Vec3::from(self.point_at_knot(parameter + step));
            let first = (ahead - behind) / (2.0 * step);
            let second = (ahead - here * 2.0 + behind) / (step * step);
            let offset = here - target;
            let slope = offset.dot(first);
            let curvature = first.dot(first) + offset.dot(second);
            if !(curvature > 0.0) {
                return None;
            }
            let mut next = parameter - slope / curvature;
            if next < low || next > high {
                clamped += 1;
                if clamped > 1 {
                    return None;
                }
                next = next.clamp(low, high);
            }
            if (next - parameter).abs() <= (high - low) * 1e-14 {
                return Some(next);
            }
            parameter = next;
        }
        None
    }

    /// Solve tangency from an external point near a knot-parameter seed.
    /// Returns only validated solutions; a spatial curve need not have one.
    pub fn tangent_from_near(&self, from: [f64; 3], seed: f64) -> Option<[f64; 3]> {
        let from = Vec3::from(from);
        let (start, end) = self.domain();
        let span = end - start;
        if !from.is_finite() || !seed.is_finite() || !span.is_finite() || span <= 1e-15 {
            return None;
        }
        let residual = |u| {
            (Vec3::from(self.point_at_knot(u)) - from).cross(Vec3::from(self.derivative_at_knot(u)))
        };
        let mut u = seed.clamp(start, end);
        for _ in 0..48 {
            let value = residual(u);
            let lo = (u - span * 1e-6).max(start);
            let hi = (u + span * 1e-6).min(end);
            let derivative = (residual(hi) - residual(lo)) / (hi - lo);
            if !value.is_finite() || !derivative.is_finite() {
                return None;
            }
            if derivative.length_squared() <= 1e-30 {
                break;
            }
            let next = (u - value.dot(derivative) / derivative.length_squared()).clamp(start, end);
            if (next - u).abs() < span * 1e-14 {
                u = next;
                break;
            }
            u = next;
        }
        let point = Vec3::from(self.point_at_knot(u));
        let derivative = Vec3::from(self.derivative_at_knot(u));
        let scale = (point - from).length() * derivative.length();
        if !point.is_finite()
            || !scale.is_finite()
            || scale <= 1e-20
            || residual(u).length() > 1e-8 * scale
        {
            return None;
        }
        Some(point.to_array())
    }

    /// The tangent at a knot parameter, by a central difference.
    pub fn tangent_at_knot(&self, u: f64) -> [f64; 3] {
        let Some((here, step)) = self.sampling_at(u) else {
            return [0.0; 3];
        };
        let behind = Vec3::from(self.point_at_knot(here - step));
        let ahead = Vec3::from(self.point_at_knot(here + step));
        ((ahead - behind) / (2.0 * step)).to_array()
    }

    /// The second derivative at a knot parameter, by a central second
    /// difference. What a curvature is worked out from.
    pub fn acceleration_at_knot(&self, u: f64) -> [f64; 3] {
        let Some((here, step)) = self.sampling_at(u) else {
            return [0.0; 3];
        };
        let behind = Vec3::from(self.point_at_knot(here - step));
        let middle = Vec3::from(self.point_at_knot(here));
        let ahead = Vec3::from(self.point_at_knot(here + step));
        ((ahead - middle * 2.0 + behind) / (step * step)).to_array()
    }

    /// Where to centre a difference at `u`, and how wide to make it.
    ///
    /// Held far enough inside the domain that both samples land on the curve.
    /// At an end a difference reaching past it would sample a clamped
    /// repetition of the last point and read the slope as half what it is.
    fn sampling_at(&self, u: f64) -> Option<(f64, f64)> {
        let (from, to) = self.domain();
        let span = to - from;
        if span <= 0.0 {
            return None;
        }
        let step = span * 1e-4;
        // A closed curve has no end to keep inside: a parameter a turn on
        // — an edge's run past the seam — is the same place, and clamping
        // it took the slope at the curve's end instead.
        if self.closed {
            return Some((wrap_parameter(u, from, to, true), step));
        }
        Some((u.clamp(from + step, to - step), step))
    }

    /// The tangent at `t` in `0..=1`.
    pub fn tangent_at(&self, t: f64) -> [f64; 3] {
        let (from, to) = self.domain();
        self.tangent_at_knot(from + (to - from) * t.clamp(0.0, 1.0))
    }

    /// Whether the curve returns to where it started.
    pub fn is_closed(&self) -> bool {
        if self.closed {
            return true;
        }
        let (from, to) = self.domain();
        Vec3::from(self.point_at_knot(from)).distance(Vec3::from(self.point_at_knot(to))) < 1e-9
    }

    /// Points along the curve, no farther from it than `tolerance`.
    ///
    /// Refined where it bends rather than sampled evenly: a curve that is
    /// nearly straight for most of its length and turns sharply once needs
    /// its points where the turn is, and a uniform sampling either misses the
    /// turn or carries thousands of points through the straight part.
    pub fn tessellate_within(&self, tolerance: f64) -> Vec<[f64; 3]> {
        let (from, to) = self.domain();
        let mut points = vec![self.point_at_knot(from)];
        self.refine(from, to, tolerance.max(1e-12), 0, &mut points);
        points.push(self.point_at_knot(to));
        points
    }

    /// Samples by maximum tangent rotation, in radians.
    pub fn tessellate_angle(&self, max_angle: f64) -> Vec<[f64; 3]> {
        let max_angle = crate::tessellation::angle(max_angle);
        let (from, to) = self.domain();
        let mut boundaries = vec![from];
        for knot in &self.knots {
            if *knot > from && *knot < to && boundaries.last() != Some(knot) {
                boundaries.push(*knot);
            }
        }
        boundaries.push(to);
        let mut points = vec![self.point_at_knot(from)];
        for pair in boundaries.windows(2) {
            self.refine_angle(pair[0], pair[1], max_angle, 0, &mut points);
        }
        points
    }

    fn refine_angle(
        &self,
        from: f64,
        to: f64,
        max_angle: f64,
        depth: u32,
        out: &mut Vec<[f64; 3]>,
    ) {
        const MAX_DEPTH: u32 = 16;
        let directions = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
            .map(|unit| self.tangent_at_knot(from + (to - from) * unit));
        let split = depth < 2 || crate::tessellation::max_direction_angle(&directions) > max_angle;
        if split && depth < MAX_DEPTH {
            let middle = 0.5 * (from + to);
            self.refine_angle(from, middle, max_angle, depth + 1, out);
            self.refine_angle(middle, to, max_angle, depth + 1, out);
        } else {
            out.push(self.point_at_knot(to));
        }
    }

    /// Splits a span until its middle sits close enough to the chord.
    fn refine(&self, from: f64, to: f64, tolerance: f64, depth: u32, out: &mut Vec<[f64; 3]>) {
        // Sixteen levels is sixty-five thousand points on one span, which no
        // tolerance a drawing uses reaches; it is a backstop against a curve
        // that never converges, not a limit.
        const MAX_DEPTH: u32 = 16;
        let middle = 0.5 * (from + to);
        let curved = Vec3::from(self.point_at_knot(middle));
        if depth < MAX_DEPTH {
            let start = Vec3::from(self.point_at_knot(from));
            let end = Vec3::from(self.point_at_knot(to));
            if start.lerp(end, 0.5).distance(curved) > tolerance {
                self.refine(from, middle, tolerance, depth + 1, out);
                out.push(curved.to_array());
                self.refine(middle, to, tolerance, depth + 1, out);
                return;
            }
        }
    }
}

/// A NURBS surface in space.
///
/// The control net is stored row by row: `control_points[i][j]` is the point
/// at `u` index `i` and `v` index `j`, which is the order every file format
/// writes and the order the evaluation below reads.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface3 {
    u_degree: usize,
    v_degree: usize,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    control_points: Vec<Vec<[f64; 3]>>,
    weights: Vec<Vec<f64>>,
    u_closed: bool,
    v_closed: bool,
    v_reversed: bool,
    samples: Samples,
}

/// The coarse grid the nearest-point search starts from, evaluated once a
/// surface: it depends on the surface alone, and a cut walking a spline
/// across a patch asked it tens of thousands of times. Not part of what the
/// surface is — two surfaces equal but for it are equal.
#[derive(Clone, Default)]
struct Samples(std::sync::OnceLock<Vec<(f64, f64, [f64; 3])>>);

impl PartialEq for Samples {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Samples {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("..")
    }
}

impl NurbsSurface3 {
    /// Builds a surface, filling in what the caller left out.
    ///
    /// `None` for a ragged control net, or one too small for the degrees:
    /// a surface with rows of different lengths is not a tensor product and
    /// there is nothing sensible to evaluate.
    pub fn new(
        u_degree: usize,
        v_degree: usize,
        control_points: Vec<Vec<[f64; 3]>>,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        weights: Option<Vec<Vec<f64>>>,
    ) -> Option<Self> {
        let rows = control_points.len();
        let columns = control_points.first()?.len();
        if rows <= u_degree || columns <= v_degree || u_degree == 0 || v_degree == 0 {
            return None;
        }
        if control_points.iter().any(|row| row.len() != columns) {
            return None;
        }
        let u_knots = if u_knots.len() == rows + u_degree + 1 {
            u_knots
        } else {
            clamped_uniform_knots(u_degree, rows)
        };
        let v_knots = if v_knots.len() == columns + v_degree + 1 {
            v_knots
        } else {
            clamped_uniform_knots(v_degree, columns)
        };
        let weights = match weights {
            Some(weights)
                if weights.len() == rows && weights.iter().all(|row| row.len() == columns) =>
            {
                weights
            }
            _ => vec![vec![1.0; columns]; rows],
        };
        let u_closed = matching_seam(
            control_points[0].iter().zip(&weights[0]),
            control_points[rows - 1].iter().zip(&weights[rows - 1]),
        );
        let v_closed = matching_seam(
            control_points
                .iter()
                .map(|row| &row[0])
                .zip(weights.iter().map(|row| &row[0])),
            control_points
                .iter()
                .map(|row| &row[columns - 1])
                .zip(weights.iter().map(|row| &row[columns - 1])),
        );
        Some(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            control_points,
            weights,
            u_closed,
            v_closed,
            v_reversed: false,
            samples: Samples::default(),
        })
    }

    /// Builds a polynomial surface from an open or periodic control net.
    pub fn from_control_net(
        u_degree: usize,
        v_degree: usize,
        mut control_points: Vec<Vec<[f64; 3]>>,
        u_periodic: bool,
        v_periodic: bool,
    ) -> Option<Self> {
        let rows = control_points.len();
        let columns = control_points.first()?.len();
        if u_degree == 0
            || v_degree == 0
            || rows <= u_degree
            || columns <= v_degree
            || control_points.iter().any(|row| row.len() != columns)
            || !control_points
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite())
        {
            return None;
        }

        if v_periodic {
            for row in &mut control_points {
                row.extend_from_within(..v_degree);
            }
        }
        if u_periodic {
            control_points.extend_from_within(..u_degree);
        }
        let rows = control_points.len();
        let columns = control_points[0].len();
        let knots = |degree: usize, count: usize, periodic: bool| {
            if periodic {
                (0..count + degree + 1).map(|index| index as f64).collect()
            } else {
                clamped_uniform_knots(degree, count)
            }
        };
        let mut surface = Self::new_strict(
            u_degree,
            v_degree,
            control_points,
            knots(u_degree, rows, u_periodic),
            knots(v_degree, columns, v_periodic),
            vec![vec![1.0; columns]; rows],
        )?;
        surface.u_closed = u_periodic;
        surface.v_closed = v_periodic;
        Some(surface)
    }

    /// Builds a surface only when every supplied value is valid.
    pub fn new_strict(
        u_degree: usize,
        v_degree: usize,
        control_points: Vec<Vec<[f64; 3]>>,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        weights: Vec<Vec<f64>>,
    ) -> Option<Self> {
        let rows = control_points.len();
        let columns = control_points.first()?.len();
        if u_degree == 0
            || v_degree == 0
            || rows <= u_degree
            || columns <= v_degree
            || control_points.iter().any(|row| row.len() != columns)
            || weights.len() != rows
            || weights.iter().any(|row| row.len() != columns)
            || u_knots.len() != rows + u_degree + 1
            || v_knots.len() != columns + v_degree + 1
            || !valid_knots(&u_knots)
            || !valid_knots(&v_knots)
            || !control_points
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite())
            || !weights
                .iter()
                .flatten()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        {
            return None;
        }
        let u_closed = matching_seam(
            control_points[0].iter().zip(&weights[0]),
            control_points[rows - 1].iter().zip(&weights[rows - 1]),
        );
        let v_closed = matching_seam(
            control_points
                .iter()
                .map(|row| &row[0])
                .zip(weights.iter().map(|row| &row[0])),
            control_points
                .iter()
                .map(|row| &row[columns - 1])
                .zip(weights.iter().map(|row| &row[columns - 1])),
        );
        Some(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            control_points,
            weights,
            u_closed,
            v_closed,
            v_reversed: false,
            samples: Samples::default(),
        })
    }

    pub fn with_periodicity(mut self, u_closed: bool, v_closed: bool) -> Self {
        self.u_closed |= u_closed;
        self.v_closed |= v_closed;
        self
    }

    pub fn with_v_reversed(mut self, reversed: bool) -> Self {
        self.v_reversed = reversed;
        self.samples = Samples::default();
        self
    }

    pub fn v_reversed(&self) -> bool {
        self.v_reversed
    }

    pub fn periodicity(&self) -> [bool; 2] {
        [self.u_closed, self.v_closed]
    }

    /// The knot ranges the surface is defined over, `u` then `v`.
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (
            (
                self.u_knots[self.u_degree],
                self.u_knots[self.control_points.len()],
            ),
            (
                self.v_knots[self.v_degree],
                self.v_knots[self.control_points[0].len()],
            ),
        )
    }

    /// The knot parameters of the nearest point on the surface.
    ///
    /// NURBS surfaces have no closed-form inverse. A coarse search finds a
    /// stable basin, then a damped Gauss-Newton iteration follows the surface
    /// tangents to the nearest point.
    pub fn parameters_at(&self, point: [f64; 3]) -> Option<(f64, f64)> {
        const GRID: usize = 16;
        const ITERATIONS: usize = 32;
        let ((u0, u1), (v0, v1)) = self.domain();
        if ![u0, u1, v0, v1].into_iter().all(f64::is_finite)
            || u1 <= u0
            || v1 <= v0
        {
            return None;
        }
        let target = Vec3::from(point);
        let samples = self.samples.0.get_or_init(|| {
            let mut samples = Vec::with_capacity((GRID + 1) * (GRID + 1));
            for u_index in 0..=GRID {
                let u = u0 + (u1 - u0) * u_index as f64 / GRID as f64;
                for v_index in 0..=GRID {
                    let v = v0 + (v1 - v0) * v_index as f64 / GRID as f64;
                    samples.push((u, v, self.point_at_knot(u, v)));
                }
            }
            samples
        });
        let mut best = (f64::INFINITY, u0, v0);
        for (u, v, at) in samples {
            let distance = (Vec3::from(*at) - target).length_squared();
            if distance.is_finite() && distance < best.0 {
                best = (distance, *u, *v);
            }
        }
        if !best.0.is_finite() {
            return None;
        }
        let (mut u, mut v) = (best.1, best.2);
        for _ in 0..ITERATIONS {
            let position = Vec3::from(self.point_at_knot(u, v));
            let residual = position - target;
            let Some((along_u, along_v)) = self.tangents_at_knot(u, v) else {
                break;
            };
            let (along_u, along_v) = (Vec3::from(along_u), Vec3::from(along_v));
            let uu = along_u.dot(along_u);
            let uv = along_u.dot(along_v);
            let vv = along_v.dot(along_v);
            let determinant = uu * vv - uv * uv;
            let ru = along_u.dot(residual);
            let rv = along_v.dot(residual);
            let step = if determinant.is_finite() && determinant > f64::EPSILON * 64.0 * uu * vv {
                [
                    (-ru * vv + uv * rv) / determinant,
                    (uv * ru - uu * rv) / determinant,
                ]
            } else if uu >= vv && uu > 0.0 {
                // A boundary collapsed to a point — a pole, a round closing
                // where its faces meet tangentially — has no tangent across
                // it, and the coarse search lands every point near it there:
                // step along the tangent it still has, off the point.
                [-ru / uu, 0.0]
            } else if vv > 0.0 {
                [0.0, -rv / vv]
            } else {
                break;
            };
            let current = residual.length_squared();
            let mut accepted = false;
            for divisor in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
                let candidate_u =
                    wrap_parameter(u + step[0] / divisor, u0, u1, self.u_closed);
                let candidate_v =
                    wrap_parameter(v + step[1] / divisor, v0, v1, self.v_closed);
                let candidate =
                    (Vec3::from(self.point_at_knot(candidate_u, candidate_v)) - target)
                        .length_squared();
                if candidate.is_finite() && candidate < current {
                    u = candidate_u;
                    v = candidate_v;
                    accepted = true;
                    break;
                }
            }
            if !accepted || step[0].hypot(step[1]) <= f64::EPSILON * 64.0 {
                break;
            }
        }
        Some((u, v))
    }

    pub fn degrees(&self) -> (usize, usize) {
        (self.u_degree, self.v_degree)
    }

    pub fn knots(&self) -> (&[f64], &[f64]) {
        (&self.u_knots, &self.v_knots)
    }

    pub fn control_points(&self) -> &[Vec<[f64; 3]>] {
        &self.control_points
    }

    pub fn weights(&self) -> &[Vec<f64>] {
        &self.weights
    }

    /// The exact surface curve at a fixed `u` (`axis = 0`) or `v` (`axis = 1`).
    pub fn isocurve(&self, axis: usize, parameter: f64) -> Option<NurbsCurve3> {
        let ((u0, u1), (v0, v1)) = self.domain();
        let (degree, mut knots, mut points, mut weights, periodic) = match axis {
            0 => {
                let u = wrap_parameter(parameter, u0, u1, self.u_closed);
                let mut points = Vec::with_capacity(self.control_points[0].len());
                let mut weights = Vec::with_capacity(self.control_points[0].len());
                for column in 0..self.control_points[0].len() {
                    let value = de_boor_by(
                        self.u_degree,
                        &self.u_knots,
                        self.control_points.len(),
                        u,
                        |row| self.homogeneous_control(row, column),
                    );
                    let weight = value[3];
                    points.push([value[0] / weight, value[1] / weight, value[2] / weight]);
                    weights.push(weight);
                }
                (
                    self.v_degree,
                    self.v_knots.clone(),
                    points,
                    weights,
                    self.v_closed,
                )
            }
            1 => {
                let v = wrap_parameter(parameter, v0, v1, self.v_closed);
                let v = if self.v_reversed { v0 + v1 - v } else { v };
                let mut points = Vec::with_capacity(self.control_points.len());
                let mut weights = Vec::with_capacity(self.control_points.len());
                for row in 0..self.control_points.len() {
                    let value = de_boor_by(
                        self.v_degree,
                        &self.v_knots,
                        self.control_points[row].len(),
                        v,
                        |column| self.homogeneous_control(row, column),
                    );
                    let weight = value[3];
                    points.push([value[0] / weight, value[1] / weight, value[2] / weight]);
                    weights.push(weight);
                }
                (
                    self.u_degree,
                    self.u_knots.clone(),
                    points,
                    weights,
                    self.u_closed,
                )
            }
            _ => return None,
        };
        if axis == 0 && self.v_reversed {
            points.reverse();
            weights.reverse();
            let sum = knots.get(degree)? + knots.get(points.len())?;
            knots = knots.into_iter().rev().map(|knot| sum - knot).collect();
        }
        Some(NurbsCurve3::new_strict(degree, points, knots, weights)?.with_periodicity(periodic))
    }

    fn homogeneous_control(&self, row: usize, column: usize) -> [f64; 4] {
        let point = self.control_points[row][column];
        let weight = self.weights[row][column];
        [
            point[0] * weight,
            point[1] * weight,
            point[2] * weight,
            weight,
        ]
    }

    fn homogeneous_at(&self, u: f64, v: f64) -> [f64; 4] {
        de_boor_by(
            self.u_degree,
            &self.u_knots,
            self.control_points.len(),
            u,
            |row| {
                de_boor_by(
                    self.v_degree,
                    &self.v_knots,
                    self.control_points[row].len(),
                    v,
                    |column| self.homogeneous_control(row, column),
                )
            },
        )
    }

    fn homogeneous_u_derivative_at(&self, u: f64, v: f64) -> [f64; 4] {
        if self.u_degree == 0 || self.control_points.len() < 2 {
            return [0.0; 4];
        }
        de_boor_by(
            self.u_degree - 1,
            &self.u_knots[1..self.u_knots.len() - 1],
            self.control_points.len() - 1,
            u,
            |row| {
                let denominator = self.u_knots[row + self.u_degree + 1] - self.u_knots[row + 1];
                let factor = if denominator.abs() > f64::EPSILON {
                    self.u_degree as f64 / denominator
                } else {
                    0.0
                };
                de_boor_by(
                    self.v_degree,
                    &self.v_knots,
                    self.control_points[row].len(),
                    v,
                    |column| {
                        let before = self.homogeneous_control(row, column);
                        let after = self.homogeneous_control(row + 1, column);
                        std::array::from_fn(|axis| (after[axis] - before[axis]) * factor)
                    },
                )
            },
        )
    }

    fn homogeneous_v_derivative_at(&self, u: f64, v: f64) -> [f64; 4] {
        if self.v_degree == 0 || self.control_points[0].len() < 2 {
            return [0.0; 4];
        }
        de_boor_by(
            self.u_degree,
            &self.u_knots,
            self.control_points.len(),
            u,
            |row| {
                de_boor_by(
                    self.v_degree - 1,
                    &self.v_knots[1..self.v_knots.len() - 1],
                    self.control_points[row].len() - 1,
                    v,
                    |column| {
                        let denominator =
                            self.v_knots[column + self.v_degree + 1] - self.v_knots[column + 1];
                        let factor = if denominator.abs() > f64::EPSILON {
                            self.v_degree as f64 / denominator
                        } else {
                            0.0
                        };
                        let before = self.homogeneous_control(row, column);
                        let after = self.homogeneous_control(row, column + 1);
                        std::array::from_fn(|axis| (after[axis] - before[axis]) * factor)
                    },
                )
            },
        )
    }

    /// The point at knot parameters `(u, v)`.
    ///
    /// Two passes of the same algorithm: along `v` through each row of the
    /// net, then along `u` through what those gave. Homogeneous the whole
    /// way, so the weights come out right.
    pub fn point_at_knot(&self, u: f64, v: f64) -> [f64; 3] {
        let ((u0, u1), (v0, v1)) = self.domain();
        let u = wrap_parameter(u, u0, u1, self.u_closed);
        let v = wrap_parameter(v, v0, v1, self.v_closed);
        let v = if self.v_reversed { v0 + v1 - v } else { v };
        let raw = self.homogeneous_at(u, v);
        if raw[3].abs() < 1e-15 {
            return [raw[0], raw[1], raw[2]];
        }
        [raw[0] / raw[3], raw[1] / raw[3], raw[2] / raw[3]]
    }

    /// The point at `(s, t)` in `0..=1` each, the domains rescaled.
    pub fn point_at(&self, s: f64, t: f64) -> [f64; 3] {
        let ((u0, u1), (v0, v1)) = self.domain();
        self.point_at_knot(
            u0 + (u1 - u0) * s.clamp(0.0, 1.0),
            v0 + (v1 - v0) * t.clamp(0.0, 1.0),
        )
    }

    /// Surface tangents at knot parameters `(u, v)`.
    pub fn tangents_at_knot(&self, u: f64, v: f64) -> Option<([f64; 3], [f64; 3])> {
        let ((u0, u1), (v0, v1)) = self.domain();
        if u1 <= u0 || v1 <= v0 {
            return None;
        }
        let u = wrap_parameter(u, u0, u1, self.u_closed);
        let v = wrap_parameter(v, v0, v1, self.v_closed);
        let (v, v_sign) = if self.v_reversed {
            (v0 + v1 - v, -1.0)
        } else {
            (v, 1.0)
        };
        let raw = self.homogeneous_at(u, v);
        if raw[3].abs() < 1e-15 {
            return None;
        }
        let point = Vec3::new(raw[0] / raw[3], raw[1] / raw[3], raw[2] / raw[3]);
        let tangent = |derivative: [f64; 4], sign: f64| {
            ((Vec3::new(derivative[0], derivative[1], derivative[2]) - point * derivative[3])
                * (sign / raw[3]))
                .to_array()
        };
        Some((
            tangent(self.homogeneous_u_derivative_at(u, v), 1.0),
            tangent(self.homogeneous_v_derivative_at(u, v), v_sign),
        ))
    }

    /// The unit normal at knot parameters `(u, v)`.
    ///
    /// `None` at a point where the net is degenerate — a pole, or a row of
    /// coincident control points — since there is no plane there to be
    /// perpendicular to and any answer would be invented.
    pub fn normal_at_knot(&self, u: f64, v: f64) -> Option<[f64; 3]> {
        let (along_u, along_v) = self.tangents_at_knot(u, v)?;
        let along_u = Vec3::from(along_u);
        let along_v = Vec3::from(along_v);
        along_u.cross(along_v).normalize().map(Vec3::to_array)
    }

    /// The same at `(s, t)` in `0..=1` each.
    pub fn normal_at(&self, s: f64, t: f64) -> Option<[f64; 3]> {
        let ((u0, u1), (v0, v1)) = self.domain();
        self.normal_at_knot(
            u0 + (u1 - u0) * s.clamp(0.0, 1.0),
            v0 + (v1 - v0) * t.clamp(0.0, 1.0),
        )
    }
}

fn matching_seam<'a, A, B>(first: A, second: B) -> bool
where
    A: IntoIterator<Item = (&'a [f64; 3], &'a f64)>,
    B: IntoIterator<Item = (&'a [f64; 3], &'a f64)>,
{
    let mut first = first.into_iter();
    let mut second = second.into_iter();
    let mut matched = false;
    loop {
        match (first.next(), second.next()) {
            (Some((a, aw)), Some((b, bw))) => {
                matched = true;
                let point_scale = a
                    .iter()
                    .chain(b)
                    .fold(1.0_f64, |scale, value| scale.max(value.abs()));
                let weight_scale = 1.0_f64.max(aw.abs()).max(bw.abs());
                let precision = f64::EPSILON * 4096.0;
                if Vec3::from(*a).distance(Vec3::from(*b)) > precision * point_scale
                    || (aw - bw).abs() > precision * weight_scale
                {
                    return false;
                }
            }
            (None, None) => return matched,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_just_short_of_a_clamped_loops_end_projects_there() {
        let w = std::f64::consts::FRAC_PI_4.cos();
        let circle = NurbsCurve3::new(
            2,
            vec![
                [1.0, 0.0, 0.0], [1.0, -1.0, 0.0], [0.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
                [-1.0, 0.0, 0.0], [-1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
            Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        )
        .unwrap();
        let below = [0.01_f64.cos(), 0.01_f64.sin(), 0.0];
        let on = circle.point_at_knot(circle.parameter_at(below));
        assert!(Vec3::from(on).distance(Vec3::from(below)) < 1e-9, "{on:?}");
    }

    fn helix_points() -> Vec<[f64; 3]> {
        vec![
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 2.0],
            [-1.0, -1.0, 3.0],
            [1.0, -1.0, 4.0],
        ]
    }

    #[test]
    fn periodic_control_curves_close_and_reverse_without_moving() {
        let curve = NurbsCurve3::from_control_polygon(
            2,
            &[
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 2.0, 1.0],
                [0.0, 2.0, 0.0],
            ],
            true,
        )
        .unwrap();
        assert!(curve.periodicity());
        assert!(Vec3::from(curve.point_at(0.0)).distance(Vec3::from(curve.point_at(1.0))) < 1e-9);
        let reversed = curve.reversed().unwrap();
        for step in 0..=8 {
            let t = step as f64 / 8.0;
            assert!(
                Vec3::from(curve.point_at(t)).distance(Vec3::from(reversed.point_at(1.0 - t)))
                    < 1e-9
            );
        }
    }

    #[test]
    fn rational_degree_elevation_preserves_every_sample() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 3.0, 2.0],
                [2.0, -1.0, 4.0],
                [4.0, 0.0, 5.0],
            ],
            vec![0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0],
            vec![1.0, 3.0, 0.5, 2.0],
        )
        .unwrap();
        let elevated = curve.elevated(2).unwrap();
        assert_eq!(elevated.degree(), 4);
        assert!(elevated.is_rational());
        for step in 0..=64 {
            let parameter = step as f64 / 64.0;
            assert!(
                Vec3::from(curve.point_at(parameter))
                    .distance(Vec3::from(elevated.point_at(parameter)))
                    < 1e-9
            );
        }
        assert!(curve.elevated(0).is_none());
    }

    #[test]
    fn weighted_periodic_control_curve_closes_and_elevates_exactly() {
        let points = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 1.0],
            [2.0, 2.0, 3.0],
            [0.0, 2.0, 1.0],
        ];
        let curve =
            NurbsCurve3::from_weighted_control_polygon(2, &points, &[1.0, 3.0, 0.75, 2.0], true)
                .unwrap();
        assert!(curve.periodicity());
        assert!(curve.is_rational());
        assert!(Vec3::from(curve.point_at(0.0)).distance(Vec3::from(curve.point_at(1.0))) < 1e-9);
        let elevated = curve.elevated(1).unwrap();
        assert!(elevated.periodicity());
        for step in 0..=64 {
            let parameter = step as f64 / 64.0;
            assert!(
                Vec3::from(curve.point_at(parameter))
                    .distance(Vec3::from(elevated.point_at(parameter)))
                    < 1e-9
            );
        }
        assert!(NurbsCurve3::from_weighted_control_polygon(
            2,
            &points,
            &[1.0, 0.0, 1.0, 1.0],
            true
        )
        .is_none());
    }

    #[test]
    fn deleting_a_control_vertex_keeps_a_valid_clamped_curve() {
        let curve = NurbsCurve3::new_strict(
            3,
            vec![[0.0; 3], [1.0, 2.0, 0.0], [2.0, 2.0, 0.0], [3.0; 3]],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![1.0; 4],
        )
        .unwrap();
        let reduced = curve.without_control_vertex(1).unwrap();
        assert_eq!((reduced.degree(), reduced.control_points().len()), (2, 3));
        assert_eq!(reduced.knots(), &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn rational_derivative_matches_a_finite_difference() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![[0.0, 0.0, 0.0], [1.0, 2.0, 1.0], [3.0, 0.0, 2.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![1.0, 3.0, 2.0],
        )
        .unwrap();
        let parameter = 0.37;
        let step = 1e-6;
        let before = Vec3::from(curve.point_at_knot(parameter - step));
        let after = Vec3::from(curve.point_at_knot(parameter + step));
        let numeric = (after - before) / (2.0 * step);
        let analytic = Vec3::from(curve.derivative_at_knot(parameter));
        assert!(
            analytic.distance(numeric) < 1e-6,
            "{analytic:?} vs {numeric:?}"
        );
    }

    #[test]
    fn tangent_from_near_validates_endpoints_interior_and_missing_solutions() {
        let curve = NurbsCurve3::new_strict(
            2,
            vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 1.0, 0.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![1.0; 3],
        )
        .unwrap();
        for (from, expected) in [
            ([0.0, -1.0, 0.0], [1.0, 1.0, 0.0]),
            ([0.0, -0.25, 0.0], [0.5, 0.25, 0.0]),
        ] {
            let point = curve.tangent_from_near(from, 0.8).unwrap();
            assert!(Vec3::from(point).distance(Vec3::from(expected)) < 1e-8);
        }
        assert!(curve.tangent_from_near([0.0, 1.0, 0.0], 0.8).is_none());
        assert!(curve.tangent_from_near([0.0, -1.0, 1.0], 0.8).is_none());
        assert!(curve.tangent_from_near([f64::NAN, 0.0, 0.0], 0.8).is_none());
        assert!(curve
            .tangent_from_near([0.0, -1.0, 0.0], f64::NAN)
            .is_none());
    }

    #[test]
    fn a_clamped_curve_ends_on_its_own_control_points() {
        let curve = NurbsCurve3::new(3, helix_points(), Vec::new(), None).unwrap();
        let start = curve.point_at(0.0);
        let end = curve.point_at(1.0);
        assert!(
            Vec3::from(start).distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-9,
            "{start:?}"
        );
        assert!(
            Vec3::from(end).distance(Vec3::new(1.0, -1.0, 4.0)) < 1e-9,
            "{end:?}"
        );
    }

    #[test]
    fn a_curve_that_wanders_is_not_flat() {
        // The reason this exists rather than the plane one: these points
        // share no plane, and projecting them onto one would lose the climb.
        let curve = NurbsCurve3::new(3, helix_points(), Vec::new(), None).unwrap();
        let heights: Vec<f64> = (0..=8)
            .map(|step| curve.point_at(step as f64 / 8.0)[2])
            .collect();
        assert!(
            heights.windows(2).all(|pair| pair[1] > pair[0]),
            "{heights:?}"
        );
    }

    #[test]
    fn a_weighted_curve_is_pulled_towards_the_heavy_point() {
        let points = vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]];
        let plain = NurbsCurve3::new(2, points.clone(), Vec::new(), None).unwrap();
        let heavy = NurbsCurve3::new(2, points, Vec::new(), Some(vec![1.0, 8.0, 1.0])).unwrap();
        assert!(heavy.is_rational());
        assert!(!plain.is_rational());
        assert!(heavy.point_at(0.5)[1] > plain.point_at(0.5)[1]);
    }

    #[test]
    fn a_finer_tolerance_gets_closer_to_the_curve() {
        // Measured by length rather than by distance to a sampled reference.
        // A reference fine enough to check a thousandth would need to be
        // finer than that itself, and comparing against a coarse one measures
        // the reference's own spacing instead of the tessellation's error.
        //
        // Length works because a chord always cuts the corner: a polyline
        // inscribed in a curve is short, and less short the closer it fits.
        let curve = NurbsCurve3::new(3, helix_points(), Vec::new(), None).unwrap();
        let length = |points: &[[f64; 3]]| -> f64 {
            points
                .windows(2)
                .map(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1])))
                .sum()
        };
        let truth = length(
            &(0..=20_000)
                .map(|step| curve.point_at(step as f64 / 20_000.0))
                .collect::<Vec<_>>(),
        );
        let coarse = curve.tessellate_within(0.1);
        let fine = curve.tessellate_within(0.001);
        assert!(fine.len() > coarse.len(), "a finer tolerance asks for more");
        for points in [&coarse, &fine] {
            assert!(length(points) <= truth + 1e-9, "a chord cannot read long");
        }
        assert!(length(&fine) > length(&coarse));
        // A sag bound is a distance, not a length, so what it buys in length
        // is a consequence rather than the promise — a tenth of a per cent
        // here on a curve six units long.
        assert!(
            length(&fine) > truth * 0.999,
            "{} vs {truth}",
            length(&fine)
        );
    }

    #[test]
    fn a_tangent_points_the_way_the_curve_runs() {
        let curve = NurbsCurve3::new(3, helix_points(), Vec::new(), None).unwrap();
        for step in 1..8 {
            let t = step as f64 / 8.0;
            let along = Vec3::from(curve.tangent_at(t));
            let ahead = Vec3::from(curve.point_at(t + 0.05)) - Vec3::from(curve.point_at(t));
            assert!(along.dot(ahead) > 0.0, "t={t}");
        }
    }

    #[test]
    fn a_second_derivative_reads_the_way_the_curve_bends() {
        // A parabola through three points: its second derivative is constant
        // and points at the inside of the bend, whatever the parameter.
        let curve = NurbsCurve3::new(
            2,
            vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        let (from, to) = curve.domain();
        for step in 0..=6 {
            let u = from + (to - from) * step as f64 / 6.0;
            let bend = Vec3::from(curve.acceleration_at_knot(u));
            assert!(bend.y < 0.0, "u={u}: {bend:?}");
            assert!(bend.x.abs() < 1e-3, "u={u}: {bend:?}");
        }
        // And a straight run does not bend at all.
        let straight = NurbsCurve3::new(
            1,
            vec![[0.0; 3], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(Vec3::from(straight.acceleration_at_knot(0.5)).length() < 1e-3);
    }

    #[test]
    fn a_tangent_at_the_very_end_is_not_halved() {
        // A difference reaching past the domain samples the clamped end
        // twice and reads the slope as half what it is, which is the shape a
        // blend leaves a visible kink at.
        let curve = NurbsCurve3::new(
            3,
            vec![[0.0; 3], [1.0, 3.0, 0.0], [4.0, 3.0, 0.0], [5.0, 0.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        let (from, to) = curve.domain();
        for end in [from, to] {
            let at_end = Vec3::from(curve.tangent_at_knot(end)).length();
            let just_inside = Vec3::from(curve.tangent_at_knot(from + (to - from) * 0.02)).length();
            assert!(
                (at_end - just_inside).abs() < 0.35 * just_inside,
                "{at_end} vs {just_inside}"
            );
        }
    }

    #[test]
    fn too_few_points_for_the_degree_is_refused() {
        assert!(NurbsCurve3::new(3, vec![[0.0; 3], [1.0; 3]], Vec::new(), None).is_none());
        assert!(NurbsCurve3::new(0, vec![[0.0; 3], [1.0; 3]], Vec::new(), None).is_none());
    }

    /// A flat 3 × 3 net, raised in the middle.
    fn hill() -> NurbsSurface3 {
        let mut net = Vec::new();
        for row in 0..3 {
            let mut across = Vec::new();
            for column in 0..3 {
                let height = if row == 1 && column == 1 { 4.0 } else { 0.0 };
                across.push([row as f64 * 5.0, column as f64 * 5.0, height]);
            }
            net.push(across);
        }
        NurbsSurface3::new(2, 2, net, Vec::new(), Vec::new(), None).unwrap()
    }

    #[test]
    fn a_surface_holds_its_own_corners() {
        let surface = hill();
        for (s, t, corner) in [
            (0.0, 0.0, [0.0, 0.0, 0.0]),
            (1.0, 0.0, [10.0, 0.0, 0.0]),
            (0.0, 1.0, [0.0, 10.0, 0.0]),
            (1.0, 1.0, [10.0, 10.0, 0.0]),
        ] {
            let point = surface.point_at(s, t);
            assert!(
                Vec3::from(point).distance(Vec3::from(corner)) < 1e-9,
                "({s}, {t}): {point:?}"
            );
        }
    }

    #[test]
    fn a_raised_control_point_lifts_the_middle_but_not_to_its_own_height() {
        // A B-spline approximates rather than interpolates its interior
        // points, which is the property that distinguishes it from a mesh of
        // the same net.
        let middle = hill().point_at(0.5, 0.5);
        assert!(middle[2] > 0.5, "{middle:?}");
        assert!(middle[2] < 4.0, "{middle:?}");
        assert!((middle[0] - 5.0).abs() < 1e-9 && (middle[1] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn surface_parameters_recover_points_on_a_curved_surface() {
        let surface = hill();
        for expected in [(0.13, 0.27), (0.5, 0.5), (0.82, 0.71)] {
            let point = surface.point_at(expected.0, expected.1);
            let actual = surface.parameters_at(point).unwrap();
            let recovered = surface.point_at_knot(actual.0, actual.1);
            assert!(
                Vec3::from(recovered).distance(Vec3::from(point)) < 1e-8,
                "expected={expected:?} actual={actual:?} recovered={recovered:?}"
            );
        }
    }

    #[test]
    fn surface_parameter_recovery_is_scale_independent() {
        let size = 1e-5;
        let surface = NurbsSurface3::new(
            1,
            1,
            vec![
                vec![[0.0, 0.0, 0.0], [0.0, size, 0.0]],
                vec![[size, 0.0, 0.0], [size, size, 0.0]],
            ],
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
        let expected = (0.13, 0.27);
        let point = surface.point_at_knot(expected.0, expected.1);
        let actual = surface.parameters_at(point).unwrap();
        let recovered = surface.point_at_knot(actual.0, actual.1);
        assert!(
            Vec3::from(recovered).distance(Vec3::from(point)) < size * 1e-9,
            "expected={expected:?} actual={actual:?} recovered={recovered:?}"
        );
    }

    #[test]
    fn a_flat_surfaces_normal_is_perpendicular_to_it() {
        let net = vec![
            vec![[0.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 10.0, 0.0]],
            vec![[5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [5.0, 10.0, 0.0]],
            vec![[10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [10.0, 10.0, 0.0]],
        ];
        let surface = NurbsSurface3::new(2, 2, net, Vec::new(), Vec::new(), None).unwrap();
        let normal = Vec3::from(surface.normal_at(0.5, 0.5).unwrap());
        assert!(normal.cross(Vec3::Z).length() < 1e-9, "{normal:?}");
    }

    #[test]
    fn a_ragged_net_is_refused_rather_than_padded() {
        let net = vec![
            vec![[0.0; 3], [1.0; 3], [2.0; 3]],
            vec![[0.0; 3], [1.0; 3]],
            vec![[0.0; 3], [1.0; 3], [2.0; 3]],
        ];
        assert!(NurbsSurface3::new(2, 2, net, Vec::new(), Vec::new(), None).is_none());
    }
}
