// Line tool — ribbon definition + interactive command.
//
// Command:  LINE — OpenCADStudio behaviour:
//   1. First click  → stores start point, prompts for next point
//   2. Each further click → immediately commits an acadrust::Line entity
//      (start→end) to the document; end becomes the new start point
//   3. Enter / Escape → ends the command

use acadrust::types::Vector3;
use acadrust::{EntityType, Line};
use crate::t;

use crate::command::{CadCommand, CmdResult, TangentObject};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

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
        use TangentObject::Circle;
        match (self.points.last().copied(), self.deferred_tangent, tangent) {
            // (A) The FIRST point is tangent to a circle: defer it. The tangent
            // point depends on the line's direction, which the next pick gives.
            // Store a provisional point so the rubber band has an origin.
            (None, _, Some(Circle { center, radius })) => {
                self.points.push(pt);
                self.deferred_tangent = Some((Circle { center, radius }, pt));
                Some(CmdResult::NeedPoint)
            }
            // (B) A deferred first tangent AND this point is also tangent to a
            // circle: the line is the common tangent of the two circles. Pick
            // the one of the (up to four) whose endpoints sit nearest the two
            // cursor hits, so the user chooses inner/outer + side by where they
            // hover.
            (
                Some(_),
                Some((Circle { center: c1, radius: r1 }, hit1)),
                Some(Circle { center: c2, radius: r2 }),
            ) => {
                self.deferred_tangent = None;
                let (t1, t2) = circle_circle_tangents(c1, r1, c2, r2)
                    .into_iter()
                    .min_by(|a, b| {
                        let da = (a.0 - hit1).length() + (a.1 - pt).length();
                        let db = (b.0 - hit1).length() + (b.1 - pt).length();
                        da.total_cmp(&db)
                    })
                    .unwrap_or((self.points[0], pt));
                self.points = vec![t1, t2];
                Some(CmdResult::CommitEntity(Self::line_between(t1, t2)))
            }
            // (C) A deferred first tangent, but this point is NOT a circle
            // tangent: the line runs from this point, tangent to the deferred
            // circle (the nearer of the two tangent points to the first hit).
            (Some(_), Some((Circle { center: c1, radius: r1 }, hit1)), _) => {
                self.deferred_tangent = None;
                let start = point_circle_tangents(pt, c1, r1)
                    .map(|(t0, t1)| {
                        if (t0 - hit1).length() <= (t1 - hit1).length() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(self.points[0]);
                self.points = vec![start, pt];
                Some(CmdResult::CommitEntity(Self::line_between(start, pt)))
            }
            // No deferred tangent (or a Line tangent, which needs no special
            // handling — a line is tangent to itself everywhere): fall back to
            // the plain point path.
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
        let last = *self.points.last()?;
        // With a deferred first tangent, slide the rubber band's start point
        // around the circle so it stays tangent toward the moving cursor,
        // instead of sticking at the provisional pick point. Keep the side the
        // user first picked (nearest that hit) for a stable preview. (#274)
        let start = match self.deferred_tangent {
            Some((TangentObject::Circle { center, radius }, hit)) => {
                point_circle_tangents(pt, center, radius)
                    .map(|(t0, t1)| {
                        if (t0 - hit).length() <= (t1 - hit).length() {
                            t0
                        } else {
                            t1
                        }
                    })
                    .unwrap_or(last)
            }
            _ => last,
        };
        Some(WireModel::solid_f64(
            "rubber_band".to_string(),
            vec![[start.x, start.y, start.z], [pt.x, pt.y, pt.z]],
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Tangent geometry (f64) ────────────────────────────────────────────────

/// The two points on a circle (`center`, `radius`, XY plane) where a line from
/// external point `p` touches tangentially, or `None` when `p` is inside/on the
/// circle. Each tangent point's radius is perpendicular to the line from `p`,
/// so it sits at the half-angle `acos(radius / |center→p|)` either side of the
/// center→p direction. (#274)
fn point_circle_tangents(p: DVec3, center: DVec3, radius: f64) -> Option<(DVec3, DVec3)> {
    let vx = p.x - center.x;
    let vy = p.y - center.y;
    let d = (vx * vx + vy * vy).sqrt();
    if d <= radius + 1e-9 {
        return None;
    }
    let base = vy.atan2(vx);
    let off = (radius / d).acos();
    let at =
        |a: f64| DVec3::new(center.x + radius * a.cos(), center.y + radius * a.sin(), center.z);
    Some((at(base + off), at(base - off)))
}

/// Up to four common tangent lines of two circles (XY plane), each returned as
/// (tangent point on circle 1, tangent point on circle 2): two external (both
/// radii point the same way) and two internal (opposite ways, present only when
/// the circles are separated). For a unit normal `n` to the tangent line,
/// `n·(c2−c1)` equals `r2−r1` (external) or `−(r1+r2)` (internal); each tangent
/// point is its centre offset by `∓r·n`. (#274)
fn circle_circle_tangents(c1: DVec3, r1: f64, c2: DVec3, r2: f64) -> Vec<(DVec3, DVec3)> {
    let dx = c2.x - c1.x;
    let dy = c2.y - c1.y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1e-9 {
        return Vec::new(); // concentric: no common tangent
    }
    let (ux, uy) = (dx / dist, dy / dist); // centre→centre unit
    let (px, py) = (-uy, ux); // its perpendicular
    let mut out = Vec::new();
    for (num, internal) in [(r2 - r1, false), (-(r1 + r2), true)] {
        let k = num / dist;
        if k.abs() > 1.0 {
            continue; // circles too close for this tangent family
        }
        let s = (1.0 - k * k).max(0.0).sqrt();
        for pm in [1.0_f64, -1.0] {
            let nx = k * ux + pm * s * px;
            let ny = k * uy + pm * s * py;
            let t1 = DVec3::new(c1.x - r1 * nx, c1.y - r1 * ny, c1.z);
            let t2 = if internal {
                DVec3::new(c2.x + r2 * nx, c2.y + r2 * ny, c2.z)
            } else {
                DVec3::new(c2.x - r2 * nx, c2.y - r2 * ny, c2.z)
            };
            out.push((t1, t2));
        }
    }
    out
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
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["LINE"] });  // LineCommand
