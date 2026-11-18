//! NURBS tools from Solidcore's NLib layer, on the kernel's own curve and surface.
//!
//! [`NurbsCurve3`] and [`NurbsSurface3`] stay what every caller holds; each
//! tool here converts to the vendored routines (`opencadkernel-nlib`), runs
//! them, and converts back. The routines want clamped knot vectors, so an
//! unclamped (periodic) curve is first cut to its domain, which keeps its
//! shape and its parameters but not its periodicity.
//!
//! - fitting within a tolerance: [`NurbsCurve3::fit_within`],
//!   [`NurbsCurve3::knots_removed_within`], [`NurbsCurve3::degree_reduced_within`]
//! - extension past an end: [`NurbsCurve3::extended`]
//! - curvature: [`NurbsCurve3::curvature_at`], [`NurbsSurface3::curvature_at`]
//! - certified deviation: [`curve_deviation`], [`surface_deviation`]
//! - surfaces from curves and edges: [`coons_surface`], [`skinned_surface`],
//!   [`fill_n_sided`], [`match_edge`]

use nlib::math::Vec3 as NVec3;
use nlib::nurbs::crv_adv::p1::{Continuity, CurveEnd};
use nlib::nurbs::crv_basic::{Boundary, Curve as NCurve};
use nlib::nurbs::crv_geom::ToleranceKind;
use nlib::nurbs::knots::Side;
use nlib::nurbs::math::{CPoint, NOW};
use nlib::nurbs::srf_basic::{Direction, Surface as NSurface};

use super::spline::Parameterization;
use super::{NurbsCurve3, NurbsSurface3, Vec3};

fn nvec(p: [f64; 3]) -> NVec3 {
    NVec3::new(p[0], p[1], p[2])
}

fn kvec(p: NVec3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn cpoint(p: [f64; 3], weight: f64, rational: bool) -> CPoint {
    if rational {
        CPoint::weighted(nvec(p), weight)
    } else {
        CPoint::from_point(nvec(p))
    }
}

fn euclid(point: CPoint) -> ([f64; 3], f64) {
    let (p, w) = point.to_euclid_and_w();
    ([p.x, p.y, p.z], if w == NOW { 1.0 } else { w })
}

fn clamped(knots: &[f64], degree: usize, (from, to): (f64, f64)) -> bool {
    let n = knots.len();
    n > 2 * degree + 1
        && knots[..=degree].iter().all(|&k| k == from)
        && knots[n - 1 - degree..].iter().all(|&k| k == to)
}

/// The curve as the vendored routines hold it, clamped to its domain.
fn to_nlib_curve(curve: &NurbsCurve3) -> Option<NCurve> {
    let rational = curve.is_rational();
    let points = curve
        .control_points()
        .iter()
        .zip(curve.weights())
        .map(|(&p, &w)| cpoint(p, w, rational))
        .collect();
    let raw = NCurve::new(points, curve.degree(), curve.knots().to_vec()).ok()?;
    let domain = curve.domain();
    if clamped(curve.knots(), curve.degree(), domain) {
        Some(raw)
    } else {
        raw.extract_seg(domain.0, domain.1).ok()
    }
}

fn from_nlib_curve(curve: &NCurve) -> Option<NurbsCurve3> {
    let (points, weights): (Vec<_>, Vec<_>) = curve.cpts().iter().map(|&p| euclid(p)).unzip();
    let knots = curve.knots().get(..=curve.m())?.to_vec();
    NurbsCurve3::new_strict(curve.degree(), points, knots, weights)
}

fn to_nlib_surface(surface: &NurbsSurface3) -> Option<NSurface> {
    let rational = surface
        .weights()
        .iter()
        .flatten()
        .any(|&w| (w - 1.0).abs() > 1e-15);
    let net = surface
        .control_points()
        .iter()
        .zip(surface.weights())
        .map(|(row, weights)| {
            row.iter()
                .zip(weights)
                .map(|(&p, &w)| cpoint(p, w, rational))
                .collect()
        })
        .collect();
    let (p, q) = surface.degrees();
    let (u, v) = surface.knots();
    NSurface::new(net, p, q, u.to_vec(), v.to_vec()).ok()
}

fn from_nlib_surface(surface: &NSurface) -> Option<NurbsSurface3> {
    let (p, q) = surface.degrees();
    let (n, m) = (surface.n(), surface.m());
    let mut points = Vec::with_capacity(n + 1);
    let mut weights = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let (row, row_weights): (Vec<_>, Vec<_>) = (0..=m)
            .map(|j| surface.cpt(i, j).ok().map(euclid))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .unzip();
        points.push(row);
        weights.push(row_weights);
    }
    let (u, v) = surface.knot_vectors().ok()?;
    NurbsSurface3::new_strict(
        p,
        q,
        points,
        u.knots().to_vec(),
        v.knots().to_vec(),
        weights,
    )
}

/// The side of a knot to evaluate on: the span the parameter starts, except
/// at the domain's end, which only the last span reaches.
fn side(at: f64, end: f64) -> Side {
    if at >= end {
        Side::Right
    } else {
        Side::Left
    }
}

/// Curvature of a curve at one parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveCurvature {
    /// The point on the curve.
    pub point: [f64; 3],
    /// Unit tangent.
    pub tangent: [f64; 3],
    /// Curvature, `1 / radius`; zero where the curve is straight.
    pub curvature: f64,
    /// Unit principal normal, towards the centre; `None` where straight.
    pub normal: Option<[f64; 3]>,
    /// Centre of the osculating circle; `None` where straight.
    pub centre: Option<[f64; 3]>,
}

/// Curvature of a surface at one parameter pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCurvature {
    /// The point on the surface.
    pub point: [f64; 3],
    /// Unit normal, the surface's own (`normal_at_knot`'s side).
    pub normal: [f64; 3],
    /// Gaussian curvature, the principal curvatures' product.
    pub gaussian: f64,
    /// Mean curvature, their average. Positive where the surface bends
    /// towards its normal.
    pub mean: f64,
    /// Principal curvatures, the larger first.
    pub principal: [f64; 2],
    /// Unit principal directions, in the order of `principal`.
    pub directions: [[f64; 3]; 2],
}

impl NurbsCurve3 {
    /// A cubic through or near `points`: interpolated, then with every knot
    /// removed whose removal keeps the curve within `tolerance` of the
    /// interpolant, so it passes within `tolerance` of each point. A
    /// tolerance of zero interpolates.
    pub fn fit_within(
        points: &[[f64; 3]],
        start_tangent: Option<[f64; 3]>,
        end_tangent: Option<[f64; 3]>,
        parameterization: Parameterization,
        tolerance: f64,
    ) -> Option<Self> {
        let curve = Self::interpolate_fit(points, start_tangent, end_tangent, parameterization)?;
        if tolerance > 0.0 && tolerance.is_finite() {
            curve.knots_removed_within(tolerance)
        } else {
            Some(curve)
        }
    }

    /// The piece between knot parameters `from` and `to`, clamped at both.
    pub fn segment(&self, from: f64, to: f64) -> Option<Self> {
        let (a, b) = self.domain();
        if !(from < to && from >= a - 1e-12 && to <= b + 1e-12) {
            return None;
        }
        let (from, to) = (from.max(a), to.min(b));
        let curve = to_nlib_curve(self)?;
        if from == a && to == b {
            return from_nlib_curve(&curve);
        }
        from_nlib_curve(&curve.extract_seg(from, to).ok()?)
    }

    /// This curve with every knot removed whose removal keeps every point
    /// within `tolerance` of where it was.
    pub fn knots_removed_within(&self, tolerance: f64) -> Option<Self> {
        if !(tolerance.is_finite() && tolerance >= 0.0) {
            return None;
        }
        from_nlib_curve(&to_nlib_curve(self)?.remove_knots(tolerance).ok()?)
    }

    /// This curve at the lowest degree that keeps every point within
    /// `tolerance` of where it was.
    pub fn degree_reduced_within(&self, tolerance: f64) -> Option<Self> {
        if !(tolerance.is_finite() && tolerance >= 0.0) {
            return None;
        }
        from_nlib_curve(&to_nlib_curve(self)?.reduce_degree(tolerance).ok()?)
    }

    /// This curve continued past an end by `distance` of arc length,
    /// curvature-continuous at the join: the end span reflected in its end
    /// tangent plane. The original part keeps its shape and parameters; the
    /// length is met to the arc-length stations' accuracy (`1e-10` of the
    /// control polygon's length).
    pub fn extended(&self, at_end: bool, distance: f64) -> Option<Self> {
        use super::ArcLengthCurve3;
        if !(distance.is_finite() && distance > 0.0) {
            return None;
        }
        let curve = to_nlib_curve(self)?;
        let base = ArcLengthCurve3::from_nurbs(self.clone())?.length();
        let target = base + distance;
        let end = if at_end { CurveEnd::End } else { CurveEnd::Start };
        // The routine's length is approximate: extend a little past the
        // target, then cut back to it along the arc.
        let mut over = 1.05;
        for _ in 0..4 {
            let longer = from_nlib_curve(
                &curve
                    .extend_by_dist(distance * over, ToleranceKind::Absolute, end, Continuity::G2)
                    .ok()?,
            )?;
            let stations = ArcLengthCurve3::from_nurbs(longer.clone())?;
            let length = stations.length();
            if length >= target {
                let (a, b) = longer.domain();
                let along = if at_end { target } else { length - target };
                let cut = a + (b - a) * stations.parameter_at_distance(along);
                return if at_end {
                    longer.segment(a, cut)
                } else {
                    longer.segment(cut, b)
                };
            }
            over *= 1.5;
        }
        None
    }

    /// Curvature, tangent and osculating circle at knot parameter `u`.
    pub fn curvature_at(&self, u: f64) -> Option<CurveCurvature> {
        let curve = to_nlib_curve(self)?;
        let (from, to) = self.domain();
        if !(u >= from && u <= to) {
            return None;
        }
        let side = side(u, to);
        let plain = curve.eval_curvature(u, side, false).ok()?;
        let mut result = CurveCurvature {
            point: kvec(plain.point).to_array(),
            tangent: kvec(plain.tangent).to_array(),
            curvature: plain.kappa,
            normal: None,
            centre: None,
        };
        if plain.kappa > 0.0 {
            if let Ok(circle) = curve.eval_curvature(u, side, true) {
                let frame = curve.eval_frenet_frame(u, side).ok();
                result.normal = frame.map(|f| kvec(f.normal).to_array());
                result.centre = circle.circle.map(|(c, _)| kvec(c).to_array());
            }
        }
        Some(result)
    }
}

impl NurbsSurface3 {
    /// Gaussian, mean and principal curvatures and the principal directions
    /// at knot parameters `(u, v)`. `None` where the surface has no normal or
    /// its fundamental forms degenerate (a pole).
    pub fn curvature_at(&self, u: f64, v: f64) -> Option<SurfaceCurvature> {
        let surface = to_nlib_surface(self)?;
        let ((u0, u1), (v0, v1)) = surface_domain(self);
        if !(u >= u0 && u <= u1 && v >= v0 && v <= v1) {
            return None;
        }
        let d = surface
            .derivs(u, v, side(u, u1), side(v, v1), false, 2, 2)
            .ok()?;
        let at = |k: usize, l: usize| d.get(k).and_then(|row| row.get(l)).map(|&p| kvec(p));
        let jet = [
            at(0, 0)?,
            at(1, 0)?,
            at(0, 1)?,
            at(2, 0)?,
            at(1, 1)?,
            at(0, 2)?,
        ];
        let mut result = geometric_properties(jet)?;
        // The kernel's normal may run the other way (a reversed `v`): the
        // curvatures' signs follow the normal.
        if let Some(own) = self.normal_at_knot(u, v) {
            if Vec3::from(own).dot(Vec3::from(result.normal)) < 0.0 {
                result.normal = (-Vec3::from(result.normal)).to_array();
                result.mean = -result.mean;
                result.principal = [-result.principal[1], -result.principal[0]];
                result.directions = [result.directions[1], result.directions[0]];
            }
        }
        Some(result)
    }
}

fn surface_domain(surface: &NurbsSurface3) -> ((f64, f64), (f64, f64)) {
    let (p, q) = surface.degrees();
    let (u, v) = surface.knots();
    (
        (u[p], u[u.len() - 1 - p]),
        (v[q], v[v.len() - 1 - q]),
    )
}

/// The curvatures from a surface's derivatives `[S, Su, Sv, Suu, Suv, Svv]`
/// through its first and second fundamental forms. Ported from Solidcore's
/// `evaluate_geometric` (`surface/surface/p2.rs`).
fn geometric_properties([point, du, dv, duu, duv, dvv]: [Vec3; 6]) -> Option<SurfaceCurvature> {
    const EFF_ZERO: f64 = 1e-12;
    const EFF_ZERO_SQ: f64 = 1e-24;
    const EFF_ZERO_SQRT: f64 = 1e-6;
    let norm = du.cross(dv);
    if norm.length_squared() < EFF_ZERO_SQ {
        return None;
    }
    let norm = norm.normalize()?;
    let e = du.dot(du);
    let f = du.dot(dv);
    let g = dv.dot(dv);
    let l = duu.dot(norm);
    let m = duv.dot(norm);
    let n = dvv.dot(norm);

    let numer1 = l * n - m * m;
    let denom1 = e * g - f * f;
    if denom1.abs() <= EFF_ZERO * numer1.abs() || denom1 == 0.0 {
        return None;
    }
    let gaussian = numer1 / denom1;
    let mean = (e * n - 2.0 * f * m + g * l) / (2.0 * denom1);
    let hhk = mean * mean - gaussian;
    let root = if hhk < EFF_ZERO_SQ { 0.0 } else { hhk.sqrt() };
    let (k1, k2) = (mean + root, mean - root);

    let (mut v1, mut v2);
    if (k1 - k2).abs() < EFF_ZERO_SQRT * (1.0 + k1.abs() + k2.abs()) {
        // An umbilic or a flat point: any two orthogonal tangent directions.
        v1 = du.normalize()?;
        v2 = v1.cross(dv).cross(v1);
    } else {
        // V = du + a dv along a principal direction:
        // (fn - gm) a^2 + (en - gl) a + (em - fl) = 0.
        let roots = nlib::geom::polynomial::solve_quadratic(
            &[e * m - f * l, e * n - g * l, f * n - g * m],
            EFF_ZERO,
        );
        match roots.first() {
            Some(&a) => {
                v1 = du + dv * a;
                if v1.length_squared() < EFF_ZERO_SQ {
                    v1 = du;
                }
            }
            // `fn - gm` and `en - gl` vanish: the direction is `dv`.
            None => v1 = dv,
        }
        v2 = norm.cross(v1);
        // Pair each direction with its curvature by the normal curvature
        // along v1.
        let (wu, wv) = (v1.dot(du), v1.dot(dv));
        let a = (g * wu - f * wv) / denom1;
        let b = (-f * wu + e * wv) / denom1;
        let i = e * a * a + 2.0 * f * a * b + g * b * b;
        if i > EFF_ZERO_SQ {
            let kn1 = (l * a * a + 2.0 * m * a * b + n * b * b) / i;
            if (kn1 - k2).abs() < (kn1 - k1).abs() {
                std::mem::swap(&mut v1, &mut v2);
            }
        }
    }
    Some(SurfaceCurvature {
        point: point.to_array(),
        normal: norm.to_array(),
        gaussian,
        mean,
        principal: [k1, k2],
        directions: [v1.normalize()?.to_array(), v2.normalize()?.to_array()],
    })
}

/// A bound on the distance between two polynomial curves at equal
/// parameters over their common domain, every rounding included. `None` for
/// rational curves or curves on different domains.
pub fn curve_deviation(a: &NurbsCurve3, b: &NurbsCurve3) -> Option<f64> {
    nlib::nurbs::crv_deviation::certified_distance(&to_nlib_curve(a)?, &to_nlib_curve(b)?).ok()
}

/// A bound on the distance between two polynomial surfaces at equal
/// parameters over their common domain, every rounding included. `None` for
/// rational surfaces or surfaces on different domains.
pub fn surface_deviation(a: &NurbsSurface3, b: &NurbsSurface3) -> Option<f64> {
    nlib::nurbs::srf_deviation::certified_distance(&to_nlib_surface(a)?, &to_nlib_surface(b)?)
        .ok()
}

/// The bilinearly blended Coons surface of four boundaries: `u_sides` are the
/// boundaries at the low and high `v` (running in `u`), `v_sides` those at the
/// low and high `u` (running in `v`). Their ends should meet at the corners.
pub fn coons_surface(u_sides: [&NurbsCurve3; 2], v_sides: [&NurbsCurve3; 2]) -> Option<NurbsSurface3> {
    let mut u = [to_nlib_curve(u_sides[0])?, to_nlib_curve(u_sides[1])?];
    let mut v = [to_nlib_curve(v_sides[0])?, to_nlib_curve(v_sides[1])?];
    from_nlib_surface(&nlib::nurbs::srf_adv::p4::create_coons_srf(&mut u, &mut v).ok()?)
}

/// The surface through `sections` in order, of degree `degree` across them
/// (`u` runs along each section, `v` across them).
pub fn skinned_surface(sections: &[NurbsCurve3], degree: usize) -> Option<NurbsSurface3> {
    let mut curves = sections
        .iter()
        .map(to_nlib_curve)
        .collect::<Option<Vec<_>>>()?;
    let surface = nlib::nurbs::srf_adv::p3::create_skin_srf(
        &mut curves,
        true,
        false,
        degree.clamp(1, sections.len().saturating_sub(1).max(1)),
        Direction::V,
        1e-9,
    )
    .ok()?;
    from_nlib_surface(&surface)
}

/// Patches filling the hole bounded by `boundaries` (three or more
/// polynomial curves, each ending where the next starts), meeting each other
/// tangent-continuously; `angle_tolerance` is in degrees.
pub fn fill_n_sided(boundaries: &[NurbsCurve3], angle_tolerance: f64) -> Option<Vec<NurbsSurface3>> {
    let mut curves = boundaries
        .iter()
        .map(to_nlib_curve)
        .collect::<Option<Vec<_>>>()?;
    let fill =
        nlib::nurbs::srf_adv::p6::fill_n_sided_hole(&mut curves, None, angle_tolerance, None)
            .ok()?;
    fill.surfaces.iter().map(from_nlib_surface).collect()
}

/// Which boundary of a surface's parameter rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSide {
    /// `u` at its low end.
    UMin,
    /// `u` at its high end.
    UMax,
    /// `v` at its low end.
    VMin,
    /// `v` at its high end.
    VMax,
}

impl SurfaceSide {
    fn boundary(self) -> Boundary {
        match self {
            Self::UMin => Boundary::Left,
            Self::UMax => Boundary::Right,
            Self::VMin => Boundary::Bottom,
            Self::VMax => Boundary::Top,
        }
    }
}

/// `surface` with the control rows next to `side` moved so that it meets
/// `target`'s `target_side` with position (`order` 0), tangent (1) or
/// curvature (2) continuity, and how far apart they were and are.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeMatch {
    /// The matched surface.
    pub surface: NurbsSurface3,
    /// Largest gap along the seam before the match.
    pub gap_before: f64,
    /// Largest gap after.
    pub gap_after: f64,
    /// Largest angle between the normals across the seam after, radians.
    pub angle_after: f64,
}

/// Matches `surface`'s `side` to `target`'s `target_side` (see [`EdgeMatch`]).
pub fn match_edge(
    surface: &NurbsSurface3,
    side: SurfaceSide,
    target: &NurbsSurface3,
    target_side: SurfaceSide,
    order: usize,
) -> Option<EdgeMatch> {
    use nlib::nurbs::srf_match::{match_boundary, MatchOrder, MatchRows};
    let order = match order {
        0 => MatchOrder::G0,
        1 => MatchOrder::G1,
        _ => MatchOrder::G2,
    };
    let rows = MatchRows {
        rows: order.order() + 1,
        blend: 0.0,
    };
    let matched = match_boundary(
        &to_nlib_surface(surface)?,
        side.boundary(),
        &to_nlib_surface(target)?,
        target_side.boundary(),
        order,
        rows,
    )
    .ok()?;
    Some(EdgeMatch {
        surface: from_nlib_surface(&matched.surface)?,
        gap_before: matched.before.gap,
        gap_after: matched.after.gap,
        angle_after: matched.after.angle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(count: usize) -> Vec<[f64; 3]> {
        (0..count)
            .map(|i| {
                let x = i as f64 * 0.5;
                [x, x.sin() * 2.0, 0.1 * x * x]
            })
            .collect()
    }

    fn quarter_circle(radius: f64) -> NurbsCurve3 {
        NurbsCurve3::new_strict(
            2,
            vec![[radius, 0.0, 0.0], [radius, radius, 0.0], [0.0, radius, 0.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0],
        )
        .unwrap()
    }

    fn length(curve: &NurbsCurve3) -> f64 {
        let (a, b) = curve.domain();
        let steps = 20_000;
        (1..=steps)
            .map(|i| {
                let s = a + (b - a) * (i - 1) as f64 / steps as f64;
                let t = a + (b - a) * i as f64 / steps as f64;
                Vec3::from(curve.point_at_knot(s)).distance(Vec3::from(curve.point_at_knot(t)))
            })
            .sum()
    }

    /// A tolerance fit keeps every fit point within the tolerance and needs
    /// fewer control points than the interpolant; zero interpolates.
    #[test]
    fn a_tolerance_fit_passes_within_the_tolerance() {
        let points = wave(41);
        let exact =
            NurbsCurve3::fit_within(&points, None, None, Parameterization::Chord, 0.0).unwrap();
        let fitted =
            NurbsCurve3::fit_within(&points, None, None, Parameterization::Chord, 0.01).unwrap();
        assert!(fitted.control_points().len() < exact.control_points().len() / 2, "{}", fitted.control_points().len());
        for point in &points {
            let at = fitted.point_at_knot(fitted.parameter_at(*point));
            let gap = Vec3::from(at).distance(Vec3::from(*point));
            assert!(gap <= 0.01 * (1.0 + 1e-6), "{gap}");
            let at = exact.point_at_knot(exact.parameter_at(*point));
            assert!(Vec3::from(at).distance(Vec3::from(*point)) < 1e-9);
        }
    }

    /// A quadratic raised to a cubic comes back down to degree 2 unchanged.
    #[test]
    fn degree_reduction_undoes_an_elevation() {
        let quadratic = NurbsCurve3::from_control_polygon(
            2,
            &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.5], [3.0, 2.5, 1.0], [4.0, 0.0, 0.0]],
            false,
        )
        .unwrap();
        let cubic = quadratic.elevated(1).unwrap();
        assert_eq!(cubic.degree(), 3);
        let reduced = cubic.degree_reduced_within(1e-9).unwrap();
        assert_eq!(reduced.degree(), 2);
        assert!(curve_deviation(&reduced.elevated(1).unwrap(), &cubic).unwrap() < 1e-9);
    }

    /// An extension keeps the original part and adds the asked length.
    #[test]
    fn an_extension_adds_the_length_and_keeps_the_curve() {
        let curve = NurbsCurve3::interpolate_fit(&wave(9), None, None, Parameterization::Chord)
            .unwrap();
        let before = length(&curve);
        for at_end in [true, false] {
            let longer = curve.extended(at_end, 1.5).unwrap();
            let grown = length(&longer) - before;
            assert!((grown - 1.5).abs() < 1e-6, "{at_end} {grown}");
            let (a, b) = curve.domain();
            for k in 0..=10 {
                let t = a + (b - a) * k as f64 / 10.0;
                let gap = Vec3::from(curve.point_at_knot(t)).distance(Vec3::from(longer.point_at_knot(t)));
                assert!(gap < 1e-9, "{gap}");
            }
        }
    }

    /// A circle of radius 2 has curvature 1/2 everywhere, its centre at the
    /// circle's; a straight piece has none.
    #[test]
    fn curve_curvature_of_a_circle_and_a_line() {
        let arc = quarter_circle(2.0);
        for k in 0..=8 {
            let c = arc.curvature_at(k as f64 / 8.0).unwrap();
            assert!((c.curvature - 0.5).abs() < 1e-12, "{}", c.curvature);
            assert!(Vec3::from(c.centre.unwrap()).length() < 1e-12);
            let towards = Vec3::from(c.normal.unwrap()).dot(-Vec3::from(c.point) / 2.0);
            assert!((towards - 1.0).abs() < 1e-12);
        }
        let line = NurbsCurve3::from_control_polygon(1, &[[0.0; 3], [3.0, 4.0, 0.0]], false).unwrap();
        let c = line.curvature_at(0.5).unwrap();
        assert_eq!(c.curvature, 0.0);
        assert!(c.centre.is_none());
        assert!((Vec3::from(c.tangent).dot(Vec3::new(0.6, 0.8, 0.0)) - 1.0).abs() < 1e-12);
    }

    /// A cylinder of radius 2 bends one way at 1/2 and not at all along its
    /// axis: Gaussian 0, mean ±1/4, the bent direction round it.
    #[test]
    fn surface_curvature_of_a_cylinder() {
        let arc = quarter_circle(2.0);
        let net = arc
            .control_points()
            .iter()
            .map(|p| vec![*p, [p[0], p[1], 3.0]])
            .collect();
        let weights = arc.weights().iter().map(|&w| vec![w, w]).collect();
        let cylinder =
            NurbsSurface3::new_strict(2, 1, net, arc.knots().to_vec(), vec![0.0, 0.0, 1.0, 1.0], weights)
                .unwrap();
        for (u, v) in [(0.0, 0.0), (0.3, 0.5), (1.0, 1.0)] {
            let c = cylinder.curvature_at(u, v).unwrap();
            assert!(c.gaussian.abs() < 1e-12, "{}", c.gaussian);
            assert!((c.mean.abs() - 0.25).abs() < 1e-12, "{}", c.mean);
            let bent = if c.principal[0].abs() > c.principal[1].abs() { 0 } else { 1 };
            assert!((c.principal[bent].abs() - 0.5).abs() < 1e-12);
            assert!(c.principal[1 - bent].abs() < 1e-12);
            assert!(c.directions[bent][2].abs() < 1e-12);
            assert!((c.directions[1 - bent][2].abs() - 1.0).abs() < 1e-12);
        }
    }

    /// The certified bound between a curve and itself moved by 0.1 is 0.1,
    /// and between a curve and its refined self is rounding.
    #[test]
    fn certified_curve_deviation() {
        let curve = NurbsCurve3::interpolate_fit(&wave(7), None, None, Parameterization::Chord)
            .unwrap();
        let moved = NurbsCurve3::new_strict(
            curve.degree(),
            curve.control_points().iter().map(|p| [p[0], p[1], p[2] + 0.1]).collect(),
            curve.knots().to_vec(),
            curve.weights().to_vec(),
        )
        .unwrap();
        let bound = curve_deviation(&curve, &moved).unwrap();
        assert!(bound >= 0.1 && bound < 0.1 + 1e-12, "{bound}");
        let same = curve_deviation(&curve, &curve.elevated(1).unwrap()).unwrap();
        assert!(same < 1e-12, "{same}");
    }

    /// A Coons patch of a square's four sides is the flat square.
    #[test]
    fn coons_patch_of_a_square() {
        let side = |a: [f64; 3], b: [f64; 3]| {
            NurbsCurve3::from_control_polygon(1, &[a, b], false).unwrap()
        };
        let (p00, p10, p01, p11) = ([0.0; 3], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [2.0, 2.0, 0.0]);
        let surface = coons_surface(
            [&side(p00, p10), &side(p01, p11)],
            [&side(p00, p01), &side(p10, p11)],
        )
        .unwrap();
        let ((u0, u1), (v0, v1)) = surface_domain(&surface);
        let centre = surface.point_at_knot((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        assert!(Vec3::from(centre).distance(Vec3::new(1.0, 1.0, 0.0)) < 1e-12);
        let flat = surface_deviation(&surface, &surface).unwrap();
        assert!(flat < 1e-12);
    }
}
