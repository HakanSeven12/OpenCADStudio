//! de Boor's algorithm, over whatever a control point happens to be.
//!
//! A B-spline is evaluated by repeatedly taking weighted averages of nearby
//! control points, and nothing in that depends on how many numbers a control
//! point holds. A plane curve averages `(x, y, w)`, a space curve
//! `(x, y, z, w)`, and a surface averages rows of either — so the algorithm is
//! written once here rather than once per dimension.
//!
//! Homogeneous throughout: a rational curve is a polynomial one in a space
//! with an extra coordinate, and dividing by that coordinate at the end is
//! what makes it rational. Keeping the division out of the recursion is what
//! makes a weighted curve come out exactly right rather than nearly.

/// The value of a B-spline at knot parameter `u`.
///
/// `points` are the control points in whatever space they live in, already
/// homogeneous if the curve is rational. `knots` must hold
/// `points.len() + degree + 1` entries; a caller that cannot promise that
/// should build the curve through a type that checks.
pub fn de_boor<const N: usize>(
    degree: usize,
    knots: &[f64],
    points: &[[f64; N]],
    u: f64,
) -> [f64; N] {
    de_boor_by(degree, knots, points.len(), u, |index| points[index])
}

pub fn de_boor_by<const N: usize, F>(
    degree: usize,
    knots: &[f64],
    point_count: usize,
    u: f64,
    mut point_at: F,
) -> [f64; N]
where
    F: FnMut(usize) -> [f64; N],
{
    if point_count == 0 {
        return [0.0; N];
    }
    if degree == 0 {
        let index = knots
            .iter()
            .rposition(|knot| *knot <= u)
            .unwrap_or(0)
            .min(point_count - 1);
        return point_at(index);
    }
    let last = point_count - 1;
    let span = span_of(degree, knots, last, u);
    const STACK_POINTS: usize = 16;
    if degree < STACK_POINTS {
        let mut working = [[0.0; N]; STACK_POINTS];
        for (step, slot) in working.iter_mut().enumerate().take(degree + 1) {
            *slot = point_at(span + step - degree);
        }
        return blend(degree, knots, span, u, &mut working[..=degree]);
    }
    let mut working = (0..=degree)
        .map(|step| point_at(span + step - degree))
        .collect::<Vec<_>>();
    blend(degree, knots, span, u, &mut working)
}

fn blend<const N: usize>(
    degree: usize,
    knots: &[f64],
    span: usize,
    u: f64,
    working: &mut [[f64; N]],
) -> [f64; N] {
    for round in 1..=degree {
        for step in (round..=degree).rev() {
            let index = span + step - degree;
            let lower = knots[index];
            let upper = knots[index + degree + 1 - round];
            let alpha = if (upper - lower).abs() < 1e-15 {
                0.0
            } else {
                (u - lower) / (upper - lower)
            };
            let (previous, current) = (working[step - 1], working[step]);
            for (slot, (before, after)) in working[step]
                .iter_mut()
                .zip(previous.iter().zip(current.iter()))
            {
                *slot = (1.0 - alpha) * before + alpha * after;
            }
        }
    }
    working[degree]
}

/// Which knot span `u` falls in, found by bisection.
///
/// Past the end it is the last span rather than none: a parameter at exactly
/// the domain's top is on the curve, and reporting it as outside would leave
/// every curve one point short.
pub fn span_of(degree: usize, knots: &[f64], last: usize, u: f64) -> usize {
    if u >= knots[last + 1] {
        return last;
    }
    let mut low = degree;
    let mut high = last + 1;
    while high - low > 1 {
        let middle = (low + high) / 2;
        if u < knots[middle] {
            high = middle;
        } else {
            low = middle;
        }
    }
    low
}

/// A clamped uniform knot vector: `degree + 1` zeros, evenly spaced interior
/// values, then `degree + 1` ones.
///
/// Clamped so the curve starts at its first control point and ends at its
/// last, which is what every drawing format assumes and what makes a spline
/// joinable to what comes before and after it.
pub fn clamped_uniform_knots(degree: usize, control_point_count: usize) -> Vec<f64> {
    let spans = control_point_count.saturating_sub(degree);
    let mut knots = Vec::with_capacity(control_point_count + degree + 1);
    knots.extend(std::iter::repeat_n(0.0, degree + 1));
    for step in 1..spans {
        knots.push(step as f64 / spans.max(1) as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    knots
}

/// Fit-point spacing used by spline interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parameterization {
    Uniform,
    Centripetal,
    Chord,
}

/// Natural or endpoint-clamped C² interpolation in any coordinate dimension.
/// The returned polynomial B-spline passes through every fit point; it is
/// not a polyline approximation. Parameter spans use the full source-space
/// distance, so a spatial fit never changes when viewed in another plane.
pub fn interpolate_open<const N: usize>(
    points: &[[f64; N]],
    start_tangent: Option<[f64; N]>,
    end_tangent: Option<[f64; N]>,
    parameterization: Parameterization,
) -> Option<(Vec<[f64; N]>, Vec<f64>)> {
    let count = points.len();
    if count < 2 || !points.iter().flatten().all(|value| value.is_finite())
        || start_tangent.iter().chain(end_tangent.iter()).flatten().any(|value| !value.is_finite())
    {
        return None;
    }
    let usable = |tangent: Option<[f64; N]>| {
        tangent.filter(|vector| vector.iter().map(|value| value * value).sum::<f64>() > 1e-18)
    };
    let start_tangent = usable(start_tangent);
    let end_tangent = usable(end_tangent);
    let spans = points.windows(2).map(|pair| {
        let distance = (0..N).map(|axis| (pair[1][axis] - pair[0][axis]).powi(2))
            .sum::<f64>().sqrt().max(1e-9);
        match parameterization {
            Parameterization::Uniform => 1.0,
            Parameterization::Centripetal => distance.sqrt(),
            Parameterization::Chord => distance,
        }
    }).collect::<Vec<_>>();
    if !spans.iter().all(|span| span.is_finite()) { return None; }

    let mut lower = vec![0.0; count];
    let mut diagonal = vec![0.0; count];
    let mut upper = vec![0.0; count];
    let mut slopes = vec![[0.0; N]; count];
    if let Some(tangent) = start_tangent {
        diagonal[0] = 1.0;
        slopes[0] = tangent;
    } else {
        diagonal[0] = 2.0;
        upper[0] = 1.0;
        for axis in 0..N {
            slopes[0][axis] = 3.0 * (points[1][axis] - points[0][axis]) / spans[0];
        }
    }
    for index in 1..count - 1 {
        let before = spans[index - 1];
        let after = spans[index];
        lower[index] = after;
        diagonal[index] = 2.0 * (before + after);
        upper[index] = before;
        for axis in 0..N {
            let incoming = (points[index][axis] - points[index - 1][axis]) / before;
            let outgoing = (points[index + 1][axis] - points[index][axis]) / after;
            slopes[index][axis] = 3.0 * (after * incoming + before * outgoing);
        }
    }
    if let Some(tangent) = end_tangent {
        diagonal[count - 1] = 1.0;
        slopes[count - 1] = tangent;
    } else {
        lower[count - 1] = 1.0;
        diagonal[count - 1] = 2.0;
        for axis in 0..N {
            slopes[count - 1][axis] = 3.0 * (points[count - 1][axis] - points[count - 2][axis]) / spans[count - 2];
        }
    }
    solve_tridiagonal_system(&lower, &diagonal, &upper, &mut slopes)?;

    let mut controls = Vec::with_capacity(3 * count - 2);
    let mut knots = vec![0.0; 4];
    let mut parameter = 0.0;
    controls.push(points[0]);
    for index in 0..count - 1 {
        let mut after = points[index];
        let mut before = points[index + 1];
        for axis in 0..N {
            after[axis] += slopes[index][axis] * spans[index] / 3.0;
            before[axis] -= slopes[index + 1][axis] * spans[index] / 3.0;
        }
        controls.extend([after, before, points[index + 1]]);
        parameter += spans[index];
        if index + 1 < count - 1 { knots.extend([parameter; 3]); }
    }
    knots.extend([parameter; 4]);
    (controls.iter().flatten().all(|value| value.is_finite()) && parameter.is_finite())
        .then_some((controls, knots))
}

pub fn interpolate_periodic<const N: usize>(
    points: &[[f64; N]],
    parameterization: Parameterization,
) -> Option<(Vec<[f64; N]>, Vec<f64>)> {
    let mut points = points.to_vec();
    if points.len() > 1 {
        let distance2 = (0..N)
            .map(|axis| (points[points.len() - 1][axis] - points[0][axis]).powi(2))
            .sum::<f64>();
        if distance2 <= 1e-18 {
            points.pop();
        }
    }
    let count = points.len();
    if count < 3 {
        return None;
    }

    let spans: Vec<f64> = (0..count)
        .map(|index| {
            let next = (index + 1) % count;
            let chord = (0..N)
                .map(|axis| (points[next][axis] - points[index][axis]).powi(2))
                .sum::<f64>()
                .sqrt()
                .max(1e-9);
            match parameterization {
                Parameterization::Uniform => 1.0,
                Parameterization::Centripetal => chord.sqrt(),
                Parameterization::Chord => chord,
            }
        })
        .collect();

    let mut lower = vec![0.0; count];
    let mut diagonal = vec![0.0; count];
    let mut upper = vec![0.0; count];
    let mut right = vec![[0.0; N]; count];
    for index in 0..count {
        let previous = (index + count - 1) % count;
        let next = (index + 1) % count;
        let before = spans[previous];
        let after = spans[index];
        lower[index] = after;
        diagonal[index] = 2.0 * (before + after);
        upper[index] = before;
        for axis in 0..N {
            let previous_slope = (points[index][axis] - points[previous][axis]) / before;
            let next_slope = (points[next][axis] - points[index][axis]) / after;
            right[index][axis] =
                3.0 * (after * previous_slope + before * next_slope);
        }
    }
    let alpha = lower[0];
    let beta = upper[count - 1];
    lower[0] = 0.0;
    upper[count - 1] = 0.0;
    let slopes = solve_cyclic_system(&lower, &diagonal, &upper, alpha, beta, right)?;

    let mut control_points = Vec::with_capacity(3 * count + 1);
    let mut boundaries = Vec::with_capacity(count + 1);
    control_points.push(points[0]);
    boundaries.push(0.0);
    let mut parameter = 0.0;
    for index in 0..count {
        let next = (index + 1) % count;
        let span = spans[index];
        let mut after = points[index];
        let mut before = points[next];
        for axis in 0..N {
            after[axis] += slopes[index][axis] * span / 3.0;
            before[axis] -= slopes[next][axis] * span / 3.0;
        }
        control_points.extend([after, before, points[next]]);
        parameter += span;
        boundaries.push(parameter);
    }

    let mut knots = vec![0.0; 4];
    for boundary in boundaries.iter().take(count).skip(1) {
        knots.extend([*boundary; 3]);
    }
    knots.extend([parameter; 4]);
    Some((control_points, knots))
}

fn solve_cyclic_system<const N: usize>(
    lower: &[f64],
    diagonal: &[f64],
    upper: &[f64],
    alpha: f64,
    beta: f64,
    mut right: Vec<[f64; N]>,
) -> Option<Vec<[f64; N]>> {
    let count = right.len();
    let gamma = -diagonal[0];
    if gamma.abs() < 1e-14 {
        return None;
    }
    let mut adjusted = diagonal.to_vec();
    adjusted[0] -= gamma;
    adjusted[count - 1] -= alpha * beta / gamma;
    solve_tridiagonal_system(lower, &adjusted, upper, &mut right)?;

    let mut correction = vec![[0.0; N]; count];
    correction[0] = [gamma; N];
    correction[count - 1] = [alpha; N];
    solve_tridiagonal_system(lower, &adjusted, upper, &mut correction)?;
    for axis in 0..N {
        let denominator = 1.0
            + correction[0][axis]
            + beta * correction[count - 1][axis] / gamma;
        if denominator.abs() < 1e-14 {
            return None;
        }
        let factor =
            (right[0][axis] + beta * right[count - 1][axis] / gamma) / denominator;
        for row in 0..count {
            right[row][axis] -= factor * correction[row][axis];
        }
    }
    right
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(right)
}

fn solve_tridiagonal_system<const N: usize>(
    lower: &[f64],
    diagonal: &[f64],
    upper: &[f64],
    right: &mut [[f64; N]],
) -> Option<()> {
    let count = right.len();
    let mut modified_upper = vec![0.0; count];
    let first = diagonal[0];
    if first.abs() < 1e-14 {
        return None;
    }
    modified_upper[0] = upper[0] / first;
    for value in &mut right[0] {
        *value /= first;
    }
    for row in 1..count {
        let pivot = diagonal[row] - lower[row] * modified_upper[row - 1];
        if pivot.abs() < 1e-14 {
            return None;
        }
        modified_upper[row] = upper[row] / pivot;
        for axis in 0..N {
            right[row][axis] =
                (right[row][axis] - lower[row] * right[row - 1][axis]) / pivot;
        }
    }
    for row in (0..count - 1).rev() {
        for axis in 0..N {
            right[row][axis] -= modified_upper[row] * right[row + 1][axis];
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clamped_curve_starts_and_ends_on_its_own_control_points() {
        let points = [[0.0, 0.0, 1.0], [1.0, 4.0, 1.0], [5.0, 4.0, 1.0], [6.0, 0.0, 1.0]];
        let knots = clamped_uniform_knots(3, 4);
        let start = de_boor(3, &knots, &points, 0.0);
        let end = de_boor(3, &knots, &points, 1.0);
        assert!((start[0]).abs() < 1e-12 && (start[1]).abs() < 1e-12, "{start:?}");
        assert!((end[0] - 6.0).abs() < 1e-12 && (end[1]).abs() < 1e-12, "{end:?}");
    }

    #[test]
    fn the_same_curve_evaluates_alike_however_many_coordinates_it_carries() {
        // The reason this is generic: a plane curve and the same curve with a
        // zero z must agree, and they only do if one algorithm evaluates both.
        let flat = [[0.0, 0.0, 1.0], [2.0, 6.0, 1.0], [8.0, 6.0, 1.0], [10.0, 0.0, 1.0]];
        let spatial: Vec<[f64; 4]> = flat
            .iter()
            .map(|point| [point[0], point[1], 0.0, point[2]])
            .collect();
        let knots = clamped_uniform_knots(3, 4);
        for step in 0..=10 {
            let t = step as f64 / 10.0;
            let here = de_boor(3, &knots, &flat, t);
            let there = de_boor(3, &knots, &spatial, t);
            assert!((here[0] - there[0]).abs() < 1e-12, "t={t}");
            assert!((here[1] - there[1]).abs() < 1e-12, "t={t}");
            assert!(there[2].abs() < 1e-12, "t={t}");
        }
    }

    #[test]
    fn a_weighted_curve_bends_towards_the_heavier_point() {
        // Degree two, three points, the middle one weighted. In homogeneous
        // coordinates that is one control point further out, and dividing at
        // the end is what makes it rational.
        let knots = clamped_uniform_knots(2, 3);
        let plain = [[0.0, 0.0, 1.0], [1.0, 1.0, 1.0], [2.0, 0.0, 1.0]];
        let heavy = [[0.0, 0.0, 1.0], [5.0, 5.0, 5.0], [2.0, 0.0, 1.0]];
        let middle = |points: &[[f64; 3]; 3]| {
            let raw = de_boor(2, &knots, points, 0.5);
            [raw[0] / raw[2], raw[1] / raw[2]]
        };
        assert!(middle(&heavy)[1] > middle(&plain)[1]);
    }

    #[test]
    fn a_degree_one_curve_is_the_polyline_through_its_points() {
        let points = [[0.0, 0.0, 1.0], [10.0, 0.0, 1.0], [10.0, 10.0, 1.0]];
        let knots = clamped_uniform_knots(1, 3);
        let half = de_boor(1, &knots, &points, 0.25);
        assert!((half[0] - 5.0).abs() < 1e-12, "{half:?}");
        assert!(half[1].abs() < 1e-12, "{half:?}");
    }

    #[test]
    fn a_span_past_the_end_is_the_last_one_rather_than_none() {
        let knots = clamped_uniform_knots(3, 6);
        assert_eq!(span_of(3, &knots, 5, 1.0), 5);
        assert_eq!(span_of(3, &knots, 5, 2.0), 5);
        assert_eq!(span_of(3, &knots, 5, 0.0), 3);
    }
}
