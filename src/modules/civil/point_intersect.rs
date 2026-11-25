//! The intersection point creation commands: each builds two shapes — a
//! direction from a start point, a distance (circle) round a radial point, a
//! picked object or an alignment, every one moved by an offset to the right
//! — or a shape and a perpendicular point, and writes a COGO point where
//! they meet. Two meeting points ask `Point or [All]`. Every point's
//! description and elevation are asked as CREATEPOINTMANUAL does; then the
//! command starts over until Enter at an object prompt or Esc.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::entities::curve::entity_curve_xy;
use document::{EntityType, Handle};
use glam::DVec3;
use ocs_geom::geom2d::{closest_point, intersect, offset_curve, offset_polyline, Circle, Curve, Tolerance, XLine};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    DirectionDirection,
    DistanceDistance,
    DirectionDistance,
    DirectionPerpendicular,
    DistancePerpendicular,
    DirectionObject,
    DistanceObject,
    ObjectObject,
    Perpendicular,
    DirectionAlignment,
    DistanceAlignment,
    ObjectAlignment,
    AlignmentAlignment,
}

const NAMES: [(&str, Kind); 13] = [
    ("CREATEPOINTDIRECTIONDIR", Kind::DirectionDirection),
    ("CREATEPOINTDISTANCEDIST", Kind::DistanceDistance),
    ("CREATEPOINTDIRECTIONDIST", Kind::DirectionDistance),
    ("CREATEPOINTDIRECTIONPERP", Kind::DirectionPerpendicular),
    ("CREATEPOINTDISTANCEPERP", Kind::DistancePerpendicular),
    ("CREATEPOINTDIRECTIONOBJ", Kind::DirectionObject),
    ("CREATEPOINTDISTANCEOBJECT", Kind::DistanceObject),
    ("CREATEPOINTOBJECTOBJECT", Kind::ObjectObject),
    ("CREATEPTINTERSECTPERP", Kind::Perpendicular),
    ("CREATEPOINTDIRECTIONALIGN", Kind::DirectionAlignment),
    ("CREATEPOINTDISTANCEALIGN", Kind::DistanceAlignment),
    ("CREATEPOINTOBJECTALIGN", Kind::ObjectAlignment),
    ("CREATEPOINTALIGNALIGN", Kind::AlignmentAlignment),
];

impl Kind {
    fn name(self) -> &'static str {
        NAMES.iter().find(|(_, k)| *k == self).map_or("", |(n, _)| n)
    }

    pub fn of(command: &str) -> Option<Kind> {
        NAMES.iter().find(|(n, _)| *n == command).map(|(_, k)| *k)
    }

    /// The two shapes in the order the reference asks them.
    fn legs(self) -> [Leg; 2] {
        use Leg::*;
        match self {
            Kind::DirectionDirection => [Direction, Direction],
            Kind::DistanceDistance => [Distance, Distance],
            Kind::DirectionDistance => [Distance, Direction],
            Kind::DirectionPerpendicular => [Direction, Perpendicular],
            Kind::DistancePerpendicular => [Distance, Perpendicular],
            Kind::DirectionObject => [Object, Direction],
            Kind::DistanceObject => [Object, Distance],
            Kind::ObjectObject => [Object, Object],
            Kind::Perpendicular => [Object, Perpendicular],
            Kind::DirectionAlignment => [Alignment, Direction],
            Kind::DistanceAlignment => [Alignment, Distance],
            Kind::ObjectAlignment => [Alignment, Object],
            Kind::AlignmentAlignment => [Alignment, Alignment],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Leg {
    Direction,
    Distance,
    Object,
    Alignment,
    Perpendicular,
}

#[derive(Clone, Debug)]
enum Step {
    Start,
    Direction,
    Quadrant,
    Bearing(u8),
    Azimuth,
    Offset,
    /// An offset given as two points: the first given.
    OffsetSecond(DVec3),
    Centre,
    Radius,
    Object,
    Perpendicular,
    Choose(Vec<DVec3>),
    Description,
    Elevation(Option<String>),
}

pub struct IntersectionPointCommand {
    kind: Kind,
    /// Which of the two shapes is being asked.
    leg: usize,
    step: Step,
    /// The first shape, once complete (`None` inside: a location was
    /// skipped with Enter, so nothing can meet it).
    first: Option<Option<Curve>>,
    /// The shape in progress: its location (`None` when Enter skipped it),
    /// its direction in degrees counter-clockwise from east, its radius.
    at: Option<DVec3>,
    direction: f64,
    radius: f64,
    offset: f64,
    /// The bearing quadrant Enter keeps.
    quadrant: u8,
    picked: Option<EntityType>,
    /// The picked object's curve and whether it runs from the end nearest
    /// the pick backwards (its offsets then go to the other side).
    object: Option<(Curve, bool)>,
    /// The meeting points waiting for their description and elevation.
    queue: VecDeque<DVec3>,
    description: Option<String>,
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A direction typed as degrees, minutes and seconds, spaced ("45 30 15.5").
fn dms(text: &str) -> Option<f64> {
    let parts: Vec<f64> = text.split_whitespace().map(number).collect::<Option<_>>()?;
    match parts[..] {
        [d] => Some(d),
        [d, m] => Some(d + m / 60.0),
        [d, m, s] => Some(d + m / 60.0 + s / 3600.0),
        _ => None,
    }
}

const QUADRANTS: &str = "Quadrants - NE = 1, SE = 2, SW = 3, NW = 4";

/// `curve` moved `offset` to the right of its direction of travel.
fn offset_right(curve: Curve, offset: f64) -> Option<Curve> {
    if offset == 0.0 {
        return Some(curve);
    }
    match &curve {
        Curve::Polyline(p) => {
            // The side point: right of the first segment.
            let (a, b) = (curve.point_at(0.0), curve.point_at(1e-6));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let side = [a[0] + dy, a[1] - dx];
            offset_polyline(p, offset.abs(), if offset > 0.0 { side } else { [2.0 * a[0] - side[0], 2.0 * a[1] - side[1]] })
                .into_iter()
                .next()
                .map(Curve::Polyline)
        }
        _ => offset_curve(&curve, -offset, 1e-9),
    }
}

/// The azimuth (degrees clockwise from north) of bearing `b` in quadrant `q`.
fn azimuth_of(q: u8, b: f64) -> f64 {
    [b, 180.0 - b, 180.0 + b, 360.0 - b][usize::from(q.clamp(1, 4)) - 1]
}

fn quadrant_letters(q: u8) -> (char, char) {
    match q {
        2 => ('S', 'E'),
        3 => ('S', 'W'),
        4 => ('N', 'W'),
        _ => ('N', 'E'),
    }
}

/// An azimuth written as a bearing, `S85° 00' 00"E`.
fn bearing_text(azimuth: f64) -> String {
    let a = azimuth.rem_euclid(360.0);
    let (ns, ew, b) = match a {
        a if a <= 90.0 => ('N', 'E', a),
        a if a <= 180.0 => ('S', 'E', 180.0 - a),
        a if a <= 270.0 => ('S', 'W', a - 180.0),
        a => ('N', 'W', 360.0 - a),
    };
    let total = (b * 3600.0).round() as i64;
    format!("{ns}{}° {:02}' {:02}\"{ew}", total / 3600, total / 60 % 60, total % 60)
}

/// An alignment's plan geometry in station order.
// ponytail: curves and spirals come as their solved chords; exact arcs when
// the alignment reader gives the elements' arcs.
pub(crate) fn alignment_curve(e: &EntityType) -> Option<Curve> {
    let EntityType::Extended(x) = e else { return None };
    let document::entities::ExtendedEntityData::Arch(a) = &x.data else { return None };
    if a.dxf_name != "AECC_ALIGNMENT" {
        return None;
    }
    let mut vertices: Vec<ocs_geom::geom2d::PolylineVertex> = Vec::new();
    for (_, points) in a.alignment_pieces() {
        for p in points {
            let q = [p.x, p.y];
            if vertices.last().is_none_or(|v| (v.position[0] - q[0]).hypot(v.position[1] - q[1]) > 1e-9) {
                vertices.push(ocs_geom::geom2d::PolylineVertex::straight(q));
            }
        }
    }
    (vertices.len() >= 2).then(|| Curve::Polyline(ocs_geom::geom2d::Polyline { vertices, closed: false }))
}

/// The foot of the perpendicular from `p` on `curve`; `None` when it would
/// fall past an end (measured: the prompt is asked again).
fn foot(curve: &Curve, p: DVec3) -> Option<DVec3> {
    let c = closest_point(curve, [p.x, p.y]);
    if matches!(curve.extent(), ocs_geom::geom2d::Extent::Bounded) && (c.t <= 0.0 || c.t >= 1.0) && c.distance > 1e-9 {
        // At an end: a foot only if the line to it meets the curve square.
        let inward = if c.t <= 0.0 { 1e-6 } else { 1.0 - 1e-6 };
        let q = curve.point_at(inward);
        let tangent = glam::DVec2::new(q[0] - c.point[0], q[1] - c.point[1]).normalize_or_zero();
        let to = glam::DVec2::new(p.x - c.point[0], p.y - c.point[1]).normalize_or_zero();
        if tangent.dot(to).abs() > 1e-6 {
            return None;
        }
    }
    Some(DVec3::new(c.point[0], c.point[1], 0.0))
}

impl IntersectionPointCommand {
    pub fn new(kind: Kind) -> Self {
        let mut c = Self {
            kind,
            leg: 0,
            step: Step::Start,
            first: None,
            at: None,
            direction: 0.0,
            radius: 0.0,
            offset: 0.0,
            quadrant: 1,
            picked: None,
            object: None,
            queue: VecDeque::new(),
            description: None,
        };
        c.begin_leg(0);
        c
    }

    fn current(&self) -> Leg {
        self.kind.legs()[self.leg]
    }

    fn begin_leg(&mut self, leg: usize) {
        self.leg = leg;
        if leg == 0 {
            self.first = None;
        }
        self.at = None;
        self.radius = 0.0;
        self.step = match self.current() {
            Leg::Direction => Step::Start,
            Leg::Distance => Step::Centre,
            Leg::Object | Leg::Alignment => Step::Object,
            Leg::Perpendicular => Step::Perpendicular,
        };
    }

    /// The shape the leg in progress makes.
    fn shape(&self) -> Option<Curve> {
        let at = self.at?;
        match self.current() {
            Leg::Direction => {
                let a = self.direction.to_radians();
                let line = Curve::XLine(XLine { base: [at.x, at.y], direction: [a.cos(), a.sin()] });
                offset_right(line, self.offset)
            }
            Leg::Distance => Some(Curve::Circle(Circle { centre: [at.x, at.y], radius: self.radius })),
            _ => None,
        }
    }

    /// The leg's shape is complete: ask the next leg, or meet the two.
    fn leg_done(&mut self, shape: Option<Curve>) -> CmdResult {
        if self.leg == 0 {
            // A skipped first shape ends the command before the
            // perpendicular point is asked (measured).
            if shape.is_none() && self.kind.legs()[1] == Leg::Perpendicular {
                return CmdResult::Done;
            }
            self.begin_leg(1);
            self.first = Some(shape);
            return CmdResult::NeedPoint;
        }
        let first = self.first.take().flatten();
        let points = match (first, shape) {
            (Some(a), Some(b)) => {
                // Ordered along the second shape; two circles give the point
                // left of the first centre → second centre line first
                // (measured).
                let tolerance = Tolerance::new(1e-9);
                let mut found: Vec<DVec3> = Vec::new();
                for c in intersect(&b, &a, tolerance) {
                    let p = DVec3::new(c.point[0], c.point[1], 0.0);
                    if !found.iter().any(|q| q.distance(p) < 1e-7) {
                        found.push(p);
                    }
                }
                if let (Curve::Circle(c1), Curve::Circle(c2)) = (&a, &b) {
                    let (o, d) = (glam::DVec2::from(c1.centre), glam::DVec2::from(c2.centre) - glam::DVec2::from(c1.centre));
                    let left = |p: &DVec3| -d.perp_dot(glam::DVec2::new(p.x, p.y) - o);
                    found.sort_by(|p, q| left(p).total_cmp(&left(q)));
                }
                found
            }
            _ => Vec::new(),
        };
        self.found(points)
    }

    fn found(&mut self, points: Vec<DVec3>) -> CmdResult {
        match points.len() {
            // Nothing meets: the command ends (measured, no message).
            0 => CmdResult::Done,
            1 => self.ask(points),
            _ => {
                self.step = Step::Choose(points);
                CmdResult::NeedPoint
            }
        }
    }

    fn ask(&mut self, points: Vec<DVec3>) -> CmdResult {
        self.queue.extend(points);
        self.step = Step::Description;
        CmdResult::NeedPoint
    }

    /// The next point's prompts, or the command over again.
    fn next(&mut self) {
        if !self.queue.is_empty() {
            self.step = Step::Description;
        } else {
            self.begin_leg(0);
        }
    }

    fn offset_given(&mut self, o: f64) -> CmdResult {
        self.offset = o;
        let shape = match self.current() {
            Leg::Direction => self.shape(),
            _ => self.object.take().and_then(|(c, reversed)| offset_right(c, if reversed { -o } else { o })),
        };
        self.leg_done(shape)
    }
}

impl CadCommand for IntersectionPointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => t("Specify start point:"),
            Step::Direction => t("Specify direction at start point or [Bearing/aZimuth]:"),
            Step::Quadrant => t("Specify quadrant (1-4) or [aZimuth/Angle]:"),
            Step::Bearing(_) => t("Specify bearing or [aZimuth/Angle]:"),
            Step::Azimuth => t("Specify azimuth or [Bearing/Angle]:"),
            Step::Offset => crate::tf!("Specify an offset <{:.3}>:", self.offset).into_owned(),
            Step::OffsetSecond(_) => crate::tf!("Specify second point <{:.3}>:", self.offset).into_owned(),
            Step::Centre => t("Please specify a location for the radial point:"),
            Step::Radius => crate::tf!("Enter radius <{:.3}>:", self.radius).into_owned(),
            Step::Object if self.current() == Leg::Alignment => t("Select an alignment object:"),
            Step::Object => t("Select an arc, line, polyline, lot line, or feature line:"),
            Step::Perpendicular => t("Please specify a location for the perpendicular point:"),
            Step::Choose(_) => t("Point or [All] <All>:"),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
            Step::Elevation(_) => t("Specify a point elevation <.>:"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Direction => vec![CmdOption::new("Bearing", "B"), CmdOption::new("aZimuth", "Z")],
            Step::Quadrant | Step::Bearing(_) => vec![CmdOption::new("aZimuth", "Z"), CmdOption::new("Angle", "A")],
            Step::Azimuth => vec![CmdOption::new("Bearing", "B"), CmdOption::new("Angle", "A")],
            Step::Choose(_) => vec![CmdOption::new("All", "A")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Start | Step::Centre | Step::Object | Step::Perpendicular => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    /// A direction and a choice also take a point.
    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Direction | Step::Choose(_) | Step::Offset | Step::OffsetSecond(_))
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Object)
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    /// An alignment prompt takes only alignments; an object prompt only
    /// curves (a typed point finds the nearest of those).
    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        match (&self.step, self.current()) {
            (Step::Object, Leg::Alignment) => Some(|e| alignment_curve(e).is_some()),
            // An alignment is found too, then refused (measured).
            (Step::Object, _) => Some(|e| alignment_curve(e).is_some() || entity_curve_xy(e).is_some()),
            _ => None,
        }
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: Handle, pick: DVec3) -> CmdResult {
        let alignment = self.current() == Leg::Alignment;
        let Some(curve) = self.picked.take().filter(|e| alignment || alignment_curve(e).is_none()).and_then(|e| if alignment { alignment_curve(&e) } else { entity_curve_xy(&e) }) else {
            return CmdResult::ReportError(t("ERROR: Invalid selection set!"));
        };
        // An object runs from its end nearest the pick (the end on a tie);
        // an alignment runs with its stations.
        let (start, end) = (curve.point_at(0.0), curve.point_at(1.0));
        let near = |p: [f64; 2]| (p[0] - pick.x).hypot(p[1] - pick.y);
        let reversed = !alignment && near(start) >= near(end);
        // Perpendicular: the object itself, with no offset; the foot must
        // fall on it (measured).
        if self.kind == Kind::Perpendicular {
            return self.leg_done(Some(curve));
        }
        // A picked line runs on past its ends (measured).
        let curve = match curve {
            Curve::Line(l) => Curve::XLine(XLine { base: l.start, direction: [l.end[0] - l.start[0], l.end[1] - l.start[1]] }),
            c => c,
        };
        self.object = Some((curve, reversed));
        self.offset = if self.leg == 0 { 0.0 } else { self.offset };
        self.step = Step::Offset;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pt = DVec3::new(pt.x, pt.y, 0.0);
        match std::mem::replace(&mut self.step, Step::Start) {
            Step::Start => {
                self.at = Some(pt);
                self.step = Step::Direction;
            }
            Step::Direction => {
                let from = self.at.unwrap_or(DVec3::ZERO);
                let d = pt - from;
                if d.length() > 0.0 {
                    self.direction = d.y.atan2(d.x).to_degrees();
                }
                self.step = Step::Offset;
                if self.leg == 0 {
                    self.offset = 0.0;
                }
            }
            Step::Centre => {
                self.at = Some(pt);
                self.step = Step::Radius;
            }
            Step::Perpendicular => {
                let first = self.first.clone().flatten();
                self.step = Step::Perpendicular;
                return match first {
                    Some(c) => match foot(&c, pt) {
                        Some(f) => {
                            self.first = None;
                            self.ask(vec![f])
                        }
                        None => CmdResult::NeedPoint,
                    },
                    None => CmdResult::Done,
                };
            }
            Step::Offset => self.step = Step::OffsetSecond(pt),
            Step::OffsetSecond(a) => return self.offset_given(a.distance(pt)),
            Step::Choose(points) => {
                let nearest = points.iter().copied().min_by(|a, b| a.distance(pt).total_cmp(&b.distance(pt)));
                return self.ask(nearest.into_iter().collect());
            }
            other => self.step = other,
        }
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let word = typed.to_ascii_uppercase();
        let invalid = || Some(CmdResult::ReportError(t("Invalid input")));
        match self.step.clone() {
            Step::Start | Step::Centre | Step::Object | Step::Perpendicular => return None,
            Step::Direction => {
                match word.as_str() {
                    "B" | "BEARING" => {
                        self.step = Step::Quadrant;
                        return Some(CmdResult::ReportMessage(t(QUADRANTS)));
                    }
                    "Z" | "AZIMUTH" => self.step = Step::Azimuth,
                    // Enter: east (measured, not the last direction).
                    "" => {
                        self.direction = 0.0;
                        self.step = Step::Offset;
                    }
                    _ => match number(typed) {
                        Some(a) => {
                            self.direction = a;
                            self.step = Step::Offset;
                        }
                        None => return invalid(),
                    },
                }
                if matches!(self.step, Step::Offset) && self.leg == 0 {
                    self.offset = 0.0;
                }
            }
            Step::Quadrant | Step::Bearing(_) | Step::Azimuth => {
                match word.as_str() {
                    "B" | "BEARING" => {
                        self.step = Step::Quadrant;
                        return Some(CmdResult::ReportMessage(t(QUADRANTS)));
                    }
                    "Z" | "AZIMUTH" => self.step = Step::Azimuth,
                    // Angle hands the main prompt back: a typed number there is an angle.
                    "A" | "ANGLE" => self.step = Step::Direction,
                    _ => {
                        let value = match self.step {
                            Step::Quadrant => match if typed.is_empty() { Ok(self.quadrant) } else { typed.parse::<u8>() } {
                                Ok(q @ 1..=4) => {
                                    self.quadrant = q;
                                    self.step = Step::Bearing(q);
                                    return Some(CmdResult::NeedPoint);
                                }
                                _ => return Some(CmdResult::ReportError(format!("{}\n{}", t("Invalid quadrant. Please try again"), t(QUADRANTS)))),
                            },
                            // A bearing turns from north or south toward east or west.
                            // Enter at a bearing or azimuth gives east, as at the main prompt (measured).
                            Step::Bearing(_) if typed.is_empty() => 0.0,
                            Step::Bearing(q) => match dms(typed) {
                                Some(b) if (0.0..=90.0).contains(&b) => 90.0 - azimuth_of(q, b),
                                Some(b) => {
                                    let (ns, ew) = quadrant_letters(q);
                                    return Some(CmdResult::ReportError(format!(
                                        "{}
{}",
                                        crate::tf!("'Transparent Commands Bearing Property' validation error - value '{}' out of range.", bearing_text(azimuth_of(q, b))),
                                        crate::tf!("Please enter a value greater than or equal to {} and less than or equal to {}", format!("{ns}0° 00' 00\"{ew}"), format!("{ns}90° 00' 00\"{ew}")),
                                    )));
                                }
                                None => return invalid(),
                            },
                            // Text it cannot read asks again, silently (measured).
                            Step::Azimuth if typed.is_empty() => 0.0,
                            Step::Azimuth => match dms(typed) {
                                Some(a) => 90.0 - a,
                                None => return Some(CmdResult::NeedPoint),
                            },
                            _ => return None,
                        };
                        self.direction = value;
                        self.step = Step::Offset;
                        if self.leg == 0 {
                            self.offset = 0.0;
                        }
                    }
                }
            }
            Step::Offset | Step::OffsetSecond(_) => {
                let o = if typed.is_empty() { Some(self.offset) } else { number(typed) };
                return Some(match o {
                    Some(o) => self.offset_given(o),
                    None => CmdResult::ReportError(crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned()),
                });
            }
            Step::Radius => {
                let r = if typed.is_empty() { Some(0.0) } else { number(typed) };
                return Some(match r {
                    Some(r) if r < 0.0 => CmdResult::ReportError(t("'Distance' validation error.  Please enter a positive value.")),
                    Some(r) => {
                        self.radius = r;
                        let shape = self.shape();
                        self.leg_done(shape)
                    }
                    None => CmdResult::ReportError(t("Invalid input")),
                });
            }
            Step::Choose(points) => {
                return Some(match word.as_str() {
                    "" | "A" | "ALL" => self.ask(points),
                    _ => CmdResult::ReportError(t("Invalid input")),
                });
            }
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                self.step = Step::Elevation(d);
            }
            Step::Elevation(d) => {
                let elevation = match typed {
                    "" | "." => None,
                    _ => match number(typed) {
                        Some(z) => Some(z),
                        None => return invalid(),
                    },
                };
                let at = self.queue.pop_front()?;
                self.next();
                return Some(CmdResult::AddCogoPoint { position: at, description: d, elevation, echo: false, then: None });
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Enter skips the location: nothing can meet the shape later.
            Step::Start => {
                self.at = None;
                self.step = Step::Direction;
                CmdResult::NeedPoint
            }
            Step::Centre => {
                self.at = None;
                self.step = Step::Radius;
                CmdResult::NeedPoint
            }
            Step::Object => CmdResult::Done,
            Step::Perpendicular => CmdResult::NeedPoint,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        // Esc at a point's description or elevation drops the points still
        // waiting and starts over; anywhere else it ends (measured).
        match self.step {
            Step::Description | Step::Elevation(_) => {
                self.queue.clear();
                self.begin_leg(0);
                CmdResult::ReportMessage(t("*Cancel*"))
            }
            _ => CmdResult::Cancel,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "CREATEPOINTDIRECTIONDIR",
        "CREATEPOINTDISTANCEDIST",
        "CREATEPOINTDIRECTIONDIST",
        "CREATEPOINTDIRECTIONPERP",
        "CREATEPOINTDISTANCEPERP",
        "CREATEPOINTDIRECTIONOBJ",
        "CREATEPOINTDISTANCEOBJECT",
        "CREATEPOINTOBJECTOBJECT",
        "CREATEPTINTERSECTPERP",
        "CREATEPOINTDIRECTIONALIGN",
        "CREATEPOINTDISTANCEALIGN",
        "CREATEPOINTOBJECTALIGN",
        "CREATEPOINTALIGNALIGN",
    ]
});
