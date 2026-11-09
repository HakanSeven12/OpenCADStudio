//! Spline forms of the IGES surfaces that are built from curves — surfaces
//! of revolution (120), tabulated cylinders (122), ruled surfaces (118) and
//! parametric spline surfaces (114) — and the circular arcs they use.

use kernel::space::{NurbsCurve3, NurbsSurface3, Vec3};

/// A circular arc from `a0` to `a1` (radians, counter-clockwise round
/// `x × y`) as a rational quadratic spline in segments of at most a quarter
/// turn.
pub fn arc(
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
) -> Option<NurbsCurve3> {
    let (points, weights, knots) = arc_net(
        Vec3::from(center),
        Vec3::from(x),
        Vec3::from(y),
        radius,
        a0,
        a1,
    );
    NurbsCurve3::new(2, points, knots, Some(weights))
}

/// The control points, weights and knots of an arc.
fn arc_net(
    c: Vec3,
    x: Vec3,
    y: Vec3,
    r: f64,
    a0: f64,
    a1: f64,
) -> (Vec<[f64; 3]>, Vec<f64>, Vec<f64>) {
    let sweep = a1 - a0;
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2) - 1e-9)
        .ceil()
        .max(1.0) as usize;
    let step = sweep / n as f64;
    let w = (step / 2.0).cos();
    let at = |a: f64, scale: f64| (c + (x * a.cos() + y * a.sin()) * (r * scale)).to_array();
    let mut points = vec![at(a0, 1.0)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for k in 0..n {
        let s = a0 + step * k as f64;
        points.push(at(s + step / 2.0, 1.0 / w));
        weights.push(w);
        points.push(at(s + step, 1.0));
        weights.push(1.0);
        let t = (k + 1) as f64 / n as f64;
        knots.extend(if k + 1 == n { vec![t; 3] } else { vec![t; 2] });
    }
    (points, weights, knots)
}

/// A curve swept round the axis through `origin` along `axis` from angle
/// `a0` to `a1` (radians): its parameter runs along the curve (u) and
/// round the axis (v).
pub fn revolve(
    curve: &NurbsCurve3,
    origin: [f64; 3],
    axis: [f64; 3],
    a0: f64,
    a1: f64,
) -> Option<NurbsSurface3> {
    let o = Vec3::from(origin);
    let k = Vec3::from(axis).normalize()?;
    let mut rows = Vec::new();
    let mut weights = Vec::new();
    let mut v_knots = Vec::new();
    for (p, w) in curve.control_points().iter().zip(curve.weights()) {
        let p = Vec3::from(*p);
        let centre = o + k * (p - o).dot(k);
        let radial = p - centre;
        let r = radial.length();
        let x = if r > 1e-12 {
            radial * (1.0 / r)
        } else {
            any_perpendicular(k)
        };
        let (points, ws, knots) = arc_net(centre, x, k.cross(x), r, a0, a1);
        rows.push(points);
        weights.push(ws.iter().map(|v| v * w).collect());
        v_knots = knots;
    }
    NurbsSurface3::new(
        curve.degree(),
        2,
        rows,
        curve.knots().to_vec(),
        v_knots,
        Some(weights),
    )
}

/// A curve swept straight along `offset` (u along the curve, v along the
/// offset).
pub fn tabulate(curve: &NurbsCurve3, offset: [f64; 3]) -> Option<NurbsSurface3> {
    let d = Vec3::from(offset);
    let rows = curve
        .control_points()
        .iter()
        .map(|p| vec![*p, (Vec3::from(*p) + d).to_array()])
        .collect();
    let weights = curve.weights().iter().map(|w| vec![*w, *w]).collect();
    NurbsSurface3::new(
        curve.degree(),
        1,
        rows,
        curve.knots().to_vec(),
        vec![0.0, 0.0, 1.0, 1.0],
        Some(weights),
    )
}

/// The surface of straight lines between matching points of two curves of
/// the same degree and knots (u along the curves, v across).
pub fn ruled(first: &NurbsCurve3, second: &NurbsCurve3) -> Option<NurbsSurface3> {
    if first.degree() != second.degree()
        || first.control_points().len() != second.control_points().len()
    {
        return None;
    }
    let normalised = |c: &NurbsCurve3| {
        let (a, b) = c.domain();
        c.knots()
            .iter()
            .map(|k| (k - a) / (b - a))
            .collect::<Vec<_>>()
    };
    if normalised(first)
        .iter()
        .zip(normalised(second))
        .any(|(a, b)| (a - b).abs() > 1e-9)
    {
        return None;
    }
    let rows = first
        .control_points()
        .iter()
        .zip(second.control_points())
        .map(|(a, b)| vec![*a, *b])
        .collect();
    let weights = first
        .weights()
        .iter()
        .zip(second.weights())
        .map(|(a, b)| vec![*a, *b])
        .collect();
    NurbsSurface3::new(
        first.degree(),
        1,
        rows,
        first.knots().to_vec(),
        vec![0.0, 0.0, 1.0, 1.0],
        Some(weights),
    )
}

/// A parametric spline surface (114) of `m × n` bicubic patches between the
/// break points `tu` and `tv`, each patch's coefficients `[x, y, z][16]`
/// (power `i` of u at index `i + 4·j`, `j` the power of v, measured from
/// the patch's first break point) — as a spline of Bézier patches. Where
/// patches meet, the first one's edge stands.
pub fn patches(tu: &[f64], tv: &[f64], coefficients: &[[[f64; 16]; 3]]) -> Option<NurbsSurface3> {
    let (m, n) = (tu.len().checked_sub(1)?, tv.len().checked_sub(1)?);
    if m == 0 || n == 0 || coefficients.len() < m * n {
        return None;
    }
    // Power to Bernstein: control point k = Σ_{i≤k} C(k,i)/C(3,i)·a_i.
    const A: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 1.0 / 3.0, 0.0, 0.0],
        [1.0, 2.0 / 3.0, 1.0 / 3.0, 0.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    let mut grid = vec![vec![[0.0; 3]; 3 * n + 1]; 3 * m + 1];
    for i in 0..m {
        for j in 0..n {
            let c = &coefficients[i * n + j];
            let (hu, hv) = (tu[i + 1] - tu[i], tv[j + 1] - tv[j]);
            for (k, row) in A.iter().enumerate() {
                for (l, col) in A.iter().enumerate() {
                    if (k == 0 && i > 0) || (l == 0 && j > 0) {
                        continue;
                    }
                    let mut p = [0.0; 3];
                    for (axis, coeffs) in c.iter().enumerate() {
                        for (a, ra) in row.iter().enumerate() {
                            for (b, cb) in col.iter().enumerate() {
                                p[axis] += ra
                                    * cb
                                    * coeffs[a + 4 * b]
                                    * hu.powi(a as i32)
                                    * hv.powi(b as i32);
                            }
                        }
                    }
                    grid[3 * i + k][3 * j + l] = p;
                }
            }
        }
    }
    let knots = |t: &[f64]| {
        let mut k = vec![t[0]; 4];
        for v in &t[1..t.len() - 1] {
            k.extend([*v; 3]);
        }
        k.extend([t[t.len() - 1]; 4]);
        k
    };
    NurbsSurface3::new(3, 3, grid, knots(tu), knots(tv), None)
}

/// A spline moved by `offset`.
pub fn translated(surface: &NurbsSurface3, offset: [f64; 3]) -> Option<NurbsSurface3> {
    let d = Vec3::from(offset);
    let (du, dv) = surface.degrees();
    let (uk, vk) = surface.knots();
    let rows = surface
        .control_points()
        .iter()
        .map(|row| {
            row.iter()
                .map(|p| (Vec3::from(*p) + d).to_array())
                .collect()
        })
        .collect();
    NurbsSurface3::new(
        du,
        dv,
        rows,
        uk.to_vec(),
        vk.to_vec(),
        Some(surface.weights().to_vec()),
    )
}

/// A line as a degree-one spline.
pub fn line(a: [f64; 3], b: [f64; 3]) -> Option<NurbsCurve3> {
    NurbsCurve3::new(1, vec![a, b], vec![0.0, 0.0, 1.0, 1.0], None)
}

pub fn any_perpendicular(k: Vec3) -> Vec3 {
    let seed = if k.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    k.cross(seed)
        .normalize()
        .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
}
