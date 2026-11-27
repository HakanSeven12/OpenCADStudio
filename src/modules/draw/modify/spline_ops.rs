// Shared B-spline utilities for modify commands (TRIM, BREAK, OFFSET, LENGTHEN).
//
// The evaluation lives in opencadkernel; this file converts a drawing's SPLINE
// entity to and from it and keeps the knot-domain call shapes the commands
// were written against.
//
// Weights are carried through now. The previous path evaluated splines with a
// non-rational B-spline and cleared the weights on every split, which turns a
// piece of a NURBS circle into a piece of a parabola — a change that looks
// fine until it is measured.

use document::entities::Spline;
use document::types::Vector3;
use document::Handle;
use ocs_geom::geom2d::NurbsCurve;
pub use crate::entities::spline::{spline_to_nurbs, spline_to_nurbs_on};

/// The drawing plane a spline sits on.
///
/// Read from whichever point list the spline actually stores: a fit-point
/// spline carries no control points, and defaulting its elevation to zero
/// would drop every such spline to the ground plane on the first edit.
fn elevation(spl: &Spline) -> f64 {
    spl.control_points
        .first()
        .or_else(|| spl.fit_points.first())
        .map(|v| v.z)
        .unwrap_or(0.0)
}

// ── Conversion ─────────────────────────────────────────────────────────────

/// Rebuild a document `Spline` from a kernel curve.
///
/// The `template` supplies entity common data and the Z elevation; splines
/// here are planar, so the kernel's 2D points are lifted back onto it.
pub fn nurbs_to_spline(curve: &NurbsCurve, template: &Spline) -> Spline {
    let z = elevation(template);
    let mut spl = template.clone();
    spl.common.handle = Handle::NULL;
    spl.degree = curve.degree() as i32;
    spl.knots = curve.knots().to_vec();
    spl.control_points = curve
        .control_points()
        .iter()
        .map(|p| Vector3::new(p[0], p[1], z))
        .collect();
    spl.weights = if curve.is_rational() {
        curve.weights().to_vec()
    } else {
        Vec::new()
    };
    // Fit points and their tangents describe the original interpolation, which
    // a piece of it no longer satisfies. What comes back is a control-point
    // spline.
    spl.fit_points.clear();
    spl.begin_tangent = Vector3::new(0.0, 0.0, 0.0);
    spl.end_tangent = Vector3::new(0.0, 0.0, 0.0);
    spl
}

// ── Domain and splitting ───────────────────────────────────────────────────

/// The knot values the spline is defined over, `(start, end)`.
pub fn spline_range(spl: &Spline) -> Option<(f64, f64)> {
    Some(spline_to_nurbs(spl)?.domain())
}

/// Split a spline at knot value `t`, returning `(left, right)`.
///
/// Both halves are exact — the kernel raises the cut to full knot
/// multiplicity rather than refitting — and a rational spline stays rational.
/// `None` when the cut falls at or outside an end.
pub fn spline_cut(spl: &Spline, t: f64) -> Option<(Spline, Spline)> {
    let curve = spline_to_nurbs(spl)?;
    let (start, end) = curve.domain();
    if (end - start).abs() < 1e-12 {
        return None;
    }
    let normalised = (t - start) / (end - start);
    let (left, right) = curve.split_at(normalised)?;
    Some((nurbs_to_spline(&left, spl), nurbs_to_spline(&right, spl)))
}

// ── Sampling ───────────────────────────────────────────────────────────────

/// Sample the spline's XY projection at `n+1` evenly spaced knot values.
/// Returns `(knot_params, xy_points)`, both of length `n+1`.
pub fn spline_sample_xy(spl: &Spline, n: usize) -> (Vec<f64>, Vec<[f64; 2]>) {
    let Some(curve) = spline_to_nurbs(spl) else {
        return (vec![], vec![]);
    };
    let (t0, t1) = curve.domain();
    let ts: Vec<f64> = (0..=n)
        .map(|i| t0 + (t1 - t0) * (i as f64 / n as f64))
        .collect();
    let pts: Vec<[f64; 2]> = ts.iter().map(|t| curve.point_at_knot(*t)).collect();
    (ts, pts)
}

/// The knot parameter closest to the XY point `(x, y)`.
pub fn spline_nearest_t(spl: &Spline, x: f64, y: f64) -> Option<f64> {
    let curve = spline_to_nurbs(spl)?;
    let (t0, t1) = curve.domain();
    let normalised = curve.parameter_at([x, y]);
    Some(t0 + normalised * (t1 - t0))
}

/// Normalise an actual knot parameter into `0..=1` over `[t0, t1]`.
pub fn t_to_rel(t_actual: f64, t0: f64, t1: f64) -> f64 {
    if (t1 - t0).abs() < 1e-12 {
        return 0.0;
    }
    ((t_actual - t0) / (t1 - t0)).clamp(0.0, 1.0)
}

// ── Wire preview ───────────────────────────────────────────────────────────

/// World-space wire points for a Spline.
///
/// Sampled per knot span rather than uniformly, so a curve whose knots bunch
/// up is cut finely where its shape actually changes.
pub fn spline_pts_wire(spl: &Spline) -> Vec<[f32; 3]> {
    let Some(curve) = spline_to_nurbs(spl) else {
        return vec![];
    };
    let elev = elevation(spl) as f32;
    curve
        .tessellate(16)
        .iter()
        .map(|p| [p[0] as f32, p[1] as f32, elev])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_periodic_spline_matches_explicit_basis() {
        let mut compact = Spline::new();
        compact.degree = 2;
        compact.flags.closed = true;
        compact.flags.periodic = true;
        compact.flags.rational = true;
        compact.control_points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(3.0, 1.0, 0.0),
            Vector3::new(2.0, 4.0, 0.0),
        ];
        compact.weights = vec![1.0, 2.0, 0.5];
        compact.knots = vec![0.0, 1.0, 3.0, 4.0];

        let mut full = compact.clone();
        full.control_points
            .extend_from_slice(&compact.control_points[..2]);
        full.weights.extend_from_slice(&compact.weights[..2]);
        full.knots = vec![-3.0, -1.0, 0.0, 1.0, 3.0, 4.0, 5.0, 7.0];

        let actual = spline_to_nurbs(&compact).unwrap();
        let expected = spline_to_nurbs(&full).unwrap();
        assert_eq!(actual.domain(), (0.0, 4.0));
        for knot in [0.0, 0.5, 1.0, 2.0, 3.0, 3.5, 4.0] {
            assert_eq!(actual.point_at_knot(knot), expected.point_at_knot(knot));
        }
    }
}
