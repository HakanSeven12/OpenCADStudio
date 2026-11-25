//! Create Points > Interpolate: COGO points between two control points
//! (or two COGO points), by count, distance, elevation, perpendicular foot,
//! increments and the crossing of a second line.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use document::{EntityType, Handle};
use glam::{DVec2, DVec3};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Interpolate,
    RelativeLocation,
    RelativeElevation,
    NumberByDistance,
    Perpendicular,
    IncrementalDistance,
    IncrementalElevation,
    Intersection,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Interpolate => "CREATEPOINTINTERPOLATE",
            Kind::RelativeLocation => "CREATEPOINTBYRELLOCATION",
            Kind::RelativeElevation => "CREATEPOINTBYRELELEV",
            Kind::NumberByDistance => "CREATEPOINTNUMBYDIST",
            Kind::Perpendicular => "CREATEPTINTERPPERPENDIC",
            Kind::IncrementalDistance => "CREATEPTINCREMENTALDIST",
            Kind::IncrementalElevation => "CREATEPTINCRMNTLELEV",
            Kind::Intersection => "CREATEPTINTERPOLINTERSEC",
        }
    }

    pub fn of(command: &str) -> Option<Kind> {
        Some(match command {
            "CREATEPOINTINTERPOLATE" => Kind::Interpolate,
            "CREATEPOINTBYRELLOCATION" => Kind::RelativeLocation,
            "CREATEPOINTBYRELELEV" => Kind::RelativeElevation,
            "CREATEPOINTNUMBYDIST" => Kind::NumberByDistance,
            "CREATEPTINTERPPERPENDIC" => Kind::Perpendicular,
            "CREATEPTINCREMENTALDIST" => Kind::IncrementalDistance,
            "CREATEPTINCRMNTLELEV" => Kind::IncrementalElevation,
            "CREATEPTINTERPOLINTERSEC" => Kind::Intersection,
            _ => return None,
        })
    }
}

/// The computed slope is echoed grade first once a grade was answered, slope
/// first once a slope was (measured: the order carries over between
/// commands).
static GRADE_FIRST: AtomicBool = AtomicBool::new(false);

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn slope_text(grade: f64) -> String {
    if grade == 0.0 { t("Horizontal") } else { format!("{:.2}:1", 100.0 / grade) }
}

fn grade_lines(grade: f64) -> [String; 2] {
    let slope = crate::tf!("Slope: (run:rise): {}", slope_text(grade)).into_owned();
    let percent = crate::tf!("Grade: (percent): {:.2}", grade).into_owned();
    if GRADE_FIRST.load(Ordering::Relaxed) { [percent, slope] } else { [slope, percent] }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Value {
    /// The run's distance (Relative Location).
    Distance,
    /// An elevation (Relative Elevation).
    Elevation,
    /// A count of points (Interpolate, Number by Distance).
    Count,
    /// A spacing (Incremental Distance).
    Spacing,
    /// An elevation step (Incremental Elevation).
    Step,
    /// A point perpendicular to the line (Perpendicular).
    Foot,
}

#[derive(Clone, PartialEq, Debug)]
enum Step {
    /// Interpolate: the first and second COGO point.
    PickPoint { second: bool },
    First,
    Entity,
    FirstElevation,
    Second,
    SecondElevation,
    Difference,
    Grade,
    Value(Value),
    Offset,
    /// Intersection: the crossing line.
    CrossFirst,
    CrossSecond,
    CrossOffset,
    Description,
}

pub struct InterpolatePointCommand {
    kind: Kind,
    step: Step,
    metric: bool,
    /// COGO point positions by handle (Interpolate picks them).
    cogo: HashMap<Handle, DVec3>,
    /// Line, arc and polyline ends by handle (the Entity option).
    ends: HashMap<Handle, (DVec2, DVec2)>,
    picked: Option<EntityType>,
    /// The control points came from the Entity option (both known).
    entity_used: bool,
    /// The Entity option was asked for the crossing line.
    entity_cross: bool,
    /// Perpendicular's Enter asked for the object: Esc goes back.
    entity_from_foot: bool,
    a: DVec3,
    b: DVec3,
    /// The control line was given by slope or grade: the distance prompt has
    /// no default (measured).
    by_grade: bool,
    as_grade: bool,
    value: f64,
    spacing: f64,
    rise_step: f64,
    count: u32,
    offset: f64,
    cross: (DVec2, DVec2),
    queue: VecDeque<DVec3>,
    description: Option<String>,
}

impl InterpolatePointCommand {
    pub fn new(kind: Kind, document: &document::Document) -> Self {
        let mut cogo = HashMap::new();
        let mut ends = HashMap::new();
        for e in document.entities() {
            let h = e.common().handle;
            if let Some(p) = crate::entities::civil::cogo_point_position(e, document) {
                cogo.insert(h, p);
            } else if let Some(span) = entity_ends(e) {
                ends.insert(h, span);
            }
        }
        InterpolatePointCommand {
            kind,
            step: if kind == Kind::Interpolate { Step::PickPoint { second: false } } else { Step::First },
            metric: document.header.insertion_units == 6,
            cogo,
            ends,
            picked: None,
            entity_used: false,
            entity_cross: false,
            entity_from_foot: false,
            a: DVec3::ZERO,
            b: DVec3::ZERO,
            by_grade: false,
            as_grade: false,
            value: 0.0,
            spacing: 10.0,
            rise_step: 0.0,
            count: 0,
            offset: 0.0,
            cross: (DVec2::ZERO, DVec2::ZERO),
            queue: VecDeque::new(),
            description: None,
        }
    }

    fn unit(&self) -> &'static str {
        if self.metric { "m" } else { "" }
    }

    fn horizontal(&self) -> f64 {
        (self.b - self.a).truncate().length()
    }

    /// The control line's grade in percent.
    fn grade(&self) -> f64 {
        let h = self.horizontal();
        if h > 0.0 { (self.b.z - self.a.z) / h * 100.0 } else { 0.0 }
    }

    /// The point `s` along the control line (beyond its ends too), with its
    /// elevation, moved `offset` to the right.
    fn at(&self, s: f64) -> DVec3 {
        let d = (self.b - self.a).truncate();
        let h = d.length();
        let dir = if h > 0.0 { d / h } else { DVec2::X };
        let z = self.a.z + self.grade() / 100.0 * s;
        (self.a.truncate() + dir * s - dir.perp() * self.offset).extend(z)
    }

    fn value_step(&self) -> Value {
        match self.kind {
            Kind::RelativeLocation => Value::Distance,
            Kind::RelativeElevation => Value::Elevation,
            Kind::Interpolate | Kind::NumberByDistance => Value::Count,
            Kind::Perpendicular => Value::Foot,
            Kind::IncrementalDistance => Value::Spacing,
            Kind::IncrementalElevation => Value::Step,
            Kind::Intersection => Value::Distance,
        }
    }

    /// The control line is known: its summary, then the command's prompt.
    fn control_given(&mut self) -> CmdResult {
        let unit = self.unit();
        let diff = self.b.z - self.a.z;
        let [g1, g2] = grade_lines(self.grade());
        let lines = [
            crate::tf!(
                "First Elev: {}, Second Elev: {}, Elevation Difference: {:.3}",
                format!("{:.3}{unit}", self.a.z),
                format!("{:.3}{unit}", self.b.z),
                diff
            )
            .into_owned(),
            crate::tf!("Horiz Dist: {:.3}", self.horizontal()).into_owned(),
            g1,
            g2,
        ];
        self.step = if self.kind == Kind::Intersection { Step::Offset } else { Step::Value(self.value_step()) };
        CmdResult::ReportMessage(lines.join("\n"))
    }

    fn back_to_value(&mut self) -> Option<String> {
        if !self.queue.is_empty() {
            self.step = Step::Description;
            return None;
        }
        self.step = match self.kind {
            // Measured: the crossing point starts the command over.
            Kind::Intersection => {
                self.by_grade = false;
                self.entity_cross = false;
                Step::First
            }
            _ => Step::Value(self.value_step()),
        };
        None
    }

    /// The points the command's value gives (offset applied).
    fn points(&self) -> Vec<DVec3> {
        let h = self.horizontal();
        match self.value_step() {
            Value::Count => (1..=self.count).map(|k| self.at(h * f64::from(k) / f64::from(self.count + 1))).collect(),
            Value::Distance => vec![self.at(self.value)],
            Value::Elevation => {
                let g = self.grade() / 100.0;
                if g == 0.0 { Vec::new() } else { vec![self.at((self.value - self.a.z) / g)] }
            }
            Value::Foot => vec![self.at(self.value)],
            // Measured: whole spacings short of the second point.
            Value::Spacing => {
                let mut out = Vec::new();
                let mut s = self.spacing;
                while self.spacing > 0.0 && s < h - 1e-9 {
                    out.push(self.at(s));
                    s += self.spacing;
                }
                out
            }
            // Measured: whole steps short of the elevation difference, each
            // placed by its share of the difference; the elevation adds the
            // step as typed (with 20 then 10 and a step of 3, the first point
            // is 30 along at 23).
            Value::Step => {
                let diff = (self.b.z - self.a.z).abs();
                let step = self.rise_step;
                let mut out = Vec::new();
                if step != 0.0 && diff != 0.0 {
                    let mut k = 1.0;
                    while k * step.abs() < diff - 1e-9 {
                        let mut p = self.at(h * k * step.abs() / diff);
                        p.z = self.a.z + k * step;
                        out.push(p);
                        k += 1.0;
                    }
                }
                out
            }
        }
    }

    /// Intersection: where the crossing line (offset to its right) meets the
    /// control line (offset to its right), with the control line's elevation.
    fn crossing(&self, cross_offset: f64) -> Option<DVec3> {
        let (p, q) = (self.at(0.0).truncate(), self.at(1.0).truncate());
        let d1 = q - p;
        let d2 = self.cross.1 - self.cross.0;
        let n2 = d2.normalize_or_zero();
        let c = self.cross.0 - n2.perp() * cross_offset;
        let den = d1.perp_dot(d2);
        if den.abs() < 1e-12 {
            return None;
        }
        let s = (c - p).perp_dot(d2) / den;
        Some(self.at(s))
    }

    fn point_given(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::First => {
                self.a = pt;
                self.entity_used = false;
                self.step = Step::FirstElevation;
            }
            Step::Second => {
                self.b = pt;
                self.step = Step::SecondElevation;
            }
            Step::CrossFirst => {
                self.cross.0 = pt.truncate();
                self.step = Step::CrossSecond;
            }
            Step::CrossSecond => {
                self.cross.1 = pt.truncate();
                self.step = Step::CrossOffset;
            }
            // Measured: a point past the control line's ends is ignored.
            Step::Value(Value::Foot) => {
                let d = (self.b - self.a).truncate();
                let h = d.length();
                let s = if h > 0.0 { (pt.truncate() - self.a.truncate()).dot(d / h) } else { 0.0 };
                if s < -1e-9 || s > h + 1e-9 {
                    return CmdResult::NeedPoint;
                }
                self.value = s;
                self.step = Step::Offset;
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn after_offset(&mut self) -> CmdResult {
        if self.kind == Kind::Intersection && self.step == Step::Offset {
            self.step = Step::CrossFirst;
            return CmdResult::NeedPoint;
        }
        let points = self.points();
        self.queue = points.into();
        if self.queue.is_empty() {
            self.back_to_value();
            return CmdResult::NeedPoint;
        }
        self.step = Step::Description;
        CmdResult::NeedPoint
    }
}

/// The ends of a line, arc or polyline (the Entity option's control points).
fn entity_ends(e: &EntityType) -> Option<(DVec2, DVec2)> {
    match e {
        EntityType::Line(l) => Some((DVec2::new(l.start.x, l.start.y), DVec2::new(l.end.x, l.end.y))),
        EntityType::Arc(a) => {
            let c = DVec2::new(a.center.x, a.center.y);
            Some((c + DVec2::from_angle(a.start_angle) * a.radius, c + DVec2::from_angle(a.end_angle) * a.radius))
        }
        EntityType::LwPolyline(p) => {
            let (f, l) = (p.vertices.first()?, p.vertices.last()?);
            Some((DVec2::new(f.location.x, f.location.y), DVec2::new(l.location.x, l.location.y)))
        }
        EntityType::Polyline3D(p) => {
            let (f, l) = (p.vertices.first()?, p.vertices.last()?);
            Some((DVec2::new(f.position.x, f.position.y), DVec2::new(l.position.x, l.position.y)))
        }
        EntityType::Polyline2D(p) => {
            let (f, l) = (p.vertices.first()?, p.vertices.last()?);
            Some((DVec2::new(f.location.x, f.location.y), DVec2::new(l.location.x, l.location.y)))
        }
        _ => None,
    }
}

impl CadCommand for InterpolatePointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        let unit = self.unit();
        match &self.step {
            Step::PickPoint { second: false } => t("Pick a first point object:"),
            Step::PickPoint { second: true } => t("Pick a second point object:"),
            Step::First | Step::CrossFirst => t("Specify first point or [Entity]:"),
            Step::Entity => t("Select an arc, line, lot line, feature line, or polyline or [Points]:"),
            Step::FirstElevation => crate::tf!("Specify an elevation for the first control point <{}>:", format!("{:.3}{unit}", self.a.z)).into_owned(),
            Step::Second | Step::CrossSecond => t("Specify second point:"),
            Step::SecondElevation => {
                crate::tf!("Specify an elevation for the second control point or [Difference/Slope/Grade] <{}>:", format!("{:.3}{unit}", self.b.z)).into_owned()
            }
            Step::Difference => t("Elevation Difference <.>:"),
            Step::Grade if self.as_grade => crate::tf!("Grade (percent) or [Slope] <{:.2}>:", 0.0).into_owned(),
            Step::Grade => crate::tf!("Slope (run:rise) or [Grade] <{}>:", t("Horizontal")).into_owned(),
            Step::Value(Value::Distance) if self.by_grade => t("Distance:"),
            Step::Value(Value::Distance) => crate::tf!("Distance <{:.3}>:", 0.0).into_owned(),
            Step::Value(Value::Elevation) => crate::tf!("Elevation <{}>:", format!("{:.3}{unit}", 0.0)).into_owned(),
            Step::Value(Value::Count) => crate::tf!("Enter number of points <{}>:", 0).into_owned(),
            Step::Value(Value::Foot) => t("Please specify a location that is perpendicular to the interpolated line:"),
            Step::Value(Value::Spacing) => crate::tf!("Distance between points <{:.4}>:", self.spacing).into_owned(),
            Step::Value(Value::Step) => crate::tf!("Elevation Difference <{}>:", format!("{:.3}{unit}", self.rise_step)).into_owned(),
            Step::Offset | Step::CrossOffset => crate::tf!("Specify an offset <{:.3}>:", self.offset).into_owned(),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::First | Step::CrossFirst => vec![CmdOption::new("Entity", "E")],
            Step::Entity => vec![CmdOption::new("Points", "P")],
            Step::SecondElevation => vec![CmdOption::new("Difference", "D"), CmdOption::new("Slope", "S"), CmdOption::new("Grade", "G")],
            Step::Grade if self.as_grade => vec![CmdOption::new("Slope", "S")],
            Step::Grade => vec![CmdOption::new("Grade", "G")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::PickPoint { .. } | Step::First | Step::Entity | Step::Second | Step::CrossFirst | Step::CrossSecond | Step::Value(Value::Foot) => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickPoint { .. } | Step::Entity)
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pick: DVec3) -> CmdResult {
        self.picked = None;
        match self.step.clone() {
            // Measured: anything but a COGO point is passed over in silence.
            Step::PickPoint { second } => {
                let Some(&p) = self.cogo.get(&handle) else { return CmdResult::NeedPoint };
                if second {
                    self.b = p;
                    self.step = Step::Value(Value::Count);
                } else {
                    self.a = p;
                    self.step = Step::PickPoint { second: true };
                }
                CmdResult::NeedPoint
            }
            // Measured: the end nearer the pick is the first control point
            // (the end one when both are as near).
            // Measured: a pick on nothing goes back to the point prompt.
            Step::Entity => {
                let Some(&(s, e)) = self.ends.get(&handle) else {
                    if handle == Handle::NULL {
                        self.entity_from_foot = false;
                        self.step = if self.entity_cross { Step::CrossFirst } else { Step::First };
                    }
                    return CmdResult::NeedPoint;
                };
                let p = _pick.truncate();
                let (first, second) = if p.distance(s) < p.distance(e) { (s, e) } else { (e, s) };
                if self.entity_cross {
                    self.cross = (first, second);
                    self.step = Step::CrossOffset;
                    return CmdResult::NeedPoint;
                }
                self.a = first.extend(0.0);
                self.b = second.extend(0.0);
                self.entity_used = true;
                self.entity_from_foot = false;
                self.step = Step::FirstElevation;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::PickPoint { .. } | Step::Entity => self.on_entity_pick(Handle::NULL, pt),
            _ => self.point_given(pt),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let coordinate = typed.contains(',') && typed.chars().any(|c| c.is_ascii_digit());
        let word = typed.to_ascii_uppercase();
        let invalid_chars = || CmdResult::ReportError(crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned());
        let retry = t("Requires numeric distance. Please try again.");
        Some(match self.step.clone() {
            Step::PickPoint { .. } => {
                if typed.is_empty() {
                    return Some(CmdResult::Done);
                }
                if coordinate {
                    return None;
                }
                CmdResult::ReportError(format!("{}\n{}", t("*Invalid selection*"), t("Expects a point or Window/Crossing/BOX/Fence/WPolygon/CPolygon/AUto")))
            }
            Step::First | Step::CrossFirst => {
                // Measured: Enter is the Entity option.
                if word.is_empty() || word == "E" || word == "ENTITY" {
                    self.entity_cross = self.step == Step::CrossFirst;
                    self.step = Step::Entity;
                    return Some(CmdResult::NeedPoint);
                }
                if coordinate {
                    return None;
                }
                CmdResult::ReportError(t("Point or option keyword required."))
            }
            Step::Second | Step::CrossSecond => {
                if coordinate {
                    return None;
                }
                if typed.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                CmdResult::ReportError(t("Point or option keyword required."))
            }
            Step::Entity => {
                if word == "P" || word == "POINTS" {
                    self.entity_from_foot = false;
                    self.step = if self.entity_cross { Step::CrossFirst } else { Step::First };
                    return Some(CmdResult::NeedPoint);
                }
                if coordinate {
                    return None;
                }
                if typed.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                CmdResult::ReportError(format!("{}\n{}", t("*Invalid selection*"), t("Expects a point or Points]:")))
            }
            Step::FirstElevation => {
                let z = if typed.is_empty() { Some(self.a.z) } else { number(typed) };
                match z {
                    Some(z) => {
                        self.a.z = z;
                        // The Entity option gave both points.
                        self.step = if self.entity_used { Step::SecondElevation } else { Step::Second };
                        CmdResult::NeedPoint
                    }
                    None => CmdResult::ReportError(t("Invalid input")),
                }
            }
            Step::SecondElevation => match word.as_str() {
                "D" | "DIFFERENCE" => {
                    self.step = Step::Difference;
                    CmdResult::NeedPoint
                }
                "S" | "SLOPE" | "G" | "GRADE" => {
                    self.as_grade = word.starts_with('G');
                    self.step = Step::Grade;
                    CmdResult::NeedPoint
                }
                _ => {
                    let z = if typed.is_empty() { Some(self.b.z) } else { number(typed) };
                    match z {
                        Some(z) => {
                            self.b.z = z;
                            self.by_grade = false;
                            self.control_given()
                        }
                        None => CmdResult::ReportError(t("Invalid input")),
                    }
                }
            },
            Step::Difference => {
                let d = if typed.is_empty() || typed == "." { Some(0.0) } else { number(typed) };
                match d {
                    Some(d) => {
                        self.b.z = self.a.z + d;
                        self.by_grade = false;
                        self.control_given()
                    }
                    None => invalid_chars(),
                }
            }
            Step::Grade => {
                if matches!(word.as_str(), "G" | "GRADE" | "S" | "SLOPE") {
                    self.as_grade = word.starts_with('G');
                    return Some(CmdResult::NeedPoint);
                }
                let v = if typed.is_empty() { Some(0.0) } else { number(typed) };
                let Some(v) = v else {
                    let first = if self.as_grade { crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned() } else { t("Invalid input") };
                    return Some(CmdResult::ReportError(format!("{first}\n{retry}")));
                };
                let grade = if self.as_grade { v } else if v == 0.0 { 0.0 } else { 100.0 / v };
                GRADE_FIRST.store(self.as_grade, Ordering::Relaxed);
                self.b.z = self.a.z + grade / 100.0 * self.horizontal();
                self.by_grade = true;
                self.control_given()
            }
            Step::Value(value) => {
                if value == Value::Foot {
                    if coordinate {
                        return None;
                    }
                    // Measured: Enter asks for an object to take a new control
                    // line from; Esc there comes back here.
                    if typed.is_empty() {
                        self.entity_from_foot = true;
                        self.entity_cross = false;
                        self.step = Step::Entity;
                        return Some(CmdResult::NeedPoint);
                    }
                    return Some(CmdResult::ReportError(t("Point or option keyword required.")));
                }
                match value {
                    Value::Count => match typed.parse::<i64>() {
                        Ok(n) if n > 0 => {
                            self.count = n as u32;
                            self.step = Step::Offset;
                            CmdResult::NeedPoint
                        }
                        _ if typed.is_empty() || typed.parse::<i64>().is_ok() => CmdResult::ReportError(t("A positive value is required")),
                        _ => CmdResult::ReportError(t("'Generic Integer' validation error.  Please enter an integer.")),
                    },
                    Value::Distance => {
                        let d = if typed.is_empty() && !self.by_grade { Some(0.0) } else { number(typed) };
                        match d {
                            Some(d) if d < 0.0 => CmdResult::ReportError(t("'Distance' validation error.  Please enter a positive value.")),
                            Some(d) => {
                                self.value = d;
                                self.step = Step::Offset;
                                CmdResult::NeedPoint
                            }
                            None if typed.is_empty() => CmdResult::NeedPoint,
                            None => CmdResult::ReportError(t("Invalid input")),
                        }
                    }
                    Value::Elevation => {
                        let z = if typed.is_empty() { Some(0.0) } else { number(typed) };
                        match z {
                            Some(z) => {
                                self.value = z;
                                self.step = Step::Offset;
                                CmdResult::NeedPoint
                            }
                            None => CmdResult::ReportError(t("Invalid Input")),
                        }
                    }
                    Value::Spacing => {
                        let d = if typed.is_empty() { Some(self.spacing) } else { number(typed) };
                        match d {
                            Some(d) => {
                                self.spacing = d;
                                self.step = Step::Offset;
                                CmdResult::NeedPoint
                            }
                            None => CmdResult::ReportError(t("Invalid Input")),
                        }
                    }
                    Value::Step => {
                        let d = if typed.is_empty() { Some(self.rise_step) } else { number(typed) };
                        match d {
                            Some(d) => {
                                self.rise_step = d;
                                self.step = Step::Offset;
                                CmdResult::NeedPoint
                            }
                            None => invalid_chars(),
                        }
                    }
                    Value::Foot => CmdResult::NeedPoint,
                }
            }
            Step::Offset | Step::CrossOffset => {
                let o = if typed.is_empty() { Some(self.offset) } else { number(typed) };
                let Some(o) = o else { return Some(invalid_chars()) };
                if self.step == Step::CrossOffset {
                    // Measured: parallel lines end the command silently.
                    let Some(at) = self.crossing(o) else {
                        return Some(CmdResult::Done);
                    };
                    self.queue = VecDeque::from([at]);
                    self.step = Step::Description;
                    return Some(CmdResult::NeedPoint);
                }
                self.offset = o;
                self.after_offset()
            }
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                let at = self.queue.pop_front()?;
                self.back_to_value();
                CmdResult::AddCogoPoint { position: at, description: d, elevation: Some(at.z), echo: false, then: None }
            }
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }

    fn on_escape(&mut self) -> CmdResult {
        // Measured: Esc at a description of a several-point command drops
        // the remaining points and asks the value again; a one-point
        // command ends there, as everywhere else.
        if self.step == Step::Entity && self.entity_from_foot {
            self.entity_from_foot = false;
            self.step = Step::Value(Value::Foot);
            return CmdResult::ReportMessage(t("*Cancel*"));
        }
        if self.step == Step::Description && matches!(self.value_step(), Value::Count | Value::Spacing | Value::Step) {
            self.queue.clear();
            self.step = Step::Value(self.value_step());
            return CmdResult::ReportMessage(t("*Cancel*"));
        }
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "CREATEPOINTINTERPOLATE",
        "CREATEPOINTBYRELLOCATION",
        "CREATEPOINTBYRELELEV",
        "CREATEPOINTNUMBYDIST",
        "CREATEPTINTERPPERPENDIC",
        "CREATEPTINCREMENTALDIST",
        "CREATEPTINCRMNTLELEV",
        "CREATEPTINTERPOLINTERSEC",
    ]
});
