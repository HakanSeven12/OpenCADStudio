//! NURBS curves.
//!
//! A spline arrives from a drawing as a degree, a knot vector, control points
//! and — if it is rational — a weight per control point. Evaluating it is de
//! Boor's algorithm, which is short and exact, so there is no reason for this
//! to be the one curve the kernel can only approximate.
//!
//! # Weights are not optional
//!
//! Dropping the weights and evaluating a rational curve as though it were
//! polynomial is a quiet, plausible-looking wrong answer: the curve still
//! passes near its control points and still looks like a spline, but conic
//! sections written as NURBS — which is how a great many CAD circles and
//! ellipses arrive once they have been through a modeller — come out visibly
//! off. Weights are carried here and applied in homogeneous coordinates.

use super::vec::Vec2;
use crate::space::spline::{de_boor_by, interpolate_periodic};
/// Fit-point spacing shared with space curves.
pub use crate::space::spline::{clamped_uniform_knots, Parameterization};

/// A non-uniform rational B-spline curve in the plane.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<[f64; 2]>,
    weights: Vec<f64>,
}

impl NurbsCurve {
    /// Builds a curve, filling in what the caller left out.
    ///
    /// A knot vector of the wrong length is replaced with a clamped uniform
    /// one, which is what a drawing with a malformed spline needs in order to
    /// still draw something sensible. Weights are optional; absent means all
    /// ones, and the curve is polynomial.
    ///
    /// `None` when there are too few control points for the degree, since
    /// there is no curve to evaluate.
    pub fn new(
        degree: usize,
        control_points: Vec<[f64; 2]>,
        knots: Vec<f64>,
        weights: Option<Vec<f64>>,
    ) -> Option<Self> {
        if degree == 0 || control_points.len() <= degree {
            return None;
        }
        let expected = control_points.len() + degree + 1;
        let knots = if knots.len() == expected {
            knots
        } else {
            clamped_uniform_knots(degree, control_points.len())
        };
        let weights = match weights {
            Some(w) if w.len() == control_points.len() && w.iter().all(|v| *v > 0.0) => w,
            _ => vec![1.0; control_points.len()],
        };
        Some(Self {
            degree,
            knots,
            control_points,
            weights,
        })
    }

    /// Builds a curve without replacing malformed source data.
    pub fn new_strict(
        degree: usize,
        control_points: Vec<[f64; 2]>,
        knots: Vec<f64>,
        weights: Vec<f64>,
    ) -> Option<Self> {
        if degree == 0
            || control_points.len() <= degree
            || knots.len() != control_points.len() + degree + 1
            || weights.len() != control_points.len()
            || !knots.iter().all(|value| value.is_finite())
            || !knots.windows(2).all(|pair| pair[0] <= pair[1])
            || knots.first() >= knots.last()
            || !control_points.iter().flatten().all(|value| value.is_finite())
            || !weights.iter().all(|weight| weight.is_finite() && *weight > 0.0)
        {
            return None;
        }
        Some(Self {
            degree,
            knots,
            control_points,
            weights,
        })
    }

    /// The degree.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The control points, in order.
    pub fn control_points(&self) -> &[[f64; 2]] {
        &self.control_points
    }

    /// The knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The weights, one per control point. All ones for a polynomial curve.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// Whether any weight differs from the others, making the curve rational.
    pub fn is_rational(&self) -> bool {
        let first = self.weights[0];
        self.weights.iter().any(|w| (w - first).abs() > 1e-12)
    }

    /// The knot values the curve is actually defined over, `(start, end)`.
    ///
    /// The knot vector runs wider than this; the first and last `degree` knots
    /// exist to define the basis, not to be evaluated at.
    pub fn domain(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.control_points.len()],
        )
    }

    /// The point at knot value `u`, clamped to the curve's [`domain`].
    ///
    /// This is de Boor's algorithm run in homogeneous coordinates, so the
    /// weights are honoured rather than ignored.
    pub fn point_at_knot(&self, u: f64) -> [f64; 2] {
        let (start, end) = self.domain();
        let result = de_boor_by(
            self.degree,
            &self.knots,
            self.control_points.len(),
            u.clamp(start, end),
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                [point[0] * weight, point[1] * weight, weight]
            },
        );
        if result[2].abs() < 1e-15 {
            [result[0], result[1]]
        } else {
            [result[0] / result[2], result[1] / result[2]]
        }
    }

    /// The derivative `dC/du` at knot value `u`.
    ///
    /// Analytic, not a difference quotient. The homogeneous curve `A(u)` is a
    /// B-spline of degree `p`, so `A'(u)` is one of degree `p − 1` over the
    /// same knots with the two ends dropped and control points
    /// `p·(Pw_{i+1} − Pw_i)/(U_{i+p+1} − U_{i+1})`. Projecting back through
    /// the quotient rule gives `C' = (A'_{xy} − C·A'_w) / A_w`, which stays
    /// exact for a rational curve — the place a difference quotient would
    /// quietly lose the weights' effect.
    ///
    /// Zero for a degree-zero curve, which has no direction to report.
    pub fn derivative_at_knot(&self, u: f64) -> [f64; 2] {
        if self.degree == 0 {
            return [0.0, 0.0];
        }
        let (start, end) = self.domain();
        let u = u.clamp(start, end);
        let degree = self.degree as f64;
        let value = de_boor_by(
            self.degree,
            &self.knots,
            self.control_points.len(),
            u,
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                [point[0] * weight, point[1] * weight, weight]
            },
        );
        let slope = de_boor_by(
            self.degree - 1,
            &self.knots[1..self.knots.len() - 1],
            self.control_points.len() - 1,
            u,
            |i| {
                let span = self.knots[i + self.degree + 1] - self.knots[i + 1];
                let scale = if span.abs() < 1e-15 {
                    0.0
                } else {
                    degree / span
                };
                let point = |index: usize| -> [f64; 3] {
                    let control = self.control_points[index];
                    let weight = self.weights[index];
                    [control[0] * weight, control[1] * weight, weight]
                };
                let (before, after) = (point(i), point(i + 1));
                [
                    (after[0] - before[0]) * scale,
                    (after[1] - before[1]) * scale,
                    (after[2] - before[2]) * scale,
                ]
            },
        );
        if value[2].abs() < 1e-15 {
            return [slope[0], slope[1]];
        }
        let point = [value[0] / value[2], value[1] / value[2]];
        [
            (slope[0] - point[0] * slope[2]) / value[2],
            (slope[1] - point[1] * slope[2]) / value[2],
        ]
    }

    /// The derivative with respect to the `0..=1` parameter, so it composes
    /// with [`point_at`](Self::point_at) rather than with
    /// [`point_at_knot`](Self::point_at_knot).
    pub fn derivative_at(&self, t: f64) -> [f64; 2] {
        let (start, end) = self.domain();
        let width = end - start;
        let slope = self.derivative_at_knot(start + t.clamp(0.0, 1.0) * width);
        [slope[0] * width, slope[1] * width]
    }

    /// The control points lifted into `(w·x, w·y, w)`.
    fn homogeneous(&self) -> Vec<[f64; 3]> {
        self.control_points
            .iter()
            .zip(self.weights.iter())
            .map(|(p, &w)| [p[0] * w, p[1] * w, w])
            .collect()
    }

    /// The point at `t`, which runs `0..=1` across the [`domain`].
    pub fn point_at(&self, t: f64) -> [f64; 2] {
        let (start, end) = self.domain();
        self.point_at_knot(start + t.clamp(0.0, 1.0) * (end - start))
    }

    /// Where `point` falls on the curve, as `0..=1`.
    ///
    /// Found by sampling and then narrowing, because a NURBS curve has no
    /// closed-form inverse. The result is the nearest point on the curve, so a
    /// point that is not on it is projected rather than rejected.
    pub fn parameter_at(&self, point: [f64; 2]) -> f64 {
        let coarse = (self.control_points.len() * 8).max(64);
        let point = Vec2::from(point);
        let distance_at = |t: f64| Vec2::from(self.point_at(t)).distance_squared(point);

        let samples: Vec<f64> =
            (0..=coarse).map(|i| distance_at(i as f64 / coarse as f64)).collect();

        // Narrow within the neighbouring samples. Bisection on the bracket
        // rather than Newton: it needs no derivative and cannot diverge where
        // the curve doubles back on itself.
        let step = 1.0 / coarse as f64;
        let narrow = |mut low: f64, mut high: f64| {
            let mut best = (low + high) * 0.5;
            for _ in 0..60 {
                if high - low < 1e-15 {
                    break;
                }
                let third = (high - low) / 3.0;
                let left = low + third;
                let right = high - third;
                if distance_at(left) < distance_at(right) {
                    high = right;
                } else {
                    low = left;
                }
                best = (low + high) * 0.5;
            }
            best.clamp(0.0, 1.0)
        };
        // Every sample nearer than both its neighbours starts a narrowing,
        // the nearest few of them: where the curve comes back past itself,
        // the nearest sample may lie on the other pass, and narrowing round
        // it alone settled on that pass. A closed curve's two ends are one
        // sample, neighbours across the seam.
        let closed = self.is_closed();
        let around = |i: usize, back: bool| -> f64 {
            match (back, i) {
                (true, 0) if closed => samples[coarse - 1],
                (true, 0) => f64::INFINITY,
                (true, _) => samples[i - 1],
                (false, i) if i == coarse && closed => samples[1],
                (false, i) if i == coarse => f64::INFINITY,
                (false, _) => samples[i + 1],
            }
        };
        let mut minima: Vec<usize> = (0..=coarse)
            .filter(|&i| samples[i] <= around(i, true) && samples[i] <= around(i, false))
            .collect();
        minima.sort_by(|a, b| samples[*a].total_cmp(&samples[*b]));
        minima.truncate(4);
        let brackets = minima.into_iter().flat_map(|i| {
            let t = i as f64 * step;
            let mut brackets = vec![((t - step).max(0.0), (t + step).min(1.0))];
            if closed && i == 0 {
                brackets.push((1.0 - step, 1.0));
            } else if closed && i == coarse {
                brackets.push((0.0, step));
            }
            brackets
        });
        brackets
            .map(|(low, high)| narrow(low, high))
            .min_by(|a, b| distance_at(*a).total_cmp(&distance_at(*b)))
            .unwrap_or(0.0)
    }

    /// Samples the curve, giving every knot span at least `per_span` pieces.
    ///
    /// Sampling per span rather than uniformly means a curve whose knots
    /// bunch up — which is where its shape changes fastest — is cut finely
    /// exactly there.
    pub fn tessellate(&self, per_span: usize) -> Vec<[f64; 2]> {
        let per_span = per_span.max(1);
        let (start, end) = self.domain();
        let mut values: Vec<f64> = self.knots[self.degree..=self.control_points.len()]
            .iter()
            .copied()
            .filter(|u| *u >= start && *u <= end)
            .collect();
        values.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        if values.len() < 2 {
            values = vec![start, end];
        }

        let mut out = Vec::new();
        for pair in values.windows(2) {
            for step in 0..per_span {
                let f = step as f64 / per_span as f64;
                out.push(self.point_at_knot(pair[0] + f * (pair[1] - pair[0])));
            }
        }
        out.push(self.point_at_knot(end));
        out
    }

    /// Whether the curve comes back to where it started.
    pub fn is_closed(&self) -> bool {
        Vec2::from(self.point_at(0.0)).distance(Vec2::from(self.point_at(1.0))) < 1e-9
    }

    /// Inserts the knot value `u` once, leaving the curve's shape unchanged.
    ///
    /// Boehm's algorithm: the control points either side of the affected span
    /// are replaced by a blend of their neighbours, in homogeneous coordinates
    /// so a rational curve stays exact. Adding knots is how a curve gets split
    /// without approximating it — see [`split_at`](Self::split_at).
    pub fn insert_knot(&mut self, u: f64) {
        let (start, end) = self.domain();
        if u <= start || u >= end {
            return;
        }
        let span = self.span_containing(u);
        let degree = self.degree;

        let homogeneous: Vec<[f64; 3]> = self
            .control_points
            .iter()
            .zip(&self.weights)
            .map(|(p, w)| [p[0] * w, p[1] * w, *w])
            .collect();

        let mut updated: Vec<[f64; 3]> = Vec::with_capacity(homogeneous.len() + 1);
        updated.extend_from_slice(&homogeneous[..=span - degree]);
        for i in (span - degree + 1)..=span {
            let lower = self.knots[i];
            let upper = self.knots[i + degree];
            let alpha = if (upper - lower).abs() < 1e-15 {
                0.0
            } else {
                (u - lower) / (upper - lower)
            };
            updated.push([
                (1.0 - alpha) * homogeneous[i - 1][0] + alpha * homogeneous[i][0],
                (1.0 - alpha) * homogeneous[i - 1][1] + alpha * homogeneous[i][1],
                (1.0 - alpha) * homogeneous[i - 1][2] + alpha * homogeneous[i][2],
            ]);
        }
        updated.extend_from_slice(&homogeneous[span..]);

        self.knots.insert(span + 1, u);
        self.control_points = updated
            .iter()
            .map(|h| {
                if h[2].abs() < 1e-15 {
                    [h[0], h[1]]
                } else {
                    [h[0] / h[2], h[1] / h[2]]
                }
            })
            .collect();
        self.weights = updated.iter().map(|h| h[2]).collect();
    }

    /// Splits the curve at `t`, which runs `0..=1` across the domain.
    ///
    /// Both halves are exact: the knot is inserted until it has full
    /// multiplicity, at which point the control points divide cleanly and each
    /// half is a clamped curve in its own right. Weights survive, so splitting
    /// a rational curve does not quietly turn it polynomial.
    ///
    /// `None` when the cut falls at or outside an end, where there is nothing
    /// to divide.
    /// The same curve traced the other way.
    ///
    /// Control points and weights reverse, and the knots are mirrored within
    /// the domain — `U'ᵢ = (first + last) − U_{m−i}` — so the shape is
    /// untouched and only the direction of travel changes.
    ///
    /// The mirroring is the part that is easy to skip. Reversing the control
    /// points and then regenerating a *clamped uniform* knot vector looks
    /// right and is not: a curve whose knots are unevenly spaced is a
    /// different curve afterwards, bulging where it used to be taut.
    pub fn reversed(&self) -> Self {
        let (start, end) = (self.knots[0], self.knots[self.knots.len() - 1]);
        let total = start + end;
        let knots: Vec<f64> = self.knots.iter().rev().map(|u| total - u).collect();
        Self {
            degree: self.degree,
            knots,
            control_points: self.control_points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
        }
    }

    /// The same curve written at a higher degree.
    ///
    /// Exact — degree elevation changes how a curve is described, never where
    /// it goes. Needed whenever two curves have to share a description: JOIN
    /// on a pair of splines of different degrees, and a B-rep edge shared by
    /// faces that disagree about it.
    ///
    /// Done by taking the curve apart into Bézier segments, elevating each —
    /// where the rule is one line — and putting them back. The result keeps
    /// each interior break at full multiplicity rather than reducing it back
    /// to the smoothness the curve actually has: legal, exact, and a few more
    /// control points than the minimal form.
    ///
    /// `None` if `by` is zero, since there is nothing to do.
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

    fn elevated_once(&self) -> Option<Self> {
        let degree = self.degree;
        let raised = degree + 1;
        let (segments, breaks) = self.bezier_segments();
        if segments.is_empty() {
            return None;
        }

        let mut control: Vec<[f64; 2]> = Vec::new();
        let mut weights: Vec<f64> = Vec::new();
        for segment in &segments {
            // Qᵢ = (i/(p+1))·Pᵢ₋₁ + (1 − i/(p+1))·Pᵢ, in homogeneous
            // coordinates so a rational curve elevates exactly too.
            let mut elevated: Vec<[f64; 3]> = Vec::with_capacity(raised + 1);
            for i in 0..=raised {
                let alpha = i as f64 / raised as f64;
                let before = if i == 0 {
                    [0.0; 3]
                } else {
                    segment[i - 1]
                };
                let after = if i > degree { [0.0; 3] } else { segment[i] };
                elevated.push([
                    before[0] * alpha + after[0] * (1.0 - alpha),
                    before[1] * alpha + after[1] * (1.0 - alpha),
                    before[2] * alpha + after[2] * (1.0 - alpha),
                ]);
            }
            // Neighbouring segments share an endpoint.
            let skip = usize::from(!control.is_empty());
            for point in &elevated[skip..] {
                let w = if point[2].abs() < 1e-15 { 1.0 } else { point[2] };
                control.push([point[0] / w, point[1] / w]);
                weights.push(w);
            }
        }

        // The breaks keep the curve's own knot values. Spacing them evenly
        // instead leaves the same point set but reparameterised, so the
        // elevated curve would no longer agree with the original about which
        // point `t` names.
        let (start, end) = self.domain();
        let mut knots: Vec<f64> = vec![start; raised + 1];
        for at in &breaks {
            knots.extend(std::iter::repeat_n(*at, raised));
        }
        knots.extend(std::iter::repeat_n(end, raised + 1));

        Some(Self {
            degree: raised,
            knots,
            control_points: control,
            weights,
        })
    }

    /// The curve taken apart into Bézier segments, each as `degree + 1`
    /// homogeneous control points.
    ///
    /// Raising every interior knot to full multiplicity makes the segments
    /// independent, which is what lets a per-Bézier rule be applied one
    /// segment at a time.
    fn bezier_segments(&self) -> (Vec<Vec<[f64; 3]>>, Vec<f64>) {
        let (start, end) = self.domain();
        let mut work = self.clone();
        loop {
            let breaks: Vec<f64> = {
                let mut seen: Vec<(f64, usize)> = Vec::new();
                for knot in &work.knots {
                    if *knot <= start + 1e-12 || *knot >= end - 1e-12 {
                        continue;
                    }
                    match seen.iter_mut().find(|(at, _)| (at - knot).abs() < 1e-12) {
                        Some((_, count)) => *count += 1,
                        None => seen.push((*knot, 1)),
                    }
                }
                seen.into_iter()
                    .filter(|(_, count)| *count < work.degree)
                    .map(|(at, _)| at)
                    .collect()
            };
            if breaks.is_empty() {
                break;
            }
            for at in breaks {
                work.insert_knot(at);
            }
        }
        // Every interior knot now sits at full multiplicity, so the distinct
        // interior values in order are the segment boundaries.
        let mut breaks: Vec<f64> = Vec::new();
        for knot in &work.knots {
            if *knot <= start + 1e-12 || *knot >= end - 1e-12 {
                continue;
            }
            if breaks.last().is_none_or(|last| (last - knot).abs() > 1e-12) {
                breaks.push(*knot);
            }
        }
        let homogeneous = work.homogeneous();
        let segments = (0..)
            .map(|index| index * work.degree)
            .take_while(|from| from + work.degree < homogeneous.len() + work.degree)
            .filter_map(|from| {
                homogeneous
                    .get(from..=from + work.degree)
                    .map(<[[f64; 3]]>::to_vec)
            })
            .collect::<Vec<_>>();
        (segments, breaks)
    }

    pub fn split_at(&self, t: f64) -> Option<(Self, Self)> {
        let (start, end) = self.domain();
        let u = start + t.clamp(0.0, 1.0) * (end - start);
        if u <= start + 1e-12 || u >= end - 1e-12 {
            return None;
        }

        let mut work = self.clone();
        let existing = work.knots.iter().filter(|k| (**k - u).abs() < 1e-12).count();
        for _ in existing..self.degree {
            work.insert_knot(u);
        }

        // With the knot at full multiplicity the control points divide at the
        // last one that carries it.
        let cut = work
            .knots
            .iter()
            .rposition(|k| (*k - u).abs() < 1e-12)?;
        let degree = self.degree;
        let left_count = cut - degree + 1;

        let left = Self {
            degree,
            knots: work.knots[..=cut]
                .iter()
                .copied()
                .chain(std::iter::once(u))
                .collect(),
            control_points: work.control_points[..left_count].to_vec(),
            weights: work.weights[..left_count].to_vec(),
        };
        let right = Self {
            degree,
            knots: std::iter::once(u)
                .chain(work.knots[cut - degree + 1..].iter().copied())
                .collect(),
            control_points: work.control_points[left_count - 1..].to_vec(),
            weights: work.weights[left_count - 1..].to_vec(),
        };
        (left.control_points.len() > degree && right.control_points.len() > degree)
            .then_some((left, right))
    }

    /// The exact portion of this curve between two normalised parameters.
    ///
    /// Parameters run over the public `0..=1` domain.  Supplying them in the
    /// opposite order returns the same portion with its direction reversed.
    /// Knot insertion is used internally, so rational weights and the original
    /// shape are preserved instead of sampling the result into a polyline.
    pub fn trimmed(&self, from: f64, to: f64) -> Option<Self> {
        if !from.is_finite() || !to.is_finite() {
            return None;
        }
        let from = from.clamp(0.0, 1.0);
        let to = to.clamp(0.0, 1.0);
        if (from - to).abs() <= 1.0e-12 {
            return None;
        }
        if from > to {
            return self.trimmed(to, from).map(|curve| curve.reversed());
        }
        if from <= 1.0e-12 && to >= 1.0 - 1.0e-12 {
            return Some(self.clone());
        }

        let left = if to < 1.0 - 1.0e-12 {
            self.split_at(to)?.0
        } else {
            self.clone()
        };
        if from <= 1.0e-12 {
            return Some(left);
        }
        let local = if to > 1.0e-12 { from / to } else { return None };
        left.split_at(local).map(|(_, right)| right)
    }

    /// A smooth curve that remains within `tolerance` of an open polyline.
    pub fn fit_polyline(points: &[[f64; 2]], tolerance: f64) -> Option<Self> {
        if !tolerance.is_finite()
            || tolerance < 0.0
            || points.iter().flatten().any(|value| !value.is_finite())
        {
            return None;
        }
        let mut points = points.to_vec();
        points.dedup();
        if points.len() < 2 {
            return None;
        }
        if tolerance == 0.0 {
            return Self::new(1, points, Vec::new(), None);
        }

        let allowance = tolerance * 0.5;
        let points = simplify_polyline(&points, allowance);
        if points.len() == 2 {
            return Self::new(1, points, Vec::new(), None);
        }
        rounded_polyline(&points, allowance)
    }

    /// The C² cubic through every one of `points`.
    ///
    /// This is the other way a drawing describes a spline: fit points it must
    /// pass through, with optional tangents at the ends, rather than a control
    /// polygon it merely approaches. Both arrive as SPLINE entities and both
    /// have to be trimmable, so a fit-point spline is turned into an exact
    /// curve here rather than sampled into a polyline.
    ///
    /// A tangent that is absent — or zero, which is how "unset" is stored —
    /// gets the natural end condition instead, leaving the second derivative
    /// zero there.
    ///
    /// `None` for fewer than two points, where there is no curve.
    pub fn interpolate(
        points: &[[f64; 2]],
        start_tangent: Option<[f64; 2]>,
        end_tangent: Option<[f64; 2]>,
        parameterization: Parameterization,
    ) -> Option<Self> {
        let count = points.len();
        if count < 2 {
            return None;
        }

        // Parameter values, left unnormalised so a unit end tangent — which is
        // how tangents are stored — stays a consistent dP/dt.
        let mut knots_at = vec![0.0f64; count];
        for i in 1..count {
            let chord = Vec2::from(points[i - 1])
                .distance(Vec2::from(points[i]))
                .max(1e-9);
            let step = match parameterization {
                Parameterization::Uniform => 1.0,
                Parameterization::Centripetal => chord.sqrt(),
                Parameterization::Chord => chord,
            };
            knots_at[i] = knots_at[i - 1] + step;
        }
        let spans: Vec<f64> = (0..count - 1)
            .map(|i| (knots_at[i + 1] - knots_at[i]).max(1e-9))
            .collect();

        let usable = |t: Option<[f64; 2]>| {
            t.filter(|v| v[0] * v[0] + v[1] * v[1] > 1e-18)
        };
        let start_tangent = usable(start_tangent);
        let end_tangent = usable(end_tangent);

        // Solve for dP/dt at each fit point, one axis at a time. The interior
        // rows are C² continuity; the end rows are either the clamped tangent
        // or the natural condition.
        let mut slopes = [vec![0.0f64; count], vec![0.0f64; count]];
        for (axis, slope) in slopes.iter_mut().enumerate() {
            let segment_slope =
                |i: usize| (points[i + 1][axis] - points[i][axis]) / spans[i];
            let mut sub = vec![0.0; count];
            let mut main = vec![0.0; count];
            let mut sup = vec![0.0; count];
            let mut rhs = vec![0.0; count];

            match start_tangent {
                Some(t) => {
                    main[0] = 1.0;
                    rhs[0] = t[axis];
                }
                None => {
                    main[0] = 2.0;
                    sup[0] = 1.0;
                    rhs[0] = 3.0 * segment_slope(0);
                }
            }
            for i in 1..count - 1 {
                sub[i] = spans[i];
                main[i] = 2.0 * (spans[i - 1] + spans[i]);
                sup[i] = spans[i - 1];
                rhs[i] = 3.0
                    * (spans[i] * segment_slope(i - 1) + spans[i - 1] * segment_slope(i));
            }
            match end_tangent {
                Some(t) => {
                    main[count - 1] = 1.0;
                    rhs[count - 1] = t[axis];
                }
                None => {
                    sub[count - 1] = 1.0;
                    main[count - 1] = 2.0;
                    rhs[count - 1] = 3.0 * segment_slope(count - 2);
                }
            }
            solve_tridiagonal(&sub, &main, &sup, &mut rhs);
            *slope = rhs;
        }

        // Each Hermite span becomes a cubic Bezier: the inner control points
        // sit a third of the span's derivative from each end. Chaining them is
        // a degree-3 B-spline whose interior knots carry multiplicity 3.
        let mut control_points = Vec::with_capacity(3 * count - 2);
        control_points.push(points[0]);
        for i in 0..count - 1 {
            let h = spans[i];
            let from = points[i];
            let to = points[i + 1];
            let m0 = [slopes[0][i] * h, slopes[1][i] * h];
            let m1 = [slopes[0][i + 1] * h, slopes[1][i + 1] * h];
            control_points.push([from[0] + m0[0] / 3.0, from[1] + m0[1] / 3.0]);
            control_points.push([to[0] - m1[0] / 3.0, to[1] - m1[1] / 3.0]);
            control_points.push(to);
        }

        let mut knots = vec![knots_at[0]; 4];
        for value in knots_at.iter().take(count - 1).skip(1) {
            knots.extend([*value; 3]);
        }
        knots.extend([knots_at[count - 1]; 4]);

        let weights = vec![1.0; control_points.len()];
        Some(Self {
            degree: 3,
            knots,
            control_points,
            weights,
        })
    }

    /// The closed C² cubic through every fit point.
    pub fn interpolate_periodic(
        points: &[[f64; 2]],
        parameterization: Parameterization,
    ) -> Option<Self> {
        let (control_points, knots) = interpolate_periodic(points, parameterization)?;
        let weights = vec![1.0; control_points.len()];
        Some(Self {
            degree: 3,
            knots,
            control_points,
            weights,
        })
    }

    /// Index of the knot span holding `u`.
    fn span_containing(&self, u: f64) -> usize {
        let last = self.control_points.len() - 1;
        if u >= self.knots[last + 1] {
            return last;
        }
        let mut low = self.degree;
        let mut high = last + 1;
        while high - low > 1 {
            let middle = (low + high) / 2;
            if u < self.knots[middle] {
                high = middle;
            } else {
                low = middle;
            }
        }
        low
    }
}

fn simplify_polyline(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() <= 2 || tolerance == 0.0 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut ranges = vec![(0usize, points.len() - 1)];
    while let Some((start, end)) = ranges.pop() {
        let mut farthest = None;
        let mut distance = tolerance;
        let from = Vec2::from(points[start]);
        let along = Vec2::from(points[end]) - from;
        let squared = along.length_squared();
        let mut previous_parameter = 0.0;
        let mut reversal = None;
        for index in start + 1..end {
            let point = Vec2::from(points[index]);
            let parameter = if squared > 0.0 {
                (point - from).dot(along) / squared
            } else {
                0.0
            };
            if parameter + 1e-12 < previous_parameter {
                reversal = Some(index);
                break;
            }
            previous_parameter = parameter;
            let here = point.distance_to_segment(from, Vec2::from(points[end]));
            if here > distance {
                distance = here;
                farthest = Some(index);
            }
        }
        if reversal.is_none() && previous_parameter > 1.0 + 1e-12 {
            reversal = Some(end - 1);
        }
        if let Some(index) = reversal.or(farthest) {
            keep[index] = true;
            ranges.push((start, index));
            ranges.push((index, end));
        }
    }
    points
        .iter()
        .copied()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(point))
        .collect()
}

type CubicSegment = ([[f64; 2]; 4], f64);

fn push_line_segment(segments: &mut Vec<CubicSegment>, from: Vec2, to: Vec2) {
    let length = from.distance(to);
    if length == 0.0 {
        return;
    }
    segments.push((
        [
            from.into(),
            from.lerp(to, 1.0 / 3.0).into(),
            from.lerp(to, 2.0 / 3.0).into(),
            to.into(),
        ],
        length,
    ));
}

fn rounded_polyline(points: &[[f64; 2]], allowance: f64) -> Option<NurbsCurve> {
    let mut segments = Vec::<CubicSegment>::new();
    let mut current = Vec2::from(points[0]);
    for window in points.windows(3) {
        let previous = Vec2::from(window[0]);
        let corner = Vec2::from(window[1]);
        let next = Vec2::from(window[2]);
        let incoming = (corner - previous).normalize()?;
        let outgoing = (next - corner).normalize()?;
        let trim = allowance
            .min(previous.distance(corner) * 0.5)
            .min(corner.distance(next) * 0.5);
        let entry = corner - incoming * trim;
        let exit = corner + outgoing * trim;
        push_line_segment(&mut segments, current, entry);
        if entry.distance(exit) > 0.0 {
            segments.push((
                [entry.into(), corner.into(), corner.into(), exit.into()],
                entry.distance(corner) + corner.distance(exit),
            ));
        }
        current = exit;
    }
    push_line_segment(&mut segments, current, Vec2::from(*points.last()?));

    let (first, rest) = segments.split_first()?;
    let mut control_points = first.0.to_vec();
    let mut knots = vec![0.0; 4];
    let mut parameter = first.1;
    for segment in rest {
        knots.extend([parameter; 3]);
        control_points.extend_from_slice(&segment.0[1..]);
        parameter += segment.1;
    }
    knots.extend([parameter; 4]);
    NurbsCurve::new_strict(3, control_points.clone(), knots, vec![1.0; control_points.len()])
}

/// In-place Thomas solve for a tridiagonal system: `a` sub-diagonal, `b` main,
/// `c` super, `d` right-hand side, overwritten with the solution.
fn solve_tridiagonal(a: &[f64], b: &[f64], c: &[f64], d: &mut [f64]) {
    let n = d.len();
    let mut scratch = vec![0.0f64; n];
    scratch[0] = c[0] / b[0];
    d[0] /= b[0];
    for i in 1..n {
        let m = b[i] - a[i] * scratch[i - 1];
        scratch[i] = c[i] / m;
        d[i] = (d[i] - a[i] * d[i - 1]) / m;
    }
    for i in (0..n - 1).rev() {
        d[i] -= scratch[i] * d[i + 1];
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2], slack: f64) -> bool {
        (a[0] - b[0]).abs() < slack && (a[1] - b[1]).abs() < slack
    }

    /// Degree 1 is a polyline, which makes the expected answers obvious.
    fn linear() -> NurbsCurve {
        NurbsCurve::new(
            1,
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            Vec::new(),
            None,
        )
        .unwrap()
    }

    /// A rational quarter circle of radius 100 about the origin, the standard
    /// case where ignoring weights gives a plausible but wrong answer.
    fn rational_quarter() -> NurbsCurve {
        let weight = std::f64::consts::FRAC_PI_4.cos();
        NurbsCurve::new(
            2,
            vec![[100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            Some(vec![1.0, weight, 1.0]),
        )
        .unwrap()
    }

    #[test]
    fn a_hairpin_projects_each_point_onto_its_own_pass() {
        // The two passes run a tenth apart, closer than the samples along
        // either: the nearest sample to a point is often on the other pass.
        let hairpin = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 0.1], [0.0, 0.1]],
            Vec::new(),
            None,
        )
        .unwrap();
        for step in 1..200 {
            let point = hairpin.point_at(step as f64 / 200.0);
            let back = hairpin.point_at(hairpin.parameter_at(point));
            assert!(close(back, point, 1e-9), "{step}: {point:?} -> {back:?}");
        }
    }

    #[test]
    fn a_point_just_short_of_a_closed_curves_end_projects_there() {
        // Four rational quarters round a unit circle, starting and ending at
        // (1, 0): the end sample ties with the start, and a point a hair
        // before the seam must not be pinned to it.
        let w = std::f64::consts::FRAC_PI_4.cos();
        let circle = NurbsCurve::new(
            2,
            vec![
                [1.0, 0.0], [1.0, -1.0], [0.0, -1.0], [-1.0, -1.0], [-1.0, 0.0],
                [-1.0, 1.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
            Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        )
        .unwrap();
        let below = [0.03_f64.cos(), 0.03_f64.sin()];
        let t = circle.parameter_at(below);
        assert!(close(circle.point_at(t), below, 1e-9), "{t}");
    }

    #[test]
    fn freehand_fit_stays_within_tolerance() {
        let source = [[0.0, 0.0], [1.0, 0.8], [2.0, -0.6], [3.0, 0.0]];
        let tolerance = 0.4;
        let curve = NurbsCurve::fit_polyline(&source, tolerance).unwrap();
        for point in curve.tessellate(32) {
            let distance = source
                .windows(2)
                .map(|pair| {
                    Vec2::from(point)
                        .distance_to_segment(pair[0].into(), pair[1].into())
                })
                .fold(f64::INFINITY, f64::min);
            assert!(distance <= tolerance + 1e-9);
        }
        for point in source {
            let on_curve = curve.point_at(curve.parameter_at(point));
            assert!(Vec2::from(point).distance(on_curve.into()) <= tolerance + 1e-9);
        }
    }

    #[test]
    fn zero_freehand_tolerance_keeps_the_polyline() {
        let source = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0]];
        let curve = NurbsCurve::fit_polyline(&source, 0.0).unwrap();
        assert_eq!(curve.degree(), 1);
        assert_eq!(curve.control_points(), source.as_slice());
    }

    #[test]
    fn the_derivative_matches_a_difference_quotient() {
        for curve in [linear(), rational_quarter()] {
            let (start, end) = curve.domain();
            let width = end - start;
            let step = width * 1e-6;
            // Sampled between the tenths rather than on them, so no sample
            // lands on an interior knot. A degree-one curve has a corner at
            // each of those, where the derivative genuinely jumps and a
            // central difference reports the average of the two sides — it
            // would be measuring its own blind spot, not the curve.
            for i in 0..10 {
                let u = start + width * ((i as f64 + 0.5) / 10.0);
                let analytic = curve.derivative_at_knot(u);
                let before = curve.point_at_knot(u - step);
                let after = curve.point_at_knot(u + step);
                let numeric = [
                    (after[0] - before[0]) / (2.0 * step),
                    (after[1] - before[1]) / (2.0 * step),
                ];
                let scale = numeric[0].abs().max(numeric[1].abs()).max(1.0);
                assert!(
                    close(analytic, numeric, 1e-4 * scale),
                    "u={u}: {analytic:?} vs {numeric:?}"
                );
            }
        }
    }

    #[test]
    fn the_derivative_of_a_circle_is_perpendicular_to_its_radius() {
        // The check a difference quotient could also pass, but which pins the
        // weights: on a rational circle the tangent is exactly perpendicular
        // to the radius everywhere, and dropping the weights breaks that.
        let curve = rational_quarter();
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            let point = Vec2::from(curve.point_at(t));
            let slope = Vec2::from(curve.derivative_at(t));
            assert!(
                (point.length() - 100.0).abs() < 1e-9,
                "not on the circle at {t}"
            );
            let alignment = point.normalize().unwrap().dot(slope.normalize().unwrap());
            assert!(alignment.abs() < 1e-9, "t={t}: alignment {alignment}");
        }
    }

    #[test]
    fn the_unit_parameter_derivative_scales_with_the_domain() {
        // `derivative_at` composes with `point_at`, so its magnitude is the
        // knot derivative times the domain width — the factor that makes a
        // length integral over 0..=1 come out right.
        let curve = rational_quarter();
        let (start, end) = curve.domain();
        let width = end - start;
        let by_knot = curve.derivative_at_knot(start + 0.5 * width);
        let by_unit = curve.derivative_at(0.5);
        assert!(close(
            by_unit,
            [by_knot[0] * width, by_knot[1] * width],
            1e-9
        ));
    }

    #[test]
    fn a_straight_run_has_a_constant_derivative() {
        // Degree one, so each span is a straight segment and the derivative
        // is the segment vector scaled by how much parameter it spans.
        let curve = linear();
        let first = curve.derivative_at_knot(0.25);
        let second = curve.derivative_at_knot(0.4);
        assert!(close(first, second, 1e-9), "{first:?} vs {second:?}");
        assert!(first[1].abs() < 1e-9, "the first span runs along X");
    }

    /// A cubic with an uneven knot vector, which is where the difference
    /// between mirroring the knots and regenerating them shows.
    fn uneven() -> NurbsCurve {
        NurbsCurve::new(
            3,
            vec![
                [0.0, 0.0],
                [1.0, 6.0],
                [4.0, 6.0],
                [6.0, 0.0],
                [9.0, 4.0],
                [12.0, 0.0],
            ],
            vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.9, 1.0, 1.0, 1.0, 1.0],
            None,
        )
        .unwrap()
    }

    #[test]
    fn reversing_traces_the_same_curve_backwards() {
        for curve in [linear(), uneven(), rational_quarter()] {
            let back = curve.reversed();
            for i in 0..=20 {
                let t = i as f64 / 20.0;
                assert!(
                    close(curve.point_at(t), back.point_at(1.0 - t), 1e-9),
                    "t={t}: {:?} vs {:?}",
                    curve.point_at(t),
                    back.point_at(1.0 - t)
                );
            }
        }
    }

    #[test]
    fn regenerating_the_knots_instead_would_move_the_curve() {
        // The failure this guards: reversing the control points and then
        // rebuilding a clamped *uniform* knot vector looks right and changes
        // the shape of any curve whose knots are not evenly spaced.
        let curve = uneven();
        let naive = NurbsCurve::new(
            curve.degree(),
            curve.control_points().iter().rev().copied().collect(),
            clamped_uniform_knots(curve.degree(), curve.control_points().len()),
            None,
        )
        .unwrap();
        let worst = (0..=20)
            .map(|i| {
                let t = i as f64 / 20.0;
                Vec2::from(curve.point_at(t)).distance(Vec2::from(naive.point_at(1.0 - t)))
            })
            .fold(0.0, f64::max);
        assert!(worst > 0.1, "the two agreed to within {worst}");
    }

    #[test]
    fn reversing_twice_is_the_original() {
        let curve = uneven();
        let there_and_back = curve.reversed().reversed();
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            assert!(close(curve.point_at(t), there_and_back.point_at(t), 1e-12));
        }
    }

    #[test]
    fn elevating_the_degree_does_not_move_the_curve() {
        for curve in [linear(), uneven(), rational_quarter()] {
            let raised = curve.elevated(1).unwrap();
            assert_eq!(raised.degree(), curve.degree() + 1);
            for i in 0..=20 {
                let t = i as f64 / 20.0;
                assert!(
                    close(curve.point_at(t), raised.point_at(t), 1e-9),
                    "degree {} at t={t}: {:?} vs {:?}",
                    curve.degree(),
                    curve.point_at(t),
                    raised.point_at(t)
                );
            }
        }
    }

    #[test]
    fn elevating_a_rational_curve_keeps_it_rational() {
        let curve = rational_quarter();
        let raised = curve.elevated(1).unwrap();
        assert!(raised.is_rational());
        // Still tracing the circle of radius 100 it started as.
        for i in 0..=10 {
            let point = Vec2::from(raised.point_at(i as f64 / 10.0));
            assert!((point.length() - 100.0).abs() < 1e-9, "{point:?}");
        }
    }

    #[test]
    fn elevating_by_more_than_one_step_compounds() {
        let curve = uneven();
        let raised = curve.elevated(3).unwrap();
        assert_eq!(raised.degree(), curve.degree() + 3);
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            assert!(close(curve.point_at(t), raised.point_at(t), 1e-8));
        }
    }

    #[test]
    fn elevating_by_nothing_has_nothing_to_do() {
        assert!(uneven().elevated(0).is_none());
    }

    #[test]
    fn a_clamped_curve_starts_and_ends_on_its_outer_control_points() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(close(curve.point_at(0.0), [0.0, 0.0], 1e-9));
        assert!(close(curve.point_at(1.0), [5.0, 0.0], 1e-9));
    }

    #[test]
    fn degree_one_reproduces_the_control_polygon() {
        let curve = linear();
        assert!(close(curve.point_at(0.0), [0.0, 0.0], 1e-9));
        assert!(close(curve.point_at(0.5), [10.0, 0.0], 1e-9));
        assert!(close(curve.point_at(1.0), [10.0, 10.0], 1e-9));
        assert!(close(curve.point_at(0.25), [5.0, 0.0], 1e-9));
    }

    #[test]
    fn a_curve_stays_inside_its_control_polygon_bounds() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        for i in 0..=50 {
            let p = curve.point_at(i as f64 / 50.0);
            assert!((0.0..=5.0).contains(&p[0]), "x escaped: {p:?}");
            assert!((0.0..=5.0).contains(&p[1]), "y escaped: {p:?}");
        }
    }

    #[test]
    fn weights_are_applied_rather_than_ignored() {
        // The standard rational quarter circle: three control points, the
        // middle one weighted 1/sqrt(2). Evaluated as a polynomial it misses
        // the unit circle badly; evaluated rationally it lies on it.
        let weight = std::f64::consts::FRAC_1_SQRT_2;
        let control = vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

        let rational =
            NurbsCurve::new(2, control.clone(), knots.clone(), Some(vec![1.0, weight, 1.0]))
                .unwrap();
        assert!(rational.is_rational());
        for i in 0..=20 {
            let p = rational.point_at(i as f64 / 20.0);
            let radius = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!(
                (radius - 1.0).abs() < 1e-9,
                "rational curve left the unit circle: {radius}"
            );
        }

        // Same control net without weights is a parabola, not a circle — the
        // difference this test exists to keep.
        let polynomial = NurbsCurve::new(2, control, knots, None).unwrap();
        assert!(!polynomial.is_rational());
        let middle = polynomial.point_at(0.5);
        let radius = (middle[0] * middle[0] + middle[1] * middle[1]).sqrt();
        assert!(
            (radius - 1.0).abs() > 0.05,
            "expected the unweighted curve to miss the circle, got {radius}"
        );
    }

    #[test]
    fn a_malformed_knot_vector_falls_back_to_a_clamped_uniform_one() {
        let curve = NurbsCurve::new(
            2,
            vec![[0.0, 0.0], [1.0, 2.0], [2.0, 0.0], [3.0, 2.0]],
            vec![0.0, 1.0], // nowhere near the right length
            None,
        )
        .unwrap();
        assert!(close(curve.point_at(0.0), [0.0, 0.0], 1e-9));
        assert!(close(curve.point_at(1.0), [3.0, 2.0], 1e-9));
    }

    #[test]
    fn too_few_control_points_is_not_a_curve() {
        assert!(NurbsCurve::new(3, vec![[0.0, 0.0], [1.0, 1.0]], Vec::new(), None).is_none());
        assert!(NurbsCurve::new(0, vec![[0.0, 0.0], [1.0, 1.0]], Vec::new(), None).is_none());
    }

    #[test]
    fn nonsense_weights_are_discarded_rather_than_dividing_by_zero() {
        let curve = NurbsCurve::new(
            2,
            vec![[0.0, 0.0], [1.0, 2.0], [2.0, 0.0]],
            Vec::new(),
            Some(vec![1.0, 0.0, 1.0]),
        )
        .unwrap();
        assert!(!curve.is_rational(), "a zero weight should be refused");
        assert!(curve.point_at(0.5)[1].is_finite());
    }

    #[test]
    fn the_parameter_inverts_the_evaluation() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0], [8.0, -3.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        for i in 1..10 {
            let t = i as f64 / 10.0;
            let point = curve.point_at(t);
            let recovered = curve.parameter_at(point);
            assert!(
                (recovered - t).abs() < 1e-4,
                "asked for {t}, got back {recovered}"
            );
        }
    }

    #[test]
    fn a_point_off_the_curve_projects_onto_it() {
        let curve = linear();
        // Above the middle of the first, horizontal, segment and nearer to it
        // than to the vertical one.
        let t = curve.parameter_at([5.0, 3.0]);
        assert!(
            close(curve.point_at(t), [5.0, 0.0], 1e-3),
            "expected the foot of the perpendicular, got {:?}",
            curve.point_at(t)
        );
    }

    #[test]
    fn tessellation_keeps_both_ends_and_stays_on_the_curve() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0], [8.0, -3.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        let points = curve.tessellate(8);
        assert!(close(points[0], curve.point_at(0.0), 1e-9));
        assert!(close(*points.last().unwrap(), curve.point_at(1.0), 1e-9));
        assert!(points.len() > 8, "expected a span per knot interval");
    }

    #[test]
    fn a_closed_curve_is_recognised() {
        let open = linear();
        assert!(!open.is_closed());

        let closed = NurbsCurve::new(
            1,
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 0.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(closed.is_closed());
    }

    #[test]
    fn inserting_a_knot_leaves_the_shape_alone() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0], [8.0, -3.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        let before: Vec<[f64; 2]> = (0..=40).map(|i| curve.point_at(i as f64 / 40.0)).collect();

        let mut inserted = curve.clone();
        inserted.insert_knot(0.37);
        assert_eq!(inserted.control_points().len(), curve.control_points().len() + 1);

        for (i, expected) in before.iter().enumerate() {
            let got = inserted.point_at(i as f64 / 40.0);
            assert!(close(got, *expected, 1e-9), "moved at {i}: {got:?} vs {expected:?}");
        }
    }

    #[test]
    fn both_halves_of_a_split_lie_on_the_original() {
        let curve = NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0], [8.0, -3.0]],
            Vec::new(),
            None,
        )
        .unwrap();
        let (left, right) = curve.split_at(0.4).expect("should split");

        // The halves meet where the cut was, and each covers its own stretch.
        assert!(close(left.point_at(0.0), curve.point_at(0.0), 1e-9));
        assert!(close(left.point_at(1.0), curve.point_at(0.4), 1e-9));
        assert!(close(right.point_at(0.0), curve.point_at(0.4), 1e-9));
        assert!(close(right.point_at(1.0), curve.point_at(1.0), 1e-9));

        for i in 0..=20 {
            let f = i as f64 / 20.0;
            assert!(
                close(left.point_at(f), curve.point_at(f * 0.4), 1e-9),
                "left half drifted at {f}"
            );
            assert!(
                close(right.point_at(f), curve.point_at(0.4 + f * 0.6), 1e-9),
                "right half drifted at {f}"
            );
        }
    }

    #[test]
    fn splitting_a_rational_curve_keeps_it_rational() {
        let weight = std::f64::consts::FRAC_1_SQRT_2;
        let curve = NurbsCurve::new(
            2,
            vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            Some(vec![1.0, weight, 1.0]),
        )
        .unwrap();
        let (left, right) = curve.split_at(0.5).expect("should split");
        // Both halves must still trace the unit circle.
        for half in [&left, &right] {
            for i in 0..=10 {
                let p = half.point_at(i as f64 / 10.0);
                let radius = (p[0] * p[0] + p[1] * p[1]).sqrt();
                assert!(
                    (radius - 1.0).abs() < 1e-9,
                    "half left the unit circle: {radius}"
                );
            }
        }
    }

    #[test]
    fn splitting_at_an_end_has_nothing_to_divide() {
        let curve = linear();
        assert!(curve.split_at(0.0).is_none());
        assert!(curve.split_at(1.0).is_none());
    }

    #[test]
    fn an_interpolated_curve_passes_through_every_fit_point() {
        let fit = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [5.0, 4.0]];
        let curve =
            NurbsCurve::interpolate(&fit, None, None, Parameterization::Chord).unwrap();
        // Each fit point sits at the knot value its parameterisation gave it,
        // and the joins carry multiplicity 3, so they are knots in the vector.
        let (start, end) = curve.domain();
        assert!(close(curve.point_at_knot(start), fit[0], 1e-9));
        assert!(close(curve.point_at_knot(end), fit[3], 1e-9));
        for target in fit {
            let t = curve.parameter_at(target);
            assert!(
                close(curve.point_at(t), target, 1e-6),
                "missed {target:?}, nearest was {:?}",
                curve.point_at(t)
            );
        }
    }

    #[test]
    fn two_fit_points_with_tangents_make_one_cubic() {
        // The case a drawing produces most often for a simple curve, and the
        // one that has no interior fit point to lean on.
        let fit = [[0.0, 0.0], [10.0, 0.0]];
        let curve = NurbsCurve::interpolate(
            &fit,
            Some([0.0, 1.0]),
            Some([0.0, -1.0]),
            Parameterization::Chord,
        )
        .unwrap();
        assert_eq!(curve.degree(), 3);
        assert_eq!(curve.control_points().len(), 4, "a single Bezier span");
        assert!(close(curve.point_at(0.0), fit[0], 1e-9));
        assert!(close(curve.point_at(1.0), fit[1], 1e-9));
        // Leaves upward and arrives heading downward, so it arches over.
        assert!(curve.point_at(0.5)[1] > 0.0, "should bow upward");
    }

    #[test]
    fn the_stored_tangents_are_followed_at_the_ends() {
        let fit = [[0.0, 0.0], [5.0, 0.0], [10.0, 0.0]];
        let curve = NurbsCurve::interpolate(
            &fit,
            Some([1.0, 2.0]),
            Some([1.0, -2.0]),
            Parameterization::Chord,
        )
        .unwrap();
        // Direction leaving the start, by finite difference.
        let a = curve.point_at(0.0);
        let b = curve.point_at(0.001);
        let leaving = (b[1] - a[1]) / (b[0] - a[0]);
        assert!((leaving - 2.0).abs() < 0.05, "left along {leaving}, wanted 2");

        let c = curve.point_at(0.999);
        let d = curve.point_at(1.0);
        let arriving = (d[1] - c[1]) / (d[0] - c[0]);
        assert!((arriving + 2.0).abs() < 0.05, "arrived along {arriving}");
    }

    #[test]
    fn interpolation_without_tangents_still_passes_through() {
        let fit = [[0.0, 0.0], [2.0, 3.0], [4.0, -1.0]];
        for mode in [
            Parameterization::Uniform,
            Parameterization::Centripetal,
            Parameterization::Chord,
        ] {
            let curve = NurbsCurve::interpolate(&fit, None, None, mode).unwrap();
            for target in fit {
                let t = curve.parameter_at(target);
                assert!(close(curve.point_at(t), target, 1e-6), "{mode:?} missed {target:?}");
            }
        }
    }

    #[test]
    fn an_interpolated_curve_can_be_split_like_any_other() {
        let fit = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [5.0, 4.0]];
        let curve =
            NurbsCurve::interpolate(&fit, None, None, Parameterization::Chord).unwrap();
        let (left, right) = curve.split_at(0.45).expect("should split");
        assert!(close(left.point_at(1.0), right.point_at(0.0), 1e-9));
        for i in 0..=10 {
            let f = i as f64 / 10.0;
            assert!(close(left.point_at(f), curve.point_at(f * 0.45), 1e-9));
        }
    }

    #[test]
    fn fewer_than_two_fit_points_is_not_a_curve() {
        assert!(NurbsCurve::interpolate(&[], None, None, Parameterization::Chord).is_none());
        assert!(
            NurbsCurve::interpolate(&[[1.0, 1.0]], None, None, Parameterization::Chord)
                .is_none()
        );
    }

    #[test]
    fn survey_coordinates_evaluate_without_drift() {
        let origin = [512_345.678, 4_512_345.678];
        let curve = NurbsCurve::new(
            3,
            vec![
                origin,
                [origin[0] + 1.0, origin[1] + 5.0],
                [origin[0] + 4.0, origin[1] + 5.0],
                [origin[0] + 5.0, origin[1]],
            ],
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(close(curve.point_at(0.0), origin, 1e-6));
        assert!(
            close(curve.point_at(1.0), [origin[0] + 5.0, origin[1]], 1e-6),
            "end drifted"
        );
    }
}
