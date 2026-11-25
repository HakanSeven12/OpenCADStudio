//! Create Points > Slope: COGO points along a slope or grade from a start
//! point, to a distance or to an ending elevation, and the high or low point
//! two grades meet at between two points.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use glam::{DVec2, DVec3};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Distance,
    Elevation,
    HighLow,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Distance => "CREATEPOINTSLOPEGRADEDIST",
            Kind::Elevation => "CREATEPOINTSLOPEGRADEELEV",
            Kind::HighLow => "CREATEPOINTHIGHLOWPOINT",
        }
    }

    pub fn of(command: &str) -> Option<Kind> {
        Some(match command {
            "CREATEPOINTSLOPEGRADEDIST" => Kind::Distance,
            "CREATEPOINTSLOPEGRADEELEV" => Kind::Elevation,
            "CREATEPOINTHIGHLOWPOINT" => Kind::HighLow,
            _ => return None,
        })
    }
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A slope as typed: run:rise ("4.00:1", Horizontal at 0) or percent.
fn slope_text(grade: f64) -> String {
    if grade == 0.0 { t("Horizontal") } else { format!("{:.2}:1", 100.0 / grade) }
}

/// A run:rise slope of 0 is vertical: the grade (percent) it stands for
/// (measured: 10 along it rises 1e9).
const VERTICAL: f64 = 1e10;

/// The two lines a slope or grade answer echoes, the answered one first.
fn echo(grade: f64, as_grade: bool) -> String {
    let slope = crate::tf!("Slope: (run:rise): {}", slope_text(grade)).into_owned();
    let percent = if grade == VERTICAL {
        crate::tf!("Grade: (percent): {}", t("Vertical")).into_owned()
    } else {
        crate::tf!("Grade: (percent): {:.2}", grade).into_owned()
    };
    if as_grade { format!("{percent}\n{slope}") } else { format!("{slope}\n{percent}") }
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Start,
    Direction,
    Second,
    /// The slope (or, with `as_grade`, the grade); `second` for the high/low
    /// point's second one.
    Grade { second: bool },
    Distance,
    EndElevation,
    Count,
    Offset,
    Ending,
    AddPoint,
    Description,
}

pub struct SlopePointCommand {
    kind: Kind,
    step: Step,
    metric: bool,
    start: DVec3,
    direction: Option<DVec2>,
    second: DVec3,
    /// The prompt asks a grade in percent rather than a run:rise slope.
    as_grade: bool,
    /// The slope prompt was toggled between slope and grade: its "First" /
    /// "Second" goes (measured).
    toggled: bool,
    grade: f64,
    first_grade: f64,
    distance: f64,
    end_elevation: f64,
    count: u32,
    offset: f64,
    queue: VecDeque<DVec3>,
    description: Option<String>,
}

impl SlopePointCommand {
    pub fn new(kind: Kind, document: &document::Document) -> Self {
        SlopePointCommand {
            kind,
            step: Step::Start,
            metric: document.header.insertion_units == 6,
            start: DVec3::ZERO,
            direction: None,
            second: DVec3::ZERO,
            as_grade: false,
            toggled: false,
            grade: 0.0,
            first_grade: 0.0,
            distance: 0.0,
            end_elevation: 0.0,
            count: 0,
            offset: 0.0,
            queue: VecDeque::new(),
            description: None,
        }
    }

    /// The start point's elevation, shown before the direction prompt.
    fn elevation_line(&self) -> String {
        let unit = if self.metric { "m" } else { "" };
        crate::tf!("Elevation <{}>", format!("{:.3}{unit}", self.start.z)).into_owned()
    }

    /// Back to the direction prompt for the next run from the same start.
    fn to_direction(&mut self) -> CmdResult {
        self.step = Step::Direction;
        CmdResult::ReportMessage(self.elevation_line())
    }

    fn after_grade(&mut self) -> CmdResult {
        self.step = match self.kind {
            Kind::Distance => Step::Distance,
            Kind::Elevation => Step::EndElevation,
            Kind::HighLow => return self.high_low(),
        };
        CmdResult::NeedPoint
    }

    /// The points of a run: `count` intermediate points and the ending one,
    /// evenly spaced to the distance; without the ending point the last
    /// intermediate one is at the distance (measured).
    fn run_points(&self, ending: bool) -> Vec<DVec3> {
        let Some(dir) = self.direction else { return Vec::new() };
        let (distance, rise) = match self.kind {
            Kind::Elevation => {
                let rise = self.end_elevation - self.start.z;
                let d = if self.grade == 0.0 { 0.0 } else { (rise / (self.grade / 100.0)).abs() };
                (d, rise)
            }
            _ => (self.distance, self.distance * self.grade / 100.0),
        };
        let n = self.count + u32::from(ending);
        if n == 0 {
            return Vec::new();
        }
        // The offset goes right of the run; a run of no length has none.
        let right = if distance > 0.0 { -dir.perp() * self.offset } else { DVec2::ZERO };
        (1..=n)
            .map(|k| {
                let f = f64::from(k) / f64::from(n);
                let at = self.start.truncate() + dir * distance * f + right;
                let z = if distance > 0.0 { self.start.z + rise * f } else { self.start.z + rise };
                at.extend(z)
            })
            .collect()
    }

    /// The point two grades from the two points meet at (measured: both
    /// rise toward the middle from their own point).
    fn high_low(&mut self) -> CmdResult {
        let d = (self.second - self.start).truncate();
        let length = d.length();
        let (g1, g2) = (self.first_grade / 100.0, self.grade / 100.0);
        let x = (self.second.z - self.start.z + g2 * length) / (g1 + g2);
        if g1 + g2 == 0.0 || !x.is_finite() || x < 0.0 || x > length || length == 0.0 {
            // Measured: the message ends the command as a plain line.
            return CmdResult::AddCogoPoints {
                points: Vec::new(),
                lines: vec![t("The calculated point was not between the selected points.")],
                done: true,
            };
        }
        let at = self.start.truncate() + d / length * x;
        let z = self.start.z + g1 * x;
        self.queue = VecDeque::from([at.extend(z)]);
        self.step = Step::AddPoint;
        CmdResult::ReportMessage(crate::tf!("New point: X: {:.3}, Y: {:.3}, Z: {:.3}", at.x, at.y, z).into_owned())
    }

    fn next(&mut self) -> Option<String> {
        if !self.queue.is_empty() {
            self.step = Step::Description;
            return None;
        }
        if self.kind == Kind::HighLow {
            self.step = Step::Start;
            None
        } else {
            self.step = Step::Direction;
            Some(self.elevation_line())
        }
    }

    /// `grade` in percent (a slope answer converted by the caller).
    fn grade_given(&mut self, grade: f64) -> CmdResult {
        let line = echo(grade, self.as_grade);
        if self.step == (Step::Grade { second: false }) && self.kind == Kind::HighLow {
            self.first_grade = grade;
            self.grade = grade;
            self.toggled = false;
            self.step = Step::Grade { second: true };
            return CmdResult::ReportMessage(line);
        }
        self.grade = grade;
        match self.after_grade() {
            CmdResult::ReportMessage(more) => CmdResult::ReportMessage(format!("{line}\n{more}")),
            CmdResult::AddCogoPoints { points, mut lines, done } => {
                lines.insert(0, line);
                CmdResult::AddCogoPoints { points, lines, done }
            }
            _ => CmdResult::ReportMessage(line),
        }
    }
}

impl CadCommand for SlopePointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => t("Specify start point:"),
            Step::Direction => t("Specify a point to define the direction of the intermediate points:"),
            Step::Second => t("Specify second point:"),
            Step::Grade { second } => {
                let default = if *second { self.first_grade } else { 0.0 };
                let slope = slope_text(default);
                match (self.kind == Kind::HighLow && !self.toggled, *second, self.as_grade) {
                    (true, false, false) => crate::tf!("First Slope (run:rise) or [Grade] <{}>:", slope).into_owned(),
                    (true, true, false) => crate::tf!("Second Slope (run:rise) or [Grade] <{}>:", slope).into_owned(),
                    (true, true, true) => crate::tf!("Second Grade (percent) or [Slope] <{:.2}>:", default).into_owned(),
                    (_, _, true) => crate::tf!("Grade (percent) or [Slope] <{:.2}>:", default).into_owned(),
                    _ => crate::tf!("Slope (run:rise) or [Grade] <{}>:", slope).into_owned(),
                }
            }
            Step::Distance => t("Distance:"),
            Step::EndElevation => t("Ending Elevation:"),
            Step::Count => crate::tf!("Enter the number of intermediate points <{}>:", self.count).into_owned(),
            Step::Offset => crate::tf!("Specify an offset <{:.3}>:", self.offset).into_owned(),
            Step::Ending => t("Add ending point [Yes/No] <Yes>:"),
            Step::AddPoint => t("Add point [Yes/No] <Yes>:"),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Grade { .. } if self.as_grade => vec![CmdOption::new("Slope", "S")],
            Step::Grade { .. } => vec![CmdOption::new("Grade", "G")],
            Step::Ending | Step::AddPoint => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Start | Step::Direction | Step::Second => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Start => {
                self.start = pt;
                if self.kind == Kind::HighLow {
                    self.step = Step::Second;
                    return CmdResult::NeedPoint;
                }
                self.to_direction()
            }
            Step::Direction => {
                let d = (pt - self.start).truncate();
                if d.length_squared() == 0.0 {
                    return CmdResult::ReportError(format!("{}\n{}", t("Invalid input"), self.elevation_line()));
                }
                self.direction = Some(d.normalize());
                self.as_grade = false;
                self.toggled = false;
                self.step = Step::Grade { second: false };
                CmdResult::NeedPoint
            }
            Step::Second => {
                self.second = pt;
                self.as_grade = false;
                self.toggled = false;
                self.step = Step::Grade { second: false };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let coordinate = typed.contains(',') && typed.chars().any(|c| c.is_ascii_digit());
        let word = typed.to_ascii_uppercase();
        let retry = t("Requires numeric distance. Please try again.");
        Some(match self.step.clone() {
            // Measured: Enter asks again.
            Step::Start | Step::Second => {
                if typed.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                if coordinate {
                    return None;
                }
                CmdResult::ReportError(t("Point or option keyword required."))
            }
            Step::Direction => {
                if coordinate {
                    return None;
                }
                // Measured: Enter keeps the last run's direction (east before
                // any run).
                if typed.is_empty() {
                    self.direction.get_or_insert(DVec2::X);
                    self.as_grade = false;
                    self.toggled = false;
                    self.step = Step::Grade { second: false };
                    return Some(CmdResult::NeedPoint);
                }
                CmdResult::ReportError(format!("{}\n{}", t("Invalid input"), self.elevation_line()))
            }
            Step::Grade { second } => {
                if word == "G" || word == "GRADE" || word == "S" || word == "SLOPE" {
                    let wants_grade = word.starts_with('G');
                    if wants_grade != self.as_grade {
                        self.as_grade = wants_grade;
                        self.toggled = true;
                    }
                    return Some(CmdResult::NeedPoint);
                }
                // Enter keeps the default (horizontal before any grade).
                let grade = if typed.is_empty() {
                    if second { self.first_grade } else { 0.0 }
                } else {
                    match number(typed) {
                        Some(v) if self.as_grade => v,
                        Some(v) if v == 0.0 => VERTICAL,
                        Some(v) => 100.0 / v,
                        None if self.as_grade => {
                            return Some(CmdResult::ReportError(format!(
                                "{}\n{retry}",
                                crate::tf!("Invalid characters found in input string '{}'.", typed)
                            )));
                        }
                        None => return Some(CmdResult::ReportError(format!("{}\n{retry}", t("Invalid input")))),
                    }
                };
                self.grade_given(grade)
            }
            Step::Distance => match number(typed) {
                Some(d) if d < 0.0 => CmdResult::ReportError(t("'Distance' validation error.  Please enter a positive value.")),
                Some(d) => {
                    self.distance = d;
                    self.step = Step::Count;
                    CmdResult::NeedPoint
                }
                None if typed.is_empty() => CmdResult::NeedPoint,
                None => CmdResult::ReportError(t("Invalid input")),
            },
            Step::EndElevation => match number(typed) {
                Some(z) => {
                    self.end_elevation = z;
                    self.step = Step::Count;
                    CmdResult::NeedPoint
                }
                None if typed.is_empty() => CmdResult::NeedPoint,
                None => CmdResult::ReportError(crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned()),
            },
            Step::Count => {
                let n = if typed.is_empty() { Some(self.count) } else { typed.parse::<u32>().ok() };
                match n {
                    Some(n) => {
                        self.count = n;
                        self.step = Step::Offset;
                        CmdResult::NeedPoint
                    }
                    None => CmdResult::ReportError(t("'Number of points' validation error.  Please enter an integer.")),
                }
            }
            Step::Offset => {
                let o = if typed.is_empty() { Some(self.offset) } else { number(typed) };
                match o {
                    Some(o) => {
                        self.offset = o;
                        self.step = Step::Ending;
                        CmdResult::NeedPoint
                    }
                    None => CmdResult::ReportError(crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned()),
                }
            }
            // A word other than Yes/No asks again without a message
            // (measured).
            Step::Ending => {
                let ending = match word.as_str() {
                    "" | "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return Some(CmdResult::NeedPoint),
                };
                let points = self.run_points(ending);
                if points.is_empty() {
                    return Some(CmdResult::Done);
                }
                self.queue = points.into();
                self.step = Step::Description;
                CmdResult::NeedPoint
            }
            Step::AddPoint => match word.as_str() {
                "" | "Y" | "YES" => {
                    self.step = Step::Description;
                    CmdResult::NeedPoint
                }
                "N" | "NO" => {
                    self.queue.clear();
                    self.step = Step::Start;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::NeedPoint,
            },
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                let at = self.queue.pop_front()?;
                let then = self.next();
                CmdResult::AddCogoPoint { position: at, description: d, elevation: Some(at.z), echo: false, then }
            }
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }

    fn on_escape(&mut self) -> CmdResult {
        // Measured: Esc at a description drops the run's remaining points
        // and goes back to the direction prompt; elsewhere it ends.
        if self.step == Step::Description && self.kind != Kind::HighLow {
            self.queue.clear();
            self.step = Step::Direction;
            return CmdResult::ReportMessage(format!("{}\n{}", t("*Cancel*"), self.elevation_line()));
        }
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["CREATEPOINTSLOPEGRADEDIST", "CREATEPOINTSLOPEGRADEELEV", "CREATEPOINTHIGHLOWPOINT"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_high_point_as_measured() {
        let doc = document::Document::new();
        let mut c = SlopePointCommand::new(Kind::Distance, &doc);
        c.direction = Some(DVec2::X);
        c.start = DVec3::new(0.0, 0.0, 0.0);
        c.grade = 5.0;
        c.distance = 30.0;
        c.count = 4;
        // 4 intermediate points and the ending one: 6, 12, ... 30.
        let p = c.run_points(true);
        assert_eq!(p.len(), 5);
        assert!((p[0] - DVec3::new(6.0, 0.0, 0.3)).length() < 1e-9);
        // Without the ending point the last intermediate one is at 30.
        c.count = 3;
        c.distance = 20.0;
        c.start = DVec3::new(0.0, 20.0, 5.0);
        c.grade = 25.0;
        let p = c.run_points(false);
        assert!((p[2] - DVec3::new(20.0, 20.0, 10.0)).length() < 1e-9);
        // Both grades rise toward the middle: 5 and 8 at 25% meet at 56.
        let mut h = SlopePointCommand::new(Kind::HighLow, &doc);
        h.start = DVec3::new(0.0, 110.0, 5.0);
        h.second = DVec3::new(100.0, 110.0, 8.0);
        h.first_grade = 25.0;
        h.grade = 25.0;
        h.high_low();
        assert!((h.queue[0] - DVec3::new(56.0, 110.0, 19.0)).length() < 1e-9);
    }
}
