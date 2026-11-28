// Line tool — ribbon definition + interactive command.
//
// Command:  LINE — OpenCADStudio behaviour:
//   1. First click  → stores start point, prompts for next point
//   2. Each further click → immediately commits an document::Line entity
//      (start→end) to the document; end becomes the new start point
//   3. Enter / Escape → ends the command

use crate::t;
use document::types::Vector3;
use document::{EntityType, Line};

use crate::command::{CadCommand, CmdResult, DeferredPick, TangentObject};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::snap::deferred::{
    point_in_direction, resolve_deferred_line, DeferredCurve, DeferredMode, LineEnd,
};
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
    /// A deferred TAN/PER reference recorded on the FIRST point. The real
    /// contact point is not fixed until the next point gives a direction;
    /// the stored `at` chooses among solutions. (#274)
    deferred: Option<DeferredPick>,
}

impl Default for LineCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl LineCommand {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            deferred: None,
        }
    }

    /// Compatibility conversion for the older tangent-only hook.
    fn deferred_from_tangent(target: TangentObject, at: DVec3) -> Option<DeferredPick> {
        let curve = match target {
            TangentObject::Circle { center, radius } => DeferredCurve::Circle {
                center,
                axis_x: DVec3::X,
                axis_y: DVec3::Y,
                radius,
            },
            TangentObject::Ellipse {
                center,
                major_axis,
                normal,
                minor_axis_ratio,
            } => DeferredCurve::Ellipse {
                center,
                major_axis,
                normal,
                minor_axis_ratio,
                start: 0.0,
                end: 0.0,
            },
            TangentObject::Line { .. } => return None,
        };
        Some(DeferredPick {
            mode: DeferredMode::Tangent,
            curve,
            at,
        })
    }

    fn line_between(a: DVec3, b: DVec3) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a.x, a.y, a.z),
            Vector3::new(b.x, b.y, b.z),
        ))
    }
    /// Resolve the two ends and commit the line. Picked coordinates are
    /// replaced by the solver's exact contact points; a zero-length result
    /// is refused.
    fn commit_pair(&mut self, first: LineEnd, other: LineEnd) -> CmdResult {
        let fallback = |end: LineEnd| match end {
            LineEnd::Point(p) => p,
            LineEnd::Deferred(d) => d.at,
        };
        let (a, b) = resolve_deferred_line(first, other)
            .unwrap_or((fallback(first), fallback(other)));
        if !a.is_finite() || !b.is_finite() || (a - b).length_squared() <= 1e-18 {
            return CmdResult::NeedPoint;
        }
        self.deferred = None;
        self.points = vec![a, b];
        CmdResult::CommitEntity(Self::line_between(a, b))
    }

    /// Resolve the stored deferred end against `other`.
    fn commit_deferred(&mut self, other: LineEnd) -> CmdResult {
        let Some(first) = self.deferred.take() else {
            return CmdResult::NeedPoint;
        };
        let result = self.commit_pair(LineEnd::Deferred(first), other);
        if matches!(result, CmdResult::NeedPoint) {
            self.points = vec![first.at];
            self.deferred = Some(first);
        }
        result
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
        if self.deferred.is_some() {
            return self.commit_deferred(LineEnd::Point(pt));
        }
        if let Some(&last) = self.points.last() {
            if (last - pt).length_squared() <= 1e-18 {
                return CmdResult::NeedPoint;
            }
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
        let deferred = tangent.and_then(|target| Self::deferred_from_tangent(target, pt));
        self.on_point_with_deferred(pt, tangent, deferred)
    }

    fn on_point_with_deferred(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
        deferred: Option<DeferredPick>,
    ) -> Option<CmdResult> {
        let pick = deferred.or_else(|| tangent.and_then(|t| Self::deferred_from_tangent(t, pt)))?;
        if self.deferred.is_none() && self.points.is_empty() {
            // Provisional origin for the rubber band; the real contact point
            // is computed once the next point is known.
            self.points.push(pick.at);
            self.deferred = Some(pick);
            return Some(CmdResult::NeedPoint);
        }
        if self.deferred.is_some() {
            return Some(self.commit_deferred(LineEnd::Deferred(pick)));
        }
        // A tangent pick with an ordinary base point (the classic T from a
        // placed point): resolve only the picked end.
        let last = self.points.last().copied().unwrap_or(pick.at);
        Some(self.commit_pair(LineEnd::Point(last), LineEnd::Deferred(pick)))
    }

    /// Typed `@d<a` / direct distance while a deferred pick is pending. The
    /// direction is authoritative: resolve the contact point from it rather
    /// than silently projecting the provisional point.
    fn on_typed_point(
        &mut self,
        pt: DVec3,
        dir: Option<DVec3>,
        dist: Option<f64>,
    ) -> Option<CmdResult> {
        let Some(pick) = self.deferred else {
            return None;
        };
        let Some(dir) = dir else {
            return None;
        };
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 0.5 {
            return None;
        }
        let distance = dist.filter(|d| d.is_finite() && *d > 0.0)?;
        let start = match pick.mode {
            DeferredMode::Tangent => point_in_direction(&pick.curve, pick.mode, dir, pick.at),
            DeferredMode::Perpendicular => match pick.curve {
                DeferredCurve::Line { p1, p2 } => {
                    let along = p2 - p1;
                    let along = along.normalize_or_zero();
                    if along.length_squared() < 0.5 || along.dot(dir).abs() > 1e-6 {
                        // A line typed in a non-perpendicular direction has no
                        // perpendicular solution; refuse instead of shortening it.
                        return Some(CmdResult::NeedPoint);
                    }
                    Some(p1 + along * (pick.at - p1).dot(along))
                }
                _ => point_in_direction(&pick.curve, pick.mode, dir, pick.at),
            },
        };
        let Some(start) = start else {
            return Some(CmdResult::NeedPoint);
        };
        let end = start + dir * distance;
        if !start.is_finite() || !end.is_finite() || (end - start).length_squared() <= 1e-18 {
            return Some(CmdResult::NeedPoint);
        }
        self.deferred = None;
        self.points = vec![start, end];
        // Keep the typed endpoint authoritative for the next segment while
        // leaving the chosen contact point as the committed start.
        let _ = pt;
        Some(CmdResult::CommitEntity(Self::line_between(start, end)))
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
        self.deferred = None;
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
                    let close = Self::line_between(*self.points.last().unwrap(), self.points[0]);
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
                    self.deferred = None;
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
        self.on_preview_wires_with_tangent(pt, None)
            .into_iter()
            .next()
    }

    fn on_preview_wires_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Vec<WireModel> {
        let deferred = tangent.and_then(|target| Self::deferred_from_tangent(target, pt));
        self.on_preview_wires_with_deferred(pt, tangent, deferred)
    }

    fn on_preview_wires_with_deferred(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
        deferred: Option<DeferredPick>,
    ) -> Vec<WireModel> {
        let last = match self.points.last() {
            Some(&p) => p,
            None => return Vec::new(),
        };
        let pick = deferred.or_else(|| tangent.and_then(|t| Self::deferred_from_tangent(t, pt)));
        let solution = match (self.deferred, pick) {
            (Some(first), Some(second)) => resolve_deferred_line(
                LineEnd::Deferred(first),
                LineEnd::Deferred(second),
            ),
            (Some(first), None) => resolve_deferred_line(LineEnd::Deferred(first), LineEnd::Point(pt)),
            (None, Some(second)) => resolve_deferred_line(LineEnd::Point(last), LineEnd::Deferred(second)),
            (None, None) => None,
        };
        let (start, end) = solution.unwrap_or((last, pt));

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
#[cfg(test)]
struct Curve3D {
    center: DVec3,
    axis_a: DVec3,
    axis_b: DVec3,
}

#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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
        TangentObject::Circle {
            center: c1,
            radius: r1,
        },
        TangentObject::Circle {
            center: c2,
            radius: r2,
        },
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
            assert!(
                perp_dot(*t1 - c1, line).abs() < 1e-6,
                "not tangent at c1 end"
            );
            assert!(
                perp_dot(*t2 - c2, line).abs() < 1e-6,
                "not tangent at c2 end"
            );
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
            assert!(
                dot.abs() < 1e-6,
                "line is not tangent to ellipse: dot={dot}"
            );
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
        assert_eq!(
            tans.len(),
            4,
            "separated ellipse and circle should have 4 common tangents"
        );

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
            assert!(
                grad_e.dot(line_dir).abs() < 1e-4,
                "line not tangent to ellipse"
            );

            // Line t1->t2 is tangent to circle at t2 (perpendicular to radius)
            let rad_c = *t2 - DVec3::new(30.0, 0.0, 0.0);
            assert!(
                rad_c.dot(line_dir).abs() < 1e-4,
                "line not tangent to circle"
            );
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
        assert_eq!(
            tans.len(),
            4,
            "separated two ellipses should have 4 common tangents"
        );

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
            assert!(
                grad1.dot(line_dir).abs() < 1e-4,
                "line not tangent to ellipse 1"
            );

            let grad2 = DVec3::new(dx2 / 225.0, dy2 / 81.0, 0.0);
            assert!(
                grad2.dot(line_dir).abs() < 1e-4,
                "line not tangent to ellipse 2"
            );
        }
    }

    fn deferred_per(curve: DeferredCurve, at: DVec3) -> DeferredPick {
        DeferredPick {
            mode: DeferredMode::Perpendicular,
            curve,
            at,
        }
    }

    fn committed_line(result: CmdResult) -> (DVec3, DVec3) {
        match result {
            CmdResult::CommitEntity(EntityType::Line(line)) => (
                DVec3::new(line.start.x, line.start.y, line.start.z),
                DVec3::new(line.end.x, line.end.y, line.end.z),
            ),
            _ => panic!("expected a committed line"),
        }
    }

    #[test]
    fn deferred_perpendicular_first_point_commits_a_perpendicular_line() {
        let mut cmd = LineCommand::new();
        let target = DeferredCurve::Line {
            p1: DVec3::new(0.0, 0.0, 0.0),
            p2: DVec3::new(10.0, 0.0, 0.0),
        };
        let pick = deferred_per(target, DVec3::new(5.0, 0.0, 0.0));
        let first = cmd.on_point_with_deferred(pick.at, None, Some(pick));
        assert!(matches!(first, Some(CmdResult::NeedPoint)));

        let preview = cmd.on_preview_wires_with_deferred(DVec3::new(5.0, 8.0, 0.0), None, None);
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].points.len(), 2);
        let (a, b) = (preview[0].points[0], preview[0].points[1]);
        assert!((a[0] - 5.0).abs() < 1e-9 && (b[1] - 8.0).abs() < 1e-9);

        let result = cmd.on_point(DVec3::new(5.0, 8.0, 0.0));
        let (a, b) = committed_line(result);
        assert!((a - DVec3::new(5.0, 0.0, 0.0)).length() < 1e-9, "{a:?}");
        assert!((b - DVec3::new(5.0, 8.0, 0.0)).length() < 1e-9, "{b:?}");
    }

    #[test]
    fn typed_polar_after_deferred_tangent_uses_the_typed_direction() {
        let mut cmd = LineCommand::new();
        let circle = TangentObject::Circle {
            center: DVec3::ZERO,
            radius: 5.0,
        };
        let pick = LineCommand::deferred_from_tangent(circle, DVec3::new(0.0, 5.0, 0.0)).unwrap();
        let first = cmd.on_point_with_deferred(pick.at, None, Some(pick));
        assert!(matches!(first, Some(CmdResult::NeedPoint)));

        // `@10<0` from the provisional point: the true tangent point is the
        // top of the circle and the line runs from there to (10, 5).
        let typed = cmd.on_typed_point(
            DVec3::new(10.0, 5.0, 0.0),
            Some(DVec3::X),
            Some(10.0),
        );
        let (a, b) = committed_line(typed.expect("typed pick commits"));
        assert!((a - DVec3::new(0.0, 5.0, 0.0)).length() < 1e-9, "{a:?}");
        assert!((b - DVec3::new(10.0, 5.0, 0.0)).length() < 1e-9, "{b:?}");
    }

    #[test]
    fn typed_polar_after_deferred_per_refuses_a_non_perpendicular_direction() {
        let mut cmd = LineCommand::new();
        let pick = deferred_per(
            DeferredCurve::Line {
                p1: DVec3::new(0.0, 0.0, 0.0),
                p2: DVec3::new(10.0, 0.0, 0.0),
            },
            DVec3::new(5.0, 0.0, 0.0),
        );
        cmd.on_point_with_deferred(pick.at, None, Some(pick));
        let refused = cmd.on_typed_point(
            DVec3::new(15.0, 0.0, 0.0),
            Some(DVec3::X),
            Some(10.0),
        );
        assert!(matches!(refused, Some(CmdResult::NeedPoint)));
        assert!(cmd.deferred.is_some(), "the deferred pick survives a refusal");
    }

    #[test]
    fn zero_length_deferred_result_is_not_committed() {
        let mut cmd = LineCommand::new();
        let pick = deferred_per(
            DeferredCurve::Line {
                p1: DVec3::new(0.0, 0.0, 0.0),
                p2: DVec3::new(10.0, 0.0, 0.0),
            },
            DVec3::new(5.0, 0.0, 0.0),
        );
        cmd.on_point_with_deferred(pick.at, None, Some(pick));
        // A point already on the target line has a degenerate perpendicular.
        let result = cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        assert!(matches!(result, CmdResult::NeedPoint));
        assert!(cmd.deferred.is_some(), "pick is retained for a fresh attempt");
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
inventory::submit!(crate::command::CommandRegistration { names: &["LINE"] }); // LineCommand
