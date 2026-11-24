//! CREATEPOINTMANUAL: COGO points one at a time, each asked for its
//! location, description and elevation and written as soon as it is
//! complete; Enter at the location ends the command, Esc drops only the
//! point in progress. The whole run is one undo step.
//!
//! The location can come from a direction mode typed at its prompt (or
//! started by the menu): `'NE` northing and easting, `'PBD` bearing and
//! distance, `'ZD` azimuth and distance, `'AD` angle and distance and `'DD`
//! deflection and distance. A mode stays on for the next points (each from
//! the last one) until Enter at its first prompt ('NE: Esc) hands the
//! location prompt back.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use glam::DVec3;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Manual,
    NorthingEasting,
    Bearing,
    Azimuth,
    Angle,
    Deflection,
}

impl Mode {
    fn of(word: &str) -> Option<Mode> {
        Some(match word.trim().trim_start_matches('\'').to_ascii_uppercase().as_str() {
            "NE" => Mode::NorthingEasting,
            "PBD" => Mode::Bearing,
            "ZD" => Mode::Azimuth,
            "AD" => Mode::Angle,
            "DD" => Mode::Deflection,
            _ => return None,
        })
    }
}

enum Step {
    Location,
    Northing,
    Easting(f64),
    /// The starting point of a bearing, azimuth, angle or deflection run.
    Start,
    /// Angle and deflection: a line, or `Points`.
    Line,
    End,
    Quadrant,
    Bearing(u8),
    Azimuth,
    Angle,
    /// Distance along the direction (degrees, measured from north for a
    /// bearing or azimuth, from east for an angle or deflection).
    Distance(f64),
    Description(DVec3),
    Elevation(DVec3, Option<String>),
}

pub struct CreatePointManualCommand {
    step: Step,
    mode: Mode,
    /// The description the next point offers: the last one given, none
    /// (".") at first.
    description: Option<String>,
    /// Northing and easting the 'NE prompts offer.
    ne: (f64, f64),
    /// Where the next direction run starts: the last point written.
    from: Option<DVec3>,
    /// The other end of the angle or deflection's reference line.
    to: Option<DVec3>,
    counter_clockwise: bool,
    /// A line picked at the line prompt (its two ends).
    picked: Option<(DVec3, DVec3)>,
}

const RESUMING: &str = "Resuming CREATEPOINTMANUAL command.";
const QUADRANTS: &str = "Quadrants - NE = 1, SE = 2, SW = 3, NW = 4";
const DIRECTION_UNIT: &str = "Current direction unit: degree, Input: DD° MM' SS.SS\" (spaced)";
const ANGLE_UNIT: &str = "Current angle unit: degree, Input: decimal";

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

/// A direction typed as degrees, minutes and seconds, spaced ("45 30 15.5").
fn dms(text: &str) -> Option<f64> {
    let parts: Vec<f64> = text.split_whitespace().map(|p| p.parse::<f64>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
    match parts[..] {
        [d] => Some(d),
        [d, m] => Some(d + m / 60.0),
        [d, m, s] => Some(d + m / 60.0 + s / 3600.0),
        _ => None,
    }
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

impl CreatePointManualCommand {
    pub fn new() -> Self {
        Self::with_mode(Mode::Manual)
    }

    /// The command started by a menu entry in a direction mode.
    pub fn with_mode(mode: Mode) -> Self {
        let mut c = Self { step: Step::Location, mode: Mode::Manual, description: None, ne: (0.0, 0.0), from: None, to: None, counter_clockwise: false, picked: None };
        c.enter_mode(mode);
        c
    }

    fn enter_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.step = match mode {
            Mode::Manual => Step::Location,
            Mode::NorthingEasting => Step::Northing,
            Mode::Bearing | Mode::Azimuth => Step::Start,
            Mode::Angle | Mode::Deflection => Step::Line,
        };
    }

    /// The first prompt of the mode's next point, chained from the last one.
    fn next_location(&mut self) {
        self.step = match self.mode {
            Mode::Manual => Step::Location,
            Mode::NorthingEasting => Step::Northing,
            Mode::Bearing if self.from.is_some() => Step::Quadrant,
            Mode::Azimuth if self.from.is_some() => Step::Azimuth,
            Mode::Angle | Mode::Deflection if self.from.is_some() => Step::End,
            _ => {
                self.enter_mode(self.mode);
                return;
            }
        };
    }

    /// Leaves the direction mode: the plain location prompt again.
    fn resume(&mut self) -> CmdResult {
        self.mode = Mode::Manual;
        self.step = Step::Location;
        CmdResult::ReportMessage(t(RESUMING))
    }

    /// The location found: the point's description is asked next.
    fn located(&mut self, at: DVec3) -> CmdResult {
        self.step = Step::Description(DVec3::new(at.x, at.y, 0.0));
        if self.mode == Mode::Manual {
            CmdResult::NeedPoint
        } else {
            CmdResult::ReportMessage(t(RESUMING))
        }
    }

    fn shown(d: &Option<String>) -> &str {
        d.as_deref().unwrap_or(".")
    }

    /// The description typed at its prompt: Enter keeps the offered one,
    /// "." gives none.
    fn description_of(&self, typed: &str) -> Option<String> {
        match typed.trim() {
            "" => self.description.clone(),
            "." => None,
            t => Some(t.to_string()),
        }
    }

    /// The point `distance` along the direction (`degrees`; from north,
    /// clockwise, for bearings and azimuths).
    fn travel(&self, degrees: f64, distance: f64) -> Option<DVec3> {
        let from = self.from?;
        let (dx, dy) = match self.mode {
            Mode::Bearing | Mode::Azimuth => {
                let a = degrees.to_radians();
                (a.sin(), a.cos())
            }
            _ => {
                // Angle: turned from the start→end direction; deflection: from
                // the line's extension back through the start. Clockwise unless
                // Counter-clockwise was chosen.
                let to = self.to?;
                let base = if self.mode == Mode::Deflection { from - to } else { to - from };
                let turn = if self.counter_clockwise { degrees } else { -degrees };
                let a = base.y.atan2(base.x) + turn.to_radians();
                (a.cos(), a.sin())
            }
        };
        Some(from + DVec3::new(dx, dy, 0.0) * distance)
    }

    fn invalid() -> Option<CmdResult> {
        Some(CmdResult::ReportError(t("Invalid input")))
    }

    fn numeric() -> Option<CmdResult> {
        Some(CmdResult::ReportError(t("Requires numeric value.")))
    }

    /// A point given at a prompt that takes one.
    fn point_at(&mut self, pt: DVec3) -> CmdResult {
        let pt = DVec3::new(pt.x, pt.y, 0.0);
        match self.step {
            Step::Location => self.located(pt),
            Step::Start => {
                self.from = Some(pt);
                self.step = match self.mode {
                    Mode::Bearing => Step::Quadrant,
                    Mode::Azimuth => Step::Azimuth,
                    _ => Step::End,
                };
                self.unit_line()
            }
            Step::End => {
                self.to = Some(pt);
                self.step = Step::Angle;
                CmdResult::ReportMessage(t(ANGLE_UNIT))
            }
            Step::Elevation(..) => CmdResult::ReportError(t("Invalid input")),
            _ => CmdResult::NeedPoint,
        }
    }

    /// The unit line shown before a direction prompt.
    fn unit_line(&self) -> CmdResult {
        match self.step {
            Step::Quadrant => CmdResult::ReportMessage(t(QUADRANTS)),
            Step::Azimuth => CmdResult::ReportMessage(t(DIRECTION_UNIT)),
            Step::Angle => CmdResult::ReportMessage(t(ANGLE_UNIT)),
            _ => CmdResult::NeedPoint,
        }
    }
}

impl Default for CreatePointManualCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for CreatePointManualCommand {
    fn name(&self) -> &'static str {
        "CREATEPOINTMANUAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Location => t("Please specify a location for the new point:"),
            Step::Northing => crate::tf!(">>>>Enter northing <{:.3}>:", self.ne.0).into_owned(),
            Step::Easting(_) => crate::tf!(">>>>Enter easting <{:.3}>:", self.ne.1).into_owned(),
            Step::Start if matches!(self.mode, Mode::Bearing | Mode::Azimuth) => t(">>Select starting point or [.P/.N/.G]:"),
            Step::Start => t(">>Specify starting point or [.P/.N/.G]:"),
            Step::Line => t(">>Select line or [Points]:"),
            Step::End => t(">>Specify ending point or [.P/.N/.G]:"),
            Step::Quadrant => t(">>Specify quadrant (1-4):"),
            Step::Bearing(_) => t(">>Specify bearing:"),
            Step::Azimuth => t(">>Specify azimuth:"),
            Step::Angle if self.counter_clockwise => t(">>Specify angle or [Clockwise]:"),
            Step::Angle => t(">>Specify angle or [Counter-clockwise]:"),
            Step::Distance(_) => t(">>Specify distance:"),
            Step::Description(_) => crate::tf!("Enter a point description <{}>:", Self::shown(&self.description)).into_owned(),
            Step::Elevation(..) => t("Specify a point elevation <.>:"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Line => vec![CmdOption::new("Points", "P")],
            Step::Angle if self.counter_clockwise => vec![CmdOption::new("Clockwise", "C")],
            Step::Angle => vec![CmdOption::new("Counter-clockwise", "C")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Location | Step::Start | Step::End | Step::Line => InputKind::Point,
            // Raw text: a number only, not an expression or a point (measured:
            // both are refused).
            _ => InputKind::FreeText,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Location | Step::Line)
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Line)
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::Line)
    }

    fn inject_picked_entity(&mut self, entity: document::EntityType) {
        self.picked = match entity {
            document::EntityType::Line(l) => Some((DVec3::new(l.start.x, l.start.y, 0.0), DVec3::new(l.end.x, l.end.y, 0.0))),
            _ => None,
        };
    }

    fn on_entity_pick(&mut self, _handle: document::Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some((start, end)) => {
                self.from = Some(start);
                self.to = Some(end);
                self.step = Step::Angle;
                CmdResult::ReportMessage(t(ANGLE_UNIT))
            }
            None => CmdResult::ReportError(t("Nothing selected.")),
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.point_at(pt)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        match std::mem::replace(&mut self.step, Step::Location) {
            Step::Location => {
                let mode = Mode::of(typed).filter(|_| typed.starts_with('\''))?;
                self.enter_mode(mode);
                return Some(self.unit_line());
            }
            Step::Line => {
                self.step = Step::Line;
                if typed.eq_ignore_ascii_case("P") || typed.eq_ignore_ascii_case("Points") {
                    self.step = Step::Start;
                    return Some(CmdResult::NeedPoint);
                }
                return Some(CmdResult::ReportError(t("Nothing selected.")));
            }
            Step::Northing => match if typed.is_empty() { Some(self.ne.0) } else { number(typed) } {
                Some(n) => {
                    self.ne.0 = n;
                    self.step = Step::Easting(n);
                }
                None => {
                    self.step = Step::Northing;
                    return Self::numeric();
                }
            },
            Step::Easting(n) => match if typed.is_empty() { Some(self.ne.1) } else { number(typed) } {
                Some(e) => {
                    self.ne.1 = e;
                    return Some(self.located(DVec3::new(e, n, 0.0)));
                }
                None => {
                    self.step = Step::Easting(n);
                    return Self::numeric();
                }
            },
            Step::Quadrant => {
                if typed.is_empty() {
                    return Some(self.resume());
                }
                match typed.parse::<u8>() {
                    Ok(q @ 1..=4) => {
                        self.step = Step::Bearing(q);
                        return Some(CmdResult::ReportMessage(t(DIRECTION_UNIT)));
                    }
                    _ => {
                        self.step = Step::Quadrant;
                        return Some(CmdResult::ReportError(format!("{}\n{}", t("Invalid quadrant. Please try again"), t(QUADRANTS))));
                    }
                }
            }
            Step::Bearing(q) => match dms(typed).filter(|b| (0.0..=90.0).contains(b)) {
                // The bearing turns from north or south toward east or west.
                Some(b) => self.step = Step::Distance([b, 180.0 - b, 180.0 + b, 360.0 - b][usize::from(q) - 1]),
                None => {
                    self.step = Step::Bearing(q);
                    return Self::invalid();
                }
            },
            Step::Azimuth => {
                if typed.is_empty() {
                    return Some(self.resume());
                }
                match dms(typed) {
                    Some(a) => self.step = Step::Distance(a),
                    None => {
                        self.step = Step::Azimuth;
                        return Self::invalid();
                    }
                }
            }
            Step::End => {
                if typed.is_empty() {
                    return Some(self.resume());
                }
                self.step = Step::End;
                return Self::invalid();
            }
            Step::Angle => {
                if typed.is_empty() {
                    return Some(self.resume());
                }
                if typed.eq_ignore_ascii_case("C") {
                    self.counter_clockwise = !self.counter_clockwise;
                    self.step = Step::Angle;
                    return Some(CmdResult::NeedPoint);
                }
                match number(typed) {
                    Some(a) => self.step = Step::Distance(a),
                    None => {
                        self.step = Step::Angle;
                        return Self::invalid();
                    }
                }
            }
            Step::Distance(direction) => match number(typed).and_then(|d| self.travel(direction, d)) {
                Some(at) => {
                    self.from = Some(at);
                    return Some(self.located(at));
                }
                None => {
                    self.step = Step::Distance(direction);
                    return Self::numeric();
                }
            },
            Step::Start => {
                self.step = Step::Start;
                return Self::invalid();
            }
            Step::Description(at) => {
                let d = self.description_of(text);
                self.description = d.clone();
                self.step = Step::Elevation(at, d);
            }
            Step::Elevation(at, d) => {
                let elevation = match typed {
                    "" | "." => None,
                    _ => match number(typed) {
                        Some(z) => Some(z),
                        None => {
                            self.step = Step::Elevation(at, d);
                            return Self::invalid();
                        }
                    },
                };
                // The mode's next point starts where this one is.
                if self.mode != Mode::Manual {
                    self.from = Some(at);
                }
                self.next_location();
                let then = match self.unit_line() {
                    CmdResult::ReportMessage(m) => Some(m),
                    _ => None,
                };
                return Some(CmdResult::AddCogoPoint { position: at, description: d, elevation, echo: true, then });
            }
        }
        Some(self.unit_line())
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Location => CmdResult::Done,
            Step::Start | Step::Line => self.resume(),
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        // Esc in a direction mode drops back to the location prompt.
        match (self.mode, &self.step) {
            (Mode::Manual, _) | (_, Step::Description(_) | Step::Elevation(..)) => CmdResult::Cancel,
            _ => self.resume(),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["CREATEPOINTMANUAL", "CREATEPOINTNORTHINGEASTING", "CREATEPOINTBEARING", "CREATEPOINTAZIMUTH", "CREATEPOINTANGLE", "CREATEPOINTDEFLECTION"]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn run(c: &mut CreatePointManualCommand, inputs: &[&str]) -> Vec<CmdResult> {
        inputs.iter().map(|i| c.on_text_input(i).unwrap()).collect()
    }

    fn added(r: &CmdResult) -> DVec3 {
        match r {
            CmdResult::AddCogoPoint { position, .. } => *position,
            _ => panic!("no point"),
        }
    }

    #[test]
    fn direction_modes_land_where_measured() {
        // 'ZD from (10,20): azimuth 1°, 45 → (10.7854, 64.9931).
        let mut c = CreatePointManualCommand::with_mode(Mode::Azimuth);
        c.on_point(DVec3::new(10.0, 20.0, 0.0));
        let r = run(&mut c, &["1", "45", "", ""]);
        let p = added(&r[3]);
        assert!((p.x - 10.7854).abs() < 1e-4 && (p.y - 64.9931).abs() < 1e-4);
        // 'PBD from (10,20): quadrant 1, 45°, 30 → (31.2132, 41.2132).
        let mut c = CreatePointManualCommand::with_mode(Mode::Bearing);
        c.on_point(DVec3::new(10.0, 20.0, 0.0));
        let r = run(&mut c, &["1", "45", "30", "", ""]);
        let p = added(&r[4]);
        assert!((p.x - 31.2132).abs() < 1e-4 && (p.y - 41.2132).abs() < 1e-4);
        // 'AD on (0,0)-(10,0): 30°, 20 → (17.3205, -10); then end (40,40): 30°, 20 → (33.5822, 1.6429).
        let mut c = CreatePointManualCommand::with_mode(Mode::Angle);
        c.on_text_input("P");
        c.on_point(DVec3::ZERO);
        c.on_point(DVec3::new(10.0, 0.0, 0.0));
        let r = run(&mut c, &["30", "20", "", ""]);
        let p = added(&r[3]);
        assert!((p.x - 17.3205).abs() < 1e-4 && (p.y + 10.0).abs() < 1e-4);
        c.on_point(DVec3::new(40.0, 40.0, 0.0));
        let r = run(&mut c, &["30", "20", "", ""]);
        let p = added(&r[3]);
        assert!((p.x - 33.5822).abs() < 1e-4 && (p.y - 1.6429).abs() < 1e-4);
        // 'DD on (0,0)-(10,0): 90°, 20 → (0, 20).
        let mut c = CreatePointManualCommand::with_mode(Mode::Deflection);
        c.on_text_input("P");
        c.on_point(DVec3::ZERO);
        c.on_point(DVec3::new(10.0, 0.0, 0.0));
        let r = run(&mut c, &["90", "20", "", ""]);
        let p = added(&r[3]);
        assert!(p.x.abs() < 1e-9 && (p.y - 20.0).abs() < 1e-9);
    }
}
