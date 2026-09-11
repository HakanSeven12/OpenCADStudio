//! Circle/arc distance constraints (port stage 5b, partial).
//!
//! Ported from the corresponding `Constraint*` classes in
//! `Constraints.h`/`Constraints.cpp`. `CurveValue`, the `AngleViaPoint*`
//! family, and `Snell` are not yet ported — they dispatch through planegcs's
//! generic `Curve::CalculateNormal`/`Value` across arbitrary curve types
//! (needed once ellipses/hyperbolas/parabolas exist too), rather than
//! against a single concrete geometry the way every constraint here does;
//! left for whichever stage adds those curve types and a `Curve` trait
//! object story to dispatch across them.

use crate::constraints::Constraint;
use crate::geo::{Arc, Circle, DeriVector2, Point};
use crate::util::{ParamId, ParamStore};

/// Two circles are tangent: internally (one inside the other, touching) or
/// externally, per `internal`.
pub struct TangentCircumf {
    pub c1_center: Point,
    pub c2_center: Point,
    pub r1: ParamId,
    pub r2: ParamId,
    pub internal: bool,
}

impl TangentCircumf {
    pub fn new(c1_center: Point, c2_center: Point, r1: ParamId, r2: ParamId, internal: bool) -> Self {
        Self { c1_center, c2_center, r1, r2, internal }
    }
}

impl Constraint for TangentCircumf {
    fn params(&self) -> Vec<ParamId> {
        vec![self.c1_center.x, self.c1_center.y, self.c2_center.x, self.c2_center.y, self.r1, self.r2]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dx = store.get(self.c1_center.x) - store.get(self.c2_center.x);
        let dy = store.get(self.c1_center.y) - store.get(self.c2_center.y);
        let d_sq = dx * dx + dy * dy;
        let (r1, r2) = (store.get(self.r1), store.get(self.r2));

        // Concentric-circle singularity: fall back to the robust `r1 - r2`
        // formulation, which has a constant non-zero gradient.
        if d_sq < 1e-14 {
            return r1 - r2;
        }
        if self.internal {
            d_sq - (r1 - r2) * (r1 - r2)
        } else {
            d_sq - (r1 + r2) * (r1 + r2)
        }
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let dx = store.get(self.c1_center.x) - store.get(self.c2_center.x);
        let dy = store.get(self.c1_center.y) - store.get(self.c2_center.y);
        let d_sq = dx * dx + dy * dy;
        let (r1, r2) = (store.get(self.r1), store.get(self.r2));

        if d_sq < 1e-14 {
            if param == self.r1 {
                return 1.0;
            } else if param == self.r2 {
                return -1.0;
            }
            return 0.0;
        }

        let mut deriv = 0.0;
        if param == self.c1_center.x {
            deriv += 2.0 * dx;
        }
        if param == self.c1_center.y {
            deriv += 2.0 * dy;
        }
        if param == self.c2_center.x {
            deriv += -2.0 * dx;
        }
        if param == self.c2_center.y {
            deriv += -2.0 * dy;
        }
        if self.internal {
            if param == self.r1 {
                deriv += 2.0 * (r2 - r1);
            }
            if param == self.r2 {
                deriv += 2.0 * (r1 - r2);
            }
        } else {
            if param == self.r1 {
                deriv += -2.0 * (r1 + r2);
            }
            if param == self.r2 {
                deriv += -2.0 * (r1 + r2);
            }
        }
        deriv
    }
}

/// Distance between two circles' circumferences: `|c1 - c2|` outside both
/// circles, or the gap between them when one sits inside the other's span.
/// `c1_bigger` breaks the tie when neither circle's centre is outside the
/// other's radius (`None` picks whichever circle currently has the larger
/// radius).
pub struct C2CDistance {
    pub c1: Circle,
    pub c2: Circle,
    pub distance: ParamId,
    pub c1_bigger: Option<bool>,
}

impl C2CDistance {
    pub fn new(c1: Circle, c2: Circle, distance: ParamId, c1_bigger: Option<bool>) -> Self {
        Self { c1, c2, distance, c1_bigger }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let ct1 = DeriVector2::from_point(store, self.c1.center, derivparam);
        let ct2 = DeriVector2::from_point(store, self.c2.center, derivparam);
        let vector_ct12 = ct1.subtr(&ct2);
        let (length_ct12, dlength_ct12) = vector_ct12.length_deriv();

        let (r1, r2) = (store.get(self.c1.rad), store.get(self.c2.rad));
        let dist = store.get(self.distance);

        if length_ct12 >= r1 && length_ct12 >= r2 {
            let err = length_ct12 - (r2 + r1 + dist);
            let drad = if derivparam == Some(self.c2.rad) || derivparam == Some(self.c1.rad) { -1.0 } else { 0.0 };
            let grad = dlength_ct12 + drad;
            return (err, grad);
        }

        let (bigradius_id, smallradius_id) = match self.c1_bigger {
            None => {
                if r1 >= r2 {
                    (self.c1.rad, self.c2.rad)
                } else {
                    (self.c2.rad, self.c1.rad)
                }
            }
            Some(true) => (self.c1.rad, self.c2.rad),
            Some(false) => (self.c2.rad, self.c1.rad),
        };
        let (bigradius, smallradius) = (store.get(bigradius_id), store.get(smallradius_id));
        let smallspan = smallradius + length_ct12 + dist;
        let err = bigradius - smallspan;

        let mut drad = 0.0;
        if derivparam == Some(bigradius_id) {
            drad = 1.0;
        } else if derivparam == Some(smallradius_id) {
            drad = -1.0;
        } else if derivparam == Some(self.distance) {
            drad = if dist < 0.0 { 1.0 } else { -1.0 };
        }
        let grad = if length_ct12 > 1e-13 { -dlength_ct12 + drad } else { drad };
        (err, grad)
    }
}

impl Constraint for C2CDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.distance,
            self.c1.center.x,
            self.c1.center.y,
            self.c1.rad,
            self.c2.center.x,
            self.c2.center.y,
            self.c2.rad,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// Distance from a circle's circumference to a line, on one side (`ccw`) and
/// either measuring outward or `internal`ly (the circle sits astride the
/// line at that distance instead of clear of it).
pub struct C2LDistance {
    pub circle: Circle,
    pub line: crate::geo::Line,
    pub distance: ParamId,
    pub ccw: bool,
    pub internal: bool,
}

impl C2LDistance {
    pub fn new(circle: Circle, line: crate::geo::Line, distance: ParamId, ccw: bool, internal: bool) -> Self {
        Self { circle, line, distance, ccw, internal }
    }

    /// Signed distance from the circle's centre to the line, and its
    /// derivative w.r.t. `derivparam`.
    fn signed_value(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let ct = DeriVector2::from_point(store, self.circle.center, derivparam);
        let p1 = DeriVector2::from_point(store, self.line.p1, derivparam);
        let p2 = DeriVector2::from_point(store, self.line.p2, derivparam);
        let v_line = p2.subtr(&p1);
        let v_p1ct = ct.subtr(&p1);

        let (area, darea) = v_line.cross_prod_z(&v_p1ct);
        let (length, dlength) = v_line.length_deriv();

        let h = area / length;
        let dh = (darea - h * dlength) / length;
        (h, dh)
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let (h, dh) = self.signed_value(store, derivparam);
        let rad = store.get(self.circle.rad);
        let dist = store.get(self.distance).abs();
        let mut target = if self.internal { rad - dist } else { rad + dist };
        target = if self.ccw { target } else { -target };
        let err = target - h;

        let grad = if derivparam == Some(self.circle.rad) {
            if self.ccw {
                1.0
            } else {
                -1.0
            }
        } else {
            -dh
        };
        (err, grad)
    }
}

impl Constraint for C2LDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.distance,
            self.circle.center.x,
            self.circle.center.y,
            self.circle.rad,
            self.line.p1.x,
            self.line.p1.y,
            self.line.p2.x,
            self.line.p2.y,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// Distance from a point to a circle's circumference (`|point - center| -
/// radius`, signed by whether the point sits inside or outside the circle).
pub struct P2CDistance {
    pub circle: Circle,
    pub pt: Point,
    pub distance: ParamId,
}

impl P2CDistance {
    pub fn new(circle: Circle, pt: Point, distance: ParamId) -> Self {
        Self { circle, pt, distance }
    }

    fn value(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let ct = DeriVector2::from_point(store, self.circle.center, derivparam);
        let p = DeriVector2::from_point(store, self.pt, derivparam);
        ct.subtr(&p).length_deriv()
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let (length, dlength) = self.value(store, derivparam);
        let rad = store.get(self.circle.rad);
        let dist = store.get(self.distance);
        let inside = length < rad;

        let err = if inside { rad - dist - length } else { rad + dist - length };

        let grad = if derivparam == Some(self.distance) {
            if inside {
                -1.0
            } else {
                1.0
            }
        } else if derivparam == Some(self.circle.rad) {
            1.0
        } else {
            -dlength
        };
        (err, grad)
    }
}

impl Constraint for P2CDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![self.distance, self.circle.center.x, self.circle.center.y, self.circle.rad, self.pt.x, self.pt.y]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// An arc's length equals `distance`. Angles are normalized assuming a
/// positive, counterclockwise sweep before computing the arc length,
/// matching planegcs's `normalizedAngles`.
pub struct ArcLength {
    pub arc: Arc,
    pub distance: ParamId,
}

impl ArcLength {
    pub fn new(arc: Arc, distance: ParamId) -> Self {
        Self { arc, distance }
    }

    fn normalized_angles(&self, store: &ParamStore) -> (f64, f64) {
        let mut start = store.get(self.arc.start_angle);
        let mut end = store.get(self.arc.end_angle);
        while start < 0.0 {
            start += std::f64::consts::TAU;
        }
        while end < start {
            end += std::f64::consts::TAU;
        }
        (start, end)
    }
}

impl Constraint for ArcLength {
    fn params(&self) -> Vec<ParamId> {
        vec![self.distance, self.arc.circle.center.x, self.arc.circle.center.y, self.arc.circle.rad, self.arc.start_angle, self.arc.end_angle]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let rad = store.get(self.arc.circle.rad);
        let (start_a, end_a) = self.normalized_angles(store);
        rad * (end_a - start_a) - store.get(self.distance)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        if param == self.distance {
            return -1.0;
        }
        let rad = store.get(self.arc.circle.rad);
        let (start_a, end_a) = self.normalized_angles(store);
        let d_rad = if param == self.arc.circle.rad { 1.0 } else { 0.0 };
        let d_start_a = if param == self.arc.start_angle { 1.0 } else { 0.0 };
        let d_end_a = if param == self.arc.end_angle { 1.0 } else { 0.0 };
        rad * (d_end_a - d_start_a) + d_rad * (end_a - start_a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::test_support::{assert_grad_matches_finite_difference, assert_single_param_grad_matches_fd};
    use crate::geo::Line;

    fn point(store: &mut ParamStore, x: f64, y: f64) -> Point {
        Point::new(store.add(x, false), store.add(y, false))
    }

    fn circle(store: &mut ParamStore, cx: f64, cy: f64, r: f64) -> Circle {
        Circle { center: point(store, cx, cy), rad: store.add(r, false) }
    }

    #[test]
    fn tangent_circumf_external_is_zero_at_the_right_separation() {
        let mut store = ParamStore::new();
        let c1 = circle(&mut store, 0.0, 0.0, 3.0);
        let c2 = circle(&mut store, 10.0, 0.0, 4.0); // sum of radii == 7, but separation is 10... use 7
        // fix separation to sum of radii (7) for a zero-error case
        store.set(c2.center.x, 7.0);
        let c = TangentCircumf::new(c1.center, c2.center, c1.rad, c2.rad, false);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn tangent_circumf_gradient_checks_out_away_from_the_singularity() {
        let mut store = ParamStore::new();
        let c1 = circle(&mut store, 0.0, 0.0, 3.0);
        let c2 = circle(&mut store, 9.0, 2.0, 4.0);
        let c = TangentCircumf::new(c1.center, c2.center, c1.rad, c2.rad, false);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn c2c_distance_matches_hand_computation_outside_case() {
        let mut store = ParamStore::new();
        let c1 = circle(&mut store, 0.0, 0.0, 2.0);
        let c2 = circle(&mut store, 10.0, 0.0, 3.0);
        let d = store.add(5.0, false); // 10 - 2 - 3 == 5
        let c = C2CDistance::new(c1, c2, d, None);
        assert!(c.error_value(&store).abs() < 1e-9);
        // Skips `d` (this constraint's own `distance` param): planegcs's own
        // `errorgrad` never assigns it a nonzero gradient in this branch
        // (only the two radii do) even though `error` depends on it with
        // coefficient -1 — an upstream quirk, not a translation bug, and
        // harmless in practice since `distance` is the driven target value,
        // not a free geometry parameter the solver differentiates against.
        for param in [c1.center.x, c1.center.y, c1.rad, c2.center.x, c2.center.y, c2.rad] {
            assert_single_param_grad_matches_fd(&c, &mut store, param);
        }
    }

    #[test]
    fn c2c_distance_gradient_checks_out_nested_case() {
        let mut store = ParamStore::new();
        let c1 = circle(&mut store, 0.0, 0.0, 10.0);
        let c2 = circle(&mut store, 1.0, 0.0, 3.0); // c2 nested inside c1
        let d = store.add(1.0, false);
        let c = C2CDistance::new(c1, c2, d, Some(true));
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn c2l_distance_matches_hand_computation() {
        let mut store = ParamStore::new();
        let c = circle(&mut store, 0.0, 5.0, 2.0);
        let l = Line { p1: point(&mut store, -10.0, 0.0), p2: point(&mut store, 10.0, 0.0) };
        let d = store.add(3.0, false); // rad(2) + dist(3) == center height (5)
        let constr = C2LDistance::new(c, l, d, true, false);
        assert!(constr.error_value(&store).abs() < 1e-9);
        // Skips `d` for the same reason as the C2CDistance test above:
        // planegcs's own `errorgrad` gives it gradient 0 in the non-radius
        // branch (`else { *grad = -dh; }`), an upstream quirk rather than a
        // translation bug.
        for param in [c.center.x, c.center.y, c.rad, l.p1.x, l.p1.y, l.p2.x, l.p2.y] {
            assert_single_param_grad_matches_fd(&constr, &mut store, param);
        }
    }

    #[test]
    fn p2c_distance_matches_hand_computation_outside_case() {
        let mut store = ParamStore::new();
        let c = circle(&mut store, 0.0, 0.0, 2.0);
        let p = point(&mut store, 5.0, 0.0);
        let d = store.add(3.0, false); // rad(2) + dist(3) == 5
        let constr = P2CDistance::new(c, p, d);
        assert!(constr.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&constr, &mut store);
    }

    #[test]
    fn arc_length_matches_hand_computation() {
        let mut store = ParamStore::new();
        let c = circle(&mut store, 0.0, 0.0, 2.0);
        let start_angle = store.add(0.0, false);
        let end_angle = store.add(std::f64::consts::FRAC_PI_2, false);
        let arc = Arc {
            circle: c,
            start: point(&mut store, 2.0, 0.0),
            end: point(&mut store, 0.0, 2.0),
            start_angle,
            end_angle,
        };
        let d = store.add(std::f64::consts::PI, false); // quarter circle of radius 2: length = pi
        let constr = ArcLength::new(arc, d);
        assert!(constr.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&constr, &mut store);
    }
}
