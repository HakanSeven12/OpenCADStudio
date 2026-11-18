//! Angle constraints (port stage 5a).
//!
//! Ported from `ConstraintP2PAngle`/`ConstraintL2LAngle` in
//! `Constraints.h`/`Constraints.cpp`. See `point_line.rs`'s module doc for
//! what's intentionally not ported (`evaluate()`, post-construction
//! `rescale()`).

use std::collections::HashMap;

use crate::constraints::Constraint;
use crate::geo::{Line, Point};
use crate::util::{ParamId, ParamStore};

const PI_18: f64 = std::f64::consts::PI / 18.0;

/// The angle from `p1` to `p2`, measured against a reference frame rotated
/// by `angle + da`, is zero — i.e. the segment `p1->p2` points along that
/// rotated frame's x-axis. `da` is a fixed offset baked in at construction
/// (planegcs uses it for the "reversed" variant of this constraint).
pub struct P2PAngle {
    pub p1: Point,
    pub p2: Point,
    pub angle: ParamId,
    pub da: f64,
}

impl P2PAngle {
    pub fn new(p1: Point, p2: Point, angle: ParamId, da: f64) -> Self {
        Self { p1, p2, angle, da }
    }
}

impl Constraint for P2PAngle {
    fn params(&self) -> Vec<ParamId> {
        vec![self.p1.x, self.p1.y, self.p2.x, self.p2.y, self.angle]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dx = store.get(self.p2.x) - store.get(self.p1.x);
        let dy = store.get(self.p2.y) - store.get(self.p1.y);
        let a = store.get(self.angle) + self.da;
        let (sa, ca) = a.sin_cos();
        let x = dx * ca + dy * sa;
        let y = -dx * sa + dy * ca;
        y.atan2(x)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let mut deriv = 0.0;
        if [self.p1.x, self.p1.y, self.p2.x, self.p2.y].contains(&param) {
            let dx0 = store.get(self.p2.x) - store.get(self.p1.x);
            let dy0 = store.get(self.p2.y) - store.get(self.p1.y);
            let a = store.get(self.angle) + self.da;
            let (sa, ca) = a.sin_cos();
            let x = dx0 * ca + dy0 * sa;
            let y = -dx0 * sa + dy0 * ca;
            let r2 = dx0 * dx0 + dy0 * dy0;
            let dx = -y / r2;
            let dy = x / r2;
            if param == self.p1.x {
                deriv += -ca * dx + sa * dy;
            }
            if param == self.p1.y {
                deriv += -sa * dx - ca * dy;
            }
            if param == self.p2.x {
                deriv += ca * dx - sa * dy;
            }
            if param == self.p2.y {
                deriv += sa * dx + ca * dy;
            }
        }
        if param == self.angle {
            deriv += -1.0;
        }
        deriv
    }

    fn max_step(&self, _store: &ParamStore, dir: &HashMap<ParamId, f64>, lim: f64) -> f64 {
        let mut lim = lim;
        if let Some(&step) = dir.get(&self.angle) {
            let step = step.abs();
            if step > PI_18 {
                lim = lim.min(PI_18 / step);
            }
        }
        lim
    }
}

/// The angle from line `l1`'s direction to line `l2`'s direction, offset by
/// `angle`, is zero.
pub struct L2LAngle {
    pub l1: Line,
    pub l2: Line,
    pub angle: ParamId,
}

impl L2LAngle {
    pub fn new(l1: Line, l2: Line, angle: ParamId) -> Self {
        Self { l1, l2, angle }
    }
}

impl Constraint for L2LAngle {
    fn params(&self) -> Vec<ParamId> {
        vec![
            self.l1.p1.x,
            self.l1.p1.y,
            self.l1.p2.x,
            self.l1.p2.y,
            self.l2.p1.x,
            self.l2.p1.y,
            self.l2.p2.x,
            self.l2.p2.y,
            self.angle,
        ]
    }

    fn error_value(&self, store: &ParamStore) -> f64 {
        let dx1 = store.get(self.l1.p2.x) - store.get(self.l1.p1.x);
        let dy1 = store.get(self.l1.p2.y) - store.get(self.l1.p1.y);
        let dx2 = store.get(self.l2.p2.x) - store.get(self.l2.p1.x);
        let dy2 = store.get(self.l2.p2.y) - store.get(self.l2.p1.y);
        let a = dy1.atan2(dx1) + store.get(self.angle);
        let (sa, ca) = a.sin_cos();
        let x2 = dx2 * ca + dy2 * sa;
        let y2 = -dx2 * sa + dy2 * ca;
        y2.atan2(x2)
    }

    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64 {
        let (l1p1x, l1p1y, l1p2x, l1p2y) = (self.l1.p1.x, self.l1.p1.y, self.l1.p2.x, self.l1.p2.y);
        let (l2p1x, l2p1y, l2p2x, l2p2y) = (self.l2.p1.x, self.l2.p1.y, self.l2.p2.x, self.l2.p2.y);
        let mut deriv = 0.0;

        if [l1p1x, l1p1y, l1p2x, l1p2y].contains(&param) {
            let dx1 = store.get(l1p2x) - store.get(l1p1x);
            let dy1 = store.get(l1p2y) - store.get(l1p1y);
            let r2 = dx1 * dx1 + dy1 * dy1;
            if param == l1p1x {
                deriv += -dy1 / r2;
            }
            if param == l1p1y {
                deriv += dx1 / r2;
            }
            if param == l1p2x {
                deriv += dy1 / r2;
            }
            if param == l1p2y {
                deriv += -dx1 / r2;
            }
        }
        if [l2p1x, l2p1y, l2p2x, l2p2y].contains(&param) {
            let dx1 = store.get(l1p2x) - store.get(l1p1x);
            let dy1 = store.get(l1p2y) - store.get(l1p1y);
            let dx2_0 = store.get(l2p2x) - store.get(l2p1x);
            let dy2_0 = store.get(l2p2y) - store.get(l2p1y);
            let a = dy1.atan2(dx1) + store.get(self.angle);
            let (sa, ca) = a.sin_cos();
            let x2 = dx2_0 * ca + dy2_0 * sa;
            let y2 = -dx2_0 * sa + dy2_0 * ca;
            let r2 = dx2_0 * dx2_0 + dy2_0 * dy2_0;
            let dx2 = -y2 / r2;
            let dy2 = x2 / r2;
            if param == l2p1x {
                deriv += -ca * dx2 + sa * dy2;
            }
            if param == l2p1y {
                deriv += -sa * dx2 - ca * dy2;
            }
            if param == l2p2x {
                deriv += ca * dx2 - sa * dy2;
            }
            if param == l2p2y {
                deriv += sa * dx2 + ca * dy2;
            }
        }
        if param == self.angle {
            deriv += -1.0;
        }
        deriv
    }

    fn max_step(&self, _store: &ParamStore, dir: &HashMap<ParamId, f64>, lim: f64) -> f64 {
        let mut lim = lim;
        if let Some(&step) = dir.get(&self.angle) {
            let step = step.abs();
            if step > PI_18 {
                lim = lim.min(PI_18 / step);
            }
        }
        lim
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
    fn p2p_angle_is_zero_when_segment_matches_the_target_angle() {
        let mut store = ParamStore::new();
        let p1 = point(&mut store, 0.0, 0.0);
        let p2 = point(&mut store, 10.0, 10.0); // 45 degrees
        let angle = store.add(std::f64::consts::FRAC_PI_4, false);
        let c = P2PAngle::new(p1, p2, angle, 0.0);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }

    #[test]
    fn l2l_angle_is_zero_when_relative_angle_matches() {
        let mut store = ParamStore::new();
        let l1 = line(&mut store, 0.0, 0.0, 10.0, 0.0); // along +x
        let l2 = line(&mut store, 0.0, 0.0, 0.0, 10.0); // along +y, 90 degrees from l1
        let angle = store.add(std::f64::consts::FRAC_PI_2, false);
        let c = L2LAngle::new(l1, l2, angle);
        assert!(c.error_value(&store).abs() < 1e-9);
        assert_grad_matches_finite_difference(&c, &mut store);
    }
}
