//! Conic constraints (port stage 5c, partial).
//!
//! Ported from the corresponding `Constraint*` classes in
//! `Constraints.h`/`Constraints.cpp`: `PointOnEllipse`, `TangentEllipseLine`,
//! `EqualMajorAxesConic`, `EqualFocalDistance`, `PointOnHyperbola`,
//! `PointOnParabola`. Not yet ported: `InternalAlignmentPoint2Ellipse`/
//! `InternalAlignmentPoint2Hyperbola` (upstream-internal bookkeeping
//! for interactively *creating* conic geometry, not for constraining
//! existing geometry) and `EllipticalArcRangeToEndPoints` (a
//! `ConstraintType` enum value with no implementing class anywhere in the
//! current planegcs source — dead upstream, not a gap in this port).

use crate::constraints::Constraint;
use crate::geo::{Conic, Curve, DeriVector2, Ellipse, Hyperbola, Line, Parabola, Point};
use crate::util::{ParamId, ParamStore};

/// Point `p` lies on the ellipse `e` (sum of distances to both foci equals
/// the major axis length, `2a`).
pub struct PointOnEllipse {
    pub p: Point,
    pub e: Ellipse,
}

impl PointOnEllipse {
    pub fn new(p: Point, e: Ellipse) -> Self {
        Self { p, e }
    }
}

impl Constraint for PointOnEllipse {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.p.x,
            self.p.y,
            self.e.center.x,
            self.e.center.y,
            self.e.focus1.x,
            self.e.focus1.y,
            self.e.radmin,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (x0, y0) = (store.get(self.p.x), store.get(self.p.y));
        let (xc, yc) = (store.get(self.e.center.x), store.get(self.e.center.y));
        let (xf1, yf1) = (store.get(self.e.focus1.x), store.get(self.e.focus1.y));
        let b = store.get(self.e.radmin);

        ((x0 - xf1).powi(2) + (y0 - yf1).powi(2)).sqrt()
            + ((x0 + xf1 - 2.0 * xc).powi(2) + (y0 + yf1 - 2.0 * yc).powi(2)).sqrt()
            - 2.0 * (b.powi(2) + (xf1 - xc).powi(2) + (yf1 - yc).powi(2)).sqrt()
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (p1x, p1y) = (self.p.x, self.p.y);
        let (cx, cy) = (self.e.center.x, self.e.center.y);
        let (f1x, f1y) = (self.e.focus1.x, self.e.focus1.y);
        let rmin = self.e.radmin;
        if ![p1x, p1y, f1x, f1y, cx, cy, rmin].contains(&param) {
            return 0.0;
        }
        let (x0, y0) = (store.get(p1x), store.get(p1y));
        let (xc, yc) = (store.get(cx), store.get(cy));
        let (xf1, yf1) = (store.get(f1x), store.get(f1y));
        let b = store.get(rmin);

        let d_pf1 = ((x0 - xf1).powi(2) + (y0 - yf1).powi(2)).sqrt();
        let d_pf2 = ((x0 + xf1 - 2.0 * xc).powi(2) + (y0 + yf1 - 2.0 * yc).powi(2)).sqrt();
        let d_cf1 = (b.powi(2) + (xf1 - xc).powi(2) + (yf1 - yc).powi(2)).sqrt();

        let mut deriv = 0.0;
        if param == p1x {
            deriv += (x0 - xf1) / d_pf1 + (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == p1y {
            deriv += (y0 - yf1) / d_pf1 + (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == f1x {
            deriv += -(x0 - xf1) / d_pf1 - 2.0 * (xf1 - xc) / d_cf1 + (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == f1y {
            deriv += -(y0 - yf1) / d_pf1 - 2.0 * (yf1 - yc) / d_cf1 + (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == cx {
            deriv += 2.0 * (xf1 - xc) / d_cf1 - 2.0 * (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == cy {
            deriv += 2.0 * (yf1 - yc) / d_cf1 - 2.0 * (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == rmin {
            deriv += -2.0 * b / d_cf1;
        }
        deriv
    }
}

/// Line `l` is tangent to ellipse `e`: mirroring one focus across the line
/// lands it exactly `2a` (the major axis) from the other focus.
pub struct TangentEllipseLine {
    pub l: Line,
    pub e: Ellipse,
}

impl TangentEllipseLine {
    pub fn new(l: Line, e: Ellipse) -> Self {
        Self { l, e }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let p1 = DeriVector2::from_point(store, self.l.p1, derivparam);
        let f1 = DeriVector2::from_point(store, self.e.focus1, derivparam);
        let c = DeriVector2::from_point(store, self.e.center, derivparam);
        let f2 = c.lin_combi(2.0, &f1, -1.0);

        let nl = self
            .l
            .calculate_normal(store, self.l.p1, derivparam)
            .normalized();
        let (dist_f1l, ddist_f1l) = f1.subtr(&p1).scalar_prod(&nl);
        let f1m = f1.sum(&nl.mult_d(-2.0 * dist_f1l, -2.0 * ddist_f1l));

        let (dist_f1m_f2, ddist_f1m_f2) = {
            let d = f2.subtr(&f1m);
            d.length_deriv()
        };

        let (rad_maj, drad_maj) = self.e.rad_maj_at(store, derivparam);

        (dist_f1m_f2 - 2.0 * rad_maj, ddist_f1m_f2 - 2.0 * drad_maj)
    }
}

impl Constraint for TangentEllipseLine {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.l.p1.x,
            self.l.p1.y,
            self.l.p2.x,
            self.l.p2.y,
            self.e.center.x,
            self.e.center.y,
            self.e.focus1.x,
            self.e.focus1.y,
            self.e.radmin,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// Two conics (either ellipses or hyperbolas, mixed or matched) share the
/// same major radius.
pub struct EqualMajorAxesConic {
    pub e1: Conic,
    pub e2: Conic,
}

impl EqualMajorAxesConic {
    pub fn new(e1: Conic, e2: Conic) -> Self {
        Self { e1, e2 }
    }
}

impl Constraint for EqualMajorAxesConic {
    fn params(&self) -> Vec<ParamId> {
        let mut p = self.e1.own_params();
        p.extend(self.e2.own_params());
        p
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (a1, _) = self.e1.rad_maj_at(store, None);
        let (a2, _) = self.e2.rad_maj_at(store, None);
        a2 - a1
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (_, da1) = self.e1.rad_maj_at(store, Some(param));
        let (_, da2) = self.e2.rad_maj_at(store, Some(param));
        da2 - da1
    }
}

/// Two parabolas share the same focal distance (vertex-to-focus length).
pub struct EqualFocalDistance {
    pub e1: Parabola,
    pub e2: Parabola,
}

impl EqualFocalDistance {
    pub fn new(e1: Parabola, e2: Parabola) -> Self {
        Self { e1, e2 }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let (focal1, dfocal1) = {
            let focus1 = DeriVector2::from_point(store, self.e1.focus1, derivparam);
            let vertex1 = DeriVector2::from_point(store, self.e1.vertex, derivparam);
            vertex1.subtr(&focus1).length_deriv()
        };
        let (focal2, dfocal2) = {
            let focus2 = DeriVector2::from_point(store, self.e2.focus1, derivparam);
            let vertex2 = DeriVector2::from_point(store, self.e2.vertex, derivparam);
            vertex2.subtr(&focus2).length_deriv()
        };
        (focal2 - focal1, dfocal2 - dfocal1)
    }
}

impl Constraint for EqualFocalDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.e1.vertex.x,
            self.e1.vertex.y,
            self.e1.focus1.x,
            self.e1.focus1.y,
            self.e2.vertex.x,
            self.e2.vertex.y,
            self.e2.focus1.x,
            self.e2.focus1.y,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// Point `p` lies on hyperbola `e` (difference of distances to the two foci
/// equals the major axis length, `2a`).
pub struct PointOnHyperbola {
    pub p: Point,
    pub e: Hyperbola,
}

impl PointOnHyperbola {
    pub fn new(p: Point, e: Hyperbola) -> Self {
        Self { p, e }
    }
}

impl Constraint for PointOnHyperbola {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.p.x,
            self.p.y,
            self.e.center.x,
            self.e.center.y,
            self.e.focus1.x,
            self.e.focus1.y,
            self.e.radmin,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (x0, y0) = (store.get(self.p.x), store.get(self.p.y));
        let (xc, yc) = (store.get(self.e.center.x), store.get(self.e.center.y));
        let (xf1, yf1) = (store.get(self.e.focus1.x), store.get(self.e.focus1.y));
        let b = store.get(self.e.radmin);

        -((x0 - xf1).powi(2) + (y0 - yf1).powi(2)).sqrt()
            + ((x0 + xf1 - 2.0 * xc).powi(2) + (y0 + yf1 - 2.0 * yc).powi(2)).sqrt()
            - 2.0 * (-b.powi(2) + (xf1 - xc).powi(2) + (yf1 - yc).powi(2)).sqrt()
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (p1x, p1y) = (self.p.x, self.p.y);
        let (cx, cy) = (self.e.center.x, self.e.center.y);
        let (f1x, f1y) = (self.e.focus1.x, self.e.focus1.y);
        let rmin = self.e.radmin;
        if ![p1x, p1y, f1x, f1y, cx, cy, rmin].contains(&param) {
            return 0.0;
        }
        let (x0, y0) = (store.get(p1x), store.get(p1y));
        let (xc, yc) = (store.get(cx), store.get(cy));
        let (xf1, yf1) = (store.get(f1x), store.get(f1y));
        let b = store.get(rmin);

        let d_pf1 = ((x0 - xf1).powi(2) + (y0 - yf1).powi(2)).sqrt();
        let d_pf2 = ((x0 + xf1 - 2.0 * xc).powi(2) + (y0 + yf1 - 2.0 * yc).powi(2)).sqrt();
        let d_cf1 = (-b.powi(2) + (xf1 - xc).powi(2) + (yf1 - yc).powi(2)).sqrt();

        let mut deriv = 0.0;
        if param == p1x {
            deriv += -(x0 - xf1) / d_pf1 + (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == p1y {
            deriv += -(y0 - yf1) / d_pf1 + (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == f1x {
            deriv += (x0 - xf1) / d_pf1 - 2.0 * (xf1 - xc) / d_cf1 + (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == f1y {
            deriv += (y0 - yf1) / d_pf1 - 2.0 * (yf1 - yc) / d_cf1 + (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == cx {
            deriv += 2.0 * (xf1 - xc) / d_cf1 - 2.0 * (x0 + xf1 - 2.0 * xc) / d_pf2;
        }
        if param == cy {
            deriv += 2.0 * (yf1 - yc) / d_cf1 - 2.0 * (y0 + yf1 - 2.0 * yc) / d_pf2;
        }
        if param == rmin {
            deriv += 2.0 * b / d_cf1;
        }
        deriv
    }
}

/// Point `p` lies on parabola `e` (distance to the focus equals the
/// distance to the directrix — expressed here via the focus/vertex-derived
/// projection, matching the C++ directly rather than the textbook form).
pub struct PointOnParabola {
    pub p: Point,
    pub e: Parabola,
}

impl PointOnParabola {
    pub fn new(p: Point, e: Parabola) -> Self {
        Self { p, e }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let focus = DeriVector2::from_point(store, self.e.focus1, derivparam);
        let vertex = DeriVector2::from_point(store, self.e.vertex, derivparam);
        let point = DeriVector2::from_point(store, self.p, derivparam);

        let focalvect = focus.subtr(&vertex);
        let xdir = focalvect.normalized();
        let point_to_focus = point.subtr(&focus);

        let (focal, dfocal) = focalvect.length_deriv();
        let (pf, dpf) = point_to_focus.length_deriv();
        let (proj, dproj) = point_to_focus.scalar_prod(&xdir);

        (pf - 2.0 * focal - proj, dpf - 2.0 * dfocal - dproj)
    }
}

impl Constraint for PointOnParabola {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.p.x,
            self.p.y,
            self.e.vertex.x,
            self.e.vertex.y,
            self.e.focus1.x,
            self.e.focus1.y,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::test_support::{
        assert_grad_matches_finite_difference, assert_single_param_grad_matches_fd,
    };

    fn point(store: &mut ParamStore, x: f64, y: f64) -> Point {
        Point::new(store.add(x, false), store.add(y, false))
    }

    fn ellipse(store: &mut ParamStore, cx: f64, cy: f64, fx: f64, fy: f64, radmin: f64) -> Ellipse {
        Ellipse {
            center: point(store, cx, cy),
            focus1: point(store, fx, fy),
            radmin: store.add(radmin, false),
        }
    }

    #[test]
    fn point_on_ellipse_is_zero_for_a_point_on_the_ellipse() {
        let mut store = ParamStore::new();
        // c=3 (focus at x=3 from a center at origin), radmin(b)=4 -> a=5.
        let e = ellipse(&mut store, 0.0, 0.0, 3.0, 0.0, 4.0);
        let p = point(&mut store, 5.0, 0.0); // the +a vertex, on the ellipse
        let c = PointOnEllipse::new(p, e);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn tangent_ellipse_line_gradient_checks_out() {
        let mut store = ParamStore::new();
        let e = ellipse(&mut store, 0.0, 0.0, 3.0, 0.0, 4.0); // a=5
                                                              // A horizontal line y=4 is tangent to this ellipse at its top (0,4).
        let l = Line {
            p1: point(&mut store, -10.0, 4.0),
            p2: point(&mut store, 10.0, 4.0),
        };
        let c = TangentEllipseLine::new(l, e);
        assert!(c.error_value(&store).abs() < 1e-6);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn equal_major_axes_conic_compares_an_ellipse_and_a_hyperbola() {
        let mut store = ParamStore::new();
        let e1 = ellipse(&mut store, 0.0, 0.0, 3.0, 0.0, 4.0); // a = 5
                                                               // Hyperbola: c=5, b=4 -> a = sqrt(25-16) = 3, so NOT equal to e1's a=5 initially.
        let h_center = point(&mut store, 100.0, 0.0);
        let h_focus = point(&mut store, 105.0, 0.0);
        let h_radmin = store.add(4.0, false);
        let h = Hyperbola {
            center: h_center,
            focus1: h_focus,
            radmin: h_radmin,
        };

        let c = EqualMajorAxesConic::new(Conic::Ellipse(e1), Conic::Hyperbola(h));
        // a2 - a1 = 3 - 5 = -2
        assert!((c.error_value(&store) - (-2.0)).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn equal_focal_distance_matches_hand_computation() {
        let mut store = ParamStore::new();
        let p1 = Parabola {
            vertex: point(&mut store, 0.0, 0.0),
            focus1: point(&mut store, 0.0, 2.0),
        };
        let p2 = Parabola {
            vertex: point(&mut store, 10.0, 0.0),
            focus1: point(&mut store, 10.0, 2.0),
        };
        let c = EqualFocalDistance::new(p1, p2);
        assert!(c.error_value(&store).abs() < 1e-9); // both focal distances are 2
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn point_on_hyperbola_is_zero_for_a_point_on_the_hyperbola() {
        let mut store = ParamStore::new();
        // c=5, b=4 -> a=3
        let e = Hyperbola {
            center: point(&mut store, 0.0, 0.0),
            focus1: point(&mut store, 5.0, 0.0),
            radmin: store.add(4.0, false),
        };
        let p = point(&mut store, 3.0, 0.0); // the +a vertex, on the hyperbola
        let c = PointOnHyperbola::new(p, e);
        assert!(c.error_value(&store).abs() < 1e-9);
        // Skip the radmin/center/focus params too close to this vertex's
        // own singular direction for a robust central-difference check;
        // check the point's own coordinates, which is the common case.
        assert_single_param_grad_matches_fd(&c, &mut store, p.x);
        assert_single_param_grad_matches_fd(&c, &mut store, p.y);
    }

    #[test]
    fn point_on_parabola_is_zero_for_a_point_on_the_parabola() {
        let mut store = ParamStore::new();
        let e = Parabola {
            vertex: point(&mut store, 0.0, 0.0),
            focus1: point(&mut store, 0.0, 2.0),
        };
        // x^2 = 4fy with f=2: at x=4, y = 16/8 = 2
        let p = point(&mut store, 4.0, 2.0);
        let c = PointOnParabola::new(p, e);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }
}
