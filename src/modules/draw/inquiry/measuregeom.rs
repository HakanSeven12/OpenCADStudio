// MEASUREGEOM command (alias MEA) — multi-mode geometry measurement.
//
// The command opens in quick measure (`Move cursor or [...] <eXit>:`) while
// the Mode option says so, else at `Enter an option [...] <Distance>:`. Each
// measurement prints its result and comes back to the option prompt with the
// last option as the default, until eXit, Enter in quick mode, or Esc.
//
//   Distance — two points (or Multiple points) → distance, the angles in and
//              from the XY plane, delta X/Y/Z.
//   Radius   — pick a Circle or Arc → radius + diameter.
//   Angle    — three points (vertex + two ray endpoints) → angle in degrees.
//   ARea     — a boundary of picked points (line and arc segments) or an
//              object → area + perimeter (circumference for a circle), with
//              Add area / Subtract area keeping a running total.
//   Volume   — the same boundary or object, extruded by a typed height (a
//              3D solid gives its own volume), with Add / Subtract volume.
//   Quick    — no picks: as the cursor moves, the nearest geometry along the
//              working plane's X and Y axes is measured live; a click
//              reports the current readout.
//   Mode     — whether the command starts in quick measure (a profile setting).
//
// All arithmetic is kept in f64 (picked points stay full precision; downcasting
// to f32 loses several hundredths of a unit at survey-scale coordinates).

use crate::t;
use document::entities::{LwPolyline, LwVertex};
use document::types::Vector2;
use document::{EntityType, Handle};
use glam::{DVec2, DVec3, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::draw::draw::polyline::{arc_for, compute_bulge, seg_exit_tangent, Sub};
use crate::modules::draw::inquiry::area::{AreaCommand, AreaMeasurement};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "MEASUREGEOM",
        label: "Measure Geometry",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("MEASUREGEOM".to_string()),
    }
}

// ── Mode option (profile) ──────────────────────────────────────────────────

static QUICK_DEFAULT: AtomicBool = AtomicBool::new(true);

/// "Always default to quick measure behavior?" — MEASUREGEOM's Mode answer.
pub fn quick_default() -> bool {
    QUICK_DEFAULT.load(Ordering::Relaxed)
}

pub fn set_quick_default(quick: bool) {
    QUICK_DEFAULT.store(quick, Ordering::Relaxed);
}

// ── Mode ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Awaiting the option keyword.
    Choose,
    /// The Mode option's Yes/No question.
    AskMode,
    Quick,
    Distance,
    /// ARea or Volume (`volume`).
    Area,
    Angle,
    Radius,
    AngleLines,
}

/// ARea / Volume running total kind.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sum {
    Single,
    Add,
    Subtract,
}

/// ARea / Volume step.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AreaStep {
    Points,
    Length,
    Objects,
    Height,
}

// ── Command implementation ──────────────────────────────────────────────────

pub const RADIUS_ICON: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/measure-radius.svg"
));
pub const ANGLE_ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/measure-angle.svg"));

pub struct MeasureGeomCommand {
    name: &'static str,
    hovered: Option<(Handle, EntityType)>,
    first_line: Option<(Handle, DVec3)>,
    mode: Mode,
    /// The option prompt's default: the last option chosen.
    last: &'static str,
    /// Picked points for the active point-pick mode.
    points: Vec<DVec3>,
    /// Per picked point, the bulge of the segment leaving it (the last
    /// one closes an ARea / Volume boundary).
    segs: Vec<f64>,
    /// ARea / Volume: Arc segment mode and its sub-option.
    arc: bool,
    arc_sub: Sub,
    /// Exit direction of the last segment, in the working plane.
    tangent: Option<Vec2>,
    /// Direction the drawing's last line or arc ended in (world): where a
    /// first arc or length segment leaves.
    start_direction: Option<DVec3>,
    volume: bool,
    sum: Sum,
    step: AreaStep,
    total: f64,
    /// Area waiting for its height (Volume), and the point a picked
    /// height is measured from.
    base_area: f64,
    height_base: DVec3,
    /// The last Add / Subtract report, printed again by Enter at the
    /// object prompt.
    last_report: Option<String>,
    /// Distance: Multiple points.
    multiple: bool,
    /// The last finished result, printed again after an invalid option.
    last_result: Option<String>,
    /// The picked entity for RADIUS, injected before `on_entity_pick`.
    picked: Option<EntityType>,
    plane: WorkingPlane,
    /// QUICK readout for the last hovered cursor position.
    quick: std::cell::RefCell<Option<String>>,
    /// QUICK: the closed areas a click can measure, gathered on first hover.
    quick_sources: std::cell::RefCell<
        Option<rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>>,
    >,
}

impl MeasureGeomCommand {
    pub fn new() -> Self {
        Self {
            name: "MEASUREGEOM",
            hovered: None,
            first_line: None,
            mode: if quick_default() {
                Mode::Quick
            } else {
                Mode::Choose
            },
            last: "Distance",
            points: vec![],
            segs: vec![],
            arc: false,
            arc_sub: Sub::None,
            tangent: None,
            start_direction: None,
            volume: false,
            sum: Sum::Single,
            step: AreaStep::Points,
            total: 0.0,
            base_area: 0.0,
            height_base: DVec3::ZERO,
            last_report: None,
            multiple: false,
            last_result: None,
            picked: None,
            plane: WorkingPlane::default(),
            quick: std::cell::RefCell::new(None),
            quick_sources: std::cell::RefCell::new(None),
        }
    }

    /// The direction the drawing's last line or arc ended in.
    pub fn with_last_direction(mut self, direction: Option<DVec3>) -> Self {
        self.start_direction = direction;
        self
    }

    pub fn radius() -> Self {
        Self {
            name: "MEASURERADIUS",
            mode: Mode::Radius,
            ..Self::new()
        }
    }

    pub fn angle() -> Self {
        Self {
            name: "MEASUREANGLE",
            mode: Mode::Angle,
            ..Self::new()
        }
    }

    /// Forget the measurement in progress (not the option default).
    fn reset(&mut self) {
        self.hovered = None;
        self.first_line = None;
        self.points.clear();
        self.segs.clear();
        self.arc = false;
        self.arc_sub = Sub::None;
        self.tangent = None;
        self.sum = Sum::Single;
        self.step = AreaStep::Points;
        self.total = 0.0;
        self.last_report = None;
        self.multiple = false;
        self.picked = None;
    }

    /// Print a finished measurement: MEASUREGEOM goes back to its option
    /// prompt, the single-purpose commands end.
    fn done(&mut self, message: String) -> CmdResult {
        if self.name != "MEASUREGEOM" {
            return CmdResult::Measurement(message);
        }
        // A refused option prints a finished area or volume again; a
        // finished distance is not repeated.
        let repeat = self.mode != Mode::Distance;
        self.reset();
        self.mode = Mode::Choose;
        self.last_result = repeat.then(|| message.clone());
        CmdResult::ReportMeasurement(message)
    }

    /// An option keyword typed at the option or quick prompt.
    fn choose(&mut self, input: &str) -> Option<CmdResult> {
        let option = match input {
            "D" | "DISTANCE" => "Distance",
            "R" | "RADIUS" => "Radius",
            "A" | "ANGLE" => "Angle",
            "AR" | "AREA" => "ARea",
            "V" | "VOLUME" => "Volume",
            "Q" | "QUICK" => "Quick",
            "M" | "MODE" => "Mode",
            "X" | "EXIT" => return Some(CmdResult::Measurement(String::new())),
            _ => return None,
        };
        Some(self.start(option))
    }

    fn start(&mut self, option: &'static str) -> CmdResult {
        self.reset();
        self.last = option;
        self.volume = option == "Volume";
        self.mode = match option {
            "Radius" => Mode::Radius,
            "Angle" => Mode::Angle,
            "ARea" | "Volume" => Mode::Area,
            "Quick" => Mode::Quick,
            "Mode" => Mode::AskMode,
            _ => Mode::Distance,
        };
        CmdResult::NeedPoint
    }

    fn option_list() -> Vec<CmdOption> {
        vec![
            CmdOption::new("Distance", "D"),
            CmdOption::new("Radius", "R"),
            CmdOption::new("Angle", "A"),
            CmdOption::new("ARea", "AR"),
            CmdOption::new("Volume", "V"),
            CmdOption {
                label: crate::tr!("measurement", "quick"),
                keyword: "Q".into(),
            },
            CmdOption::new("Mode", "M"),
            CmdOption::new("eXit", "X"),
        ]
    }

    /// `Invalid option keyword.` at the option prompt, with the last result
    /// printed again.
    fn invalid_option(&self) -> CmdResult {
        let error = t!("Invalid option keyword.").into_owned();
        match self.last_result.clone() {
            Some(result) => CmdResult::ReportErrorAndMeasurement(error, result),
            None => CmdResult::ReportError(error),
        }
    }

    /// The error a sub-prompt of a chain (line length, an arc option)
    /// prints for a word or for Enter (`enter`); None where Enter takes the
    /// default (the tangent direction, the chord direction) or where no
    /// sub-prompt is open.
    fn sub_prompt_error(&self, enter: bool) -> Option<String> {
        if self.step == AreaStep::Length {
            return Some(t!("Requires numeric distance or second point.").into_owned());
        }
        if !self.arc {
            return None;
        }
        let angle = || t!("Requires valid numeric angle or second point.").into_owned();
        Some(match self.arc_sub {
            Sub::None => return None,
            Sub::ArcDirection if enter => return None,
            _ if enter && self.chord_step() => return None,
            Sub::ArcAngle | Sub::ArcRadiusAngle { .. } | Sub::ArcDirection => angle(),
            Sub::ArcRadius | Sub::ArcAngleRadius { .. } => {
                t!("Requires numeric distance or two points.").into_owned()
            }
            Sub::ArcAngleEnd { .. } | Sub::ArcCenterEnd { .. } | Sub::ArcRadiusEnd { .. } => {
                crate::tr!("measurement", "point-or-keyword")
            }
            _ => crate::tr!("measurement", "invalid-2d-point"),
        })
    }

    /// A line-length or arc sub-prompt of a boundary or a Multiple points
    /// chain is open.
    fn chain_sub_prompt(&self) -> bool {
        let chain = self.mode == Mode::Area || (self.mode == Mode::Distance && self.multiple);
        chain
            && (self.step == AreaStep::Length
                || (self.step == AreaStep::Points && self.arc && self.arc_sub != Sub::None))
    }

    /// Enter at a sub-prompt: the chord and tangent directions take their
    /// default (the direction the chain leaves in), every other one is
    /// refused and asked again.
    fn sub_prompt_enter(&mut self) -> CmdResult {
        if let Some(error) = self.sub_prompt_error(true) {
            return CmdResult::ReportError(error);
        }
        if self.chord_step() {
            return self
                .arc_chord(self.tangent_degrees())
                .unwrap_or(CmdResult::NeedPoint);
        }
        self.arc_sub = Sub::ArcDirectionEnd {
            dir: self.heading(),
        };
        CmdResult::NeedPoint
    }

    fn keyword_error() -> CmdResult {
        CmdResult::ReportError(t!("Point or option keyword required.").into_owned())
    }

    /// Text that starts with a letter is an option keyword; anything else
    /// (a coordinate, a distance) is left to the point reader.
    fn is_keyword(input: &str) -> bool {
        input.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    }

    // ── ARea / Volume boundary ─────────────────────────────────────────────

    /// Total is offered once the boundary has an arc segment or two
    /// segments of non-zero length, even when they are collinear (the
    /// closed boundary may then enclose no area); a repeated point adds a
    /// zero-length segment that does not count.
    fn total_offered(&self) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        let segments = self.points.len() - 1;
        if self.segs.iter().take(segments).any(|bulge| *bulge != 0.0) {
            return true;
        }
        self.points
            .windows(2)
            .filter(|pair| (pair[1] - pair[0]).length() > 1e-9)
            .count()
            >= 2
    }

    fn local(&self, point: DVec3) -> DVec3 {
        self.plane.to_local(point)
    }

    fn last_local(&self) -> Option<DVec3> {
        self.points.last().map(|point| self.local(*point))
    }

    fn last2(&self) -> Option<DVec2> {
        self.last_local().map(|point| point.truncate())
    }

    fn push_point(&mut self, point: DVec3) -> CmdResult {
        if let Some(last) = self.last_local() {
            self.tangent = seg_exit_tangent(last, self.local(point), 0.0).or(self.tangent);
        }
        self.points.push(point);
        self.segs.push(0.0);
        CmdResult::NeedPoint
    }

    /// An arc segment from the last point to `e` (working-plane coordinates).
    fn push_arc(&mut self, e: DVec2, bulge: f64) -> CmdResult {
        let Some(last) = self.last_local() else {
            return CmdResult::NeedPoint;
        };
        let end = DVec3::new(e.x, e.y, last.z);
        let index = self.points.len() - 1;
        self.segs[index] = bulge;
        self.tangent = seg_exit_tangent(last, end, bulge);
        self.arc_sub = Sub::None;
        self.points.push(self.plane.to_world(end));
        self.segs.push(0.0);
        CmdResult::NeedPoint
    }

    fn undo_point(&mut self) -> CmdResult {
        // Undo goes back to the mode of the segment it removes: undoing an
        // arc stays in arc mode, undoing a line returns to line mode.
        let n = self.segs.len();
        self.arc = n >= 2 && self.segs[n - 2] != 0.0;
        self.points.pop();
        self.segs.pop();
        if let Some(last) = self.segs.last_mut() {
            *last = 0.0;
        }
        self.arc_sub = Sub::None;
        let count = self.points.len();
        self.tangent = (count >= 2)
            .then(|| {
                seg_exit_tangent(
                    self.local(self.points[count - 2]),
                    self.local(self.points[count - 1]),
                    self.segs[count - 2],
                )
            })
            .flatten();
        CmdResult::NeedPoint
    }

    fn heading(&self) -> DVec2 {
        self.entry_tangent()
            .map_or(DVec2::X, |tangent| tangent.as_dvec2())
    }

    /// Direction the next segment leaves in: the last segment's, or for the
    /// first one the drawing's last line or arc direction.
    fn entry_tangent(&self) -> Option<Vec2> {
        self.tangent.or_else(|| {
            let local = self.plane.vector_to_local(self.start_direction?).truncate();
            local.try_normalize().map(|direction| direction.as_vec2())
        })
    }

    /// Arc option answered with a typed value.
    fn arc_value(&mut self, value: f64) -> Option<CmdResult> {
        let a = self.last2()?;
        let angle_ok = |r: f64| r.abs() > 1e-9 && r.abs() < std::f64::consts::TAU;
        let next = match self.arc_sub {
            Sub::ArcAngle if angle_ok(value.to_radians()) => Sub::ArcAngleEnd {
                angle: value.to_radians(),
            },
            Sub::ArcAngleRadius { angle } if value > 0.0 => {
                Sub::ArcAngleRadiusDir { angle, r: value }
            }
            Sub::ArcRadius if value > 0.0 => Sub::ArcRadiusEnd { r: value },
            Sub::ArcRadiusAngle { r } if angle_ok(value.to_radians()) => Sub::ArcRadiusAngleDir {
                r,
                angle: value.to_radians(),
            },
            sub @ Sub::ArcCenterAngle { .. } => {
                let (e, bulge) =
                    arc_for(sub, a, DVec2::new(value.to_radians(), 0.0), self.entry_tangent())?;
                return Some(self.push_arc(e, bulge));
            }
            sub @ Sub::ArcCenterLength { .. } => {
                let (e, bulge) = arc_for(sub, a, DVec2::new(value, 0.0), self.entry_tangent())?;
                return Some(self.push_arc(e, bulge));
            }
            _ => return None,
        };
        self.arc_sub = next;
        Some(CmdResult::NeedPoint)
    }

    /// Arc mode answered with a point.
    fn arc_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(a) = self.last2() else {
            return CmdResult::NeedPoint;
        };
        let p = self.local(pt).truncate();
        let sub = self.arc_sub;
        if sub.is_scalar() {
            let value = if sub.is_angle() {
                (p - a).y.atan2((p - a).x).to_degrees()
            } else {
                (p - a).length()
            };
            return self.arc_value(value).unwrap_or(CmdResult::NeedPoint);
        }
        match sub {
            // The endpoint of an arc leaving along the last segment. One
            // lying on that direction makes no arc: the point is dropped and
            // the arc prompt is not printed again.
            Sub::None if (p - a).length_squared() > 1e-12 => {
                let bulge = compute_bulge(a, self.heading(), p);
                if bulge == 0.0 {
                    return CmdResult::NeedPointQuiet;
                }
                return self.push_arc(p, bulge);
            }
            Sub::ArcCenter if (p - a).length_squared() > 1e-12 => {
                self.arc_sub = Sub::ArcCenterEnd { c: p }
            }
            Sub::ArcDirection => {
                if let Some(dir) = (p - a).try_normalize() {
                    self.arc_sub = Sub::ArcDirectionEnd { dir };
                }
            }
            Sub::ArcSecond if (p - a).length_squared() > 1e-12 => {
                self.arc_sub = Sub::ArcSecondEnd { s: p }
            }
            Sub::None | Sub::ArcCenter | Sub::ArcSecond => {}
            _ => {
                if let Some((e, bulge)) = arc_for(sub, a, p, self.entry_tangent()) {
                    return self.push_arc(e, bulge);
                }
            }
        }
        CmdResult::NeedPoint
    }

    /// Direction of the last segment, degrees: the chord direction default.
    fn tangent_degrees(&self) -> f64 {
        let t = self.heading();
        t.y.atan2(t.x).to_degrees().rem_euclid(360.0)
    }

    fn arc_chord(&mut self, degrees: f64) -> Option<CmdResult> {
        let a = self.last2()?;
        let p = a + DVec2::from_angle(degrees.to_radians());
        let (e, bulge) = arc_for(self.arc_sub, a, p, self.entry_tangent())?;
        Some(self.push_arc(e, bulge))
    }

    fn chord_step(&self) -> bool {
        matches!(
            self.arc_sub,
            Sub::ArcAngleRadiusDir { .. } | Sub::ArcRadiusAngleDir { .. }
        )
    }

    fn arc_prompt(&self) -> String {
        if self.chord_step() {
            let default = format!("{:.4}", self.tangent_degrees());
            let default = default.trim_end_matches('0').trim_end_matches('.');
            return crate::tr!("measurement", "arc-chord-direction", default = default);
        }
        match self.arc_sub {
            // MEASUREGEOM shows no Ctrl hint here, after CEnter included.
            Sub::ArcAngle | Sub::ArcRadiusAngle { .. } | Sub::ArcCenterAngle { .. } => {
                crate::tr!("measurement", "arc-included-angle")
            }
            Sub::ArcAngleEnd { .. } => crate::tr!("measurement", "arc-end-center-radius"),
            Sub::ArcCenter | Sub::ArcAngleCenter { .. } => crate::tr!("measurement", "arc-center"),
            Sub::ArcCenterEnd { .. } => crate::tr!("measurement", "arc-end-angle-length"),
            Sub::ArcCenterLength { .. } => crate::tr!("measurement", "arc-chord-length"),
            Sub::ArcDirection => crate::tr!("measurement", "arc-tangent-direction"),
            Sub::ArcDirectionEnd { .. } => crate::tr!("measurement", "arc-end-direction"),
            Sub::ArcRadius | Sub::ArcAngleRadius { .. } => crate::tr!("measurement", "arc-radius"),
            Sub::ArcRadiusEnd { .. } => crate::tr!("measurement", "arc-end-angle"),
            Sub::ArcSecond => crate::tr!("measurement", "arc-second"),
            Sub::ArcSecondEnd { .. } => crate::tr!("measurement", "arc-end"),
            _ if self.points.len() >= 2 => crate::tr!("measurement", "arc-endpoint-close"),
            _ => crate::tr!("measurement", "arc-endpoint"),
        }
    }

    /// Area and perimeter of the boundary (closed back to its first point),
    /// with `cursor` as a further line point when given.
    fn boundary_measurement(&self, cursor: Option<DVec3>) -> AreaMeasurement {
        let mut points = self.points.clone();
        let mut bulges = self.segs.clone();
        if let Some(cursor) = cursor {
            if let Some(last) = bulges.last_mut() {
                *last = 0.0;
            }
            points.push(cursor);
            bulges.push(0.0);
        }
        if bulges.iter().all(|bulge| *bulge == 0.0) {
            return AreaCommand::point_measurement(&points, true);
        }
        // Arcs: measured exactly as a closed bulged polyline in the plane.
        let mut polyline = LwPolyline::new();
        polyline.is_closed = true;
        polyline.vertices = points
            .iter()
            .zip(bulges)
            .map(|(point, bulge)| {
                let local = self.local(*point);
                let mut vertex = LwVertex::new(Vector2::new(local.x, local.y));
                vertex.bulge = bulge;
                vertex
            })
            .collect();
        AreaCommand::entity_measurement(&EntityType::LwPolyline(polyline))
            .unwrap_or_else(|| AreaCommand::point_measurement(&points, true))
    }

    fn area_message(measurement: AreaMeasurement, circle: bool) -> String {
        let area = area_text(measurement.area);
        match measurement.perimeter {
            Some(perimeter) if circle => crate::tr!(
                "measurement",
                "area-circumference",
                area = area,
                perimeter = length_text(perimeter)
            ),
            Some(perimeter) => crate::tr!(
                "measurement",
                "area-perimeter",
                area = area,
                perimeter = length_text(perimeter)
            ),
            None => crate::tr!("measurement", "area-only", area = area),
        }
    }

    fn sign(&self) -> f64 {
        if self.sum == Sum::Subtract {
            -1.0
        } else {
            1.0
        }
    }

    fn total_message(&self) -> String {
        let total = area_text(self.total);
        if self.volume {
            crate::tr!("measurement", "total-volume", total = total)
        } else {
            crate::tr!("measurement", "total-area", total = total)
        }
    }

    /// Report one area or volume `value` (`message` is its result line):
    /// alone it ends the measurement, under Add / Subtract it joins the
    /// running total and the boundary starts over.
    fn report(&mut self, value: f64, message: String) -> CmdResult {
        if self.sum == Sum::Single {
            return self.done(message);
        }
        self.total += self.sign() * value;
        let message = format!("{message}\n{}", self.total_message());
        self.points.clear();
        self.segs.clear();
        self.arc = false;
        self.arc_sub = Sub::None;
        self.tangent = None;
        self.last_report = Some(message.clone());
        CmdResult::ReportMeasurement(message)
    }

    fn finish_boundary(&mut self) -> CmdResult {
        let measurement = self.boundary_measurement(None);
        if self.volume {
            self.base_area = measurement.area;
            self.height_base = self.points.last().copied().unwrap_or_default();
            self.step = AreaStep::Height;
            return CmdResult::NeedPoint;
        }
        self.report(measurement.area, Self::area_message(measurement, false))
    }

    fn finish_volume(&mut self, volume: f64) -> CmdResult {
        let message = crate::tr!(
            "measurement",
            "volume-result",
            volume = area_text(volume)
        );
        self.report(volume, message)
    }

    /// Volume of a 3D solid or body, from its B-rep.
    fn solid_volume(entity: &EntityType) -> Option<f64> {
        let acis = match entity {
            EntityType::Solid3D(solid) => &solid.acis_data,
            EntityType::Body(body) => &body.acis_data,
            _ => return None,
        };
        crate::scene::convert::solid3d_tess::kernel_acis_body(acis)
            .and_then(|body| kernel::brep::mass_properties(&body))
            .map(|properties| properties.volume.abs())
    }

    /// eXit at the first-corner prompt: Add / Subtract print the total once
    /// more; either way the option prompt comes back.
    fn exit_area(&mut self) -> CmdResult {
        if self.sum != Sum::Single {
            let message = self.total_message();
            return self.done(message);
        }
        self.reset();
        self.mode = Mode::Choose;
        CmdResult::NeedPoint
    }

    fn area_text(&mut self, input: &str) -> Option<CmdResult> {
        match self.step {
            AreaStep::Height => {
                // A point gives the height from the base point.
                if Self::is_point_text(input) {
                    return None;
                }
                return Some(match crate::entities::common::parse_typed_length(input) {
                    Some(height) => self.finish_height(height),
                    None => CmdResult::ReportError(
                        t!("Requires numeric distance or two points.").into_owned(),
                    ),
                });
            }
            AreaStep::Objects => {
                // Points and Last are object picks; any other keyword is not.
                if !Self::is_keyword(input) || matches!(input, "L" | "LAST") {
                    return None;
                }
                return Some(CmdResult::ReportError(crate::tr!(
                    "measurement",
                    "invalid-selection"
                )));
            }
            AreaStep::Points | AreaStep::Length => {}
        }
        if !self.points.is_empty() {
            return self.chain_text(input);
        }
        if !Self::is_keyword(input) {
            return None;
        }
        match input {
            "O" | "OBJECT" => self.step = AreaStep::Objects,
            "A" | "ADD" | "ADD AREA" | "ADD VOLUME" if self.sum != Sum::Add => {
                if self.sum == Sum::Single {
                    self.total = 0.0;
                }
                self.sum = Sum::Add;
            }
            "S" | "SUBTRACT" | "SUBTRACT AREA" | "SUBTRACT VOLUME" if self.sum != Sum::Subtract => {
                if self.sum == Sum::Single {
                    self.total = 0.0;
                }
                self.sum = Sum::Subtract;
            }
            "X" | "EXIT" => return Some(self.exit_area()),
            _ => return Some(Self::keyword_error()),
        }
        Some(CmdResult::NeedPoint)
    }

    /// A typed coordinate rather than a single value.
    fn is_point_text(input: &str) -> bool {
        input.contains(',') || input.starts_with('@') || input.contains('<')
    }

    /// Keywords and values of a chain of picked points (an ARea / Volume
    /// boundary or Distance's Multiple points): Arc, Length, Undo, Close,
    /// Total and the arc options.
    fn chain_text(&mut self, input: &str) -> Option<CmdResult> {
        if self.step == AreaStep::Length {
            if Self::is_point_text(input) {
                return None;
            }
            let Some(length) = crate::entities::common::parse_typed_length(input) else {
                return self.sub_prompt_error(false).map(CmdResult::ReportError);
            };
            let last = self.last_local()?;
            let end = last + self.heading().extend(0.0) * length;
            self.step = AreaStep::Points;
            return Some(self.push_point(self.plane.to_world(end)));
        }
        if self.arc && self.chord_step() {
            if Self::is_point_text(input) {
                return None;
            }
            let Some(radians) = crate::entities::common::parse_typed_angle(input) else {
                return self.sub_prompt_error(false).map(CmdResult::ReportError);
            };
            return self.arc_chord(radians.to_degrees());
        }
        if self.arc && self.arc_sub != Sub::None {
            let next = match (self.arc_sub, input) {
                (Sub::ArcAngleEnd { angle }, "CE" | "CENTER" | "CENTRE") => {
                    Sub::ArcAngleCenter { angle }
                }
                (Sub::ArcAngleEnd { angle }, "R" | "RADIUS") => Sub::ArcAngleRadius { angle },
                (Sub::ArcCenterEnd { c }, "A" | "ANGLE") => Sub::ArcCenterAngle { c },
                (Sub::ArcCenterEnd { c }, "L" | "LENGTH") => Sub::ArcCenterLength { c },
                (Sub::ArcRadiusEnd { r }, "A" | "ANGLE") => Sub::ArcRadiusAngle { r },
                (sub, text) if sub.is_scalar() && !Self::is_point_text(text) => {
                    let value = if sub.is_angle() {
                        crate::entities::common::parse_typed_angle(text).map(f64::to_degrees)
                    } else {
                        crate::entities::common::parse_typed_length(text)
                    };
                    return match value {
                        Some(v) => self.arc_value(v),
                        None => self.sub_prompt_error(false).map(CmdResult::ReportError),
                    };
                }
                (_, text) if Self::is_keyword(text) => {
                    return self.sub_prompt_error(false).map(CmdResult::ReportError);
                }
                _ => return None,
            };
            self.arc_sub = next;
            return Some(CmdResult::NeedPoint);
        }
        if !Self::is_keyword(input) {
            return None;
        }
        let distance = self.mode == Mode::Distance;
        // The first point stays: Undo there has nothing left to undo.
        let can_undo = self.points.len() > 1;
        if matches!(input, "U" | "UNDO") && !can_undo {
            let message = crate::tr!("measurement", "all-undone");
            // A Distance chain prints only the first line; an area boundary
            // adds "Command has been completely undone.".
            let message = if distance {
                message.lines().next().unwrap_or_default().to_string()
            } else {
                message
            };
            return Some(CmdResult::ReportMessage(message));
        }
        if self.arc {
            self.arc_sub = match input {
                "A" | "ANGLE" => Sub::ArcAngle,
                "CE" | "CENTER" | "CENTRE" => Sub::ArcCenter,
                "D" | "DIRECTION" => Sub::ArcDirection,
                "R" | "RADIUS" => Sub::ArcRadius,
                "S" | "SECOND" | "SECOND PT" => Sub::ArcSecond,
                "L" | "LINE" => {
                    self.arc = false;
                    Sub::None
                }
                "CL" | "CLOSE" if self.points.len() >= 2 => return Some(self.close(true)),
                "U" | "UNDO" if can_undo => return Some(self.undo_point()),
                _ => return Some(Self::keyword_error()),
            };
            return Some(CmdResult::NeedPoint);
        }
        Some(match input {
            "A" | "ARC" => {
                self.arc = true;
                CmdResult::NeedPoint
            }
            "L" | "LENGTH" => {
                self.step = AreaStep::Length;
                CmdResult::NeedPoint
            }
            "U" | "UNDO" if can_undo => self.undo_point(),
            "C" | "CLOSE" if distance && self.points.len() >= 2 => self.close(false),
            "T" | "TOTAL" if distance || self.total_offered() => self.finish_chain(),
            _ => Self::keyword_error(),
        })
    }

    /// Close the chain back to its first point (with a tangent arc in arc
    /// mode) and finish it.
    fn close(&mut self, arc: bool) -> CmdResult {
        let (Some(first), Some(last)) = (self.points.first().copied(), self.last2()) else {
            return CmdResult::NeedPoint;
        };
        let start = self.local(first).truncate();
        let bulge = if arc {
            compute_bulge(last, self.heading(), start)
        } else {
            0.0
        };
        if self.mode == Mode::Distance {
            self.push_arc(start, bulge);
        } else if let Some(closing) = self.segs.last_mut() {
            *closing = bulge;
        }
        self.finish_chain()
    }

    /// Total / Enter: Distance prints the chain length, ARea measures the
    /// boundary, Volume asks its height.
    fn finish_chain(&mut self) -> CmdResult {
        if self.mode == Mode::Distance {
            let message = self.running_distance();
            return self.done(message);
        }
        self.finish_boundary()
    }

    /// Length of the picked chain, arcs included, with `cursor` as a further
    /// line point when given.
    fn chain_length(&self, cursor: Option<DVec3>) -> f64 {
        let mut points: Vec<DVec3> = self.points.iter().map(|p| self.local(*p)).collect();
        let mut bulges = self.segs.clone();
        if let Some(cursor) = cursor {
            points.push(self.local(cursor));
            bulges.push(0.0);
        }
        points
            .windows(2)
            .zip(bulges)
            .map(|(pair, bulge)| {
                if bulge == 0.0 {
                    return (pair[1] - pair[0]).length();
                }
                let sweep = 4.0 * bulge.atan();
                (pair[1] - pair[0]).truncate().length() * (sweep * 0.5) / (sweep * 0.5).sin()
            })
            .sum()
    }

    fn running_distance(&self) -> String {
        crate::tr!(
            "measurement",
            "distance",
            value = length_text(self.chain_length(None))
        )
    }

    /// Multiple points: every change of the chain prints its length.
    /// Switching between line and arc mode prints it too, and a refused
    /// entry prints it again after the error. With only the first point
    /// there is no chain yet and nothing is printed.
    fn report_chain_change(&mut self, before: (usize, bool), result: CmdResult) -> CmdResult {
        if self.mode != Mode::Distance || !self.multiple || self.points.len() < 2 {
            return result;
        }
        let changed = self.points.len() != before.0 || self.arc != before.1;
        match result {
            CmdResult::NeedPoint | CmdResult::NeedPointQuiet if changed => {
                CmdResult::ReportMeasurement(self.running_distance())
            }
            // A sub-prompt (line length, an arc option) only repeats itself.
            CmdResult::ReportError(error) if self.sub_prompt_error(false).is_some() => {
                CmdResult::ReportError(error)
            }
            CmdResult::ReportError(error) => {
                CmdResult::ReportErrorAndMeasurement(error, self.running_distance())
            }
            result => result,
        }
    }

    /// The height typed (or measured from the base point) for Volume.
    fn finish_height(&mut self, height: f64) -> CmdResult {
        let volume = self.base_area * height;
        // Back to the step the height was asked from.
        self.step = if self.points.is_empty() {
            AreaStep::Objects
        } else {
            AreaStep::Points
        };
        self.finish_volume(volume)
    }

    fn area_object(&mut self, entity: EntityType, point: DVec3) -> CmdResult {
        if self.volume {
            if let Some(volume) = Self::solid_volume(&entity) {
                return self.finish_volume(volume);
            }
        }
        // A line (no enclosed area) is refused like a text.
        let Some(measurement) =
            AreaCommand::entity_measurement(&entity).filter(|measurement| measurement.area > 0.0)
        else {
            return CmdResult::ReportError(crate::tr!("measurement", "not-measurable"));
        };
        if self.volume {
            self.base_area = measurement.area;
            self.height_base = point;
            self.step = AreaStep::Height;
            return CmdResult::NeedPoint;
        }
        let circle = matches!(entity, EntityType::Circle(_));
        self.report(measurement.area, Self::area_message(measurement, circle))
    }

    fn area_prompt(&self) -> String {
        match self.step {
            AreaStep::Height => return crate::tr!("measurement", "height"),
            AreaStep::Length => return crate::tr!("measurement", "length-of-line"),
            AreaStep::Objects => {
                return match self.sum {
                    Sum::Single => t!("Select objects:").into_owned(),
                    Sum::Add => crate::tr!("measurement", "add-objects"),
                    Sum::Subtract => crate::tr!("measurement", "subtract-objects"),
                }
            }
            AreaStep::Points => {}
        }
        if self.points.is_empty() {
            return match (self.volume, self.sum) {
                (false, Sum::Single) => crate::tr!("measurement", "area-first"),
                (false, Sum::Add) => crate::tr!("measurement", "area-first-add"),
                (false, Sum::Subtract) => crate::tr!("measurement", "area-first-subtract"),
                (true, Sum::Single) => crate::tr!("measurement", "volume-first"),
                (true, Sum::Add) => crate::tr!("measurement", "volume-first-add"),
                (true, Sum::Subtract) => crate::tr!("measurement", "volume-first-subtract"),
            };
        }
        let prefix = match self.sum {
            Sum::Single => String::new(),
            Sum::Add => crate::tr!("measurement", "add-mode"),
            Sum::Subtract => crate::tr!("measurement", "subtract-mode"),
        };
        let prompt = if self.arc {
            self.arc_prompt()
        } else if self.total_offered() {
            crate::tr!("measurement", "next-point-total")
        } else {
            crate::tr!("measurement", "next-point")
        };
        prefix + &prompt
    }

    fn area_options(&self) -> Vec<CmdOption> {
        match self.step {
            AreaStep::Points => {}
            _ => return Vec::new(),
        }
        if self.points.is_empty() {
            let objects = CmdOption::new("Object", "O");
            let add = CmdOption {
                label: if self.volume {
                    crate::tr!("measurement", "option-add-volume")
                } else {
                    crate::tr!("measurement", "option-add-area")
                },
                keyword: "A".into(),
            };
            let subtract = CmdOption {
                label: if self.volume {
                    crate::tr!("measurement", "option-subtract-volume")
                } else {
                    crate::tr!("measurement", "option-subtract-area")
                },
                keyword: "S".into(),
            };
            let exit = CmdOption::new("eXit", "X");
            return match self.sum {
                Sum::Single => vec![objects, add, subtract, exit],
                Sum::Add => vec![objects, subtract, exit],
                Sum::Subtract => vec![objects, add, exit],
            };
        }
        self.chain_options()
    }

    /// Options of a point chain step (ARea / Volume boundary, Multiple points).
    fn chain_options(&self) -> Vec<CmdOption> {
        if self.step != AreaStep::Points {
            return Vec::new();
        }
        let distance = self.mode == Mode::Distance;
        if self.arc {
            return match self.arc_sub {
                Sub::None => {
                    let mut options = vec![
                        CmdOption::new("Angle", "A"),
                        CmdOption::new("CEnter", "CE"),
                    ];
                    if self.points.len() >= 2 {
                        options.push(CmdOption::new("CLose", "CL"));
                    }
                    options.extend([
                        CmdOption::new("Direction", "D"),
                        CmdOption::new("Line", "L"),
                        CmdOption::new("Radius", "R"),
                        CmdOption::new("Second pt", "S"),
                        CmdOption::new("Undo", "U"),
                    ]);
                    options
                }
                Sub::ArcAngleEnd { .. } => vec![
                    CmdOption::new("CEnter", "CE"),
                    CmdOption::new("Radius", "R"),
                ],
                Sub::ArcCenterEnd { .. } => {
                    vec![CmdOption::new("Angle", "A"), CmdOption::new("Length", "L")]
                }
                Sub::ArcRadiusEnd { .. } => vec![CmdOption::new("Angle", "A")],
                _ => Vec::new(),
            };
        }
        let mut options = vec![CmdOption::new("Arc", "A")];
        if distance && self.points.len() >= 2 {
            options.push(CmdOption::new("Close", "C"));
        }
        options.push(CmdOption::new("Length", "L"));
        options.push(CmdOption::new("Undo", "U"));
        if distance || self.total_offered() {
            options.push(CmdOption::new("Total", "T"));
        }
        options
    }

    /// Points of the boundary for display, arcs sampled, in world space.
    fn boundary_display(&self) -> Vec<DVec3> {
        let mut out = Vec::new();
        for (index, point) in self.points.iter().enumerate() {
            out.push(*point);
            let bulge = self.segs[index];
            let Some(next) = self.points.get(index + 1) else {
                break;
            };
            if bulge == 0.0 {
                continue;
            }
            let (a, b) = (self.local(*point), self.local(*next));
            let chord = (b - a).truncate();
            let sweep = 4.0 * bulge.atan();
            let left = DVec2::new(-chord.y, chord.x) * 0.5 / (sweep * 0.5).tan();
            let centre = (a.truncate() + b.truncate()) * 0.5 + left;
            let start = a.truncate() - centre;
            for step in 1..16 {
                let rotated = DVec2::from_angle(sweep * step as f64 / 16.0).rotate(start);
                out.push(self.plane.to_world((centre + rotated).extend(a.z)));
            }
        }
        out
    }
    fn radius_msg(radius: f64) -> String {
        t!(
            "Radius = %{radius},  Diameter = %{diameter}",
            radius = length_text(radius),
            diameter = length_text(radius * 2.0)
        )
        .into_owned()
    }

    // Normalize only after rescaling, retaining f64 precision without an
    // arbitrary drawing-unit tolerance or overflowing on long vectors.
    fn direction(vector: DVec3) -> Option<DVec3> {
        if !vector.is_finite() {
            return None;
        }
        let scale = vector.abs().max_element();
        if scale == 0.0 {
            return None;
        }
        let vector = vector / scale;
        Some(vector / vector.length())
    }

    fn angle_degrees(a: DVec3, b: DVec3) -> Option<f64> {
        let a = Self::direction(a)?;
        let b = Self::direction(b)?;
        Some(a.cross(b).length().atan2(a.dot(b)).to_degrees())
    }

    fn line_direction(entity: &EntityType) -> Option<DVec3> {
        let EntityType::Line(line) = entity else {
            return None;
        };
        Self::direction(
            DVec3::new(line.end.x, line.end.y, line.end.z)
                - DVec3::new(line.start.x, line.start.y, line.start.z),
        )
    }

    // Unoriented lines: the smaller angle, independent of endpoint order.
    fn line_angle(a: DVec3, b: DVec3) -> Option<f64> {
        Self::angle_degrees(a, b).map(|angle| angle.min(180.0 - angle))
    }

    fn angle_value_msg(angle: f64) -> String {
        let aprec = crate::entities::common::unit_context().auprec.max(0) as usize;
        let angle = crate::entities::common::without_negative_zero(format!("{angle:.aprec$}"));
        t!("Angle = %{angle}°", angle = angle).into_owned()
    }

    /// Build a cyan preview wire connecting the picked points and the cursor.
    ///
    /// Keeps full f64 coordinates through [`WireModel::solid_f64`]: previews
    /// cast straight to f32 drift about half a metre away from their snapped
    /// points on UTM-scale drawings.
    fn preview_wire(name: &str, pts: Vec<DVec3>) -> WireModel {
        WireModel::solid_f64(
            name.to_string(),
            pts.into_iter().map(|point| point.to_array()).collect(),
            WireModel::CYAN,
            false,
        )
    }

    fn radius_at(entity: &EntityType, point: DVec3) -> Option<f64> {
        match entity {
            EntityType::Circle(circle) => {
                let radius = circle.radius;
                (radius.is_finite() && radius > 0.0 && (radius * 2.0).is_finite()).then_some(radius)
            }
            EntityType::Arc(arc) => {
                let radius = arc.radius;
                (radius.is_finite() && radius > 0.0 && (radius * 2.0).is_finite()).then_some(radius)
            }
            EntityType::Region(_) => {
                let source = crate::scene::dimension_assoc::radial_source_at(
                    entity,
                    document::types::Vector3::new(point.x, point.y, point.z),
                )?;
                (source.radius.is_finite()
                    && source.radius > 0.0
                    && (source.radius * 2.0).is_finite())
                .then_some(source.radius)
            }
            _ => None,
        }
    }

    /// DISTANCE readout for the two collected points.
    fn distance_msg(plane: WorkingPlane, p1: DVec3, p2: DVec3) -> String {
        use crate::entities::common::format_angle;
        let format_length = length_text;
        let delta = plane.to_local(p2) - plane.to_local(p1);
        let in_plane = delta.y.atan2(delta.x);
        let from_plane = delta.z.atan2(delta.truncate().length());
        let first = crate::tr!(
            "measurement",
            "distance-result",
            distance = format_length(delta.length()),
            in_plane = format_angle(in_plane.rem_euclid(std::f64::consts::TAU)),
            from_plane = format_angle(from_plane)
        );
        let second = crate::tr!(
            "measurement",
            "distance-deltas",
            dx = format_length(delta.x),
            dy = format_length(delta.y),
            dz = format_length(delta.z)
        );
        format!("{first}\n{second}")
    }

    /// The Area readout for an area and perimeter measured outright.
    fn area_perimeter_msg(area: f64, perimeter: f64) -> String {
        Self::area_message(
            AreaMeasurement {
                area,
                perimeter: Some(perimeter),
            },
            false,
        )
    }

    fn angle_msg(vertex: DVec3, a: DVec3, b: DVec3) -> Option<String> {
        Self::angle_degrees(a - vertex, b - vertex).map(Self::angle_value_msg)
    }

    /// Signed offset along local `axis` (0 = X, 1 = Y) from `c` to where the
    /// segment `a`–`b` crosses the line through `c` along that axis.
    fn crossing(a: DVec3, b: DVec3, c: DVec3, axis: usize) -> Option<f64> {
        let other = 1 - axis;
        let (da, db) = (a[other] - c[other], b[other] - c[other]);
        if da * db > 0.0 || da == db {
            return None;
        }
        let t = da / (da - db);
        Some(a[axis] + t * (b[axis] - a[axis]) - c[axis])
    }

    /// Nearest crossings of the drawn geometry with the working plane's X and
    /// Y lines through `cursor`, as local offsets `[-X, +X, -Y, +Y]`.
    fn quick_hits(
        scene: &crate::scene::Scene,
        plane: WorkingPlane,
        cursor: DVec3,
    ) -> ([Option<f64>; 4], [Option<Handle>; 4]) {
        let c = plane.to_local(cursor);
        // Long enough to leave the drawing from anywhere the cursor can be.
        let reach = scene
            .model_space_extents()
            .map(|(lo, hi)| {
                let (lo, hi) = (lo.as_dvec3(), hi.as_dvec3());
                (hi - lo).length() + (cursor - (lo + hi) * 0.5).length()
            })
            .unwrap_or(0.0)
            + 1.0;
        let mut hits = [None; 4];
        let mut handles = [None; 4];
        for (axis, direction) in [plane.x, plane.y].into_iter().enumerate() {
            let (a, b) = (cursor - direction * reach, cursor + direction * reach);
            let pad = reach * 1e-9;
            let candidates = scene.interaction_candidates_xy([
                a.x.min(b.x) - pad,
                a.y.min(b.y) - pad,
                a.x.max(b.x) + pad,
                a.y.max(b.y) + pad,
            ]);
            for wire in candidates.iter() {
                if !wire.display_visible
                    || wire.snap_only
                    || wire.point_marker.is_some()
                    || !wire.text_verts.is_empty()
                {
                    continue;
                }
                let mut previous: Option<DVec3> = None;
                for (index, point) in wire.points.iter().enumerate() {
                    if point[0].is_nan() || point[1].is_nan() {
                        previous = None;
                        continue;
                    }
                    let local = plane.to_local(wire.point_world(index, 0.0));
                    if let Some(offset) = previous
                        .and_then(|previous| Self::crossing(previous, local, c, axis))
                        .filter(|offset| offset.abs() > pad)
                    {
                        let index = axis * 2 + usize::from(offset > 0.0);
                        if hits[index].is_none_or(|best: f64| offset.abs() < best.abs()) {
                            hits[index] = Some(offset);
                            handles[index] = crate::scene::Scene::handle_from_wire_name(&wire.name);
                        }
                    }
                    previous = Some(local);
                }
            }
        }
        (hits, handles)
    }

    /// What QUICK shows of an object one of its lines reaches at `hit`
    /// (world): a line or straight polyline segment's length, as that edge,
    /// or an arc's or circle's radius, as a line from the centre. The edge
    /// comes back too, for the distance between two parallel ones.
    fn quick_object(entity: &EntityType, hit: DVec3) -> Option<(Vec<DVec3>, String, Option<[DVec3; 2]>)> {
        let v = |p: document::Vector3| DVec3::new(p.x, p.y, p.z);
        let length = |a: DVec3, b: DVec3| {
            let value = crate::entities::common::format_length(a.distance(b));
            (vec![a, b], crate::tr!("measurement", "quick-length", value = value), Some([a, b]))
        };
        let radius = |center: DVec3, r: f64| (vec![center, hit], Self::radius_msg(r), None);
        match entity {
            EntityType::Line(line) => Some(length(v(line.start), v(line.end))),
            EntityType::Circle(c) if c.normal.z > 0.0 => Some(radius(v(c.center), c.radius)),
            EntityType::Arc(a) if a.normal.z > 0.0 => Some(radius(v(a.center), a.radius)),
            // ponytail: plan-view polylines only; others need the OCS turned into the plane.
            EntityType::LwPolyline(pl) if pl.normal.z > 0.0 && pl.vertices.len() > 1 => {
                let n = pl.vertices.len();
                let segments = if pl.is_closed { n } else { n - 1 };
                let at = |i: usize| {
                    let p = pl.vertices[i % n].location;
                    DVec3::new(p.x, p.y, pl.elevation)
                };
                let flat = DVec3::new(hit.x, hit.y, pl.elevation);
                let nearest = (0..segments).min_by(|&i, &j| {
                    let d = |k: usize| {
                        let (a, b) = (at(k), at(k + 1));
                        let t = ((flat - a).dot(b - a) / (b - a).length_squared().max(1e-300))
                            .clamp(0.0, 1.0);
                        flat.distance(a + (b - a) * t)
                    };
                    d(i).total_cmp(&d(j))
                })?;
                let (a, b) = (at(nearest), at(nearest + 1));
                let bulge = pl.vertices[nearest].bulge;
                if bulge.abs() < 1e-12 {
                    Some(length(a, b))
                } else {
                    let r = a.distance(b) * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
                    Some((Vec::new(), Self::radius_msg(r), None))
                }
            }
            _ => None,
        }
    }

    /// QUICK additions for the objects `handles` reach at `hits`: each
    /// object's measure once, and the distance across two parallel edges one
    /// cast line reaches (when they are not square to it, so the plain
    /// reading is not already that distance).
    fn quick_objects(
        scene: &crate::scene::Scene,
        plane: WorkingPlane,
        cursor: DVec3,
        hits: [Option<f64>; 4],
        handles: [Option<Handle>; 4],
    ) -> (Vec<WireModel>, Vec<String>) {
        let c = plane.to_local(cursor);
        let mut wires = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let mut edges: [Option<[DVec3; 2]>; 4] = [None; 4];
        for slot in 0..4 {
            let (Some(offset), Some(handle)) = (hits[slot], handles[slot]) else {
                continue;
            };
            let Some(entity) = scene.document.get_entity(handle) else {
                continue;
            };
            let mut local = c;
            local[slot / 2] += offset;
            let Some((points, line, edge)) = Self::quick_object(entity, plane.to_world(local)) else {
                continue;
            };
            edges[slot] = edge;
            if lines.contains(&line) {
                continue;
            }
            if points.len() == 2 {
                wires.push(Self::preview_wire("measuregeom_quick_object", points));
            }
            lines.push(line);
        }
        for axis in 0..2 {
            let (Some([a, b]), Some([p, q])) = (edges[axis * 2], edges[axis * 2 + 1]) else {
                continue;
            };
            let Some(dir) = Self::direction(b - a) else {
                continue;
            };
            let cast = if axis == 0 { plane.x } else { plane.y };
            let parallel =
                Self::direction(q - p).is_some_and(|other| other.cross(dir).length() < 1e-9);
            if !parallel || dir.dot(cast).abs() < 1e-9 {
                continue;
            }
            let foot = |x: DVec3, a: DVec3| a + dir * (x - a).dot(dir);
            let mid = foot(cursor, a);
            let across = foot(mid, p);
            wires.push(Self::preview_wire("measuregeom_quick_parallel", vec![mid, across]));
            let value = crate::entities::common::format_length(mid.distance(across));
            lines.push(crate::tr!("measurement", "quick-parallel", value = value));
        }
        (wires, lines)
    }

    /// QUICK click: the area and perimeter of the closed region around `pt`.
    fn quick_area(&self, pt: DVec3) -> Option<String> {
        let sources = self.quick_sources.borrow();
        let local = self.plane.to_local(pt);
        let xy = [local.x, local.y];
        let faces = crate::scene::boundary_faces_around(sources.as_ref()?, xy, 0.0);
        let rings = crate::modules::draw::draw::hatch::resolve_hatch_rings(&faces, xy)?;
        let measure = |ring: &Vec<[f64; 2]>| {
            let n = ring.len();
            (0..n).fold((0.0, 0.0), |(area, perimeter), i| {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                (
                    area + a[0] * b[1] - b[0] * a[1],
                    perimeter + (b[0] - a[0]).hypot(b[1] - a[1]),
                )
            })
        };
        let (outer, holes) = rings.split_first()?;
        let (area, mut perimeter) = measure(outer);
        let mut area = (area * 0.5).abs();
        for hole in holes {
            let (a, p) = measure(hole);
            area -= (a * 0.5).abs();
            perimeter += p;
        }
        Some(Self::area_perimeter_msg(area, perimeter))
    }

    /// QUICK measurement lines (world) and readout for crossings `hits`.
    fn quick_result(
        plane: WorkingPlane,
        cursor: DVec3,
        hits: [Option<f64>; 4],
    ) -> (Vec<WireModel>, Option<String>) {
        let c = plane.to_local(cursor);
        let mut wires = Vec::new();
        let mut lines = Vec::new();
        for axis in 0..2 {
            let (low, high) = (hits[axis * 2], hits[axis * 2 + 1]);
            if low.is_none() && high.is_none() {
                continue;
            }
            let (from, to) = (low.unwrap_or(0.0), high.unwrap_or(0.0));
            let mut start = c;
            let mut end = c;
            start[axis] += from;
            end[axis] += to;
            wires.push(Self::preview_wire(
                "measuregeom_quick",
                vec![plane.to_world(start), plane.to_world(end)],
            ));
            let value = length_text(to - from);
            lines.push(if axis == 0 {
                crate::tr!("measurement", "quick-horizontal", value = value)
            } else {
                crate::tr!("measurement", "quick-vertical", value = value)
            });
        }
        let readout = (!lines.is_empty()).then(|| lines.join("\n"));
        (wires, readout)
    }
}

impl CadCommand for MeasureGeomCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match self.mode {
            Mode::Choose => {
                return crate::tr!("measurement", "option-prompt", default = self.last)
            }
            Mode::AskMode => return crate::tr!("measurement", "mode-prompt"),
            Mode::Quick => return crate::tr!("measurement", "quick-options", command = self.name),
            Mode::Area => return self.area_prompt(),
            Mode::Distance => {
                return match (self.points.len(), self.multiple) {
                    (0, _) => crate::tr!("measurement", "first-point"),
                    (_, false) => crate::tr!("measurement", "second-point"),
                    _ if self.step == AreaStep::Length => {
                        crate::tr!("measurement", "length-of-line")
                    }
                    _ if self.arc => self.arc_prompt(),
                    (1, true) => crate::tr!("measurement", "next-point-multiple"),
                    (_, true) => crate::tr!("measurement", "next-point-multiple-close"),
                }
            }
            Mode::Angle => match self.points.len() {
                0 => return crate::tr!("measurement", "angle-start", command = self.name),
                1 => t!("MEASUREGEOM  Specify first ray point:").into_owned(),
                _ => t!("MEASUREGEOM  Specify second ray point:").into_owned(),
            },
            Mode::Radius => {
                t!("MEASUREGEOM  Select arc, circle, or circular region edge:").into_owned()
            }
            Mode::AngleLines => {
                return if self.first_line.is_none() {
                    crate::tr!("measurement", "first-line", command = self.name)
                } else {
                    crate::tr!("measurement", "second-line", command = self.name)
                }
            }
        }
        .replace("MEASUREGEOM", self.name)
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn wants_text_input(&self) -> bool {
        match self.mode {
            Mode::Choose | Mode::AskMode | Mode::Quick | Mode::Distance => true,
            Mode::Area => self.step != AreaStep::Objects,
            _ => self.point_step_accepts_keywords(),
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        match self.mode {
            Mode::Quick => true,
            Mode::Distance | Mode::Area => self.step == AreaStep::Points,
            Mode::Angle => self.points.is_empty(),
            Mode::AngleLines => self.first_line.is_none(),
            _ => false,
        }
    }

    fn has_measurement_readout(&self) -> bool {
        true
    }

    fn measurement_readout(&self, cursor: DVec3) -> Option<String> {
        match self.mode {
            Mode::Quick => self.quick.borrow().clone(),
            Mode::Distance if !self.points.is_empty() && !self.arc && cursor.is_finite() => {
                Some(crate::tr!(
                    "measurement",
                    "distance",
                    value = length_text(self.chain_length(Some(cursor)))
                ))
            }
            Mode::Area
                if self.step == AreaStep::Points
                    && !self.arc
                    && self.points.len() >= 2
                    && cursor.is_finite() =>
            {
                Some(Self::area_message(
                    self.boundary_measurement(Some(cursor)),
                    false,
                ))
            }
            Mode::Angle if self.points.len() >= 2 => {
                Self::angle_msg(self.points[0], self.points[1], cursor)
            }
            Mode::Radius => self
                .hovered
                .as_ref()
                .and_then(|(_, e)| Self::radius_at(e, cursor))
                .map(Self::radius_msg),
            Mode::AngleLines => {
                let (first_handle, first) = self.first_line?;
                let (handle, entity) = self.hovered.as_ref()?;
                if first_handle == *handle {
                    return None;
                }
                Self::line_angle(first, Self::line_direction(entity)?).map(Self::angle_value_msg)
            }
            _ => None,
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.mode {
            Mode::Choose | Mode::Quick => Self::option_list(),
            Mode::AskMode => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Mode::Area => self.area_options(),
            Mode::Distance if self.points.len() == 1 && !self.multiple => vec![CmdOption {
                label: crate::tr!("measurement", "option-multiple-points"),
                keyword: "M".into(),
            }],
            Mode::Distance if self.multiple => self.chain_options(),
            Mode::Angle if self.points.is_empty() => vec![CmdOption {
                label: crate::tr!("measurement", "lines"),
                keyword: "LINES".into(),
            }],
            Mode::AngleLines if self.first_line.is_none() => vec![CmdOption {
                label: crate::tr!("measurement", "points"),
                keyword: "POINTS".into(),
            }],
            _ => Vec::new(),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim().to_ascii_uppercase();
        match self.mode {
            Mode::Choose => {
                return Some(match self.choose(&input) {
                    Some(result) => result,
                    None => self.invalid_option(),
                });
            }
            // A letter that is no option is left alone: quick measure keeps
            // following the cursor.
            Mode::Quick => return self.choose(&input),
            Mode::AskMode => {
                let quick = match input.as_str() {
                    "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return Some(CmdResult::ReportError(t!("Invalid option keyword.").into_owned())),
                };
                set_quick_default(quick);
                self.mode = Mode::Choose;
                return Some(CmdResult::NeedPoint);
            }
            Mode::Area => return self.area_text(&input),
            Mode::Distance => {
                if self.multiple {
                    let before = (self.points.len(), self.arc);
                    let result = self.chain_text(&input)?;
                    return Some(self.report_chain_change(before, result));
                }
                if !Self::is_keyword(&input) {
                    return None;
                }
                return Some(match input.as_str() {
                    "M" | "MULTIPLE" | "MULTIPLE POINTS" if self.points.len() == 1 => {
                        self.multiple = true;
                        CmdResult::NeedPoint
                    }
                    _ => Self::keyword_error(),
                });
            }
            _ => {}
        }
        if self.mode == Mode::Angle
            && self.points.is_empty()
            && matches!(input.as_str(), "L" | "LINES")
        {
            self.mode = Mode::AngleLines;
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == Mode::AngleLines
            && self.first_line.is_none()
            && matches!(input.as_str(), "P" | "POINTS")
        {
            self.mode = Mode::Angle;
            self.hovered = None;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, Mode::Radius | Mode::AngleLines)
            || (self.mode == Mode::Area && self.step == AreaStep::Objects)
    }

    fn typed_point_picks_entity(&self) -> bool {
        self.needs_entity_pick()
    }

    fn entity_pick_accepts_last(&self) -> bool {
        self.mode == Mode::Area
    }

    // QUICK reads the scene under the cursor; it never resolves an object, so
    // every click lands in `on_point`.
    fn needs_structure_point_pick(&self) -> bool {
        self.mode == Mode::Quick
    }

    fn object_pick_hover_previews(
        &self,
        scene: &crate::scene::Scene,
        cursor: DVec3,
    ) -> Vec<WireModel> {
        if self.mode != Mode::Quick || !cursor.is_finite() {
            return Vec::new();
        }
        if self.quick_sources.borrow().is_none() {
            *self.quick_sources.borrow_mut() = Some(scene.boundary_sources_on_plane(self.plane, 1e-6));
        }
        let (hits, handles) = Self::quick_hits(scene, self.plane, cursor);
        let (mut wires, readout) = Self::quick_result(self.plane, cursor, hits);
        let (object_wires, object_lines) = Self::quick_objects(scene, self.plane, cursor, hits, handles);
        wires.extend(object_wires);
        let readout = match readout {
            Some(text) if !object_lines.is_empty() => Some(format!("{text}\n{}", object_lines.join("\n"))),
            other => other,
        };
        *self.quick.borrow_mut() = readout;
        wires
    }

    fn object_pick_miss_message(&self) -> &'static str {
        ""
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn wants_hover_entity(&self, handle: Handle) -> bool {
        self.needs_entity_pick()
            && !handle.is_null()
            && self.hovered.as_ref().map(|(h, _)| *h) != Some(handle)
    }

    fn inject_hover_entity(&mut self, handle: Handle, entity: EntityType) {
        self.hovered = Some((handle, entity));
    }

    fn on_hover_entity(&mut self, handle: Handle, _point: DVec3) -> Vec<WireModel> {
        if handle.is_null() || self.hovered.as_ref().map(|(h, _)| *h) != Some(handle) {
            self.hovered = None;
        }
        Vec::new()
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        // Never reuse an entity injected for a previous pick.
        let picked = self.picked.take();
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            Mode::Area => match picked {
                Some(entity) => self.area_object(entity, point),
                None => CmdResult::NeedPoint,
            },
            Mode::Radius => match picked.as_ref().and_then(|e| Self::radius_at(e, point)) {
                Some(radius) => self.done(Self::radius_msg(radius)),
                None => CmdResult::ReportError(crate::tr!("measurement", "invalid-radius")),
            },
            Mode::AngleLines => {
                let Some(direction) = picked.as_ref().and_then(Self::line_direction) else {
                    return CmdResult::ReportError(crate::tr!("measurement", "invalid-line"));
                };
                if let Some((first_handle, first)) = self.first_line {
                    if first_handle == handle {
                        return CmdResult::ReportError(crate::tr!("measurement", "different-line"));
                    }
                    match Self::line_angle(first, direction) {
                        Some(angle) => self.done(Self::angle_value_msg(angle)),
                        None => CmdResult::ReportError(crate::tr!("measurement", "invalid-line")),
                    }
                } else {
                    self.first_line = Some((handle, direction));
                    CmdResult::NeedPoint
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        // A point is no option either.
        if self.mode == Mode::Choose {
            return self.invalid_option();
        }
        if self.mode == Mode::Quick {
            if let Some(area) = self.quick_area(pt) {
                return CmdResult::ReportMeasurement(area);
            }
            return match self.quick.borrow().clone() {
                Some(text) => CmdResult::ReportMeasurement(text),
                None => CmdResult::NeedPoint,
            };
        }
        if matches!(self.mode, Mode::Distance | Mode::Angle | Mode::Area) && !pt.is_finite() {
            return CmdResult::ReportError(crate::tr!("measurement", "invalid-point"));
        }
        if self.mode == Mode::Angle
            && !self.points.is_empty()
            && Self::direction(pt - self.points[0]).is_none()
        {
            return CmdResult::ReportError(crate::tr!("measurement", "degenerate-ray"));
        }
        match self.mode {
            Mode::Distance if self.multiple => {
                if self.step != AreaStep::Points {
                    return CmdResult::NeedPoint;
                }
                let before = (self.points.len(), self.arc);
                let result = if self.arc {
                    self.arc_point(pt)
                } else {
                    self.push_point(pt)
                };
                self.report_chain_change(before, result)
            }
            Mode::Distance => {
                self.push_point(pt);
                if self.points.len() == 2 {
                    let message = Self::distance_msg(self.plane, self.points[0], self.points[1]);
                    self.done(message)
                } else {
                    CmdResult::NeedPoint
                }
            }
            Mode::Angle => {
                self.points.push(pt);
                if self.points.len() == 3 {
                    match Self::angle_msg(self.points[0], self.points[1], self.points[2]) {
                        Some(text) => self.done(text),
                        None => {
                            self.points.pop();
                            CmdResult::ReportError(crate::tr!("measurement", "degenerate-ray"))
                        }
                    }
                } else {
                    CmdResult::NeedPoint
                }
            }
            Mode::Area if self.step == AreaStep::Points => {
                if self.arc {
                    self.arc_point(pt)
                } else {
                    self.push_point(pt)
                }
            }
            // A picked height runs from the base point.
            Mode::Area if self.step == AreaStep::Height => {
                let height = (pt - self.height_base).length();
                self.finish_height(height)
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.mode {
            // Enter repeats the last option.
            Mode::Choose => self.start(self.last),
            Mode::AskMode => {
                set_quick_default(false);
                self.mode = Mode::Choose;
                CmdResult::NeedPoint
            }
            Mode::Quick => CmdResult::Measurement(String::new()),
            Mode::Distance | Mode::Area if self.chain_sub_prompt() => {
                let before = (self.points.len(), self.arc);
                let result = self.sub_prompt_enter();
                self.report_chain_change(before, result)
            }
            Mode::Distance if self.multiple => self.finish_chain(),
            Mode::Area => match self.step {
                // Enter with nothing measured just leaves the object prompt.
                AreaStep::Objects if self.sum == Sum::Single => self.exit_area(),
                AreaStep::Objects => {
                    self.step = AreaStep::Points;
                    match self.last_report.clone() {
                        Some(report) => CmdResult::ReportMeasurement(report),
                        None => CmdResult::NeedPoint,
                    }
                }
                AreaStep::Height => CmdResult::ReportError(t!("Invalid point.").into_owned()),
                AreaStep::Length => self.sub_prompt_enter(),
                AreaStep::Points if self.points.is_empty() => {
                    self.step = AreaStep::Objects;
                    CmdResult::NeedPoint
                }
                // In arc mode Enter finishes like Total, whatever the boundary.
                AreaStep::Points if self.arc || self.total_offered() => self.finish_boundary(),
                AreaStep::Points => Self::keyword_error(),
            },
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        // Esc at a sub-prompt of a chain only leaves it: back to the line
        // or arc prompt, Distance printing the chain length again. A second
        // Esc ends the command.
        if self.chain_sub_prompt() {
            self.step = AreaStep::Points;
            self.arc_sub = Sub::None;
            if self.mode == Mode::Distance {
                return CmdResult::ReportMeasurement(self.running_distance());
            }
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.mode {
            Mode::Distance if self.multiple && !self.points.is_empty() => {
                let mut pts = self.boundary_display();
                pts.push(pt);
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            Mode::Distance | Mode::Angle => {
                if self.points.is_empty() {
                    return None;
                }
                let mut pts = self.points.clone();
                if self.mode == Mode::Angle && self.points.len() >= 2 {
                    pts = vec![self.points[1], self.points[0]];
                }
                pts.push(pt);
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            Mode::Area if self.step == AreaStep::Points && !self.points.is_empty() => {
                let mut pts = self.boundary_display();
                pts.push(pt);
                pts.push(self.points[0]);
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            _ => None,
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["MEASUREGEOM", "MEA", "MEASURERADIUS", "MEASUREANGLE"]
}); // MeasureGeomCommand

#[cfg(test)]
mod measurement_tests {
    use super::*;
    use crate::entities::common::{set_unit_context, unit_context};
    use document::{
        entities::{Arc, Circle, Line},
        types::Vector3,
    };

    fn units() {
        let mut context = unit_context();
        context.lunits = 2;
        context.luprec = 4;
        context.auprec = 2;
        set_unit_context(context);
    }
    fn result(result: CmdResult) -> String {
        match result {
            CmdResult::Measurement(text) | CmdResult::ReportMeasurement(text) => text,
            _ => panic!("expected measurement"),
        }
    }
    fn pick(command: &mut MeasureGeomCommand, entity: EntityType, id: u64) -> CmdResult {
        command.inject_picked_entity(entity);
        command.on_entity_pick(Handle::new(id), DVec3::ZERO)
    }
    fn line(end: Vector3) -> EntityType {
        EntityType::Line(Line::from_points(Vector3::ZERO, end))
    }

    #[test]
    fn region_radius_uses_the_hovered_and_picked_boundary() {
        units();
        use kernel::geom2d::{Circle as KernelCircle, Curve};
        let region = crate::scene::model::presspull_model::region_from_loops(
            &[
                vec![Curve::Circle(KernelCircle {
                    centre: [0.0, 0.0],
                    radius: 10.0,
                })],
                vec![Curve::Circle(KernelCircle {
                    centre: [0.0, 0.0],
                    radius: 2.0,
                })],
            ],
            WorkingPlane::default(),
        )
        .unwrap();
        let before = format!("{region:?}");
        let mut command = MeasureGeomCommand::radius();
        let handle = Handle::new(1);
        command.inject_hover_entity(handle, region.clone());
        for radius in [10.0, 2.0] {
            let point = DVec3::X * radius;
            command.on_hover_entity(handle, point);
            assert_eq!(
                command.measurement_readout(point),
                Some(MeasureGeomCommand::radius_msg(radius))
            );
            command.inject_picked_entity(region.clone());
            assert_eq!(
                result(command.on_entity_pick(handle, point)),
                MeasureGeomCommand::radius_msg(radius)
            );
        }
        assert_eq!(format!("{region:?}"), before);
        command.on_hover_entity(Handle::NULL, DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
    }

    #[test]
    fn radius_circle_and_arc_respect_length_precision() {
        units();
        for entity in [
            EntityType::Circle(Circle::from_coords(500000.0, 100000.0, 0.0, 2.34567)),
            EntityType::Arc(Arc::from_center_radius_angles(
                Vector3::ZERO,
                2.34567,
                0.0,
                std::f64::consts::FRAC_PI_2,
            )),
        ] {
            let text = result(pick(&mut MeasureGeomCommand::radius(), entity, 1));
            assert!(text.contains("2.3457"), "{text}");
            assert!(text.contains("4.6913"), "{text}");
        }
    }

    #[test]
    fn radius_rejects_invalid_entities_and_stale_injection() {
        let mut command = MeasureGeomCommand::radius();
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::ReportError(_)
        ));
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                pick(
                    &mut command,
                    EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, radius)),
                    1
                ),
                CmdResult::ReportError(_)
            ));
        }
        command.inject_picked_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)));
        assert!(matches!(
            command.on_entity_pick(Handle::NULL, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_entity_pick(Handle::new(2), DVec3::ZERO),
            CmdResult::ReportError(_)
        ));
    }

    #[test]
    fn known_angles_extreme_scales_and_degenerate_rays() {
        for degrees in [0.0_f64, 30.0, 45.0, 90.0, 135.0, 180.0] {
            let radians = degrees.to_radians();
            let ray = DVec3::new(radians.cos(), radians.sin(), 0.0);
            for scale in [1e-200, 1.0, 1e200] {
                let actual =
                    MeasureGeomCommand::angle_degrees(DVec3::X * scale, ray * scale).unwrap();
                assert!((actual - degrees).abs() < 1e-10);
            }
        }
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::X, DVec3::Z),
            Some(90.0)
        );
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::ZERO, DVec3::X),
            None
        );
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::NAN, DVec3::X),
            None
        );
    }

    #[test]
    fn invalid_angle_points_do_not_advance_and_survey_precision_is_retained() {
        units();
        let mut command = MeasureGeomCommand::angle();
        let vertex = DVec3::new(500000.001, 600000.002, 100.0);
        assert!(matches!(
            command.on_point(DVec3::NAN),
            CmdResult::ReportError(_)
        ));
        assert!(command.points.is_empty());
        command.on_point(vertex);
        assert!(matches!(
            command.on_point(vertex),
            CmdResult::ReportError(_)
        ));
        assert_eq!(command.points.len(), 1);
        command.on_point(vertex + DVec3::X * 0.001);
        assert!(matches!(
            command.on_point(vertex),
            CmdResult::ReportError(_)
        ));
        assert_eq!(command.points.len(), 2);
        assert!(result(command.on_point(vertex + DVec3::Y * 0.001)).contains("90.00°"));
    }

    #[test]
    fn angle_preview_two_rays_and_live_value() {
        units();
        let mut command = MeasureGeomCommand::angle();
        command.on_point(DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::Y).is_none());
        command.on_point(DVec3::X);
        assert!(command
            .measurement_readout(DVec3::Y)
            .unwrap()
            .contains("90.00°"));
        assert!(command
            .measurement_readout(DVec3::X + DVec3::Y)
            .unwrap()
            .contains("45.00°"));
        assert_eq!(
            command.on_mouse_move(DVec3::Y).unwrap().points,
            vec![[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
    }

    #[test]
    fn two_lines_smaller_angle_and_invalid_selections() {
        units();
        let mut command = MeasureGeomCommand::angle();
        command.on_text_input("LINES");
        assert!(command.needs_entity_pick());
        assert!(matches!(
            pick(&mut command, line(Vector3::ZERO), 1),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(
            pick(
                &mut command,
                EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 1.0)),
                2
            ),
            CmdResult::ReportError(_)
        ));
        assert!(
            result(pick(&mut command, line(Vector3::new(-1.0, -1.0, 0.0)), 2)).contains("45.00°")
        );
        assert_eq!(
            MeasureGeomCommand::line_angle(DVec3::X, -DVec3::X),
            Some(0.0)
        );
    }

    #[test]
    fn hover_clears_on_miss_without_completing_measurement() {
        units();
        let mut command = MeasureGeomCommand::radius();
        let handle = Handle::new(5);
        assert!(command.wants_hover_entity(handle));
        command.inject_hover_entity(
            handle,
            EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)),
        );
        command.on_hover_entity(handle, DVec3::ZERO);
        assert!(command
            .measurement_readout(DVec3::ZERO)
            .unwrap()
            .contains("2.0000"));
        assert!(!command.wants_hover_entity(handle));
        assert!(command.picked.is_none());
        command.on_hover_entity(Handle::NULL, DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
        command.inject_hover_entity(handle, line(Vector3::UNIT_X));
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
    }

    #[test]
    fn measuregeom_all_four_modes_remain_available() {
        units();
        assert_eq!(MeasureGeomCommand::new().options().len(), 8);
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("D");
        command.on_point(DVec3::ZERO);
        assert!(result(command.on_point(DVec3::new(3.0, 4.0, 0.0))).contains("5.0000"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("AR");
        for p in [DVec3::ZERO, DVec3::X * 4.0, DVec3::Y * 3.0] {
            command.on_point(p);
        }
        let text = result(command.on_enter());
        assert!(text.contains("6.0000") && text.contains("12.0000"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("A");
        command.on_point(DVec3::ZERO);
        command.on_point(DVec3::X);
        assert!(result(command.on_point(DVec3::Y)).contains("90.00°"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("R");
        assert!(result(pick(
            &mut command,
            EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 3.0)),
            1
        ))
        .contains("3.0000"));
    }
}

/// Command-line results keep every LUPREC place.
fn area_text(value: f64) -> String {
    crate::entities::common::with_all_places(|| crate::entities::common::format_area(value))
}

fn length_text(value: f64) -> String {
    crate::entities::common::with_all_places(|| crate::entities::common::format_length(value))
}
