//! The point creation commands that place COGO points from objects: each
//! picks an object (Automatic a set), works out locations from it, and asks
//! every point's description and elevation as CREATEPOINTMANUAL does before
//! writing it. Esc goes back one prompt; at the object prompt it ends.

use crate::command::{CadCommand, CmdResult, InputKind, SelectionEntity};
use crate::entities::curve::entity_curve;
use document::{EntityType, Handle};
use glam::DVec3;
use ocs_geom::space::PlanarCurve;
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// A station and offset along the object.
    StationOffset,
    /// A distance along the object.
    Along,
    /// The object's two ends.
    OnObject,
    /// The ends (and an arc's centre) of every selected line and arc.
    Automatic,
    /// Equal segments along the object.
    Divide,
    /// An interval along the object between two stations.
    Measure,
    /// A polyline's vertices at one elevation asked first.
    PolylineManual,
    /// A polyline's vertices at the polyline's own elevation.
    PolylineAuto,
    /// The station seeing three known points under two measured angles.
    Resection,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::StationOffset => "CREATEPTSTATIONOFFSETOBJ",
            Kind::Along => "CREATEPTALONGLNCURVESPIR",
            Kind::OnObject => "CREATEPTONLINECURVESPIRAL",
            Kind::Automatic => "CREATEPOINTAUTOMATIC",
            Kind::Divide => "CREATEPOINTDIVIDEOBJECT",
            Kind::Measure => "CREATEPOINTMEASUREOBJECT",
            Kind::PolylineManual => "CREATEPTPLYLINECTRVERTMAN",
            Kind::PolylineAuto => "CREATEPTPLYLNCTRVERTAUTO",
            Kind::Resection => "CREATEPOINTRESECTION",
        }
    }

    pub fn of(command: &str) -> Option<Kind> {
        Some(match command {
            "CREATEPTSTATIONOFFSETOBJ" => Kind::StationOffset,
            "CREATEPTALONGLNCURVESPIR" => Kind::Along,
            "CREATEPTONLINECURVESPIRAL" => Kind::OnObject,
            "CREATEPOINTAUTOMATIC" => Kind::Automatic,
            "CREATEPOINTDIVIDEOBJECT" => Kind::Divide,
            "CREATEPOINTMEASUREOBJECT" => Kind::Measure,
            "CREATEPTPLYLINECTRVERTMAN" => Kind::PolylineManual,
            "CREATEPTPLYLNCTRVERTAUTO" => Kind::PolylineAuto,
            "CREATEPOINTRESECTION" => Kind::Resection,
            _ => return None,
        })
    }
}

/// The picked object's curve, read from its end nearest the pick.
struct Path {
    curve: PlanarCurve,
    reversed: bool,
}

impl Path {
    fn length(&self) -> f64 {
        self.curve.length()
    }

    /// The point `d` along the path, moved `offset` to the right of the
    /// direction of travel; past either end the path runs on straight.
    fn at(&self, d: f64, offset: f64) -> DVec3 {
        let len = self.length();
        let inner = d.clamp(0.0, len);
        let raw = if self.reversed { len - inner } else { inner };
        let p = DVec3::from_array(self.curve.point_at_distance(raw));
        let mut t = DVec3::from_array(self.curve.tangent_at(self.curve.parameter_at_distance(raw)));
        t.z = 0.0;
        let mut t = t.normalize_or_zero();
        if self.reversed {
            t = -t;
        }
        let p = p + t * (d - inner) + DVec3::new(t.y, -t.x, 0.0) * offset;
        DVec3::new(p.x, p.y, 0.0)
    }
}

#[derive(Clone)]
enum Step {
    DefaultElevation,
    Object,
    StartStation,
    EndStation,
    Station,
    Segments,
    Offset,
    Interval,
    Distance,
    Description,
    Elevation(Option<String>),
    /// Resection: the second and third known points, then the two angles.
    Second(DVec3),
    Third(DVec3, DVec3),
    FirstAngle([DVec3; 3]),
    SecondAngle([DVec3; 3]),
}

pub struct ObjectPointCommand {
    kind: Kind,
    step: Step,
    picked: Option<EntityType>,
    picked_set: Vec<(Handle, EntityType)>,
    path: Option<Path>,
    start_station: f64,
    end_station: f64,
    station: f64,
    segments: u32,
    offset: f64,
    interval: f64,
    /// Points worked out and waiting for their description (and elevation
    /// when it is not known yet).
    queue: VecDeque<(DVec3, Option<f64>)>,
    /// Polyline vertices - manual: the elevation every vertex takes.
    default_elevation: f64,
    /// Resection: the angles offered, the last ones given.
    angles: (f64, f64),
    description: Option<String>,
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Three-point resection: the station from which the first known point turns
/// clockwise by `a` degrees to the second and by `b` to the third. The
/// station lies on the circle through 1 and 2 seeing them under `a` and on
/// the one through 1 and 3 seeing them under `b`; it is point 1 mirrored in
/// the line joining the two centres.
fn resection(p: [DVec3; 3], a: f64, b: f64) -> Result<DVec3, &'static str> {
    use glam::DVec2;
    let mul = |x: DVec2, y: DVec2| DVec2::new(x.x * y.x - x.y * y.y, x.x * y.y + x.y * y.x);
    let div = |x: DVec2, y: DVec2| mul(x, DVec2::new(y.x, -y.y)) / y.length_squared();
    let colinear = |deg: f64| (deg.rem_euclid(180.0)).min(180.0 - deg.rem_euclid(180.0)) < 1e-9;
    if colinear(a) {
        return Err("Points 1 and 2 are colinear.");
    }
    if colinear(b) {
        return Err("Points 1 and 3 are colinear.");
    }
    let [p1, p2, p3] = p.map(|v| DVec2::new(v.x, v.y));
    // The centre seeing `p`–`q` under the clockwise angle `deg`.
    let centre = |q: DVec2, deg: f64| {
        let e = DVec2::from_angle((-2.0 * deg).to_radians());
        div(mul(e, p1) - q, e - DVec2::X)
    };
    let (c2, c3) = (centre(p2, a), centre(p3, b));
    let u = (c3 - c2).normalize_or_zero();
    if u == DVec2::ZERO {
        return Err("A solution does not exist.");
    }
    let r = p1 - c2;
    let s = c2 + 2.0 * u * r.dot(u) - r;
    // The circles also hold the station seeing the pair under the angle
    // plus 180°: only a station matching both angles is a solution.
    let az = |q: DVec2| (q.x - s.x).atan2(q.y - s.y).to_degrees();
    let same = |x: f64, y: f64| ((x - y).rem_euclid(360.0) + 1e-6).rem_euclid(360.0) < 2e-6;
    if s.distance(p1) < 1e-9 || !same(az(p2) - az(p1), a) || !same(az(p3) - az(p1), b) {
        return Err("A solution does not exist.");
    }
    Ok(DVec3::new(s.x, s.y, 0.0))
}

fn flat(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], 0.0)
}

impl ObjectPointCommand {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            step: if kind == Kind::PolylineManual { Step::DefaultElevation } else { Step::Object },
            picked: None,
            picked_set: Vec::new(),
            path: None,
            start_station: 0.0,
            end_station: 0.0,
            station: 0.0,
            segments: 1,
            offset: 0.0,
            interval: 10.0,
            queue: VecDeque::new(),
            default_elevation: 0.0,
            angles: (0.0, 0.0),
            description: None,
        }
    }

    /// Asks the next queued point, or goes back to where more are made.
    fn next(&mut self) -> CmdResult {
        self.step = if !self.queue.is_empty() {
            Step::Description
        } else {
            match self.kind {
                Kind::StationOffset => Step::Station,
                Kind::Along => Step::Distance,
                _ => Step::Object,
            }
        };
        CmdResult::NeedPoint
    }

    fn queue_and_ask(&mut self, points: Vec<DVec3>) -> CmdResult {
        self.queue.extend(points.into_iter().map(|p| (p, None)));
        self.next()
    }

    /// A polyline's vertices with the elevation each point takes: the one
    /// asked first (manual) or the polyline's own (automatic).
    fn polyline_vertices(&self, e: &EntityType) -> Option<Vec<(DVec3, Option<f64>)>> {
        let points: Vec<DVec3> = match e {
            EntityType::LwPolyline(p) => p.vertices.iter().map(|v| DVec3::new(v.location.x, v.location.y, p.elevation)).collect(),
            EntityType::Polyline2D(p) => p.vertices.iter().map(|v| DVec3::new(v.location.x, v.location.y, p.elevation)).collect(),
            EntityType::Polyline3D(p) => p.vertices.iter().map(|v| DVec3::new(v.position.x, v.position.y, v.position.z)).collect(),
            _ => return None,
        };
        let manual = self.kind == Kind::PolylineManual;
        Some(points.into_iter().map(|p| (DVec3::new(p.x, p.y, 0.0), Some(if manual { self.default_elevation } else { p.z }))).collect())
    }

    /// The ends of a line and an arc's end, start and centre, in the order
    /// the reference writes them.
    fn automatic_points(e: &EntityType) -> Vec<DVec3> {
        match e {
            EntityType::Line(l) => vec![DVec3::new(l.start.x, l.start.y, 0.0), DVec3::new(l.end.x, l.end.y, 0.0)],
            EntityType::Arc(a) => {
                let at = |angle: f64| DVec3::new(a.center.x + a.radius * angle.cos(), a.center.y + a.radius * angle.sin(), 0.0);
                vec![at(a.end_angle), at(a.start_angle), DVec3::new(a.center.x, a.center.y, 0.0)]
            }
            _ => Vec::new(),
        }
    }

    /// Divide: the `segments + 1` points from end to end.
    fn divided(&self) -> Vec<DVec3> {
        let Some(path) = &self.path else { return Vec::new() };
        let n = self.segments.max(1);
        (0..=n).map(|i| path.at(path.length() * f64::from(i) / f64::from(n), self.offset)).collect()
    }

    /// Measure: every interval from the starting station up to the ending
    /// one; the starting station is the path's start.
    fn measured(&self) -> Vec<DVec3> {
        let Some(path) = &self.path else { return Vec::new() };
        let mut out = Vec::new();
        let mut s = self.start_station;
        while s <= self.end_station + 1e-9 && out.len() < 100_000 {
            out.push(path.at(s - self.start_station, self.offset));
            s += self.interval;
        }
        out
    }

    /// A value typed at a numbered prompt (Enter keeps `default`).
    fn value(typed: &str, default: f64) -> Option<f64> {
        if typed.is_empty() {
            Some(default)
        } else {
            number(typed)
        }
    }
}

impl CadCommand for ObjectPointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::DefaultElevation => t("Default Elevation <.>:"),
            Step::Object if self.kind == Kind::Resection => t("Specify first (backsight or reference) point:"),
            Step::Second(_) => t("Specify second point:"),
            Step::Third(..) => t("Specify third point:"),
            Step::FirstAngle(_) => crate::tf!("Specify the angle between the first and second point <{:.4} (d)>:", self.angles.0).into_owned(),
            Step::SecondAngle(_) => crate::tf!("Specify the angle between the first and third point <{:.4} (d)>:", self.angles.1).into_owned(),
            Step::Object if self.kind == Kind::Automatic => t("Select arcs, lines, lot lines, or feature lines:"),
            Step::Object if matches!(self.kind, Kind::PolylineManual | Kind::PolylineAuto) => t("Select a polyline object:"),
            Step::Object => t("Select an arc, line, polyline, lot line, or feature line:"),
            Step::StartStation => crate::tf!("Starting station <{:.3}>:", self.start_station).into_owned(),
            Step::EndStation => crate::tf!("Ending station <{:.3}>:", self.end_station).into_owned(),
            Step::Station => crate::tf!("Specify a desired station <{:.3}>:", self.station).into_owned(),
            Step::Segments => crate::tf!("Enter the number of segments <{}>:", self.segments).into_owned(),
            Step::Offset => crate::tf!("Specify an offset <{:.3}>:", self.offset).into_owned(),
            Step::Interval => crate::tf!("Enter an interval <{:.3}>:", self.interval).into_owned(),
            Step::Distance => t("Specify a distance:"),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
            Step::Elevation(_) => t("Specify a point elevation <.>:"),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Object | Step::Second(_) | Step::Third(..) => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Object) && !matches!(self.kind, Kind::Automatic | Kind::Resection)
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Object) && self.kind == Kind::Automatic
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        if self.kind == Kind::Automatic {
            Some(|e| matches!(e, EntityType::Line(_) | EntityType::Arc(_)))
        } else {
            None
        }
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.picked_set = entities.into_iter().map(|e| (e.handle, e.entity)).collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        // The set keeps growing until Enter, as the reference's does.
        CmdResult::NeedPoint
    }

    fn on_entity_pick(&mut self, _handle: Handle, pick: DVec3) -> CmdResult {
        if matches!(self.kind, Kind::PolylineManual | Kind::PolylineAuto) {
            if let Some(points) = self.picked.take().and_then(|e| self.polyline_vertices(&e)) {
                self.queue.extend(points);
            }
            return self.next();
        }
        let Some(curve) = self.picked.take().and_then(|e| entity_curve(&e)) else {
            return CmdResult::NeedPoint;
        };
        let len = curve.length();
        let start = flat(curve.point_at_distance(0.0));
        let end = flat(curve.point_at_distance(len));
        let pick = DVec3::new(pick.x, pick.y, 0.0);
        // Stations run from the end nearest the pick (the end on a tie).
        let reversed = pick.distance(start) >= pick.distance(end);
        if self.kind == Kind::OnObject {
            return self.queue_and_ask(vec![start, end]);
        }
        self.path = Some(Path { curve, reversed });
        self.end_station = len;
        self.step = match self.kind {
            Kind::Along => Step::Distance,
            Kind::Divide => Step::Segments,
            _ => Step::StartStation,
        };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pt = DVec3::new(pt.x, pt.y, 0.0);
        match self.step {
            Step::Object if self.kind == Kind::Resection => {
                self.step = Step::Second(pt);
                CmdResult::NeedPoint
            }
            Step::Second(a) => {
                self.step = Step::Third(a, pt);
                CmdResult::NeedPoint
            }
            Step::Third(a, b) => {
                self.step = Step::FirstAngle([a, b, pt]);
                CmdResult::NeedPoint
            }
            Step::Object => CmdResult::NeedPoint,
            _ => CmdResult::ReportError(t("Invalid input")),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let bad = Some(CmdResult::ReportError(t("Invalid input")));
        match self.step.clone() {
            Step::Object | Step::Second(_) | Step::Third(..) => return None,
            Step::FirstAngle(p) => match Self::value(typed, self.angles.0) {
                Some(a) => {
                    self.angles.0 = a;
                    self.step = Step::SecondAngle(p);
                }
                None => return bad,
            },
            Step::SecondAngle(p) => match Self::value(typed, self.angles.1) {
                Some(b) => {
                    self.angles.1 = b;
                    return Some(match resection(p, self.angles.0, b) {
                        Ok(at) => self.queue_and_ask(vec![at]),
                        Err(why) => {
                            self.step = Step::Object;
                            CmdResult::ReportError(format!("{}
{}", t("Resection point is unsolvable"), t(why)))
                        }
                    });
                }
                None => return bad,
            },
            // "." or Enter: no elevation given, the vertices take 0 (measured).
            Step::DefaultElevation => match typed {
                "" | "." => {
                    self.default_elevation = 0.0;
                    self.step = Step::Object;
                }
                _ => match number(typed) {
                    Some(z) => {
                        self.default_elevation = z;
                        self.step = Step::Object;
                    }
                    None => return bad,
                },
            },
            Step::StartStation => match Self::value(typed, self.start_station) {
                Some(s) => {
                    self.start_station = s;
                    // The ending station offered is the path's far end.
                    self.end_station = s + self.path.as_ref().map_or(0.0, Path::length);
                    self.step = if self.kind == Kind::Measure { Step::EndStation } else { Step::Station };
                }
                None => return bad,
            },
            Step::EndStation => match Self::value(typed, self.end_station) {
                Some(s) => {
                    self.end_station = s;
                    self.step = Step::Offset;
                }
                None => return bad,
            },
            Step::Station => match Self::value(typed, self.station) {
                Some(s) => {
                    self.station = s;
                    self.step = Step::Offset;
                }
                None => return bad,
            },
            Step::Segments => match if typed.is_empty() { Some(self.segments) } else { typed.parse::<u32>().ok().filter(|n| *n > 0) } {
                Some(n) => {
                    self.segments = n;
                    self.step = Step::Offset;
                }
                None => return bad,
            },
            Step::Offset => match Self::value(typed, self.offset) {
                Some(o) => {
                    self.offset = o;
                    let points = match self.kind {
                        Kind::Divide => self.divided(),
                        Kind::Measure => {
                            self.step = Step::Interval;
                            return Some(CmdResult::NeedPoint);
                        }
                        _ => vec![self.path.as_ref()?.at(self.station - self.start_station, o)],
                    };
                    return Some(self.queue_and_ask(points));
                }
                None => return bad,
            },
            Step::Interval => match Self::value(typed, self.interval).filter(|v| *v > 0.0) {
                Some(v) => {
                    self.interval = v;
                    let points = self.measured();
                    return Some(self.queue_and_ask(points));
                }
                None => return bad,
            },
            Step::Distance => match number(typed) {
                Some(d) => {
                    let p = self.path.as_ref()?.at(d, 0.0);
                    return Some(self.queue_and_ask(vec![p]));
                }
                None => return bad,
            },
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                // A polyline vertex already has its elevation.
                if let Some(&(at, Some(z))) = self.queue.front() {
                    self.queue.pop_front();
                    self.next();
                    return Some(CmdResult::AddCogoPoint { position: at, description: d, elevation: Some(z), echo: false, then: None });
                }
                self.step = Step::Elevation(d);
            }
            Step::Elevation(d) => {
                let elevation = match typed {
                    "" | "." => None,
                    _ => match number(typed) {
                        Some(z) => Some(z),
                        None => return bad,
                    },
                };
                let (at, _) = self.queue.pop_front()?;
                self.next();
                return Some(CmdResult::AddCogoPoint { position: at, description: d, elevation, echo: false, then: None });
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Object if self.kind == Kind::Automatic && !self.picked_set.is_empty() => {
                // The drawing's order: the reference writes older objects first.
                let mut set = std::mem::take(&mut self.picked_set);
                set.sort_by_key(|(h, _)| h.value());
                let points = set.iter().flat_map(|(_, e)| Self::automatic_points(e)).collect();
                self.queue_and_ask(points)
            }
            Step::Object => CmdResult::Done,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        // Each Esc goes back one prompt (measured: desired station →
        // starting station → object, distance → object); the points in
        // progress are dropped.
        self.queue.clear();
        self.step = match (self.kind, &self.step) {
            (_, Step::Object | Step::DefaultElevation) => return CmdResult::Cancel,
            (Kind::StationOffset, Step::Station) => Step::StartStation,
            (Kind::StationOffset, Step::Offset | Step::Description | Step::Elevation(_)) => Step::Station,
            (Kind::Along, Step::Description | Step::Elevation(_)) => Step::Distance,
            _ => Step::Object,
        };
        CmdResult::ReportMessage(t("*Cancel*"))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "CREATEPTSTATIONOFFSETOBJ",
        "CREATEPTALONGLNCURVESPIR",
        "CREATEPTONLINECURVESPIRAL",
        "CREATEPOINTAUTOMATIC",
        "CREATEPOINTDIVIDEOBJECT",
        "CREATEPOINTMEASUREOBJECT",
        "CREATEPTPLYLINECTRVERTMAN",
        "CREATEPTPLYLNCTRVERTAUTO",
        "CREATEPOINTRESECTION",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::Line;
    use document::types::Vector3;

    fn line() -> EntityType {
        EntityType::Line(Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(100.0, 0.0, 0.0)))
    }

    fn points(c: &mut ObjectPointCommand, inputs: &[&str]) -> Vec<DVec3> {
        inputs
            .iter()
            .filter_map(|i| match c.on_text_input(i) {
                Some(CmdResult::AddCogoPoint { position, .. }) => Some(position),
                _ => None,
            })
            .collect()
    }

    fn picked(kind: Kind, at: f64) -> ObjectPointCommand {
        let mut c = ObjectPointCommand::new(kind);
        c.inject_picked_entity(line());
        c.on_entity_pick(Handle::NULL, DVec3::new(at, 0.0, 0.0));
        c
    }

    #[test]
    fn resection_finds_the_measured_station() {
        // Measured: (0,0), (100,0), (50,70) under 223.602819° and
        // 111.801409° (clockwise from the first) → (50,20); the
        // counter-clockwise pair has no solution.
        let p = [DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(50.0, 70.0, 0.0)];
        let s = resection(p, 223.602819, 111.801409).unwrap();
        assert!((s.x - 50.0).abs() < 1e-4 && (s.y - 20.0).abs() < 1e-4);
        assert_eq!(resection(p, 136.397181, 248.198591), Err("A solution does not exist."));
        assert_eq!(resection(p, 0.0, 0.0), Err("Points 1 and 2 are colinear."));
    }

    #[test]
    fn object_points_land_where_measured() {
        // Station/offset picked at the middle (a tie: from the end): 25, 5 →
        // (75,5); 60, -5 → (40,-5).
        let mut c = picked(Kind::StationOffset, 50.0);
        assert_eq!(points(&mut c, &["", "25", "5", "", "", "60", "-5", "", ""]), vec![DVec3::new(75.0, 5.0, 0.0), DVec3::new(40.0, -5.0, 0.0)]);
        // Along from the end nearest the pick.
        let mut c = picked(Kind::Along, 10.0);
        assert_eq!(points(&mut c, &["30", "", "", "60", "", ""]), vec![DVec3::new(30.0, 0.0, 0.0), DVec3::new(60.0, 0.0, 0.0)]);
        // Divide in 4: five points end to end.
        let mut c = picked(Kind::Divide, 10.0);
        let p = points(&mut c, &["4", "", "", "", "", "", "", "", "", "", "", ""]);
        assert_eq!(p.iter().map(|p| p.x).collect::<Vec<_>>(), vec![0.0, 25.0, 50.0, 75.0, 100.0]);
        // Measure every 30 from 0 to 100: 0, 30, 60, 90.
        let mut c = picked(Kind::Measure, 10.0);
        let p = points(&mut c, &["", "", "", "30", "", "", "", "", "", "", "", ""]);
        assert_eq!(p.iter().map(|p| p.x).collect::<Vec<_>>(), vec![0.0, 30.0, 60.0, 90.0]);
        // From the far end: stations 10..70 every 25, offset 2 → x 100, 75, 50 at y 2.
        let mut c = picked(Kind::Measure, 90.0);
        let p = points(&mut c, &["10", "70", "2", "25", "", "", "", "", "", ""]);
        assert_eq!(p, vec![DVec3::new(100.0, 2.0, 0.0), DVec3::new(75.0, 2.0, 0.0), DVec3::new(50.0, 2.0, 0.0)]);
    }
}
