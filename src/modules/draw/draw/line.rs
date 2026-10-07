// Line tool — ribbon definition + interactive command.
//
// Command:  LINE — OpenCADStudio behaviour:
//   1. First click  → stores start point, prompts for next point
//   2. Each further click → immediately commits an codec::Line entity
//      (start→end) to the document; end becomes the new start point
//   3. Enter / Escape → ends the command

use codec::types::Vector3;
use codec::{EntityType, Line};
use crate::t;

use crate::command::{CadCommand, CmdResult, TangentObject};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::{DVec2, DVec3};

// ── Ribbon definition ─────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LINE",
        label: "Line",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("LINE".to_string()),
    }
}

// ── Command implementation ────────────────────────────────────────────────

pub struct LineCommand {
    /// Every point picked so far. `points[0]` is the start (needed by Close);
    /// `points.last()` is the start of the next segment. Each pick after the
    /// first commits one Line entity, so the count of committed segments is
    /// `points.len() - 1`.
    points: Vec<DVec3>,
    /// A tangent reference recorded on the FIRST point (the circle + the
    /// approximate cursor hit used to pick among solutions). A tangent picked
    /// first is deferred — the tangent point isn't fixed until the next point
    /// gives a direction — and resolved on that next pick as either a common
    /// tangent of two circles or a tangent from a point to the circle. (#274)
    deferred_tangent: Option<(TangentObject, DVec3)>,
}

impl LineCommand {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            deferred_tangent: None,
        }
    }

    fn line_between(a: DVec3, b: DVec3) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a.x, a.y, a.z),
            Vector3::new(b.x, b.y, b.z),
        ))
    }
}

impl CadCommand for LineCommand {
    fn name(&self) -> &'static str {
        "LINE"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => t!("LINE  Specify first point:").into_owned(),
            1 => t!("LINE  Specify next point  [Undo]:").into_owned(),
            _ => t!("LINE  Specify next point  [Close/Undo]:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        // Covers the LINE prompt of commercial solutions: Undo once a point is placed, Close
        // once a closing segment is possible, eXit always.
        match self.points.len() {
            0 => Vec::new(),
            1 => vec![CmdOption::new("Undo", "U"), CmdOption::new("eXit", "X")],
            _ => vec![
                CmdOption::new("Close", "C"),
                CmdOption::new("Undo", "U"),
                CmdOption::new("eXit", "X"),
            ],
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(&last) = self.points.last() {
            let line = Self::line_between(last, pt);
            self.points.push(pt);
            CmdResult::CommitEntity(line)
        } else {
            self.points.push(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_point_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Option<CmdResult> {
        match (self.points.last().copied(), self.deferred_tangent, tangent) {
            // (A) The FIRST point is tangent to a circle or ellipse: defer it.
            // The tangent point depends on the line's direction, which the next pick gives.
            // Store a provisional point so the rubber band has an origin.
            (None, _, Some(target @ (TangentObject::Circle { .. } | TangentObject::Ellipse { .. }))) => {
                self.points.push(pt);
                self.deferred_tangent = Some((target, pt));
                Some(CmdResult::NeedPoint)
            }
            // (B) A deferred first tangent AND this point is also tangent to a curve (circle or ellipse):
            // The line is the common tangent of the two curves.
            (
                Some(_),
                Some((target1, hit1)),
                Some(target2 @ (TangentObject::Circle { .. } | TangentObject::Ellipse { .. })),
            ) => {
                self.deferred_tangent = None;
                let (t1, t2) = common_curve_tangents(target1, target2)
                    .into_iter()
                    .min_by(|a, b| {
                        let da = (a.0 - hit1).length_squared() + (a.1 - pt).length_squared();
                        let db = (b.0 - hit1).length_squared() + (b.1 - pt).length_squared();
                        da.total_cmp(&db)
                    })
                    .unwrap_or((self.points[0], pt));
                self.points = vec![t1, t2];
                Some(CmdResult::CommitEntity(Self::line_between(t1, t2)))
            }
            // (C) A deferred first tangent, but this point is NOT a curve tangent pick:
            // The line runs from the tangent point on the deferred curve to this point.
            (Some(_), Some((target1, hit1)), _) => {
                self.deferred_tangent = None;
                let start = point_target_tangents(pt, target1)
                    .map(|(t0, t1)| {
                        if (t0 - hit1).length_squared() <= (t1 - hit1).length_squared() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(self.points[0]);
                self.points = vec![start, pt];
                Some(CmdResult::CommitEntity(Self::line_between(start, pt)))
            }
            // (D) First point was already placed (ordinary point or previous segment end),
            // and this pick IS tangent to a circle or ellipse:
            (Some(last), None, Some(target @ (TangentObject::Circle { .. } | TangentObject::Ellipse { .. }))) => {
                let end = point_target_tangents(last, target)
                    .map(|(t0, t1)| {
                        if (t0 - pt).length_squared() <= (t1 - pt).length_squared() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(pt);
                let line = Self::line_between(last, end);
                self.points.push(end);
                Some(CmdResult::CommitEntity(line))
            }
            // No deferred tangent (or a Line tangent): fall back to the plain point path.
            _ => None,
        }
    }

    fn resolved_anchor(&self) -> Option<DVec3> {
        self.points.last().copied()
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.points.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.deferred_tangent = None;
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        // Accept Close / Undo once at least the first point is placed.
        !self.points.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // The next-point pick also takes C / U, so the polar dynamic-input
        // distance/angle boxes stay visible while the keywords are available.
        !self.points.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "C" | "CLOSE" => {
                // Need at least two points to draw a closing segment back to
                // the start; then finish the command.
                if self.points.len() >= 2 {
                    let close = Self::line_between(
                        *self.points.last().unwrap(),
                        self.points[0],
                    );
                    Some(CmdResult::CommitAndExit(close))
                } else {
                    Some(CmdResult::NeedPoint)
                }
            }
            // eXit: end the command keeping every segment drawn so far
            // (the third LINE keyword of commercial solutions).
            "X" | "EXIT" => Some(CmdResult::Cancel),
            "U" | "UNDO" => {
                if self.points.len() >= 2 {
                    // Drop the last vertex and revert its committed segment.
                    self.points.pop();
                    Some(CmdResult::UndoDocument)
                } else if self.points.len() == 1 {
                    // Only the start is placed (nothing committed yet) — clear
                    // it so the next pick restarts the line.
                    self.points.clear();
                    self.deferred_tangent = None;
                    Some(CmdResult::NeedPoint)
                } else {
                    Some(CmdResult::NeedPoint)
                }
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        // Ctrl+Z while drawing = the U option: revert the last committed
        // segment and keep drawing from the previous point. With nothing
        // placed yet the document undo takes over.
        if self.points.is_empty() {
            return None;
        }
        self.on_text_input("U")
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.on_preview_wires_with_tangent(pt, None).into_iter().next()
    }

    fn on_preview_wires_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Vec<WireModel> {
        let last = match self.points.last() {
            Some(&p) => p,
            None => return Vec::new(),
        };

        let (start, end) = match (self.deferred_tangent, tangent) {
            (Some((target1, hit1)), Some(target2 @ (TangentObject::Circle { .. } | TangentObject::Ellipse { .. }))) => {
                common_curve_tangents(target1, target2)
                    .into_iter()
                    .min_by(|a, b| {
                        let da = (a.0 - hit1).length_squared() + (a.1 - pt).length_squared();
                        let db = (b.0 - hit1).length_squared() + (b.1 - pt).length_squared();
                        da.total_cmp(&db)
                    })
                    .unwrap_or((last, pt))
            }
            (Some((target1, hit1)), _) => {
                let start = point_target_tangents(pt, target1)
                    .map(|(t0, t1)| {
                        if (t0 - hit1).length_squared() <= (t1 - hit1).length_squared() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(last);
                (start, pt)
            }
            (None, Some(target @ (TangentObject::Circle { .. } | TangentObject::Ellipse { .. }))) => {
                let end = point_target_tangents(last, target)
                    .map(|(t0, t1)| {
                        if (t0 - pt).length_squared() <= (t1 - pt).length_squared() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(pt);
                (last, end)
            }
            _ => (last, pt),
        };

        vec![WireModel::solid_f64(
            "rubber_band".to_string(),
            vec![[start.x, start.y, start.z], [end.x, end.y, end.z]],
            WireModel::CYAN,
            false,
        )]
    }
}

// ── Tangent geometry (f64) ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
struct Curve3D {
    center: DVec3,
    axis_a: DVec3,
    axis_b: DVec3,
}

impl Curve3D {
    fn from_tangent_object(obj: TangentObject) -> Option<Self> {
        match obj {
            TangentObject::Circle { center, radius } => {
                if radius < 1e-9 {
                    return None;
                }
                Some(Self {
                    center,
                    axis_a: DVec3::new(radius, 0.0, 0.0),
                    axis_b: DVec3::new(0.0, radius, 0.0),
                })
            }
            TangentObject::Ellipse {
                center,
                major_axis,
                normal,
                minor_axis_ratio,
            } => {
                let a = major_axis.length();
                if a < 1e-9 {
                    return None;
                }
                let mut n = normal.normalize_or_zero();
                if n.length_squared() < 0.5 {
                    n = DVec3::Z;
                }
                let u = major_axis / a;
                let v = n.cross(u);
                let b = a * minor_axis_ratio;
                Some(Self {
                    center,
                    axis_a: major_axis,
                    axis_b: v * b,
                })
            }
            TangentObject::Line { .. } => None,
        }
    }
}

/// The two points on a curve (Circle or Ellipse) where a line from external point `p`
/// touches tangentially, or `None` when `p` is inside or on the curve.
fn point_target_tangents(p: DVec3, target: TangentObject) -> Option<(DVec3, DVec3)> {
    let curve = Curve3D::from_tangent_object(target)?;
    let d = p - curve.center;
    let a = curve.axis_a.length();
    let b = curve.axis_b.length();
    if a < 1e-9 || b < 1e-9 {
        return None;
    }
    let u_dir = curve.axis_a / a;
    let v_dir = curve.axis_b / b;

    let u0 = d.dot(u_dir);
    let v0 = d.dot(v_dir);
    let u_norm = u0 / a;
    let v_norm = v0 / b;
    let dist = (u_norm * u_norm + v_norm * v_norm).sqrt();
    if dist <= 1.0 + 1e-9 {
        return None;
    }

    let theta0 = v_norm.atan2(u_norm);
    let delta = (1.0 / dist).acos();
    let t0 = theta0 + delta;
    let t1 = theta0 - delta;

    let pt0 = curve.center + curve.axis_a * t0.cos() + curve.axis_b * t0.sin();
    let pt1 = curve.center + curve.axis_a * t1.cos() + curve.axis_b * t1.sin();
    Some((pt0, pt1))
}

#[cfg(test)]
fn point_circle_tangents(p: DVec3, center: DVec3, radius: f64) -> Option<(DVec3, DVec3)> {
    point_target_tangents(p, TangentObject::Circle { center, radius })
}

/// Common tangent lines between any two curves (Circle-Circle, Circle-Ellipse, Ellipse-Ellipse).
/// Each tangent is returned as (tangent point on curve 1, tangent point on curve 2).
fn common_curve_tangents(target1: TangentObject, target2: TangentObject) -> Vec<(DVec3, DVec3)> {
    let c1 = match Curve3D::from_tangent_object(target1) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let c2 = match Curve3D::from_tangent_object(target2) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let c1_2d = DVec2::new(c1.center.x, c1.center.y);
    let a1_2d = DVec2::new(c1.axis_a.x, c1.axis_a.y);
    let b1_2d = DVec2::new(c1.axis_b.x, c1.axis_b.y);

    let c2_2d = DVec2::new(c2.center.x, c2.center.y);
    let a2_2d = DVec2::new(c2.axis_a.x, c2.axis_a.y);
    let b2_2d = DVec2::new(c2.axis_b.x, c2.axis_b.y);

    let delta_c = c2_2d - c1_2d;
    if delta_c.length_squared() < 1e-9 {
        return Vec::new();
    }

    let eval = |theta: f64, is_int: bool| -> (f64, DVec2, DVec2) {
        let n = DVec2::new(theta.cos(), theta.sin());
        let su1 = n.dot(a1_2d);
        let sv1 = n.dot(b1_2d);
        let r1 = (su1 * su1 + sv1 * sv1).sqrt().max(1e-9);

        let su2 = n.dot(a2_2d);
        let sv2 = n.dot(b2_2d);
        let r2 = (su2 * su2 + sv2 * sv2).sqrt().max(1e-9);

        let f = if is_int {
            n.dot(delta_c) - r2 - r1
        } else {
            n.dot(delta_c) + r2 - r1
        };

        let t1 = c1_2d + (a1_2d * su1 + b1_2d * sv1) / r1;
        let t2 = if is_int {
            c2_2d - (a2_2d * su2 + b2_2d * sv2) / r2
        } else {
            c2_2d + (a2_2d * su2 + b2_2d * sv2) / r2
        };
        (f, t1, t2)
    };

    let mut results = Vec::new();
    let n_steps = 72;
    let dt = std::f64::consts::TAU / (n_steps as f64);

    for &is_int in &[false, true] {
        for i in 0..n_steps {
            let t_a = i as f64 * dt;
            let t_b = (i + 1) as f64 * dt;
            let (fa, _, _) = eval(t_a, is_int);
            let (fb, _, _) = eval(t_b, is_int);

            let mut root = None;
            if fa.abs() < 1e-12 {
                root = Some(t_a);
            } else if fa.signum() != fb.signum() {
                let mut lo = t_a;
                let mut hi = t_b;
                let mut f_lo = fa;
                let mut best_t = 0.5 * (lo + hi);
                for _ in 0..28 {
                    let mid = 0.5 * (lo + hi);
                    let (fm, _, _) = eval(mid, is_int);
                    best_t = mid;
                    if fm.abs() < 1e-13 {
                        break;
                    }
                    if fm.signum() == f_lo.signum() {
                        lo = mid;
                        f_lo = fm;
                    } else {
                        hi = mid;
                    }
                }
                root = Some(best_t);
            }

            if let Some(r_angle) = root {
                let (f_final, t1_2d, t2_2d) = eval(r_angle, is_int);
                if f_final.abs() < 1e-6 {
                    let t1 = DVec3::new(t1_2d.x, t1_2d.y, c1.center.z);
                    let t2 = DVec3::new(t2_2d.x, t2_2d.y, c2.center.z);
                    if !results.iter().any(|(p1, p2): &(DVec3, DVec3)| {
                        (*p1 - t1).length_squared() < 1e-6 && (*p2 - t2).length_squared() < 1e-6
                    }) {
                        results.push((t1, t2));
                    }
                }
            }
        }
    }
    results
}

#[cfg(test)]
fn circle_circle_tangents(c1: DVec3, r1: f64, c2: DVec3, r2: f64) -> Vec<(DVec3, DVec3)> {
    common_curve_tangents(
        TangentObject::Circle { center: c1, radius: r1 },
        TangentObject::Circle { center: c2, radius: r2 },
    )
}

#[cfg(test)]
mod tangent_tests {
    use super::*;

    fn perp_dot(a: DVec3, b: DVec3) -> f64 {
        a.x * b.x + a.y * b.y
    }

    #[test]
    fn point_circle_tangents_are_perpendicular_to_the_radius() {
        let c = DVec3::new(0.0, 0.0, 0.0);
        let p = DVec3::new(10.0, 0.0, 0.0);
        let (t0, t1) = point_circle_tangents(p, c, 5.0).unwrap();
        for t in [t0, t1] {
            assert!(((t - c).length() - 5.0).abs() < 1e-9, "off circle");
            assert!(perp_dot(t - c, t - p).abs() < 1e-9, "radius not ⊥ line");
        }
        // A point inside the circle has no tangent.
        assert!(point_circle_tangents(DVec3::new(1.0, 0.0, 0.0), c, 5.0).is_none());
    }

    #[test]
    fn separated_circles_have_four_common_tangents() {
        let c1 = DVec3::new(0.0, 0.0, 0.0);
        let c2 = DVec3::new(4.0, 0.0, 0.0);
        let tans = circle_circle_tangents(c1, 1.0, c2, 1.0);
        assert_eq!(tans.len(), 4, "2 external + 2 internal");
        for (t1, t2) in &tans {
            assert!(((*t1 - c1).length() - 1.0).abs() < 1e-9, "t1 off circle 1");
            assert!(((*t2 - c2).length() - 1.0).abs() < 1e-9, "t2 off circle 2");
            // The tangent line t1→t2 is perpendicular to each radius.
            let line = *t2 - *t1;
            assert!(perp_dot(*t1 - c1, line).abs() < 1e-6, "not tangent at c1 end");
            assert!(perp_dot(*t2 - c2, line).abs() < 1e-6, "not tangent at c2 end");
        }
    }

    #[test]
    fn overlapping_circles_have_only_external_tangents() {
        let tans = circle_circle_tangents(DVec3::ZERO, 2.0, DVec3::new(1.5, 0.0, 0.0), 2.0);
        assert_eq!(tans.len(), 2, "overlap → the 2 internal tangents vanish");
    }

    #[test]
    fn point_ellipse_tangents_are_tangent_to_ellipse() {
        let e = TangentObject::Ellipse {
            center: DVec3::ZERO,
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5, // minor radius = 10
        };
        let p = DVec3::new(20.0, 20.0, 0.0);
        let (t0, t1) = point_target_tangents(p, e).expect("point is outside ellipse");

        for t in [t0, t1] {
            // Must lie on ellipse (x/20)^2 + (y/10)^2 = 1
            let on_ell = ((t.x / 20.0).powi(2) + (t.y / 10.0).powi(2) - 1.0).abs();
            assert!(on_ell < 1e-6, "point {t:?} not on ellipse: {on_ell}");

            // The line (p - t) must be perpendicular to the ellipse normal at t.
            // For (x/a)^2 + (y/b)^2 = 1, gradient/normal is (x/a^2, y/b^2).
            let grad = DVec3::new(t.x / (20.0 * 20.0), t.y / (10.0 * 10.0), 0.0);
            let line_dir = p - t;
            let dot = grad.dot(line_dir);
            assert!(dot.abs() < 1e-6, "line is not tangent to ellipse: dot={dot}");
        }

        // A point inside the ellipse has no tangents
        assert!(point_target_tangents(DVec3::new(5.0, 5.0, 0.0), e).is_none());
    }

    #[test]
    fn separated_ellipse_and_circle_have_four_common_tangents() {
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
        };

        let tans = common_curve_tangents(e1, c2);
        assert_eq!(tans.len(), 4, "separated ellipse and circle should have 4 common tangents");

        for (t1, t2) in &tans {
            // t1 on ellipse
            let dx1 = t1.x - (-30.0);
            let dy1 = t1.y;
            let on_e1 = ((dx1 / 20.0).powi(2) + (dy1 / 10.0).powi(2) - 1.0).abs();
            assert!(on_e1 < 1e-4, "t1 not on ellipse: {on_e1}");

            // t2 on circle
            let on_c2 = ((*t2 - DVec3::new(30.0, 0.0, 0.0)).length() - 10.0).abs();
            assert!(on_c2 < 1e-4, "t2 not on circle: {on_c2}");

            // Line t1->t2 is tangent to ellipse at t1
            let grad_e = DVec3::new(dx1 / 400.0, dy1 / 100.0, 0.0);
            let line_dir = *t2 - *t1;
            assert!(grad_e.dot(line_dir).abs() < 1e-4, "line not tangent to ellipse");

            // Line t1->t2 is tangent to circle at t2 (perpendicular to radius)
            let rad_c = *t2 - DVec3::new(30.0, 0.0, 0.0);
            assert!(rad_c.dot(line_dir).abs() < 1e-4, "line not tangent to circle");
        }
    }

    #[test]
    fn separated_two_ellipses_have_four_common_tangents() {
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-35.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let e2 = TangentObject::Ellipse {
            center: DVec3::new(35.0, 0.0, 0.0),
            major_axis: DVec3::new(15.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.6,
        };

        let tans = common_curve_tangents(e1, e2);
        assert_eq!(tans.len(), 4, "separated two ellipses should have 4 common tangents");

        for (t1, t2) in &tans {
            let dx1 = t1.x - (-35.0);
            let dy1 = t1.y;
            let on_e1 = ((dx1 / 20.0).powi(2) + (dy1 / 10.0).powi(2) - 1.0).abs();
            assert!(on_e1 < 1e-4, "t1 not on ellipse 1: {on_e1}");

            let dx2 = t2.x - 35.0;
            let dy2 = t2.y;
            let on_e2 = ((dx2 / 15.0).powi(2) + (dy2 / 9.0).powi(2) - 1.0).abs();
            assert!(on_e2 < 1e-4, "t2 not on ellipse 2: {on_e2}");

            let line_dir = *t2 - *t1;
            let grad1 = DVec3::new(dx1 / 400.0, dy1 / 100.0, 0.0);
            assert!(grad1.dot(line_dir).abs() < 1e-4, "line not tangent to ellipse 1");

            let grad2 = DVec3::new(dx2 / 225.0, dy2 / 81.0, 0.0);
            assert!(grad2.dot(line_dir).abs() < 1e-4, "line not tangent to ellipse 2");
        }
    }

    #[test]
    fn line_command_tangent_to_ellipse_workflow() {
        let mut cmd = LineCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
        };

        // Pick 1: Tangent on Ellipse 1 (top vertex area)
        let r1 = cmd.on_point_with_tangent(DVec3::new(-30.0, 10.0, 0.0), Some(e1));
        assert!(matches!(r1, Some(CmdResult::NeedPoint)));

        // Live preview with tangent on circle 2
        let preview = cmd.on_preview_wires_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        assert_eq!(preview.len(), 1);

        // Pick 2: Tangent on Circle 2
        let r2 = cmd.on_point_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        assert!(matches!(r2, Some(CmdResult::CommitEntity(_))));

        // Start a new line: Pick 1 as normal point, Pick 2 as tangent to ellipse
        let mut cmd2 = LineCommand::new();
        cmd2.on_point(DVec3::new(0.0, 30.0, 0.0));
        let r_second = cmd2.on_point_with_tangent(DVec3::new(-30.0, 10.0, 0.0), Some(e1));
        assert!(matches!(r_second, Some(CmdResult::CommitEntity(_))));
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["LINE"] });  // LineCommand
