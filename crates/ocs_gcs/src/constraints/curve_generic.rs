//! Constraints that hold one or more curves generically (as `Rc<dyn Curve>`)
//! rather than as a concrete geometry type: `CurveValue`, the four
//! `AngleVia*` variants, and `Snell`.
//!
//! Ported from planegcs's `ConstraintCurveValue`, `ConstraintAngleViaPoint`,
//! `ConstraintAngleViaTwoPoints`, `ConstraintAngleViaPointAndParam`,
//! `ConstraintAngleViaPointAndTwoParams`, and `ConstraintSnell`
//! (`Constraints.h`/`.cpp`). These needed generic curve dispatch in the C++
//! too (a `Curve*` field, `Curve::Copy()`), so `Rc<dyn Curve>` here is a
//! direct, not merely convenient, translation of that.
//!
//! The four `AngleVia*` constraints share one error/gradient formula —
//! "rotate curve 1's normal by the target angle, measure the angle from
//! that to curve 2's normal" — differing only in *how* each curve's normal
//! is sampled (at a stored [`Point`], or at a fixed curve parameter via
//! [`Curve::calculate_normal_at_param`]). [`angle_via_normals_error`] and
//! [`angle_via_normals_grad`] factor that shared formula out once.

use std::rc::Rc;

use crate::geo::{Curve, DeriVector2, Point};
use crate::util::{ParamId, ParamStore};

use super::Constraint;

fn rotate_by_angle(n: &DeriVector2, ang: f64) -> DeriVector2 {
    let (si, co) = ang.sin_cos();
    DeriVector2::new(n.x * co - n.y * si, n.x * si + n.y * co)
}

/// The shared `AngleVia*` error: the angle from `n1` (rotated by `ang`) to
/// `n2`, via `atan2` of their cross/dot products — equivalent to
/// `atan2(n2) - (atan2(n1) + ang)` except at zero normals, where this stays
/// zero instead of blowing up.
fn angle_via_normals_error(ang: f64, n1: DeriVector2, n2: DeriVector2) -> f64 {
    let n1r = rotate_by_angle(&n1, ang);
    (-n2.x * n1r.y + n2.y * n1r.x).atan2(n2.x * n1r.x + n2.y * n1r.y)
}

/// `d(atan2(n.y, n.x))/dparam`, from `n`'s own derivative fields.
fn datan2_dparam(n: &DeriVector2) -> f64 {
    let len2 = n.x * n.x + n.y * n.y;
    (n.x * n.dy - n.y * n.dx) / len2
}

/// The shared `AngleVia*` gradient w.r.t. one parameter: `n1`/`n2` must
/// already have been sampled with `derivparam` set to that parameter (they
/// are *not* the rotated-by-angle vectors `error` uses — matching the C++,
/// which differentiates the un-rotated normals directly).
fn angle_via_normals_grad(is_angle_param: bool, n1: &DeriVector2, n2: &DeriVector2) -> f64 {
    (if is_angle_param { -1.0 } else { 0.0 }) - datan2_dparam(n1) + datan2_dparam(n2)
}

/// Ties a point's coordinate to a curve's parametric value: `p.x` (or
/// `p.y`) must equal the curve's x (or y) at parameter `u`.
pub struct CurveValue {
    p: Point,
    /// Must be `p.x` or `p.y` — which coordinate this constrains.
    pcoord: ParamId,
    crv: Rc<dyn Curve>,
    u: ParamId,
}

impl CurveValue {
    /// `pcoord` must be `p.x` or `p.y`.
    pub fn new(p: Point, pcoord: ParamId, crv: Rc<dyn Curve>, u: ParamId) -> Self {
        debug_assert!(pcoord == p.x || pcoord == p.y, "pcoord must be p.x or p.y");
        Self { p, pcoord, crv, u }
    }

    fn err_vec(&self, store: &ParamStore, derivparam: Option<ParamId>) -> DeriVector2 {
        let u = store.get(self.u);
        let du = if derivparam == Some(self.u) { 1.0 } else { 0.0 };
        let p_to = self.crv.value(store, u, du, derivparam);
        let p_from = DeriVector2::from_point(store, self.p, derivparam);
        p_from.subtr(&p_to)
    }
}

impl Constraint for CurveValue {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.p.x, self.p.y, self.u];
        params.extend(self.crv.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let err_vec = self.err_vec(store, None);
        if self.pcoord == self.p.x { err_vec.x } else { err_vec.y }
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let err_vec = self.err_vec(store, Some(param));
        if self.pcoord == self.p.x { err_vec.dx } else { err_vec.dy }
    }
}

/// Angle (measured via each curve's normal) between two curves at a single
/// shared point `poa`.
pub struct AngleViaPoint {
    crv1: Rc<dyn Curve>,
    crv2: Rc<dyn Curve>,
    poa: Point,
    angle: ParamId,
}

impl AngleViaPoint {
    pub fn new(crv1: Rc<dyn Curve>, crv2: Rc<dyn Curve>, poa: Point, angle: ParamId) -> Self {
        Self { crv1, crv2, poa, angle }
    }
}

impl Constraint for AngleViaPoint {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.angle, self.poa.x, self.poa.y];
        params.extend(self.crv1.own_params());
        params.extend(self.crv2.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let ang = store.get(self.angle);
        let n1 = self.crv1.calculate_normal(store, self.poa, None);
        let n2 = self.crv2.calculate_normal(store, self.poa, None);
        angle_via_normals_error(ang, n1, n2)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let n1 = self.crv1.calculate_normal(store, self.poa, Some(param));
        let n2 = self.crv2.calculate_normal(store, self.poa, Some(param));
        angle_via_normals_grad(param == self.angle, &n1, &n2)
    }
}

/// As [`AngleViaPoint`], but each curve's normal is taken at its own point
/// (`poa1` for `crv1`, `poa2` for `crv2`) instead of a single shared one —
/// planegcs uses this for curve types (B-splines) whose normal is only
/// defined at their own stored start/end points.
pub struct AngleViaTwoPoints {
    crv1: Rc<dyn Curve>,
    crv2: Rc<dyn Curve>,
    poa1: Point,
    poa2: Point,
    angle: ParamId,
}

impl AngleViaTwoPoints {
    pub fn new(crv1: Rc<dyn Curve>, crv2: Rc<dyn Curve>, poa1: Point, poa2: Point, angle: ParamId) -> Self {
        Self { crv1, crv2, poa1, poa2, angle }
    }
}

impl Constraint for AngleViaTwoPoints {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.angle, self.poa1.x, self.poa1.y, self.poa2.x, self.poa2.y];
        params.extend(self.crv1.own_params());
        params.extend(self.crv2.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let ang = store.get(self.angle);
        let n1 = self.crv1.calculate_normal(store, self.poa1, None);
        let n2 = self.crv2.calculate_normal(store, self.poa2, None);
        angle_via_normals_error(ang, n1, n2)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let n1 = self.crv1.calculate_normal(store, self.poa1, Some(param));
        let n2 = self.crv2.calculate_normal(store, self.poa2, Some(param));
        angle_via_normals_grad(param == self.angle, &n1, &n2)
    }
}

/// As [`AngleViaPoint`], but `crv1`'s normal is taken at a fixed curve
/// parameter `cparam` (via [`Curve::calculate_normal_at_param`]) instead of
/// at a stored point — for constraining a curve's tangent at a specific
/// parametric location that isn't itself a sketch point.
pub struct AngleViaPointAndParam {
    crv1: Rc<dyn Curve>,
    crv2: Rc<dyn Curve>,
    poa: Point,
    cparam: ParamId,
    angle: ParamId,
}

impl AngleViaPointAndParam {
    pub fn new(crv1: Rc<dyn Curve>, crv2: Rc<dyn Curve>, poa: Point, cparam: ParamId, angle: ParamId) -> Self {
        Self { crv1, crv2, poa, cparam, angle }
    }
}

impl Constraint for AngleViaPointAndParam {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.angle, self.poa.x, self.poa.y, self.cparam];
        params.extend(self.crv1.own_params());
        params.extend(self.crv2.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let ang = store.get(self.angle);
        let n1 = self.crv1.calculate_normal_at_param(store, store.get(self.cparam), None);
        let n2 = self.crv2.calculate_normal(store, self.poa, None);
        angle_via_normals_error(ang, n1, n2)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let n1 = self.crv1.calculate_normal_at_param(store, store.get(self.cparam), Some(param));
        let n2 = self.crv2.calculate_normal(store, self.poa, Some(param));
        angle_via_normals_grad(param == self.angle, &n1, &n2)
    }
}

/// As [`AngleViaPointAndParam`], but *both* curves' normals are taken at
/// fixed curve parameters (`cparam1`, `cparam2`) — no stored point involved
/// at all. `point` is kept only for constructor-signature parity with the
/// C++ (which keeps an unused `Point p` field too, per its own `TODO: Do we
/// need point here at all?`); it plays no role in the error/gradient.
pub struct AngleViaPointAndTwoParams {
    crv1: Rc<dyn Curve>,
    crv2: Rc<dyn Curve>,
    point: Point,
    cparam1: ParamId,
    cparam2: ParamId,
    angle: ParamId,
}

impl AngleViaPointAndTwoParams {
    pub fn new(
        crv1: Rc<dyn Curve>,
        crv2: Rc<dyn Curve>,
        point: Point,
        cparam1: ParamId,
        cparam2: ParamId,
        angle: ParamId,
    ) -> Self {
        Self { crv1, crv2, point, cparam1, cparam2, angle }
    }
}

impl Constraint for AngleViaPointAndTwoParams {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.angle, self.point.x, self.point.y, self.cparam1, self.cparam2];
        params.extend(self.crv1.own_params());
        params.extend(self.crv2.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let ang = store.get(self.angle);
        let n1 = self.crv1.calculate_normal_at_param(store, store.get(self.cparam1), None);
        let n2 = self.crv2.calculate_normal_at_param(store, store.get(self.cparam2), None);
        angle_via_normals_error(ang, n1, n2)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let n1 = self.crv1.calculate_normal_at_param(store, store.get(self.cparam1), Some(param));
        let n2 = self.crv2.calculate_normal_at_param(store, store.get(self.cparam2), Some(param));
        angle_via_normals_grad(param == self.angle, &n1, &n2)
    }
}

/// Snell's law: relates the angles of incidence of two rays (`ray1`,
/// `ray2`) crossing a `boundary` curve at a shared point `poa`, via
/// refractive indices `n1`/`n2`. `flip_n1`/`flip_n2` flip the sign
/// convention for a ray, matching the C++'s `flipn1`/`flipn2`.
pub struct Snell {
    ray1: Rc<dyn Curve>,
    ray2: Rc<dyn Curve>,
    boundary: Rc<dyn Curve>,
    poa: Point,
    n1: ParamId,
    n2: ParamId,
    flip_n1: bool,
    flip_n2: bool,
}

impl Snell {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ray1: Rc<dyn Curve>,
        ray2: Rc<dyn Curve>,
        boundary: Rc<dyn Curve>,
        poa: Point,
        n1: ParamId,
        n2: ParamId,
        flip_n1: bool,
        flip_n2: bool,
    ) -> Self {
        Self { ray1, ray2, boundary, poa, n1, n2, flip_n1, flip_n2 }
    }

    /// Returns `(sin1, dsin1, sin2, dsin2)` for the given `derivparam`
    /// (`None` for the plain value, `Some(param)` for a derivative pass),
    /// with `flip_n1`/`flip_n2` already applied.
    fn sines(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64, f64, f64) {
        let tang1 = self.ray1.calculate_normal(store, self.poa, derivparam).rotate90cw().normalized();
        let tang2 = self.ray2.calculate_normal(store, self.poa, derivparam).rotate90cw().normalized();
        let tang_b = self.boundary.calculate_normal(store, self.poa, derivparam).rotate90cw().normalized();
        let (mut sin1, mut dsin1) = tang1.scalar_prod(&tang_b);
        let (mut sin2, mut dsin2) = tang2.scalar_prod(&tang_b);
        if self.flip_n1 {
            sin1 = -sin1;
            dsin1 = -dsin1;
        }
        if self.flip_n2 {
            sin2 = -sin2;
            dsin2 = -dsin2;
        }
        (sin1, dsin1, sin2, dsin2)
    }
}

impl Constraint for Snell {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.n1, self.n2, self.poa.x, self.poa.y];
        params.extend(self.ray1.own_params());
        params.extend(self.ray2.own_params());
        params.extend(self.boundary.own_params());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (sin1, _, sin2, _) = self.sines(store, None);
        store.get(self.n1) * sin1 - store.get(self.n2) * sin2
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (sin1, dsin1, sin2, dsin2) = self.sines(store, Some(param));
        let dn1 = if param == self.n1 { 1.0 } else { 0.0 };
        let dn2 = if param == self.n2 { 1.0 } else { 0.0 };
        dn1 * sin1 + store.get(self.n1) * dsin1 - dn2 * sin2 - store.get(self.n2) * dsin2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::test_support::assert_grad_matches_finite_difference;
    use crate::geo::{Circle, Line};

    fn make_point(store: &mut ParamStore, x: f64, y: f64) -> Point {
        Point::new(store.add(x, false), store.add(y, false))
    }

    #[test]
    fn curve_value_zero_error_when_point_matches_curve_at_u() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 0.0, 0.0);
        let rad = store.add(5.0, true);
        let circle: Rc<dyn Curve> = Rc::new(Circle { center, rad });
        let u = store.add(0.7, false);
        let p = make_point(&mut store, 5.0 * 0.7_f64.cos(), 5.0 * 0.7_f64.sin());

        let c = CurveValue::new(p, p.x, circle.clone(), u);
        assert!(c.error_value(&store).abs() < 1e-9);

        let cy = CurveValue::new(p, p.y, circle, u);
        assert!(cy.error_value(&store).abs() < 1e-9);
    }

    #[test]
    fn curve_value_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 1.0, -2.0);
        let rad = store.add(4.0, true);
        let circle: Rc<dyn Curve> = Rc::new(Circle { center, rad });
        let u = store.add(0.3, true);
        let p = make_point(&mut store, 10.0, 10.0); // deliberately off the circle

        let c = CurveValue::new(p, p.x, circle, u);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    fn make_lines_at_angle(store: &mut ParamStore, angle_rad: f64) -> (Rc<dyn Curve>, Rc<dyn Curve>, Point) {
        let poa = make_point(store, 0.0, 0.0);
        let l1_end = make_point(store, 10.0, 0.0);
        let (si, co) = angle_rad.sin_cos();
        let l2_end = make_point(store, 10.0 * co, 10.0 * si);
        let l1: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: l1_end });
        let l2: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: l2_end });
        (l1, l2, poa)
    }

    #[test]
    fn angle_via_point_zero_error_when_angle_matches() {
        let mut store = ParamStore::new();
        let (l1, l2, poa) = make_lines_at_angle(&mut store, std::f64::consts::FRAC_PI_3);
        let angle = store.add(std::f64::consts::FRAC_PI_3, true);

        let c = AngleViaPoint::new(l1, l2, poa, angle);
        assert!(c.error_value(&store).abs() < 1e-9, "err={}", c.error_value(&store));
    }

    #[test]
    fn angle_via_point_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        let (l1, l2, poa) = make_lines_at_angle(&mut store, 0.9);
        let angle = store.add(0.4, true); // deliberately not yet satisfied

        let c = AngleViaPoint::new(l1, l2, poa, angle);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn angle_via_two_points_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        let poa1 = make_point(&mut store, 0.0, 0.0);
        let l1: Rc<dyn Curve> = Rc::new(Line { p1: poa1, p2: make_point(&mut store, 10.0, 0.0) });
        let poa2 = make_point(&mut store, 5.0, 5.0);
        let l2: Rc<dyn Curve> = Rc::new(Line { p1: poa2, p2: make_point(&mut store, 5.0, 15.0) });
        let angle = store.add(0.6, true);

        let c = AngleViaTwoPoints::new(l1, l2, poa1, poa2, angle);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    // `calculate_normal_at_param` has a real, documented upstream
    // limitation (see its doc comment in `geo.rs`): for any curve whose
    // normal formula depends on point position (Circle, the conics), the
    // gradient it produces is incomplete for that curve's own shape
    // parameters, not just for the `u`-value parameter itself. `Line` is
    // exempt (its normal is position-independent), so using lines on the
    // "at param" side here is what makes these two tests able to check the
    // *whole* gradient with no exclusions, unlike the case that first
    // surfaced the limitation (a circle on that side).

    #[test]
    fn angle_via_point_and_param_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        // crv1's normal is taken "at param" (cparam) rather than at a
        // point — a Line's normal doesn't depend on position, so this is
        // exact for every parameter, including cparam.
        let l1: Rc<dyn Curve> =
            Rc::new(Line { p1: make_point(&mut store, 0.0, 0.0), p2: make_point(&mut store, 10.0, 0.0) });
        let cparam = store.add(0.5, true);

        let center = make_point(&mut store, 5.0, 5.0);
        let rad = store.add(3.0, true);
        let poa = make_point(&mut store, 8.0, 5.0); // on the circle, angle=0 from center
        let circle: Rc<dyn Curve> = Rc::new(Circle { center, rad });
        let angle = store.add(1.1, true);

        let c = AngleViaPointAndParam::new(l1, circle, poa, cparam, angle);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn angle_via_point_and_two_params_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        let l1: Rc<dyn Curve> =
            Rc::new(Line { p1: make_point(&mut store, 0.0, 0.0), p2: make_point(&mut store, 10.0, 0.0) });
        let cparam1 = store.add(0.5, true);

        let l2: Rc<dyn Curve> =
            Rc::new(Line { p1: make_point(&mut store, 3.0, 3.0), p2: make_point(&mut store, 3.0, 13.0) });
        let cparam2 = store.add(1.2, true);

        let angle = store.add(0.3, true);
        let unused_point = make_point(&mut store, 0.0, 0.0);

        let c = AngleViaPointAndTwoParams::new(l1, l2, unused_point, cparam1, cparam2, angle);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn snell_zero_error_at_normal_incidence_with_matched_indices() {
        // Boundary is the y-axis (vertical line); both rays run straight
        // along +x through the origin, i.e. normal incidence (sin = 0 on
        // both sides), so error is zero for *any* n1/n2.
        let mut store = ParamStore::new();
        let poa = make_point(&mut store, 0.0, 0.0);
        let boundary: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: make_point(&mut store, 0.0, 10.0) });
        let ray1: Rc<dyn Curve> = Rc::new(Line { p1: make_point(&mut store, -5.0, 0.0), p2: poa });
        let ray2: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: make_point(&mut store, 5.0, 0.0) });
        let n1 = store.add(1.0, true);
        let n2 = store.add(1.5, true);

        let c = Snell::new(ray1, ray2, boundary, poa, n1, n2, false, false);
        assert!(c.error_value(&store).abs() < 1e-9, "err={}", c.error_value(&store));
    }

    #[test]
    fn snell_grad_matches_finite_difference() {
        let mut store = ParamStore::new();
        let poa = make_point(&mut store, 0.0, 0.0);
        let boundary: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: make_point(&mut store, 0.0, 10.0) });
        // Rays at an angle to the boundary, so sines are nonzero.
        let ray1: Rc<dyn Curve> = Rc::new(Line { p1: make_point(&mut store, -5.0, -2.0), p2: poa });
        let ray2: Rc<dyn Curve> = Rc::new(Line { p1: poa, p2: make_point(&mut store, 5.0, -3.0) });
        let n1 = store.add(1.0, true);
        let n2 = store.add(1.5, true);

        let c = Snell::new(ray1, ray2, boundary, poa, n1, n2, false, true);
        assert_grad_matches_finite_difference(&c, &mut store);
    }
}
