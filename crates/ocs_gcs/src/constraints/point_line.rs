//! Point/line primitive constraints (port stage 5a).
//!
//! Ported from the corresponding `Constraint*` classes in
//! `Constraints.h`/`Constraints.cpp`. Two things every constraint type here
//! omits, both intentionally and both because they serve FreeCAD's
//! *driven*-dimension bookkeeping rather than the solve itself:
//! - `evaluate()` (computes a driven dimension's value from geometry without
//!   solving) has no port — nothing in the solve path calls it.
//! - `rescale()`'s coefficient parameter (used to *re*-scale a constraint
//!   after construction, e.g. when FreeCAD's UI edits geometry) isn't
//!   exposed; the scale planegcs computes at construction time (for
//!   `Parallel`/`Perpendicular`) is computed once, in `new`, from the
//!   `ParamStore`'s values at that moment.

use std::collections::HashMap;

use crate::constraints::Constraint;
use crate::geo::{Line, Point};
use crate::util::{ParamId, ParamStore};

/// `param1 == ratio * param2`.
///
/// Faithfully ported including an upstream quirk: `grad_value` w.r.t.
/// `param2` is `-1`, not `-ratio`, even though `error_value` is
/// `param1 - ratio*param2` (whose true derivative w.r.t. `param2` is
/// `-ratio`). This matches planegcs's `ConstraintEqual::grad` exactly
/// (`Constraints.cpp`, `ConstraintEqual::grad`) — for the overwhelmingly
/// common `ratio == 1.0` case the two coincide, so this is only observable
/// when a caller passes a non-default ratio.
pub struct Equal {
    pub param1: ParamId,
    pub param2: ParamId,
    pub ratio: f64,
}

impl Equal {
    pub fn new(param1: ParamId, param2: ParamId, ratio: f64) -> Self {
        Self { param1, param2, ratio }
    }
}

impl Constraint for Equal {
    fn params(&self) -> Vec<ParamId> {
        vec![self.param1, self.param2]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        store.get(self.param1) - self.ratio * store.get(self.param2)
    }

    fn grad_value(&self, _store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        if param == self.param1 {
            deriv += 1.0;
        }
        if param == self.param2 {
            deriv -= 1.0;
        }
        deriv
    }
}

/// `param2 - param1 == difference`.
pub struct Difference {
    pub param1: ParamId,
    pub param2: ParamId,
    pub difference: ParamId,
}

impl Difference {
    pub fn new(param1: ParamId, param2: ParamId, difference: ParamId) -> Self {
        Self { param1, param2, difference }
    }
}

impl Constraint for Difference {
    fn params(&self) -> Vec<ParamId> {
        vec![self.param1, self.param2, self.difference]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        (store.get(self.param2) - store.get(self.param1)) - store.get(self.difference)
    }

    fn grad_value(&self, _store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        if param == self.param1 {
            deriv -= 1.0;
        }
        if param == self.param2 {
            deriv += 1.0;
        }
        if param == self.difference {
            deriv -= 1.0;
        }
        deriv
    }
}

/// `center == sum(points[i] * weights[i])`.
pub struct CenterOfGravity {
    pub center: ParamId,
    pub points: Vec<ParamId>,
    pub weights: Vec<f64>,
}

impl CenterOfGravity {
    pub fn new(center: ParamId, points: Vec<ParamId>, weights: Vec<f64>) -> Self {
        assert_eq!(points.len(), weights.len());
        Self { center, points, weights }
    }
}

impl Constraint for CenterOfGravity {
    fn params(&self) -> Vec<ParamId> {
        let mut p = vec![self.center];
        p.extend(&self.points);
        p
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let sum: f64 = self.points.iter().zip(&self.weights).map(|(&p, &w)| store.get(p) * w).sum();
        store.get(self.center) - sum
    }

    fn grad_value(&self, _store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        if param == self.center {
            deriv = 1.0;
        }
        for (&p, &w) in self.points.iter().zip(&self.weights) {
            if param == p {
                deriv = -w;
            }
        }
        deriv
    }
}

/// `point * sum(weights[i]*factors[i]) == sum(poles[i]*weights[i]*factors[i])`
/// (a knot point staying at the position its B-spline poles determine, in
/// homogeneous coordinates).
pub struct WeightedLinearCombination {
    pub point: ParamId,
    pub poles: Vec<ParamId>,
    pub weights: Vec<ParamId>,
    pub factors: Vec<f64>,
}

impl WeightedLinearCombination {
    pub fn new(point: ParamId, poles: Vec<ParamId>, weights: Vec<ParamId>, factors: Vec<f64>) -> Self {
        assert_eq!(poles.len(), weights.len());
        assert_eq!(poles.len(), factors.len());
        Self { point, poles, weights, factors }
    }
}

impl Constraint for WeightedLinearCombination {
    fn params(&self) -> Vec<ParamId> {
        let mut p = vec![self.point];
        p.extend(&self.poles);
        p.extend(&self.weights);
        p
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let mut sum = 0.0;
        let mut wsum = 0.0;
        for i in 0..self.poles.len() {
            let wcontrib = store.get(self.weights[i]) * self.factors[i];
            wsum += wcontrib;
            sum += store.get(self.poles[i]) * wcontrib;
        }
        store.get(self.point) * wsum - sum
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        if param == self.point {
            let wsum: f64 = (0..self.poles.len()).map(|i| store.get(self.weights[i]) * self.factors[i]).sum();
            return wsum;
        }
        for i in 0..self.poles.len() {
            if param == self.poles[i] {
                return -(store.get(self.weights[i]) * self.factors[i]);
            }
            if param == self.weights[i] {
                return (store.get(self.point) - store.get(self.poles[i])) * self.factors[i];
            }
        }
        0.0
    }
}

/// `|p1 - p2| == distance`.
pub struct P2PDistance {
    pub p1: Point,
    pub p2: Point,
    pub distance: ParamId,
}

impl P2PDistance {
    pub fn new(p1: Point, p2: Point, distance: ParamId) -> Self {
        Self { p1, p2, distance }
    }

    fn value(&self, store: &ParamStore) -> f64 {
        let dx = store.get(self.p1.x) - store.get(self.p2.x);
        let dy = store.get(self.p1.y) - store.get(self.p2.y);
        (dx * dx + dy * dy).sqrt()
    }
}

impl Constraint for P2PDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![self.p1.x, self.p1.y, self.p2.x, self.p2.y, self.distance]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.value(store) - store.get(self.distance)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        if [self.p1.x, self.p1.y, self.p2.x, self.p2.y].contains(&param) {
            let dx = store.get(self.p1.x) - store.get(self.p2.x);
            let dy = store.get(self.p1.y) - store.get(self.p2.y);
            let d = (dx * dx + dy * dy).sqrt();
            if param == self.p1.x {
                deriv += dx / d;
            }
            if param == self.p1.y {
                deriv += dy / d;
            }
            if param == self.p2.x {
                deriv += -dx / d;
            }
            if param == self.p2.y {
                deriv += -dy / d;
            }
        }
        if param == self.distance {
            deriv += -1.0;
        }
        deriv
    }

    fn max_step(&self, store: &ParamStore, dir: &HashMap<ParamId, f64>, lim: f64) -> f64 {
        let mut lim = lim;
        if let Some(&d) = dir.get(&self.distance) {
            if d < 0.0 {
                lim = lim.min(-store.get(self.distance) / d);
            }
        }
        let mut ddx = 0.0;
        let mut ddy = 0.0;
        if let Some(&v) = dir.get(&self.p1.x) {
            ddx += v;
        }
        if let Some(&v) = dir.get(&self.p1.y) {
            ddy += v;
        }
        if let Some(&v) = dir.get(&self.p2.x) {
            ddx -= v;
        }
        if let Some(&v) = dir.get(&self.p2.y) {
            ddy -= v;
        }
        let dd = (ddx * ddx + ddy * ddy).sqrt();
        let dist = store.get(self.distance);
        if dd > dist {
            let dx = store.get(self.p1.x) - store.get(self.p2.x);
            let dy = store.get(self.p1.y) - store.get(self.p2.y);
            let d = (dx * dx + dy * dy).sqrt();
            if dd > d {
                lim = lim.min(d.max(dist) / dd);
            }
        }
        lim
    }
}

/// Signed distance from point `p` to line `l`, matching `distance` in sign
/// per `ccw` (counterclockwise winding puts the point on the positive side).
pub struct P2LDistance {
    pub p: Point,
    pub l: Line,
    pub distance: ParamId,
    pub ccw: bool,
}

impl P2LDistance {
    pub fn new(p: Point, l: Line, distance: ParamId, ccw: bool) -> Self {
        Self { p, l, distance, ccw }
    }

    fn signed_value(&self, store: &ParamStore) -> f64 {
        let (x0, y0) = (store.get(self.p.x), store.get(self.p.y));
        let (x1, y1) = (store.get(self.l.p1.x), store.get(self.l.p1.y));
        let (x2, y2) = (store.get(self.l.p2.x), store.get(self.l.p2.y));
        let dx = x2 - x1;
        let dy = y2 - y1;
        let d = (dx * dx + dy * dy).sqrt();
        let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;
        area / d
    }
}

impl Constraint for P2LDistance {
    fn params(&self) -> Vec<ParamId> {
        vec![self.p.x, self.p.y, self.l.p1.x, self.l.p1.y, self.l.p2.x, self.l.p2.y, self.distance]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dist = if self.ccw { store.get(self.distance).abs() } else { -store.get(self.distance).abs() };
        self.signed_value(store) - dist
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        let (p0x, p0y) = (self.p.x, self.p.y);
        let (p1x, p1y) = (self.l.p1.x, self.l.p1.y);
        let (p2x, p2y) = (self.l.p2.x, self.l.p2.y);
        if [p0x, p0y, p1x, p1y, p2x, p2y].contains(&param) {
            let (x0, y0) = (store.get(p0x), store.get(p0y));
            let (x1, y1) = (store.get(p1x), store.get(p1y));
            let (x2, y2) = (store.get(p2x), store.get(p2y));
            let dx = x2 - x1;
            let dy = y2 - y1;
            let d2 = dx * dx + dy * dy;
            let d = d2.sqrt();
            let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;

            if param == p0x {
                deriv += (y1 - y2) / d;
            }
            if param == p0y {
                deriv += (x2 - x1) / d;
            }
            if param == p1x {
                deriv += ((y2 - y0) * d + (dx / d) * area) / d2;
            }
            if param == p1y {
                deriv += ((x0 - x2) * d + (dy / d) * area) / d2;
            }
            if param == p2x {
                deriv += ((y0 - y1) * d - (dx / d) * area) / d2;
            }
            if param == p2y {
                deriv += ((x1 - x0) * d - (dy / d) * area) / d2;
            }
        }
        if param == self.distance {
            deriv += if self.ccw { -1.0 } else { 1.0 };
        }
        deriv
    }

    fn max_step(&self, store: &ParamStore, dir: &HashMap<ParamId, f64>, lim: f64) -> f64 {
        let mut lim = lim;
        if let Some(&d) = dir.get(&self.distance) {
            if d < 0.0 {
                lim = lim.min(-store.get(self.distance) / d);
            }
        }
        let (x0, y0) = (store.get(self.p.x), store.get(self.p.y));
        let (x1, y1) = (store.get(self.l.p1.x), store.get(self.l.p1.y));
        let (x2, y2) = (store.get(self.l.p2.x), store.get(self.l.p2.y));
        let mut darea = 0.0;
        if let Some(&v) = dir.get(&self.p.x) {
            darea += (y1 - y2) * v;
        }
        if let Some(&v) = dir.get(&self.p.y) {
            darea += (x2 - x1) * v;
        }
        if let Some(&v) = dir.get(&self.l.p1.x) {
            darea += (y2 - y0) * v;
        }
        if let Some(&v) = dir.get(&self.l.p1.y) {
            darea += (x0 - x2) * v;
        }
        if let Some(&v) = dir.get(&self.l.p2.x) {
            darea += (y0 - y1) * v;
        }
        if let Some(&v) = dir.get(&self.l.p2.y) {
            darea += (x1 - x0) * v;
        }
        darea = darea.abs();
        if darea > 0.0 {
            let dx = x2 - x1;
            let dy = y2 - y1;
            let mut area = 0.3 * store.get(self.distance) * (dx * dx + dy * dy).sqrt();
            if darea > area {
                area = area.max(0.3 * (-x0 * dy + y0 * dx + x1 * y2 - x2 * y1).abs());
                if darea > area {
                    lim = lim.min(area / darea);
                }
            }
        }
        lim
    }
}

/// Point `p` lies on the (infinite) line through `l`.
pub struct PointOnLine {
    pub p: Point,
    pub l: Line,
}

impl PointOnLine {
    pub fn new(p: Point, l: Line) -> Self {
        Self { p, l }
    }
}

impl Constraint for PointOnLine {
    fn params(&self) -> Vec<ParamId> {
        vec![self.p.x, self.p.y, self.l.p1.x, self.l.p1.y, self.l.p2.x, self.l.p2.y]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let (x0, y0) = (store.get(self.p.x), store.get(self.p.y));
        let (x1, y1) = (store.get(self.l.p1.x), store.get(self.l.p1.y));
        let (x2, y2) = (store.get(self.l.p2.x), store.get(self.l.p2.y));
        let dx = x2 - x1;
        let dy = y2 - y1;
        let d = (dx * dx + dy * dy).sqrt();
        let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;
        area / d
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (p0x, p0y) = (self.p.x, self.p.y);
        let (p1x, p1y) = (self.l.p1.x, self.l.p1.y);
        let (p2x, p2y) = (self.l.p2.x, self.l.p2.y);
        if ![p0x, p0y, p1x, p1y, p2x, p2y].contains(&param) {
            return 0.0;
        }
        let (x0, y0) = (store.get(p0x), store.get(p0y));
        let (x1, y1) = (store.get(p1x), store.get(p1y));
        let (x2, y2) = (store.get(p2x), store.get(p2y));
        let dx = x2 - x1;
        let dy = y2 - y1;
        let d2 = dx * dx + dy * dy;
        let d = d2.sqrt();
        let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;
        let mut deriv = 0.0;
        if param == p0x {
            deriv += (y1 - y2) / d;
        }
        if param == p0y {
            deriv += (x2 - x1) / d;
        }
        if param == p1x {
            deriv += ((y2 - y0) * d + (dx / d) * area) / d2;
        }
        if param == p1y {
            deriv += ((x0 - x2) * d + (dy / d) * area) / d2;
        }
        if param == p2x {
            deriv += ((y0 - y1) * d - (dx / d) * area) / d2;
        }
        if param == p2y {
            deriv += ((x1 - x0) * d - (dy / d) * area) / d2;
        }
        deriv
    }
}

/// Point `p` is equidistant from `l`'s two endpoints (on the perpendicular
/// bisector of the segment `l`). Ported from the combined `errorgrad` the
/// C++ uses here (`ConstraintPointOnPerpBisector::errorgrad`), rather than
/// separate `error`/`grad` overrides.
pub struct PointOnPerpBisector {
    pub p: Point,
    pub l: Line,
}

impl PointOnPerpBisector {
    pub fn new(p: Point, l: Line) -> Self {
        Self { p, l }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        use crate::geo::DeriVector2;
        let p0 = DeriVector2::from_point(store, self.p, derivparam);
        let p1 = DeriVector2::from_point(store, self.l.p1, derivparam);
        let p2 = DeriVector2::from_point(store, self.l.p2, derivparam);

        let d1 = p0.subtr(&p1);
        let d2 = p0.subtr(&p2);
        let d = p2.subtr(&p1).normalized();

        let (projd1, dprojd1) = d1.scalar_prod(&d);
        let (projd2, dprojd2) = d2.scalar_prod(&d);

        (projd1 + projd2, dprojd1 + dprojd2)
    }
}

impl Constraint for PointOnPerpBisector {
    fn params(&self) -> Vec<ParamId> {
        vec![self.p.x, self.p.y, self.l.p1.x, self.l.p1.y, self.l.p2.x, self.l.p2.y]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        self.error_grad(store, None).0
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        self.error_grad(store, Some(param)).1
    }
}

/// Lines `l1`/`l2` have the same direction (their direction vectors' cross
/// product is zero). `scale` is fixed at construction to
/// `1/sqrt(|l1|^2 * |l2|^2)`, matching planegcs's `rescale()` for this type.
pub struct Parallel {
    pub l1: Line,
    pub l2: Line,
    scale: f64,
}

impl Parallel {
    pub fn new(store: &ParamStore, l1: Line, l2: Line) -> Self {
        let scale = Self::compute_scale(store, &l1, &l2);
        Self { l1, l2, scale }
    }

    fn compute_scale(store: &ParamStore, l1: &Line, l2: &Line) -> f64 {
        let dx1 = store.get(l1.p1.x) - store.get(l1.p2.x);
        let dy1 = store.get(l1.p1.y) - store.get(l1.p2.y);
        let dx2 = store.get(l2.p1.x) - store.get(l2.p2.x);
        let dy2 = store.get(l2.p1.y) - store.get(l2.p2.y);
        1.0 / ((dx1 * dx1 + dy1 * dy1) * (dx2 * dx2 + dy2 * dy2)).sqrt()
    }
}

impl Constraint for Parallel {
    fn params(&self) -> Vec<ParamId> {
        vec![self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y, self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y]
    }

    fn scale(&self) -> f64 {
        self.scale
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dx1 = store.get(self.l1.p1.x) - store.get(self.l1.p2.x);
        let dy1 = store.get(self.l1.p1.y) - store.get(self.l1.p2.y);
        let dx2 = store.get(self.l2.p1.x) - store.get(self.l2.p2.x);
        let dy2 = store.get(self.l2.p1.y) - store.get(self.l2.p2.y);
        dx1 * dy2 - dy1 * dx2
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (l1p1x, l1p1y, l1p2x, l1p2y) = (self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y);
        let (l2p1x, l2p1y, l2p2x, l2p2y) = (self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y);
        let dx1 = store.get(l1p1x) - store.get(l1p2x);
        let dy1 = store.get(l1p1y) - store.get(l1p2y);
        let dx2 = store.get(l2p1x) - store.get(l2p2x);
        let dy2 = store.get(l2p1y) - store.get(l2p2y);
        let mut deriv = 0.0;
        if param == l1p1x {
            deriv += dy2;
        }
        if param == l1p2x {
            deriv += -dy2;
        }
        if param == l1p1y {
            deriv += -dx2;
        }
        if param == l1p2y {
            deriv += dx2;
        }
        if param == l2p1x {
            deriv += -dy1;
        }
        if param == l2p2x {
            deriv += dy1;
        }
        if param == l2p1y {
            deriv += dx1;
        }
        if param == l2p2y {
            deriv += -dx1;
        }
        deriv
    }
}

/// Lines `l1`/`l2` are at right angles (their direction vectors' dot product
/// is zero). Same scale treatment as [`Parallel`].
pub struct Perpendicular {
    pub l1: Line,
    pub l2: Line,
    scale: f64,
}

impl Perpendicular {
    pub fn new(store: &ParamStore, l1: Line, l2: Line) -> Self {
        let scale = Parallel::compute_scale(store, &l1, &l2);
        Self { l1, l2, scale }
    }
}

impl Constraint for Perpendicular {
    fn params(&self) -> Vec<ParamId> {
        vec![self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y, self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y]
    }

    fn scale(&self) -> f64 {
        self.scale
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dx1 = store.get(self.l1.p1.x) - store.get(self.l1.p2.x);
        let dy1 = store.get(self.l1.p1.y) - store.get(self.l1.p2.y);
        let dx2 = store.get(self.l2.p1.x) - store.get(self.l2.p2.x);
        let dy2 = store.get(self.l2.p1.y) - store.get(self.l2.p2.y);
        dx1 * dx2 + dy1 * dy2
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (l1p1x, l1p1y, l1p2x, l1p2y) = (self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y);
        let (l2p1x, l2p1y, l2p2x, l2p2y) = (self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y);
        let dx1 = store.get(l1p1x) - store.get(l1p2x);
        let dy1 = store.get(l1p1y) - store.get(l1p2y);
        let dx2 = store.get(l2p1x) - store.get(l2p2x);
        let dy2 = store.get(l2p1y) - store.get(l2p2y);
        let mut deriv = 0.0;
        if param == l1p1x {
            deriv += dx2;
        }
        if param == l1p2x {
            deriv += -dx2;
        }
        if param == l1p1y {
            deriv += dy2;
        }
        if param == l1p2y {
            deriv += -dy2;
        }
        if param == l2p1x {
            deriv += dx1;
        }
        if param == l2p2x {
            deriv += -dx1;
        }
        if param == l2p1y {
            deriv += dy1;
        }
        if param == l2p2y {
            deriv += -dy1;
        }
        deriv
    }
}

/// The midpoint of `l1` lies on the (infinite) line through `l2`.
pub struct MidpointOnLine {
    pub l1: Line,
    pub l2: Line,
}

impl MidpointOnLine {
    pub fn new(l1: Line, l2: Line) -> Self {
        Self { l1, l2 }
    }
}

impl Constraint for MidpointOnLine {
    fn params(&self) -> Vec<ParamId> {
        vec![self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y, self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let x0 = (store.get(self.l1.p1.x) + store.get(self.l1.p2.x)) / 2.0;
        let y0 = (store.get(self.l1.p1.y) + store.get(self.l1.p2.y)) / 2.0;
        let (x1, y1) = (store.get(self.l2.p1.x), store.get(self.l2.p1.y));
        let (x2, y2) = (store.get(self.l2.p2.x), store.get(self.l2.p2.y));
        let dx = x2 - x1;
        let dy = y2 - y1;
        let d = (dx * dx + dy * dy).sqrt();
        let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;
        area / d
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (l1p1x, l1p1y, l1p2x, l1p2y) = (self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y);
        let (l2p1x, l2p1y, l2p2x, l2p2y) = (self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y);
        if ![l1p1x, l1p1y, l1p2x, l1p2y, l2p1x, l2p1y, l2p2x, l2p2y].contains(&param) {
            return 0.0;
        }
        let x0 = (store.get(l1p1x) + store.get(l1p2x)) / 2.0;
        let y0 = (store.get(l1p1y) + store.get(l1p2y)) / 2.0;
        let (x1, y1) = (store.get(l2p1x), store.get(l2p1y));
        let (x2, y2) = (store.get(l2p2x), store.get(l2p2y));
        let dx = x2 - x1;
        let dy = y2 - y1;
        let d2 = dx * dx + dy * dy;
        let d = d2.sqrt();
        let area = -x0 * dy + y0 * dx + x1 * y2 - x2 * y1;
        let mut deriv = 0.0;
        if param == l1p1x {
            deriv += (y1 - y2) / (2.0 * d);
        }
        if param == l1p1y {
            deriv += (x2 - x1) / (2.0 * d);
        }
        if param == l1p2x {
            deriv += (y1 - y2) / (2.0 * d);
        }
        if param == l1p2y {
            deriv += (x2 - x1) / (2.0 * d);
        }
        if param == l2p1x {
            deriv += ((y2 - y0) * d + (dx / d) * area) / d2;
        }
        if param == l2p1y {
            deriv += ((x0 - x2) * d + (dy / d) * area) / d2;
        }
        if param == l2p2x {
            deriv += ((y0 - y1) * d - (dx / d) * area) / d2;
        }
        if param == l2p2y {
            deriv += ((x1 - x0) * d - (dy / d) * area) / d2;
        }
        deriv
    }
}

/// `l1` and `l2` have equal length. Ported from the combined `errorgrad`
/// the C++ uses here (`ConstraintEqualLineLength::errorgrad`), including its
/// surrogate-gradient fallback: when both lines are axis-aligned the same
/// way, the true gradient underflows toward zero and the solver's diagnosis
/// pass would misread that as "fully constrained" rather than "at a
/// horizontal/vertical extremum" — so a tiny signed surrogate derivative is
/// substituted whenever the true one drops below `1e-10`.
pub struct EqualLineLength {
    pub l1: Line,
    pub l2: Line,
}

impl EqualLineLength {
    pub fn new(l1: Line, l2: Line) -> Self {
        Self { l1, l2 }
    }

    fn error_grad(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        use crate::geo::DeriVector2;
        let p1 = DeriVector2::from_point(store, self.l1.p1, derivparam);
        let p2 = DeriVector2::from_point(store, self.l1.p2, derivparam);
        let p3 = DeriVector2::from_point(store, self.l2.p1, derivparam);
        let p4 = DeriVector2::from_point(store, self.l2.p2, derivparam);

        let v1 = p1.subtr(&p2);
        let v2 = p3.subtr(&p4);

        let (length1, dlength1) = v1.length_deriv();
        let (length2, dlength2) = v2.length_deriv();

        let err = length2 - length1;
        let mut grad = dlength2 - dlength1;

        if grad.abs() < 1e-10 {
            const SURROGATE: f64 = 1e-10;
            if let Some(param) = derivparam {
                if param == self.l1.p1.x {
                    grad = if v1.x > 0.0 { SURROGATE } else { -SURROGATE };
                }
                if param == self.l1.p1.y {
                    grad = if v1.y > 0.0 { SURROGATE } else { -SURROGATE };
                }
                if param == self.l1.p2.x {
                    grad = if v1.x > 0.0 { -SURROGATE } else { SURROGATE };
                }
                if param == self.l1.p2.y {
                    grad = if v1.y > 0.0 { -SURROGATE } else { SURROGATE };
                }
                if param == self.l2.p1.x {
                    grad = if v2.x > 0.0 { SURROGATE } else { -SURROGATE };
                }
                if param == self.l2.p1.y {
                    grad = if v2.y > 0.0 { SURROGATE } else { -SURROGATE };
                }
                if param == self.l2.p2.x {
                    grad = if v2.x > 0.0 { -SURROGATE } else { SURROGATE };
                }
                if param == self.l2.p2.y {
                    grad = if v2.y > 0.0 { -SURROGATE } else { SURROGATE };
                }
            }
        }

        (err, grad)
    }
}

impl Constraint for EqualLineLength {
    fn params(&self) -> Vec<ParamId> {
        vec![self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y, self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y]
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
    use crate::constraints::test_support::assert_grad_matches_finite_difference;

    fn line(store: &mut ParamStore, x1: f64, y1: f64, x2: f64, y2: f64) -> Line {
        Line {
            p1: Point::new(store.add(x1, false), store.add(y1, false)),
            p2: Point::new(store.add(x2, false), store.add(y2, false)),
        }
    }

    fn point(store: &mut ParamStore, x: f64, y: f64) -> Point {
        Point::new(store.add(x, false), store.add(y, false))
    }

    #[test]
    fn equal_is_zero_when_params_match_and_gradient_checks_out_at_ratio_one() {
        let mut store = ParamStore::new();
        let a = store.add(3.0, false);
        let b = store.add(3.0, false);
        let c = Equal::new(a, b, 1.0);
        assert!(c.error_value(&store).abs() < 1e-12);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn difference_matches_hand_computation() {
        let mut store = ParamStore::new();
        let a = store.add(5.0, false);
        let b = store.add(8.0, false);
        let d = store.add(3.0, false);
        let c = Difference::new(a, b, d);
        assert!(c.error_value(&store).abs() < 1e-12); // 8 - 5 == 3
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn center_of_gravity_matches_hand_computation() {
        let mut store = ParamStore::new();
        let p1 = store.add(0.0, false);
        let p2 = store.add(10.0, false);
        let center = store.add(5.0, false); // midpoint == center of gravity at equal weights
        let c = CenterOfGravity::new(center, vec![p1, p2], vec![0.5, 0.5]);
        assert!(c.error_value(&store).abs() < 1e-12);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn weighted_linear_combination_matches_hand_computation() {
        let mut store = ParamStore::new();
        let pole0 = store.add(0.0, false);
        let pole1 = store.add(10.0, false);
        let w0 = store.add(1.0, false);
        let w1 = store.add(1.0, false);
        // factors [0.5, 0.5] both weight 1 -> point should equal (0*1*0.5 + 10*1*0.5) / (1*0.5+1*0.5) = 5
        let point_param = store.add(5.0, false);
        let c = WeightedLinearCombination::new(point_param, vec![pole0, pole1], vec![w0, w1], vec![0.5, 0.5]);
        assert!(c.error_value(&store).abs() < 1e-12);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn p2p_distance_matches_pythagoras() {
        let mut store = ParamStore::new();
        let p1 = point(&mut store, 0.0, 0.0);
        let p2 = point(&mut store, 3.0, 4.0);
        let d = store.add(5.0, false);
        let c = P2PDistance::new(p1, p2, d);
        assert!(c.error_value(&store).abs() < 1e-12);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn p2l_distance_matches_hand_computation() {
        let mut store = ParamStore::new();
        let p = point(&mut store, 0.0, 5.0);
        let l = line(&mut store, -10.0, 0.0, 10.0, 0.0); // the x axis
        let d = store.add(5.0, false);
        let c = P2LDistance::new(p, l, d, true);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn point_on_line_is_zero_for_a_collinear_point() {
        let mut store = ParamStore::new();
        let p = point(&mut store, 5.0, 5.0);
        let l = line(&mut store, 0.0, 0.0, 10.0, 10.0);
        let c = PointOnLine::new(p, l);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn point_on_perp_bisector_is_zero_for_an_equidistant_point() {
        let mut store = ParamStore::new();
        let p = point(&mut store, 5.0, 100.0); // equidistant from (0,0) and (10,0)
        let l = line(&mut store, 0.0, 0.0, 10.0, 0.0);
        let c = PointOnPerpBisector::new(p, l);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn parallel_is_zero_for_parallel_lines_and_gradient_checks_out() {
        let mut store = ParamStore::new();
        let l1 = line(&mut store, 0.0, 0.0, 10.0, 5.0);
        let l2 = line(&mut store, 1.0, 1.0, 11.0, 6.0);
        let c = Parallel::new(&store, l1, l2);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn perpendicular_is_zero_for_perpendicular_lines_and_gradient_checks_out() {
        let mut store = ParamStore::new();
        let l1 = line(&mut store, 0.0, 0.0, 10.0, 0.0);
        let l2 = line(&mut store, 0.0, 0.0, 0.0, 10.0);
        let c = Perpendicular::new(&store, l1, l2);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn midpoint_on_line_is_zero_when_satisfied() {
        let mut store = ParamStore::new();
        let l1 = line(&mut store, 0.0, 10.0, 10.0, 10.0); // midpoint (5, 10)
        let l2 = line(&mut store, 5.0, 0.0, 5.0, 20.0); // vertical line through x=5
        let c = MidpointOnLine::new(l1, l2);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn equal_line_length_is_zero_for_equal_length_lines() {
        let mut store = ParamStore::new();
        let l1 = line(&mut store, 0.0, 0.0, 3.0, 4.0); // length 5
        let l2 = line(&mut store, 0.0, 0.0, 5.0, 0.0); // length 5
        let c = EqualLineLength::new(l1, l2);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }
}
