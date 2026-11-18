//! Shared tessellation policy.

use std::f64::consts::{FRAC_PI_2, PI};

/// Default maximum change of direction between samples: 7.5 degrees.
pub const DEFAULT_ANGLE: f64 = PI / 24.0;
/// Coarsest chordal display angle: 20 degrees.
pub const DISPLAY_ANGLE: f64 = PI / 9.0;
/// Finest display angle: 5 degrees.
pub const FINE_ANGLE: f64 = PI / 36.0;

/// Whether a knot span is a sliver of the widest span, `widths` being every
/// span's parameter width. A sliver is crossed with one chord to its end
/// instead of being sampled: a span a thousandth of its neighbours' width
/// with far-off control points (a closing span some exporters append) would
/// otherwise draw a long spike that the curve's sampling by parameter never
/// shows (#3221).
pub fn sliver_span(width: f64, widths: impl IntoIterator<Item = f64>) -> bool {
    let widest = widths.into_iter().fold(0.0_f64, f64::max);
    width < 1e-3 * widest
}

/// Returns a finite angular threshold in radians.
pub fn angle(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value.min(FRAC_PI_2)
    } else {
        DEFAULT_ANGLE
    }
}

/// Maps a display resolution to a maximum direction change.
pub fn angle_for_resolution(resolution: f64) -> f64 {
    let resolution = if resolution.is_finite() && resolution > 0.0 {
        resolution.clamp(0.01, 10.0)
    } else {
        1.0
    };
    angle(DEFAULT_ANGLE / resolution.sqrt()).max(FINE_ANGLE)
}

/// Maps display resolution to an angular cap for chordal tessellation.
pub fn display_angle_for_resolution(resolution: f64) -> f64 {
    let resolution = if resolution.is_finite() && resolution > 0.0 {
        resolution.clamp(0.01, 10.0)
    } else {
        1.0
    };
    angle(DISPLAY_ANGLE / resolution.max(1.0).sqrt()).max(FINE_ANGLE)
}

/// The cosine of the angle between two directions. Two stationary samples
/// agree; a zero direction beside a nonzero one returns `-1`, since that can
/// be a cusp and cannot satisfy an angular bound without further refinement.
fn direction_cosine<const N: usize>(a: [f64; N], b: [f64; N]) -> f64 {
    let dot = (0..N).map(|axis| a[axis] * b[axis]).sum::<f64>();
    let aa = (0..N).map(|axis| a[axis] * a[axis]).sum::<f64>();
    let bb = (0..N).map(|axis| b[axis] * b[axis]).sum::<f64>();
    if aa <= f64::MIN_POSITIVE && bb <= f64::MIN_POSITIVE {
        return 1.0;
    }
    let length = (aa * bb).sqrt();
    if !length.is_finite() || length <= f64::MIN_POSITIVE {
        -1.0
    } else {
        (dot / length).clamp(-1.0, 1.0)
    }
}

/// Largest separation between any two sampled directions. The arc cosine
/// falls as the cosine rises, so it is taken once, of the smallest cosine.
pub fn max_direction_angle<const N: usize>(directions: &[[f64; N]]) -> f64 {
    let mut smallest: f64 = 1.0;
    for from in 0..directions.len() {
        for to in from + 1..directions.len() {
            smallest = smallest.min(direction_cosine(directions[from], directions[to]));
        }
    }
    smallest.acos()
}

/// Samples a parametric space curve by tangent rotation.
pub fn sample_curve3_angle<P, T>(
    point_at: P,
    tangent_at: T,
    max_angle: f64,
) -> Vec<[f64; 3]>
where
    P: Fn(f64) -> [f64; 3],
    T: Fn(f64) -> [f64; 3],
{
    fn refine<P, T>(
        point_at: &P,
        tangent_at: &T,
        from: f64,
        to: f64,
        max_angle: f64,
        depth: u32,
        out: &mut Vec<[f64; 3]>,
    ) where
        P: Fn(f64) -> [f64; 3],
        T: Fn(f64) -> [f64; 3],
    {
        const MAX_DEPTH: u32 = 16;
        let directions = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
            .map(|unit| tangent_at(from + (to - from) * unit));
        let split = depth < 2 || max_direction_angle(&directions) > max_angle;
        if split && depth < MAX_DEPTH {
            let middle = 0.5 * (from + to);
            refine(
                point_at,
                tangent_at,
                from,
                middle,
                max_angle,
                depth + 1,
                out,
            );
            refine(
                point_at,
                tangent_at,
                middle,
                to,
                max_angle,
                depth + 1,
                out,
            );
        } else {
            out.push(point_at(to));
        }
    }

    let max_angle = angle(max_angle);
    let mut out = vec![point_at(0.0)];
    refine(
        &point_at,
        &tangent_at,
        0.0,
        1.0,
        max_angle,
        0,
        &mut out,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stationary_tangents_agree_without_hiding_a_cusp() {
        assert_eq!(max_direction_angle(&[[0.0, 0.0], [0.0, 0.0]]), 0.0);
        assert_eq!(
            max_direction_angle(&[[0.0, 0.0], [1.0, 0.0]]),
            std::f64::consts::PI,
            "a stationary point beside a moving one can be a cusp and still needs refinement"
        );
    }
}
