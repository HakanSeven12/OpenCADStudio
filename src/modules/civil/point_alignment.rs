//! The point creation commands that place COGO points along an alignment:
//! at a station and offset, at equal divisions, at a station interval, at
//! the alignment's geometry points, radial or perpendicular to it, from a
//! station/offset file, and at a profile's geometry points. Divide, Measure,
//! At Geometry Points, Radial or Perpendicular and Profile Geometry Points
//! ask for a profile of the alignment when it has a profile view: a point
//! within the profile's stations takes its elevation from it.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::entities::curve::entity_curve;
use document::entities::{ExtendedEntityData, LwPolyline, LwVertex};
use document::types::Vector2;
use document::{EntityType, Handle};
use glam::{DVec2, DVec3};
use ocs_geom::geom2d::Curve;
use ocs_geom::space::PlanarCurve;
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    StationOffset,
    Divide,
    Measure,
    GeometryPoints,
    Radial,
    Import,
    ProfileGeometry,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::StationOffset => "CREATEPOINTSTATIONOFFSET",
            Kind::Divide => "CREATEPOINTDIVIDEALIGN",
            Kind::Measure => "CREATEPOINTMEASUREALIGN",
            Kind::GeometryPoints => "CREATEPOINTATGEOMETRYPOINTS",
            Kind::Radial => "CREATEPOINTRADIALORPERP",
            Kind::Import => "CREATEPOINTIMPORTFROMFILE",
            Kind::ProfileGeometry => "CREATEPROFILEGEOMPOINTS",
        }
    }

    pub fn of(command: &str) -> Option<Kind> {
        Some(match command {
            "CREATEPOINTSTATIONOFFSET" => Kind::StationOffset,
            "CREATEPOINTDIVIDEALIGN" => Kind::Divide,
            "CREATEPOINTMEASUREALIGN" => Kind::Measure,
            "CREATEPOINTATGEOMETRYPOINTS" => Kind::GeometryPoints,
            "CREATEPOINTRADIALORPERP" => Kind::Radial,
            "CREATEPOINTIMPORTFROMFILE" => Kind::Import,
            "CREATEPROFILEGEOMPOINTS" => Kind::ProfileGeometry,
            _ => return None,
        })
    }

    /// The commands that ask which profile gives the elevations.
    fn asks_profile(self) -> bool {
        self != Kind::Import
    }
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A station as the reference writes it: hundreds, `+`, the rest with two
/// decimals (`4+35.24`, `-0+50.00`).
pub fn format_station(s: f64) -> String {
    let cents = (s.abs() * 100.0).round() as i64;
    let sign = if s < 0.0 && cents > 0 { "-" } else { "" };
    format!("{sign}{}+{:02}.{:02}", cents / 10_000, cents / 100 % 100, cents % 100)
}

/// A profile of the picked alignment: its PVIs (station, elevation, curve
/// length) and the polylines it is drawn as in each profile view.
#[derive(Clone)]
struct Profile {
    name: String,
    pvis: Vec<(f64, f64, f64)>,
    drawn: Vec<Vec<DVec2>>,
}

impl Profile {
    /// The elevation at `station`: the tangents, and a symmetric parabola
    /// over each PVI with a vertical curve. None outside the profile.
    fn elevation(&self, station: f64) -> Option<f64> {
        let (first, last) = (self.pvis.first()?, self.pvis.last()?);
        if station < first.0 - 1e-9 || station > last.0 + 1e-9 {
            return None;
        }
        let grade = |i: usize| {
            let (a, b) = (self.pvis[i], self.pvis[i + 1]);
            (b.1 - a.1) / (b.0 - a.0)
        };
        for i in 1..self.pvis.len().saturating_sub(1) {
            let (s, z, l) = self.pvis[i];
            let half = l / 2.0;
            if half > 0.0 && (s - half..=s + half).contains(&station) {
                let (g1, g2) = (grade(i - 1), grade(i));
                let x = station - (s - half);
                return Some(z - g1 * half + g1 * x + (g2 - g1) / (2.0 * l) * x * x);
            }
        }
        let i = self.pvis.partition_point(|p| p.0 <= station).clamp(1, self.pvis.len() - 1);
        let (a, b) = (self.pvis[i - 1], self.pvis[i]);
        Some(if b.0 > a.0 { a.1 + (b.1 - a.1) * (station - a.0) / (b.0 - a.0) } else { a.1 })
    }

    /// The profile's geometry points: its ends, a PVI without a vertical
    /// curve, and the start and end of every vertical curve (measured).
    fn geometry_stations(&self) -> Vec<f64> {
        let n = self.pvis.len();
        let mut out = Vec::new();
        for (i, &(s, _, l)) in self.pvis.iter().enumerate() {
            if i == 0 || i == n - 1 || l <= 0.0 {
                out.push(s);
            } else {
                out.extend([s - l / 2.0, s + l / 2.0]);
            }
        }
        out
    }
}

/// The picked alignment: its plan geometry from its start, the station
/// there, its solved pieces and its profiles.
struct Alignment {
    path: PlanarCurve,
    curve: Curve,
    start: f64,
    pieces: Vec<(document::classes::civil::AlignmentPiece, f64, Vec<DVec2>)>,
    profiles: Vec<Profile>,
    /// Whether one of its profile views shows it.
    viewed: bool,
}

impl Alignment {
    fn length(&self) -> f64 {
        self.path.length()
    }

    fn end(&self) -> f64 {
        self.start + self.length()
    }

    fn contains(&self, station: f64) -> bool {
        (self.start - 1e-9..=self.end() + 1e-9).contains(&station)
    }

    fn tangent(&self, d: f64) -> DVec2 {
        let t = self.path.tangent_at(self.path.parameter_at_distance(d.clamp(0.0, self.length())));
        DVec2::new(t[0], t[1]).normalize_or_zero()
    }

    /// The point at `station`, `offset` to the right of the alignment.
    fn at(&self, station: f64, offset: f64) -> DVec3 {
        let d = (station - self.start).clamp(0.0, self.length());
        let p = self.path.point_at_distance(d);
        let t = self.tangent(d);
        DVec3::new(p[0] + t.y * offset, p[1] - t.x * offset, 0.0)
    }

    /// The station and right offset of `p`; none when its foot falls
    /// beyond either end.
    fn station_offset(&self, p: DVec2) -> Option<(f64, f64)> {
        let len = self.length();
        let d = crate::modules::civil::point_intersect::travelled_fraction(&self.curve, [p.x, p.y]) * len;
        let foot = self.path.point_at_distance(d);
        let foot = DVec2::new(foot[0], foot[1]);
        let t = self.tangent(d);
        let v = p - foot;
        let at_end = d <= 1e-9 || d >= len - 1e-9;
        if at_end && v.length() > 1e-9 && v.normalize().dot(t).abs() > 1e-6 {
            return None;
        }
        Some((self.start + d, v.dot(DVec2::new(t.y, -t.x))))
    }

    /// The station of a point near the alignment (its projection).
    fn station_of(&self, p: DVec2) -> f64 {
        self.start + crate::modules::civil::point_intersect::travelled_fraction(&self.curve, [p.x, p.y]) * self.length()
    }

    /// The alignment's geometry points in the order the reference writes
    /// them, each with the station its elevation is read at: the start, at
    /// every piece after the first its start (a curve's centre first and its
    /// middle after it), the end; then over each curve group the spirals'
    /// PIs, the curve's PI and the group's PI where it differs (measured).
    fn geometry_points(&self) -> Vec<(DVec2, f64)> {
        use document::classes::civil::AlignmentPiece as P;
        let n = self.pieces.len();
        // Each piece's ends and end tangents straight from its solved points:
        // a line's direction, a curve's tangent off its centre; a spiral takes
        // its neighbours' tangents (it meets a line and a curve tangentially).
        let mut centre = vec![None; n];
        let mut ends: Vec<(DVec2, DVec2, Option<DVec2>, Option<DVec2>)> = Vec::with_capacity(n);
        for (i, (kind, _, pts)) in self.pieces.iter().enumerate() {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            let chord = |u: DVec2, v: DVec2| (v - u).normalize_or_zero();
            let (ta, tb) = match kind {
                P::Line => (Some(chord(a, b)), Some(chord(a, b))),
                P::Curve => match circumcentre(a, pts[pts.len() / 2], b) {
                    Some(c) => {
                        centre[i] = Some(c);
                        let dir = (a - c).perp_dot(pts[1] - a).signum();
                        let t = |p: DVec2| (p - c).perp().normalize_or_zero() * dir;
                        (Some(t(a)), Some(t(b)))
                    }
                    None => (Some(chord(a, b)), Some(chord(a, b))),
                },
                _ => (None, None),
            };
            ends.push((a, b, ta, tb));
        }
        for i in 0..n {
            let (_, _, ta, tb) = ends[i];
            let pts = &self.pieces[i].2;
            let k = pts.len();
            let ta = ta.or_else(|| i.checked_sub(1).and_then(|j| ends[j].3)).unwrap_or_else(|| (pts[1] - pts[0]).normalize_or_zero());
            let tb = tb.or_else(|| ends.get(i + 1).and_then(|e| e.2)).unwrap_or_else(|| (pts[k - 1] - pts[k - 2]).normalize_or_zero());
            ends[i].2 = Some(ta);
            ends[i].3 = Some(tb);
        }
        let mut out = Vec::new();
        for (i, (kind, _, _)) in self.pieces.iter().enumerate() {
            let (a, b, _, _) = ends[i];
            let s0 = self.station_of(a);
            if let Some(c) = centre[i] {
                out.push((c, s0));
            }
            out.push((a, s0));
            if *kind == P::Curve {
                let mid = (s0 + self.station_of(b)) / 2.0;
                let p = self.at(mid, 0.0);
                out.push((DVec2::new(p.x, p.y), mid));
            }
        }
        if let Some(e) = ends.last() {
            out.push((e.1, self.station_of(e.1)));
        }
        // PIs: where the tangents at the two ends of a piece (or group) meet.
        let meet = |i: usize, j: usize| -> Option<DVec2> {
            let (p, u) = (ends[i].0, ends[i].2?);
            let (q, v) = (ends[j].1, ends[j].3?);
            let den = u.perp_dot(v);
            (den.abs() > 1e-12).then(|| p + u * ((q - p).perp_dot(v) / den))
        };
        let mut i = 0;
        while i < n {
            if self.pieces[i].0 == P::Line {
                i += 1;
                continue;
            }
            let group = i;
            while i < n && self.pieces[i].0 != P::Line {
                i += 1;
            }
            let group_pi = meet(group, i - 1);
            let mut curve_pi = None;
            for j in group..i {
                let Some(pi) = meet(j, j) else { continue };
                if self.pieces[j].0 == P::Curve {
                    curve_pi = Some(pi);
                    if group_pi.is_some_and(|g| g.distance(pi) > 1e-6) {
                        out.push((pi, self.station_of(pi)));
                    }
                    if let Some(g) = group_pi {
                        out.push((g, self.station_of(g)));
                    }
                } else {
                    out.push((pi, self.station_of(pi)));
                }
            }
            if curve_pi.is_none() {
                if let Some(g) = group_pi {
                    out.push((g, self.station_of(g)));
                }
            }
        }
        out
    }
}

/// The centre of the circle through three points; none when they are in line.
fn circumcentre(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let (u, v) = (b - a, c - a);
    let d = 2.0 * u.perp_dot(v);
    (d.abs() > 1e-12).then(|| a + (u.perp() * v.length_squared() - v.perp() * u.length_squared()) / d)
}

/// The profiles of the alignment `handle`, and whether a profile view
/// shows it.
fn profiles_of(document: &document::Document, handle: u64) -> (Vec<Profile>, bool) {
    fn arch(e: &EntityType) -> Option<&document::classes::arch::ArchEntityData> {
        match e {
            EntityType::Extended(x) => match &x.data {
                ExtendedEntityData::Arch(a) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    let mut profiles: Vec<(u64, Profile)> = Vec::new();
    let mut views = Vec::new();
    for e in document.entities() {
        let Some(a) = arch(e) else { continue };
        let handles = a.decode().handles;
        if !handles.contains(&handle) {
            continue;
        }
        match a.dxf_name.as_str() {
            "AECC_PROFILE" => {
                let pvis: Vec<(f64, f64, f64)> = a.profile_pvis().iter().map(|p| (p.point.x, p.point.y, p.curve_length)).collect();
                if pvis.len() >= 2 {
                    let name = a.profile_name().unwrap_or_default();
                    profiles.push((e.common().handle.value(), Profile { name, pvis, drawn: Vec::new() }));
                }
            }
            "AECC_PROFILE_VIEW" => views.push((a.profile_view_frame(), handles)),
            _ => {}
        }
    }
    for (frame, handles) in &views {
        let Some(frame) = frame else { continue };
        for (h, p) in profiles.iter_mut().filter(|(h, _)| handles.contains(h)) {
            let _ = h;
            let mut run = Vec::new();
            let mut s = p.pvis[0].0;
            let end = p.pvis[p.pvis.len() - 1].0;
            while s <= end + 1e-9 {
                if let Some(z) = p.elevation(s) {
                    let d = frame.to_drawing(s, z);
                    run.push(DVec2::new(d.x, d.y));
                }
                s += ((end - p.pvis[0].0) / 200.0).max(1e-3);
            }
            p.drawn.push(run);
        }
    }
    (profiles.into_iter().map(|(_, p)| p).collect(), !views.is_empty())
}

#[derive(Clone)]
enum Step {
    Alignment,
    Profile,
    Station,
    Offset,
    Segments,
    StartStation,
    EndStation,
    Interval,
    Radial,
    Format,
    Delimiter,
    ElevationIndicator,
    StationIndicator,
    Description,
    Elevation(Option<String>),
}

/// The file formats and delimiters Import from File lists before asking.
fn formats() -> String {
    [
        t("1. Station, Offset"),
        t("2. Station, Offset, Elevation"),
        t("3. Station, Offset, Rod reading, HI"),
        t("4. Station, Offset, Description"),
        t("5. Station, Offset, Elevation, Description"),
        t("6. Station, Offset, Rod reading, HI, Description"),
    ]
    .join("\n")
}

fn delimiters() -> String {
    [t("1. Space"), t("2. Comma")].join("\n")
}

pub struct AlignmentPointCommand {
    kind: Kind,
    step: Step,
    picked: Option<EntityType>,
    alignment: Option<Alignment>,
    profile: Option<usize>,
    /// Profiles a pick in a profile view offered, nearest found on the pick.
    station: f64,
    start_station: f64,
    end_station: f64,
    offset: f64,
    segments: u32,
    interval: f64,
    queue: VecDeque<(DVec3, Option<f64>)>,
    description: Option<String>,
    /// Import: the file's text and the answers given.
    file: String,
    format: u8,
    comma: bool,
    elevation_indicator: f64,
    station_indicator: f64,
    /// The drawing is in metres: offsets and stations in messages carry `m`.
    metric: bool,
}

impl AlignmentPointCommand {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            step: Step::Alignment,
            picked: None,
            alignment: None,
            profile: None,
            station: 0.0,
            start_station: 0.0,
            end_station: 0.0,
            offset: 0.0,
            segments: 1,
            interval: 10.0,
            queue: VecDeque::new(),
            description: None,
            file: String::new(),
            format: 0,
            comma: false,
            elevation_indicator: -99999.0,
            station_indicator: -99999.0,
            metric: false,
        }
    }

    /// Import from File: the picked file's text; the format is asked first.
    pub fn import(text: String) -> Self {
        Self { file: text, step: Step::Format, ..Self::new(Kind::Import) }
    }

    /// The line shown before the first prompt (the import's format menu).
    pub fn intro(&self) -> Option<String> {
        matches!(self.step, Step::Format).then(formats)
    }

    fn profile(&self) -> Option<&Profile> {
        self.alignment.as_ref().zip(self.profile).and_then(|(a, i)| a.profiles.get(i))
    }

    /// The elevation the chosen profile gives at `station`.
    fn elevation_at(&self, station: f64) -> Option<f64> {
        self.profile().and_then(|p| p.elevation(station))
    }

    /// Asks the next queued point, or goes back to where more are made.
    fn next(&mut self) -> CmdResult {
        self.step = if !self.queue.is_empty() {
            Step::Description
        } else if self.kind == Kind::StationOffset {
            Step::Station
        } else {
            Step::Alignment
        };
        CmdResult::NeedPoint
    }

    fn queue_and_ask(&mut self, points: Vec<(DVec3, Option<f64>)>) -> CmdResult {
        self.queue.extend(points);
        self.next()
    }

    /// After the alignment and the profile: the first prompt of the kind.
    fn after_profile(&mut self) -> CmdResult {
        let Some(a) = &self.alignment else { return CmdResult::NeedPoint };
        self.start_station = a.start;
        self.end_station = a.end();
        self.step = match self.kind {
            Kind::Divide => Step::Segments,
            Kind::Radial => Step::Radial,
            Kind::StationOffset => Step::Station,
            _ => Step::StartStation,
        };
        CmdResult::NeedPoint
    }

    /// The stations at which a division or an interval places points.
    fn stations(&self) -> Vec<f64> {
        let Some(a) = &self.alignment else { return Vec::new() };
        match self.kind {
            Kind::Divide => {
                let n = self.segments.max(1);
                (0..=n).map(|i| a.start + a.length() * f64::from(i) / f64::from(n)).collect()
            }
            _ => {
                // Measured: the last interval is not shortened to the ending
                // station; the run steps past it once (100..160 by 50 gives 100,
                // 150, 200) but never beyond the alignment's end.
                let n = ((self.end_station - self.start_station) / self.interval - 1e-9).ceil().max(0.0);
                let last = a.start + a.length() + 1e-9;
                (0..=(n.min(100_000.0) as u32))
                    .map(|k| self.start_station + self.interval * f64::from(k))
                    .filter(|s| *s <= last)
                    .collect()
            }
        }
    }

    fn placed(&self, stations: Vec<f64>, offset: f64) -> Vec<(DVec3, Option<f64>)> {
        let Some(a) = &self.alignment else { return Vec::new() };
        stations.into_iter().map(|s| (a.at(s, offset), self.elevation_at(s))).collect()
    }

    /// Import: every row of the file as a point (rows the alignment cannot
    /// place reported and skipped).
    fn imported(&self) -> (Vec<(DVec3, Option<String>, Option<f64>)>, Vec<String>) {
        let Some(a) = &self.alignment else { return (Vec::new(), Vec::new()) };
        let mut points = Vec::new();
        let mut lines = Vec::new();
        let same = |v: f64, indicator: f64| (v - indicator).abs() < 1e-9;
        for row in self.file.lines() {
            let fields: Vec<&str> = if self.comma { row.split(',').map(str::trim).collect() } else { row.split_whitespace().collect() };
            if fields.iter().all(|f| f.is_empty()) {
                continue;
            }
            let values = match self.format {
                2 | 5 => 3,
                3 | 6 => 4,
                _ => 2,
            };
            let mut nums = Vec::new();
            let mut bad = None;
            for i in 0..values {
                match fields.get(i).copied().filter(|f| !f.is_empty()) {
                    None => nums.push(0.0),
                    Some(f) => match number(f) {
                        Some(v) => nums.push(v),
                        None => {
                            bad = Some(f);
                            break;
                        }
                    },
                }
            }
            if let Some(f) = bad {
                lines.push(format!("{} ({f})", crate::tf!("Invalid characters found in input string '{}'.", f)));
                continue;
            }
            let (station, offset) = (nums[0], nums[1]);
            if same(station, self.station_indicator) {
                continue;
            }
            if !a.contains(station) {
                lines.push(format!("{}   ({})", t("The specified station is not valid for this object."), fields[0]));
                continue;
            }
            let offset = if same(offset, self.station_indicator) { 0.0 } else { offset };
            let elevation = match self.format {
                2 | 5 => (!same(nums[2], self.elevation_indicator)).then_some(nums[2]),
                3 | 6 => (!same(nums[2], self.elevation_indicator) && !same(nums[3], self.elevation_indicator)).then(|| nums[3] - nums[2]),
                _ => None,
            };
            let description = matches!(self.format, 4..=6)
                .then(|| fields.get(values..).map(|rest| rest.join(if self.comma { "," } else { " " })).unwrap_or_default())
                .filter(|d| !d.is_empty());
            points.push((a.at(station, offset), description, elevation));
        }
        (points, lines)
    }

    /// A station typed at a starting or ending station prompt.
    fn station_given(&mut self, typed: &str, ending: bool) -> Option<CmdResult> {
        let default = if ending { self.end_station } else { self.start_station };
        let s = if typed.is_empty() { default } else {
            match number(typed) {
                Some(s) => s,
                None => return Some(CmdResult::ReportError(t("Invalid input"))),
            }
        };
        if s < 0.0 {
            return Some(CmdResult::ReportError(t("'Transparent Commands Station Property' validation error.  Please enter a positive value.")));
        }
        let a = self.alignment.as_ref()?;
        if !a.contains(s) || (ending && s < self.start_station - 1e-9) {
            return Some(CmdResult::ReportError(format!("{}  ", t("The specified station is not valid for this object."))));
        }
        if ending {
            self.end_station = s;
            return Some(match self.kind {
                Kind::Measure => {
                    self.step = Step::Offset;
                    CmdResult::NeedPoint
                }
                // At Geometry Points takes every geometry point whatever
                // the stations given (measured).
                Kind::GeometryPoints => {
                    let a = self.alignment.as_ref()?;
                    let points: Vec<(DVec3, Option<f64>)> =
                        a.geometry_points().into_iter().map(|(p, s)| (DVec3::new(p.x, p.y, 0.0), self.elevation_at(s))).collect();
                    self.queue_and_ask(points)
                }
                _ => {
                    // Profile Geometry Points: written at once, no prompts.
                    let (from, to) = (self.start_station, self.end_station);
                    let points: Vec<(DVec3, Option<String>, Option<f64>)> = self
                        .profile()
                        .map(|p| p.geometry_stations().into_iter().filter(|s| (from - 1e-9..=to + 1e-9).contains(s)).map(|s| (a.at(s, 0.0), None, p.elevation(s))).collect())
                        .unwrap_or_default();
                    self.step = Step::Alignment;
                    CmdResult::AddCogoPoints { points, lines: Vec::new(), done: false }
                }
            });
        }
        self.start_station = s;
        self.step = Step::EndStation;
        Some(CmdResult::NeedPoint)
    }
}

impl CadCommand for AlignmentPointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Alignment => t("Select alignment:"),
            Step::Profile => t("Select a profile or [None] <None>:"),
            Step::Station => t("Specify station along baseline:"),
            Step::Offset => crate::tf!("Specify an offset <{:.3}>:", self.offset).into_owned(),
            Step::Segments => crate::tf!("Enter the number of segments <{}>:", self.segments).into_owned(),
            Step::StartStation => crate::tf!("Starting station <{}>:", format_station(self.start_station)).into_owned(),
            Step::EndStation => crate::tf!("Ending station <{}>:", format_station(self.end_station)).into_owned(),
            Step::Interval => crate::tf!("Enter an interval <{:.3}>:", self.interval).into_owned(),
            Step::Radial => t("Specify a point that is radial or perpendicular to the current alignment:"),
            Step::Format => t("Enter file format (1/2/3/4/5/6): <0>:"),
            Step::Delimiter => t("Enter a delimiter (1/2): <0>:"),
            Step::ElevationIndicator if matches!(self.format, 3 | 6) => crate::tf!("Enter an invalid indicator for Rod hi <{}>:", self.elevation_indicator).into_owned(),
            Step::ElevationIndicator => crate::tf!("Enter an invalid indicator for Elevation <{}>:", self.elevation_indicator).into_owned(),
            Step::StationIndicator => crate::tf!("Enter an invalid indicator for Station Offset <{}>:", self.station_indicator).into_owned(),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
            Step::Elevation(_) => t("Specify a point elevation <.>:"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Profile => vec![CmdOption::new("None", "N")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Alignment | Step::Profile | Step::Radial => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Alignment | Step::Profile)
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        match self.step {
            Step::Alignment => Some(|e| crate::modules::civil::point_intersect::alignment_curve(e).is_some()),
            _ => None,
        }
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn inject_picked_from(&mut self, document: &document::Document) {
        if !matches!(self.step, Step::Alignment) {
            return;
        }
        let Some(e) = self.picked.as_ref() else { return };
        let Some(curve) = crate::modules::civil::point_intersect::alignment_curve(e) else { return };
        let EntityType::Extended(x) = e else { return };
        let ExtendedEntityData::Arch(data) = &x.data else { return };
        let Curve::Polyline(pl) = &curve else { return };
        let mut lw = LwPolyline::new();
        lw.vertices = pl.vertices.iter().map(|v| LwVertex::with_bulge(Vector2::new(v.position[0], v.position[1]), v.bulge)).collect();
        let Some(path) = entity_curve(&EntityType::LwPolyline(lw)) else { return };
        let pieces = data
            .alignment_pieces_with_curvature()
            .into_iter()
            .filter(|(_, _, p)| p.len() >= 2)
            .map(|(k, c, p)| (k, c, p.iter().map(|v| DVec2::new(v.x, v.y)).collect()))
            .collect();
        self.metric = document.header.insertion_units == 6;
        let (profiles, viewed) = profiles_of(document, x.common.handle.value());
        self.alignment = Some(Alignment { path, curve, start: data.alignment_start_station().unwrap_or(0.0), pieces, profiles, viewed });
    }

    fn on_entity_pick(&mut self, _handle: Handle, pick: DVec3) -> CmdResult {
        let picked = self.picked.take();
        match self.step {
            Step::Profile => {
                // A pick on a profile view takes the profile drawn nearest it.
                let p = DVec2::new(pick.x, pick.y);
                let nearest = self.alignment.as_ref().and_then(|a| {
                    a.profiles
                        .iter()
                        .enumerate()
                        .flat_map(|(i, pr)| pr.drawn.iter().flat_map(|run| run.windows(2)).map(move |w| (i, segment_distance(p, w[0], w[1]))))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                });
                match nearest {
                    Some((i, _)) if picked.as_ref().is_some_and(is_profile_view) => {
                        self.profile = Some(i);
                        self.after_profile()
                    }
                    _ => CmdResult::ReportError(t("Selected entity must be of type: Profile")),
                }
            }
            _ => {
                if picked.is_none() || self.alignment.is_none() {
                    self.alignment = None;
                    return CmdResult::ReportError(t("ERROR: Invalid selection set!"));
                }
                self.profile = None;
                if self.kind == Kind::Import {
                    let (points, lines) = self.imported();
                    return CmdResult::AddCogoPoints { points, lines, done: true };
                }
                if self.alignment.as_ref().is_some_and(|a| self.kind.asks_profile() && a.viewed && !a.profiles.is_empty()) {
                    self.step = Step::Profile;
                    return CmdResult::NeedPoint;
                }
                self.after_profile()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Radial => {
                let Some(a) = &self.alignment else { return CmdResult::NeedPoint };
                // A point whose foot falls beyond the alignment is passed
                // over (measured).
                let Some((station, offset)) = a.station_offset(DVec2::new(pt.x, pt.y)) else { return CmdResult::NeedPoint };
                let at = a.at(station, 0.0);
                let elevation = self.elevation_at(station);
                let unit = if self.metric { "m" } else { "" };
                self.queue.push_back((at, elevation));
                self.next();
                CmdResult::ReportMessage(format!("{}\n{}", crate::tf!("Station: {}", format_station(station)), crate::tf!("Offset: {}", format!("{offset:.3}{unit}"))))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let bad = || Some(CmdResult::ReportError(t("Invalid input")));
        let integer = |typed: &str| -> Result<i64, CmdResult> {
            typed.parse::<i64>().map_err(|_| CmdResult::ReportError(t("'Generic Integer' validation error.  Please enter an integer.")))
        };
        match self.step.clone() {
            // A word at the alignment prompt picks nothing (measured).
            Step::Alignment if !typed.is_empty() && !typed.chars().any(|c| c.is_ascii_digit()) => {
                return Some(CmdResult::ReportError(crate::tr!("measurement", "angle-invalid-second")));
            }
            Step::Alignment | Step::Radial if !typed.is_empty() && typed.chars().any(|c| c.is_ascii_digit()) && typed.contains(',') => return None,
            Step::Alignment => return None,
            Step::Radial => return bad(),
            Step::Profile => {
                let a = self.alignment.as_ref()?;
                let word = typed.to_lowercase();
                if typed.is_empty() || word == "n" || word == "none" {
                    self.profile = None;
                } else {
                    match a.profiles.iter().position(|p| p.name.to_lowercase() == word) {
                        Some(i) => self.profile = Some(i),
                        None => return Some(CmdResult::ReportError(t("Selected entity must be of type: Profile"))),
                    }
                }
                return Some(self.after_profile());
            }
            Step::Station => {
                if typed.is_empty() {
                    return Some(CmdResult::Done);
                }
                let Some(s) = number(typed) else { return bad() };
                let a = self.alignment.as_ref()?;
                if !a.contains(s) {
                    let unit = if self.metric { "m" } else { "" };
                    return Some(CmdResult::ReportError(crate::tf!("Station {}:  Either outside of the baseline limits, or within gap created by a station equation.", format!("{}{unit}", format_station(s))).into_owned()));
                }
                self.station = s;
                self.step = Step::Offset;
            }
            Step::Offset => {
                let o = if typed.is_empty() { self.offset } else {
                    match number(typed) {
                        Some(o) => o,
                        None => return Some(CmdResult::ReportError(crate::tf!("Invalid characters found in input string '{}'.", typed).into_owned())),
                    }
                };
                self.offset = o;
                let points = match self.kind {
                    Kind::StationOffset => {
                        let a = self.alignment.as_ref()?;
                        vec![(a.at(self.station, o), self.elevation_at(self.station))]
                    }
                    Kind::Divide => self.placed(self.stations(), o),
                    _ => {
                        self.step = Step::Interval;
                        return Some(CmdResult::NeedPoint);
                    }
                };
                return Some(self.queue_and_ask(points));
            }
            Step::Segments => {
                let n = if typed.is_empty() { i64::from(self.segments) } else {
                    match integer(typed) {
                        Ok(n) => n,
                        Err(e) => return Some(e),
                    }
                };
                if n <= 0 {
                    return Some(CmdResult::ReportError(t("A positive value is required")));
                }
                self.segments = n as u32;
                self.step = Step::Offset;
            }
            Step::StartStation => return self.station_given(typed, false),
            Step::EndStation => return self.station_given(typed, true),
            Step::Interval => {
                let v = if typed.is_empty() { self.interval } else {
                    match number(typed) {
                        Some(v) => v,
                        None => return bad(),
                    }
                };
                if v <= 0.0 {
                    return Some(CmdResult::ReportError(t("A positive value is required")));
                }
                self.interval = v;
                let points = self.placed(self.stations(), self.offset);
                return Some(self.queue_and_ask(points));
            }
            Step::Format | Step::Delimiter => {
                let max = if matches!(self.step, Step::Format) { 6 } else { 2 };
                let menu = if matches!(self.step, Step::Format) { formats() } else { delimiters() };
                let n = match integer(typed) {
                    Ok(n) if (1..=max).contains(&n) => n,
                    Ok(_) => return Some(CmdResult::ReportError(format!("{}\n{menu}", t("Invalid input")))),
                    Err(_) if typed.is_empty() => return Some(CmdResult::ReportError(format!("{}\n{menu}", t("Invalid input")))),
                    Err(CmdResult::ReportError(e)) => return Some(CmdResult::ReportError(format!("{e}\n{menu}"))),
                    Err(e) => return Some(e),
                };
                if matches!(self.step, Step::Format) {
                    self.format = n as u8;
                    self.step = Step::Delimiter;
                    return Some(CmdResult::ReportMessage(delimiters()));
                }
                self.comma = n == 2;
                self.step = if matches!(self.format, 2 | 3 | 5 | 6) { Step::ElevationIndicator } else { Step::StationIndicator };
            }
            Step::ElevationIndicator | Step::StationIndicator => {
                let v = if typed.is_empty() { None } else {
                    match number(typed) {
                        Some(v) => Some(v),
                        None => return bad(),
                    }
                };
                if matches!(self.step, Step::ElevationIndicator) {
                    if let Some(v) = v {
                        self.elevation_indicator = v;
                    }
                    self.step = Step::StationIndicator;
                } else {
                    if let Some(v) = v {
                        self.station_indicator = v;
                    }
                    self.step = Step::Alignment;
                }
            }
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                // A point the profile gives an elevation is not asked one.
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
                        None => return bad(),
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
            Step::Alignment => CmdResult::Done,
            // Enter at the radial point prompt goes back to the alignment.
            Step::Radial => {
                self.step = Step::Alignment;
                CmdResult::NeedPoint
            }
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        // Divide goes back one level (point → segments → alignment); the
        // others end (measured).
        self.queue.clear();
        match (self.kind, &self.step) {
            (Kind::Divide, Step::Description | Step::Elevation(_)) => {
                self.step = Step::Segments;
                CmdResult::ReportMessage(t("*Cancel*"))
            }
            (Kind::Divide, Step::Segments | Step::Offset) => {
                self.step = Step::Alignment;
                CmdResult::ReportMessage(t("*Cancel*"))
            }
            _ => CmdResult::Cancel,
        }
    }
}

/// Whether `e` is a profile view (its profiles are picked through it).
fn is_profile_view(e: &EntityType) -> bool {
    matches!(e, EntityType::Extended(x) if matches!(&x.data, ExtendedEntityData::Arch(a) if a.dxf_name == "AECC_PROFILE_VIEW"))
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "CREATEPOINTSTATIONOFFSET",
        "CREATEPOINTDIVIDEALIGN",
        "CREATEPOINTMEASUREALIGN",
        "CREATEPOINTATGEOMETRYPOINTS",
        "CREATEPOINTRADIALORPERP",
        "CREATEPOINTIMPORTFROMFILE",
        "CREATEPROFILEGEOMPOINTS",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stations_and_profile_elevations_as_measured() {
        assert_eq!(format_station(0.0), "0+00.00");
        assert_eq!(format_station(435.2352), "4+35.24");
        assert_eq!(format_station(9999.0), "99+99.00");
        assert_eq!(format_station(-50.0), "-0+50.00");
        // PVIs (0,100) (120,108, curve 60) (240,102): measured 106.844983
        // at 106.781875 and 107.136799 at 126.781875.
        let p = Profile { name: "PR1".into(), pvis: vec![(0.0, 100.0, 0.0), (120.0, 108.0, 60.0), (240.0, 102.0, 0.0)], drawn: Vec::new() };
        assert!((p.elevation(106.781875).unwrap() - 106.844983).abs() < 1e-5);
        assert!((p.elevation(126.781875).unwrap() - 107.136799).abs() < 1e-5);
        assert_eq!(p.elevation(250.0), None);
        assert_eq!(p.geometry_stations(), vec![0.0, 90.0, 150.0, 240.0]);
    }
}
