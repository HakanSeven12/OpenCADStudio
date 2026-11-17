//! Small vector helpers and the curve evaluation the mapper needs to find the
//! parameters of an edge's vertices on its curve.

use solidcore_record::step::model::{BSplineCurve, Frame, P3};

/// `a - b`.
pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a * s`.
pub fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// The dot product.
pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn dist(a: P3, b: P3) -> f64 {
    norm(sub(a, b))
}

/// `a / |a|`, or `None` for a zero or non-finite vector.
pub(crate) fn unit(a: P3) -> Option<P3> {
    let n = norm(a);
    (n.is_finite() && n > 0.0).then(|| scale(a, 1.0 / n))
}

/// ISO 10303-42 `build_axes` for an `AXIS2_PLACEMENT_3D`: Z = axis (default +Z),
/// X = `ref_direction` (default +X, or +Z when the axis is +X) made perpendicular to
/// Z, Y = Z x X.
pub(crate) fn build_frame(origin: P3, axis: Option<P3>, ref_dir: Option<P3>) -> Option<Frame> {
    let z = unit(axis.unwrap_or([0.0, 0.0, 1.0]))?;
    let v = match ref_dir {
        Some(r) => r,
        None if z != [1.0, 0.0, 0.0] => [1.0, 0.0, 0.0],
        None => [0.0, 0.0, 1.0],
    };
    let x = if let Some(x) = unit(sub(v, scale(z, dot(v, z)))) {
        x
    } else {
        // ref_direction parallel to the axis: pick any perpendicular.
        let a = if z[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        unit(sub(a, scale(z, dot(a, z))))?
    };
    let y = cross(z, x);
    Some(Frame { origin, x, y, z })
}

/// Expanded knot vector, or `None` when the multiplicities do not fit.
pub fn expand_knots(knots: &[f64], mults: &[u32], expected: usize) -> Option<Vec<f64>> {
    let mut total: usize = 0;
    for &m in mults {
        total = total.checked_add(m as usize)?;
    }
    if total != expected || knots.len() != mults.len() {
        return None;
    }
    let mut out = Vec::new();
    for (k, &m) in knots.iter().zip(mults) {
        for _ in 0..m {
            out.push(*k);
        }
    }
    Some(out)
}

/// The point at `t` (de Boor), homogeneous when rational; `None` for a curve the reader
/// refuses with [`check_bspline_curve`](crate::map::check_bspline_curve) (fewer than
/// degree + 1 points, knots not increasing, multiplicities or weights out of range),
/// which a model built by hand may carry.
pub fn bspline_point(c: &BSplineCurve, t: f64) -> Option<P3> {
    let p = c.degree as usize;
    let n = c.points.len();
    crate::map::check_bspline_curve(c).ok()?;
    let u = expand_knots(&c.knots, &c.mults, n.checked_add(p)?.checked_add(1)?)?;
    let lo = *u.get(p)?;
    let hi = *u.get(n)?;
    let t = t.clamp(lo, hi);
    // span k with u[k] <= t < u[k+1], k in [p, n-1]
    let mut k = p;
    while k + 1 < n && *u.get(k + 1)? <= t {
        k += 1;
    }
    let mut d: Vec<[f64; 4]> = Vec::new();
    for j in 0..=p {
        let i = (k + j).checked_sub(p)?;
        let pt = *c.points.get(i)?;
        let w = match &c.weights {
            Some(ws) => *ws.get(i)?,
            None => 1.0,
        };
        d.push([pt[0] * w, pt[1] * w, pt[2] * w, w]);
    }
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = (k + j).checked_sub(p)?;
            let a_lo = *u.get(i)?;
            let a_hi = *u.get(i + p + 1 - r)?;
            let den = a_hi - a_lo;
            let a = if den == 0.0 { 0.0 } else { (t - a_lo) / den };
            let prev = *d.get(j - 1)?;
            let cur = d.get_mut(j)?;
            for (c, pv) in cur.iter_mut().zip(prev) {
                *c = (1.0 - a) * pv + a * *c;
            }
        }
    }
    let h = *d.get(p)?;
    (h[3] != 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
}

/// The parameter range of a curve, `None` for an unbounded one.
pub(crate) fn bspline_range(c: &BSplineCurve) -> Option<(f64, f64)> {
    let p = c.degree as usize;
    let n = c.points.len();
    let u = expand_knots(&c.knots, &c.mults, n.checked_add(p)?.checked_add(1)?)?;
    Some((*u.get(p)?, *u.get(n)?))
}

/// The parameter of the point of `c` closest to `q`: the best of a dense sample,
/// refined by bisection of the distance on the neighbouring interval.
pub(crate) fn bspline_param(c: &BSplineCurve, q: P3) -> Option<f64> {
    let (lo, hi) = bspline_range(c)?;
    let spans = c.knots.len().max(2);
    let n = (spans * 16).min(4096);
    let step = (hi - lo) / n as f64;
    let mut best = (f64::INFINITY, lo);
    for i in 0..=n {
        let t = if i == n { hi } else { lo + step * i as f64 };
        let d = dist(bspline_point(c, t)?, q);
        if d < best.0 {
            best = (d, t);
        }
    }
    let (mut a, mut b) = ((best.1 - step).max(lo), (best.1 + step).min(hi));
    for _ in 0..100 {
        let m1 = a + (b - a) / 3.0;
        let m2 = b - (b - a) / 3.0;
        if dist(bspline_point(c, m1)?, q) < dist(bspline_point(c, m2)?, q) {
            b = m2;
        } else {
            a = m1;
        }
    }
    let t = 0.5 * (a + b);
    // Snap to an end when that end is at least as close (clamped ends are exact).
    let dt = dist(bspline_point(c, t)?, q);
    if dist(bspline_point(c, lo)?, q) <= dt {
        return Some(lo);
    }
    if dist(bspline_point(c, hi)?, q) <= dt {
        return Some(hi);
    }
    Some(t)
}

/// The angle (degrees, in (-180, 180]) of `q` on an ellipse with semi axes `a`, `b`.
pub(crate) fn conic_angle(frame: &Frame, a: f64, b: f64, q: P3) -> f64 {
    let d = sub(q, frame.origin);
    let x = dot(d, frame.x) / a;
    let y = dot(d, frame.y) / b;
    y.atan2(x).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames() {
        let f = build_frame([0.0; 3], None, None).unwrap();
        assert_eq!(f.x, [1.0, 0.0, 0.0]);
        assert_eq!(f.y, [0.0, 1.0, 0.0]);
        let f = build_frame([0.0; 3], Some([1.0, 0.0, 0.0]), None).unwrap();
        assert_eq!(f.x, [0.0, 0.0, 1.0]);
        assert_eq!(f.y, [0.0, -1.0, 0.0]);
        assert!(build_frame([0.0; 3], Some([0.0; 3]), None).is_none());
    }

    #[test]
    fn bspline_eval_and_project() {
        // Quadratic rational quarter circle.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let c = BSplineCurve {
            degree: 2,
            points: vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            weights: Some(vec![1.0, w, 1.0]),
            knots: vec![0.0, 1.0],
            mults: vec![3, 3],
        };
        let m = bspline_point(&c, 0.5).unwrap();
        assert!((norm(m) - 1.0).abs() < 1e-12);
        assert_eq!(bspline_point(&c, 1.0).unwrap(), [0.0, 1.0, 0.0]);
        assert_eq!(bspline_param(&c, [0.0, 1.0, 0.0]).unwrap(), 1.0);
        let t = bspline_param(&c, m).unwrap();
        assert!((t - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bspline_with_decreasing_knots_has_no_point() {
        // a model built by hand, not read: the reader's `check_knots` refuses these knots
        let c = BSplineCurve {
            degree: 1,
            points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            weights: None,
            knots: vec![1.0, 0.0],
            mults: vec![2, 2],
        };
        assert_eq!(bspline_point(&c, 0.5), None);
        let c = BSplineCurve {
            knots: vec![0.0, f64::NAN],
            ..c
        };
        assert_eq!(bspline_point(&c, 0.5), None);
        // an interior NaN knot: the evaluated bounds are finite and ordered
        let c = BSplineCurve {
            points: vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            knots: vec![0.0, f64::NAN, 1.0],
            mults: vec![2, 1, 2],
            ..c
        };
        assert_eq!(bspline_point(&c, 0.5), None);
    }

    #[test]
    fn bspline_with_too_few_points_has_no_point() {
        // degree 2 needs 3 points; the knots [0, 0, 1, 1] fit one point and put
        // u[p] = 1 above u[n] = 0
        let c = BSplineCurve {
            degree: 2,
            points: vec![[0.0; 3]],
            weights: None,
            knots: vec![0.0, 1.0],
            mults: vec![2, 2],
        };
        assert_eq!(bspline_point(&c, 0.5), None);
    }

    #[test]
    fn bspline_with_multiplicity_out_of_range_has_no_point() {
        // a multiplicity of 0, and one above degree + 1 (the reader's `check_knots`
        // refuses both)
        let c = BSplineCurve {
            degree: 1,
            points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            weights: None,
            knots: vec![0.0, 0.5, 1.0],
            mults: vec![2, 0, 2],
        };
        assert_eq!(bspline_point(&c, 0.5), None);
        let c = BSplineCurve {
            points: vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            knots: vec![0.0, 1.0],
            mults: vec![3, 2],
            ..c
        };
        assert_eq!(bspline_point(&c, 0.5), None);
    }
}
