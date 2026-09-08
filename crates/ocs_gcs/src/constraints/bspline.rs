//! `PointOnBSpline`: ties one coordinate of a point to a B-spline's
//! parametric value at `u`.
//!
//! Ported from planegcs's `ConstraintPointOnBSpline` (`Constraints.h`/`.cpp`).
//! The C++ caches which window of `degree + 1` poles is currently active
//! (`startpole`) and only recomputes it when `theparam` drifts outside the
//! cached window's knot span — a solver-loop optimization, since the same
//! constraint's `error()`/`grad()` are called many times per solve with `u`
//! barely changing between calls. This port always recomputes the window
//! fresh via [`BSpline::find_span`] instead: `find_span` is an O(pole
//! count) linear scan, cheap at ordinary sketch scale, and skipping the
//! cache sidesteps needing interior mutability (a `Cell<usize>`) on a type
//! that otherwise, like every other `Constraint` impl here, only needs
//! shared access to evaluate.
//!
//! `SlopeAtBSplineKnot` (planegcs's other B-spline constraint, enforcing
//! tangent continuity across a knot) is not ported: it requires
//! reconstructing knot multiplicity from the flattened knot vector this
//! port's [`BSpline`] uses (see its struct docs) purely to precompute
//! construction-time combination factors, and is a narrower, less commonly
//! needed constraint than `PointOnBSpline` — deferred rather than rushed.

use crate::geo::{spline_value, BSpline};
use crate::util::{ParamId, ParamStore};

use super::Constraint;

/// Which coordinate of the constrained point this ties to the spline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coord {
    X,
    Y,
}

pub struct PointOnBSpline {
    point: ParamId,
    param: ParamId,
    coord: Coord,
    bsp: BSpline,
}

impl PointOnBSpline {
    pub fn new(point: ParamId, param: ParamId, coord: Coord, bsp: BSpline) -> Self {
        Self { point, param, coord, bsp }
    }

    fn pole_at(&self, startpole: usize, i: usize) -> ParamId {
        match self.coord {
            Coord::X => self.bsp.pole_x_at(startpole, i),
            Coord::Y => self.bsp.pole_y_at(startpole, i),
        }
    }

    fn numpoints(&self) -> usize {
        self.bsp.degree + 1
    }

    /// `(k, startpole)` for the knot span containing the current `param`
    /// value — recomputed fresh every call, see the module doc comment.
    fn window(&self, store: &ParamStore) -> (usize, usize) {
        let k = self.bsp.find_span(store.get(self.param));
        (k, k - self.bsp.degree)
    }
}

impl Constraint for PointOnBSpline {
    fn params(&self) -> Vec<ParamId> {
        let mut params = vec![self.point, self.param];
        for pole in &self.bsp.poles {
            params.push(match self.coord {
                Coord::X => pole.x,
                Coord::Y => pole.y,
            });
        }
        params.extend(self.bsp.weights.iter().copied());
        params
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (k, startpole) = self.window(store);
        let numpoints = self.numpoints();

        let mut d = vec![0.0; numpoints];
        for i in 0..numpoints {
            d[i] = store.get(self.pole_at(startpole, i)) * store.get(self.bsp.weight_at(startpole, i));
        }
        let sum = spline_value(store.get(self.param), k, self.bsp.degree, &mut d, &self.bsp.knots);
        for i in 0..numpoints {
            d[i] = store.get(self.bsp.weight_at(startpole, i));
        }
        let wsum = spline_value(store.get(self.param), k, self.bsp.degree, &mut d, &self.bsp.knots);

        store.get(self.point) * wsum - sum
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (k, startpole) = self.window(store);
        let numpoints = self.numpoints();
        let u = store.get(self.param);
        let mut deriv = 0.0;

        if param == self.point {
            let mut d = vec![0.0; numpoints];
            for i in 0..numpoints {
                d[i] = store.get(self.bsp.weight_at(startpole, i));
            }
            deriv += spline_value(u, k, self.bsp.degree, &mut d, &self.bsp.knots);
        }

        if param == self.param {
            let denom = |i: usize| self.bsp.knots[startpole + i + self.bsp.degree] - self.bsp.knots[startpole + i];

            let mut d = vec![0.0; numpoints - 1];
            for i in 1..numpoints {
                d[i - 1] = (store.get(self.pole_at(startpole, i)) * store.get(self.bsp.weight_at(startpole, i))
                    - store.get(self.pole_at(startpole, i - 1)) * store.get(self.bsp.weight_at(startpole, i - 1)))
                    / denom(i);
            }
            let slopevalue = spline_value(u, k, self.bsp.degree - 1, &mut d, &self.bsp.knots);

            for i in 1..numpoints {
                d[i - 1] =
                    (store.get(self.bsp.weight_at(startpole, i)) - store.get(self.bsp.weight_at(startpole, i - 1))) / denom(i);
            }
            let wslopevalue = spline_value(u, k, self.bsp.degree - 1, &mut d, &self.bsp.knots);

            deriv += (store.get(self.point) * wslopevalue - slopevalue) * self.bsp.degree as f64;
        }

        for i in 0..numpoints {
            let pole_i = self.pole_at(startpole, i);
            let weight_i = self.bsp.weight_at(startpole, i);
            if param == pole_i {
                let factor_i = self.bsp.get_lin_comb_factor(u, k, startpole + i, self.bsp.degree);
                deriv += -(store.get(weight_i) * factor_i);
            }
            if param == weight_i {
                let factor_i = self.bsp.get_lin_comb_factor(u, k, startpole + i, self.bsp.degree);
                deriv += (store.get(self.point) - store.get(pole_i)) * factor_i;
            }
        }

        deriv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::test_support::assert_grad_matches_finite_difference;
    use crate::geo::{Curve, Point};

    /// The same clamped cubic-Bézier-as-BSpline shape as `geo.rs`'s own
    /// tests, so its `value()` is already independently verified there.
    fn make_bspline(store: &mut ParamStore, poles_xy: [(f64, f64); 4]) -> BSpline {
        let poles: Vec<Point> =
            poles_xy.iter().map(|&(x, y)| Point::new(store.add(x, false), store.add(y, false))).collect();
        let weights = vec![store.add(1.0, false); 4];
        BSpline {
            start: poles[0],
            end: poles[3],
            poles,
            weights,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            degree: 3,
            periodic: false,
        }
    }

    #[test]
    fn zero_error_when_point_matches_the_spline_at_u() {
        let mut store = ParamStore::new();
        let bsp = make_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let u = 0.5;
        let on_curve = bsp.value(&store, u, 0.0, None);

        let point = store.add(on_curve.x, false);
        let param = store.add(u, true);
        let c = PointOnBSpline::new(point, param, Coord::X, bsp.clone());
        assert!(c.error_value(&store).abs() < 1e-9, "err={}", c.error_value(&store));

        let point_y = store.add(on_curve.y, false);
        let cy = PointOnBSpline::new(point_y, param, Coord::Y, bsp);
        assert!(cy.error_value(&store).abs() < 1e-9, "err={}", cy.error_value(&store));
    }

    #[test]
    fn nonzero_error_when_point_is_off_the_spline() {
        let mut store = ParamStore::new();
        let bsp = make_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let point = store.add(100.0, false);
        let param = store.add(0.5, true);
        let c = PointOnBSpline::new(point, param, Coord::X, bsp);
        assert!(c.error_value(&store).abs() > 1.0);
    }

    #[test]
    fn grad_matches_finite_difference_x_coord() {
        let mut store = ParamStore::new();
        let bsp = make_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let point = store.add(2.5, true); // deliberately off the spline
        let param = store.add(0.4, true);
        let c = PointOnBSpline::new(point, param, Coord::X, bsp);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn grad_matches_finite_difference_y_coord() {
        let mut store = ParamStore::new();
        let bsp = make_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let point = store.add(1.0, true);
        let param = store.add(0.7, true);
        let c = PointOnBSpline::new(point, param, Coord::Y, bsp);
        assert_grad_matches_finite_difference(&c, &mut store);
    }
}
