// REVCLOUD command — lay a revision cloud along a rectangle, a polygon, a
// freehand path or an object, or reshape part of one (Modify).

use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{Curve, Polyline, PolylineVertex};
use kernel::space::{PlanarCurve, Vec3};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::entities::curve::{curve_points, entity_curve, ocs_plane};
use crate::entities::revcloud::{self as cloud, OnCloud};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/revcloud.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "REVCLOUD",
        label: "Rev Cloud",
        icon: ICON,
        event: ModuleEvent::Command("REVCLOUD".to_string()),
    }
}

/// The calligraphy style of the clouds REVCLOUD draws, kept for the session.
static CALLIGRAPHY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationMode {
    Freehand,
    Rectangular,
    Polygonal,
}

impl CreationMode {
    fn current() -> Self {
        match cloud::create_mode() {
            0 => Self::Freehand,
            2 => Self::Polygonal,
            _ => Self::Rectangular,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Freehand => 0,
            Self::Rectangular => 1,
            Self::Polygonal => 2,
        }
    }
}

struct Pending {
    entity: EntityType,
    /// The object the cloud replaces (Object, Modify).
    replaces: Option<Handle>,
    /// Report "Revision cloud finished." once made.
    reports: bool,
}

struct ModifyState {
    handle: Handle,
    source: EntityType,
    start: OnCloud,
    path: Vec<[f64; 2]>,
    end: Option<OnCloud>,
}

enum Stage {
    Create,
    ArcLength(Option<DVec3>),
    Style,
    Object,
    Reverse(Pending),
    ModifySelect,
    ModifyStart(ModifyState),
    ModifyDraw(ModifyState),
    ModifyErase(ModifyState),
}

pub struct RevCloudCommand {
    points: Vec<DVec3>,
    arc_length: f64,
    /// The arc length is the drawing's REVCLOUDAPPROXARCLEN from now on.
    arc_stored: bool,
    creation: CreationMode,
    calligraphy: bool,
    stage: Stage,
    tracing: bool,
    plane: WorkingPlane,
    /// The settings line comes before the next prompt.
    header: bool,
    picked: Option<EntityType>,
    world_per_pixel: f64,
}

/// A length as the prompts show it: four places, trailing zeros dropped.
fn length_text(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_string() } else { text.to_string() }
}

impl RevCloudCommand {
    /// `arc_length` is REVCLOUDAPPROXARCLEN, or the view's default when the
    /// drawing holds none.
    pub fn new(arc_length: f64, stored: bool) -> Self {
        Self {
            points: Vec::new(),
            arc_length,
            arc_stored: stored,
            creation: CreationMode::current(),
            calligraphy: CALLIGRAPHY.load(std::sync::atomic::Ordering::Relaxed),
            stage: Stage::Create,
            tracing: false,
            plane: WorkingPlane::default(),
            header: true,
            picked: None,
            world_per_pixel: 0.0,
        }
    }

    fn variance(&self) -> bool {
        cloud::arc_variance()
    }

    fn header_line(&self) -> String {
        let (min, max) = cloud::chord_range(self.arc_length, self.variance());
        let style = if self.calligraphy {
            crate::tr!("revcloud-cmd", "style-calligraphy")
        } else {
            crate::tr!("revcloud-cmd", "style-normal")
        };
        let kind = match self.creation {
            CreationMode::Rectangular => crate::tr!("revcloud-cmd", "type-rectangular"),
            CreationMode::Polygonal => crate::tr!("revcloud-cmd", "type-polygonal"),
            CreationMode::Freehand => crate::tr!("revcloud-cmd", "type-freehand"),
        };
        crate::tr!(
            "revcloud-cmd",
            "header",
            min = length_text(min),
            max = length_text(max),
            style = style,
            kind = kind
        )
    }

    fn set_creation(&mut self, mode: CreationMode) {
        if self.creation != mode {
            self.header = true;
        }
        self.creation = mode;
        cloud::set_create_mode(mode.code());
        self.stage = Stage::Create;
        self.points.clear();
        self.tracing = false;
    }

    fn local(&self, point: DVec3) -> [f64; 2] {
        let local = self.plane.to_local(point);
        [local.x, local.y]
    }

    /// A cloud along the path through `points` (world), in the working plane.
    fn cloud_through(&self, points: &[DVec3], closed: bool, preview: bool) -> Option<EntityType> {
        let curve = Curve::Polyline(Polyline {
            vertices: points
                .iter()
                .map(|point| PolylineVertex::straight(self.local(*point)))
                .collect(),
            closed,
        });
        // Previews keep one chord length so they do not flicker.
        let variance = self.variance() && !preview;
        let cloud = cloud::cloud_along(&curve, self.arc_length, variance, self.calligraphy, false)?;
        Some(self.plane.place_entity(EntityType::LwPolyline(cloud)))
    }

    fn rectangle(&self, first: DVec3, opposite: DVec3) -> Option<Vec<DVec3>> {
        let a = self.plane.to_local(first);
        let b = self.plane.to_local(opposite);
        let size = (a.x - b.x).abs().max((a.y - b.y).abs());
        if (a.x - b.x).abs() <= size * 1.0e-12 || (a.y - b.y).abs() <= size * 1.0e-12 {
            return None;
        }
        // The first corner, then up (or down) its side to the opposite one.
        Some(
            [
                DVec3::new(a.x, a.y, a.z),
                DVec3::new(a.x, b.y, a.z),
                DVec3::new(b.x, b.y, a.z),
                DVec3::new(b.x, a.y, a.z),
            ]
            .into_iter()
            .map(|point| self.plane.to_world(point))
            .collect(),
        )
    }

    fn invalid_shape(&mut self) -> CmdResult {
        CmdResult::Measurement(crate::tr!("revcloud-cmd", "invalid-shape"))
    }

    fn commit(&mut self, entity: EntityType, reports: bool) -> CmdResult {
        self.arc_stored = true;
        if reports {
            CmdResult::CommitAndReport(entity, crate::tr!("revcloud-cmd", "finished"))
        } else {
            CmdResult::CommitAndExit(entity)
        }
    }

    fn finish(&mut self, pending: Pending, reverse: bool) -> CmdResult {
        let mut entity = pending.entity;
        if reverse {
            if let EntityType::LwPolyline(polyline) = entity {
                entity = EntityType::LwPolyline(cloud::reversed(polyline));
            }
        }
        self.arc_stored = true;
        match pending.replaces {
            Some(handle) => {
                entity.common_mut().handle = handle;
                if pending.reports {
                    CmdResult::UpdateEntityAndReport {
                        handle,
                        entity,
                        message: crate::tr!("revcloud-cmd", "finished"),
                    }
                } else {
                    CmdResult::UpdateEntityAndFinish { handle, entity }
                }
            }
            None if pending.reports => {
                CmdResult::CommitAndReport(entity, crate::tr!("revcloud-cmd", "finished"))
            }
            None => CmdResult::CommitAndExit(entity),
        }
    }

    /// The path of an object a cloud can follow.
    fn object_curve(entity: &EntityType) -> Option<PlanarCurve> {
        match entity {
            EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::Circle(_)
            | EntityType::Ellipse(_)
            | EntityType::Spline(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline2D(_) => entity_curve(entity),
            _ => None,
        }
    }

    fn preview(entity: &EntityType, name: &str) -> Option<WireModel> {
        let curve = entity_curve(entity)?;
        let points = curve_points(&curve);
        (points.len() >= 2).then(|| WireModel::solid_f64(name.to_string(), points, WireModel::CYAN, false))
    }

    fn pick_tolerance(&self) -> f64 {
        if self.world_per_pixel > 0.0 {
            self.world_per_pixel * 5.0
        } else {
            self.arc_length * 0.1
        }
    }

    fn on_cloud(state: &ModifyState, point: DVec3) -> Option<OnCloud> {
        let EntityType::LwPolyline(polyline) = &state.source else {
            return None;
        };
        let plane = ocs_plane(polyline.normal, polyline.elevation);
        let local = plane.project(point.to_array())?;
        cloud::nearest(polyline, local)
    }

    fn world(state: &ModifyState, point: [f64; 2]) -> DVec3 {
        let EntityType::LwPolyline(polyline) = &state.source else {
            return DVec3::ZERO;
        };
        DVec3::from_array(ocs_plane(polyline.normal, polyline.elevation).point_at(point))
    }

    fn take_state(&mut self) -> Option<ModifyState> {
        match std::mem::replace(&mut self.stage, Stage::Create) {
            Stage::ModifyStart(state) | Stage::ModifyDraw(state) | Stage::ModifyErase(state) => Some(state),
            other => {
                self.stage = other;
                None
            }
        }
    }

    fn main_prompt(&self) -> String {
        match self.creation {
            CreationMode::Rectangular => crate::tr!("revcloud-cmd", "first-corner"),
            CreationMode::Polygonal => crate::tr!("revcloud-cmd", "start-point"),
            CreationMode::Freehand => crate::tr!("revcloud-cmd", "first-point"),
        }
    }
}

impl CadCommand for RevCloudCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "REVCLOUD"
    }

    fn set_world_per_pixel(&mut self, world_per_pixel: f64) {
        self.world_per_pixel = world_per_pixel;
    }

    fn prompt(&self) -> String {
        let prompt = match &self.stage {
            Stage::Create => match self.creation {
                _ if self.points.is_empty() && !self.tracing => self.main_prompt(),
                CreationMode::Rectangular => crate::tr!("revcloud-cmd", "opposite"),
                CreationMode::Polygonal if self.points.len() == 1 => crate::tr!("revcloud-cmd", "next"),
                CreationMode::Polygonal => crate::tr!("revcloud-cmd", "next-undo"),
                CreationMode::Freehand => crate::tr!("revcloud-cmd", "guide"),
            },
            Stage::ArcLength(None) => crate::tr!(
                "revcloud-cmd",
                "arc",
                length = length_text(self.arc_length)
            ),
            Stage::ArcLength(Some(_)) => crate::tr!("revcloud-cmd", "second"),
            Stage::Style => crate::tr!(
                "revcloud-cmd",
                "style",
                style = if self.calligraphy {
                    crate::tr!("revcloud-cmd", "style-calligraphy")
                } else {
                    crate::tr!("revcloud-cmd", "style-normal")
                }
            ),
            Stage::Object => crate::tr!("revcloud-cmd", "object"),
            Stage::Reverse(_) => crate::tr!("revcloud-cmd", "reverse"),
            Stage::ModifySelect => crate::tr!("revcloud-cmd", "modify-select"),
            Stage::ModifyStart(_) => crate::tr!("revcloud-cmd", "modify-start"),
            Stage::ModifyDraw(state) if state.path.len() <= 1 => {
                crate::tr!("revcloud-cmd", "modify-first")
            }
            Stage::ModifyDraw(_) => crate::tr!("revcloud-cmd", "next-undo"),
            Stage::ModifyErase(_) => crate::tr!("revcloud-cmd", "modify-side"),
        };
        if self.header && matches!(self.stage, Stage::Create) {
            format!("{}\n{}", self.header_line(), prompt)
        } else {
            prompt
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.stage {
            Stage::Create if self.points.is_empty() && !self.tracing => vec![
                CmdOption::new("Arc length", "A"),
                CmdOption::new("Object", "O"),
                CmdOption::new("Rectangular", "R"),
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("Freehand", "F"),
                CmdOption::new("Style", "S"),
                CmdOption::new("Modify", "M"),
            ],
            Stage::Create if self.creation == CreationMode::Polygonal && self.points.len() >= 2 => {
                vec![CmdOption::new("Undo", "U")]
            }
            Stage::Style => vec![CmdOption::new("Normal", "N"), CmdOption::new("Calligraphy", "C")],
            Stage::Reverse(_) => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Stage::ModifyDraw(state) if state.path.len() <= 1 => {
                vec![CmdOption::new("First point", "F")]
            }
            Stage::ModifyDraw(_) => vec![CmdOption::new("Undo", "U")],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.header = false;
        let tolerance = self.pick_tolerance();
        match &mut self.stage {
            Stage::Create => match self.creation {
                CreationMode::Rectangular => {
                    let Some(first) = self.points.first().copied() else {
                        self.points.push(point);
                        return CmdResult::NeedPoint;
                    };
                    let Some(corners) = self.rectangle(first, point) else {
                        return self.invalid_shape();
                    };
                    match self.cloud_through(&corners, true, false) {
                        Some(entity) => self.commit(entity, false),
                        None => self.invalid_shape(),
                    }
                }
                CreationMode::Polygonal => {
                    self.points.push(point);
                    CmdResult::NeedPoint
                }
                CreationMode::Freehand => {
                    if !self.tracing {
                        self.points = vec![point];
                        self.tracing = true;
                        return CmdResult::NeedPoint;
                    }
                    self.finish_freehand(point)
                }
            },
            Stage::ArcLength(first) => match first.take() {
                None => {
                    *first = Some(point);
                    CmdResult::NeedPoint
                }
                Some(start) => {
                    let length = start.distance(point);
                    if length > 0.0 {
                        self.arc_length = length;
                        self.arc_stored = true;
                        self.stage = Stage::Create;
                        CmdResult::NeedPoint
                    } else {
                        // A zero length asks for the second point again.
                        self.stage = Stage::ArcLength(Some(start));
                        CmdResult::ReportError(crate::tr!("revcloud-cmd", "positive"))
                    }
                }
            },
            Stage::ModifyStart(_) => {
                let Some(mut state) = self.take_state() else {
                    return CmdResult::NeedPoint;
                };
                if let Some(start) = Self::on_cloud(&state, point) {
                    state.start = start;
                    state.path = vec![start.point];
                }
                self.stage = Stage::ModifyDraw(state);
                CmdResult::NeedPoint
            }
            Stage::ModifyDraw(state) => {
                // A point within one of the cloud's own arc lengths (not the
                // command's current one) ends the new part.
                let reach = match &state.source {
                    EntityType::LwPolyline(polyline) => {
                        cloud::arc_length_of(polyline).unwrap_or(self.arc_length)
                    }
                    _ => self.arc_length,
                };
                if state.path.len() >= 2 {
                    if let Some(end) = Self::on_cloud(state, point)
                        .filter(|end| end.distance <= reach)
                    {
                        state.path.push(end.point);
                        state.end = Some(end);
                        let state = self.take_state().expect("modify state");
                        self.stage = Stage::ModifyErase(state);
                        return CmdResult::NeedPoint;
                    }
                }
                let EntityType::LwPolyline(polyline) = &state.source else {
                    return CmdResult::Cancel;
                };
                let plane = ocs_plane(polyline.normal, polyline.elevation);
                if let Some(local) = plane.project(point.to_array()) {
                    state.path.push(local);
                }
                CmdResult::NeedPoint
            }
            Stage::ModifyErase(state) => {
                let Some(side) = Self::on_cloud(state, point).filter(|side| side.distance <= tolerance) else {
                    return CmdResult::NeedPoint;
                };
                let EntityType::LwPolyline(polyline) = &state.source else {
                    return CmdResult::Cancel;
                };
                let arc_length = cloud::arc_length_of(polyline).unwrap_or(self.arc_length);
                let Some(end) = state.end else {
                    return CmdResult::Cancel;
                };
                let Some(modified) = cloud::modified(polyline, state.start, &state.path, end, side, arc_length)
                else {
                    return CmdResult::NeedPoint;
                };
                let handle = state.handle;
                self.stage = Stage::Reverse(Pending {
                    entity: EntityType::LwPolyline(modified),
                    replaces: Some(handle),
                    reports: false,
                });
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.header = false;
        match &self.stage {
            Stage::Create if self.points.is_empty() && !self.tracing => {
                self.stage = Stage::Object;
                CmdResult::NeedPoint
            }
            Stage::Create if self.creation == CreationMode::Polygonal => {
                let points = std::mem::take(&mut self.points);
                if points.len() < 3 {
                    return self.invalid_shape();
                }
                match self.cloud_through(&points, true, false) {
                    Some(entity) => self.commit(entity, false),
                    None => self.invalid_shape(),
                }
            }
            Stage::Create if self.creation == CreationMode::Freehand && self.tracing => {
                // Enter leaves the cloud open.
                self.tracing = false;
                let points = std::mem::take(&mut self.points);
                match self.cloud_through(&points, false, false) {
                    Some(entity) => {
                        self.stage = Stage::Reverse(Pending {
                            entity,
                            replaces: None,
                            reports: true,
                        });
                        CmdResult::NeedPoint
                    }
                    None => self.invalid_shape(),
                }
            }
            Stage::ArcLength(_) | Stage::Style => {
                self.stage = Stage::Create;
                CmdResult::NeedPoint
            }
            Stage::Reverse(_) => match std::mem::replace(&mut self.stage, Stage::Create) {
                Stage::Reverse(pending) => self.finish(pending, false),
                _ => CmdResult::Cancel,
            },
            Stage::Object | Stage::ModifySelect | Stage::ModifyErase(_) | Stage::ModifyStart(_) => {
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn input_kind(&self) -> InputKind {
        match self.stage {
            Stage::Style | Stage::Reverse(_) => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.stage, Stage::Create | Stage::ModifyDraw(_) | Stage::ArcLength(None))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        self.header = false;
        let input = text.trim().trim_start_matches('_');
        let keyword = input.to_ascii_uppercase();
        match &mut self.stage {
            // A typed point starts a length given as two points.
            Stage::ArcLength(None) if input.contains(',') => return None,
            Stage::ArcLength(None) => {
                return Some(match input.parse::<f64>() {
                    Ok(value) if value.is_finite() && value > 0.0 => {
                        self.arc_length = value;
                        self.arc_stored = true;
                        self.stage = Stage::Create;
                        CmdResult::NeedPoint
                    }
                    Ok(_) => CmdResult::ReportError(crate::tr!("revcloud-cmd", "positive")),
                    Err(_) => CmdResult::ReportError(crate::tr!("revcloud-cmd", "numeric")),
                });
            }
            Stage::Style => {
                let calligraphy = match keyword.as_str() {
                    "N" | "NORMAL" => false,
                    "C" | "CALLIGRAPHY" => true,
                    _ => return Some(CmdResult::ReportError(crate::tr!("revcloud-cmd", "keyword"))),
                };
                self.calligraphy = calligraphy;
                CALLIGRAPHY.store(calligraphy, std::sync::atomic::Ordering::Relaxed);
                self.stage = Stage::Create;
                let name = if calligraphy {
                    crate::tr!("revcloud-cmd", "style-calligraphy")
                } else {
                    crate::tr!("revcloud-cmd", "style-normal")
                };
                return Some(CmdResult::ReportMeasurement(name));
            }
            Stage::Reverse(_) => {
                let reverse = match keyword.as_str() {
                    "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return Some(CmdResult::ReportError(crate::tr!("revcloud-cmd", "keyword"))),
                };
                return Some(match std::mem::replace(&mut self.stage, Stage::Create) {
                    Stage::Reverse(pending) => self.finish(pending, reverse),
                    _ => CmdResult::Cancel,
                });
            }
            Stage::ModifyDraw(state) => {
                return match keyword.as_str() {
                    "F" | "FIRST" if state.path.len() <= 1 => {
                        let state = self.take_state()?;
                        self.stage = Stage::ModifyStart(state);
                        Some(CmdResult::NeedPoint)
                    }
                    "U" | "UNDO" if state.path.len() > 1 => {
                        state.path.pop();
                        Some(CmdResult::NeedPoint)
                    }
                    _ => None,
                };
            }
            Stage::Create if self.points.is_empty() && !self.tracing => {}
            Stage::Create => {
                if self.creation == CreationMode::Polygonal
                    && matches!(keyword.as_str(), "U" | "UNDO")
                    && self.points.len() >= 2
                {
                    self.points.pop();
                    return Some(CmdResult::NeedPoint);
                }
                return None;
            }
            _ => return None,
        }
        match keyword.as_str() {
            "A" | "ARC" | "ARCLENGTH" => self.stage = Stage::ArcLength(None),
            "O" | "OBJECT" => self.stage = Stage::Object,
            "R" | "RECTANGULAR" => self.set_creation(CreationMode::Rectangular),
            "P" | "POLYGONAL" => self.set_creation(CreationMode::Polygonal),
            "F" | "FREEHAND" => self.set_creation(CreationMode::Freehand),
            "S" | "STYLE" => self.stage = Stage::Style,
            "M" | "MODIFY" => self.stage = Stage::ModifySelect,
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.stage, Stage::Object | Stage::ModifySelect)
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    fn typed_point_picks_entity(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        self.header = false;
        let Some(entity) = self.picked.take().filter(|entity| entity.common().handle == handle) else {
            return CmdResult::NeedPoint;
        };
        match self.stage {
            Stage::Object => {
                let made = Self::object_curve(&entity)
                    .and_then(|curve| cloud_on_plane(&curve, self.arc_length, self.variance(), self.calligraphy));
                let Some(mut made) = made else {
                    return CmdResult::ReportError(crate::tr!("revcloud-cmd", "unable"));
                };
                let mut common = entity.common().clone();
                common.extended_data = made.common().extended_data.clone();
                *made.common_mut() = common;
                self.stage = Stage::Reverse(Pending {
                    entity: made,
                    replaces: Some(handle),
                    reports: true,
                });
                CmdResult::NeedPoint
            }
            Stage::ModifySelect => {
                let EntityType::LwPolyline(polyline) = &entity else {
                    return CmdResult::NeedPoint;
                };
                if !polyline.is_closed {
                    return CmdResult::NeedPoint;
                }
                let plane = ocs_plane(polyline.normal, polyline.elevation);
                let Some(start) = plane
                    .project(point.to_array())
                    .and_then(|local| cloud::nearest(polyline, local))
                else {
                    return CmdResult::NeedPoint;
                };
                self.stage = Stage::ModifyDraw(ModifyState {
                    handle,
                    source: entity,
                    start,
                    path: vec![start.point],
                    end: None,
                });
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        match &mut self.stage {
            Stage::Create if self.creation == CreationMode::Polygonal && self.points.len() >= 2 => {
                self.points.pop();
                Some(CmdResult::NeedPoint)
            }
            Stage::ModifyDraw(state) if state.path.len() > 1 => {
                state.path.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn drawing_variable_update(&self) -> Option<(&'static str, String)> {
        self.arc_stored
            .then(|| (cloud::ARC_LENGTH_VARIABLE, format!("{:.6}", self.arc_length)))
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match &mut self.stage {
            Stage::Create if self.creation == CreationMode::Freehand && self.tracing => {
                let spacing = self.arc_length * 0.25;
                if self.points.last().is_none_or(|last| last.distance(point) >= spacing) {
                    self.points.push(point);
                }
                let closing = self.closes_freehand(point);
                let points = self.points.clone();
                let entity = self.cloud_through(&points, closing, true)?;
                Self::preview(&entity, "revcloud_freehand_preview")
            }
            Stage::Create if self.creation == CreationMode::Rectangular => {
                let first = *self.points.first()?;
                let corners = self.rectangle(first, point)?;
                let entity = self.cloud_through(&corners, true, true)?;
                Self::preview(&entity, "revcloud_rectangular_preview")
            }
            Stage::Create if self.creation == CreationMode::Polygonal && !self.points.is_empty() => {
                let mut points = self.points.clone();
                points.push(point);
                let entity = self.cloud_through(&points, points.len() >= 3, true)?;
                Self::preview(&entity, "revcloud_polygonal_preview")
            }
            Stage::Reverse(pending) => Self::preview(&pending.entity, "revcloud_reverse_preview"),
            Stage::ModifyDraw(state) => {
                let mut points: Vec<DVec3> = state.path.iter().map(|p| Self::world(state, *p)).collect();
                points.push(point);
                Some(WireModel::solid_f64(
                    "revcloud_modify_preview".to_string(),
                    points.iter().map(|p| [p.x, p.y, p.z]).collect(),
                    WireModel::CYAN,
                    false,
                ))
            }
            _ => None,
        }
    }
}

impl RevCloudCommand {
    /// Whether the freehand path has come back to where it started.
    fn closes_freehand(&self, cursor: DVec3) -> bool {
        let Some(first) = self.points.first() else {
            return false;
        };
        let travelled: f64 = self.points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        travelled > self.arc_length * 2.0 && first.distance(cursor) <= cloud::chord_range(self.arc_length, self.variance()).0
    }

    /// A click while guiding: a closed cloud once the path is back at its
    /// start, else an open one (then Reverse direction).
    fn finish_freehand(&mut self, point: DVec3) -> CmdResult {
        if self.closes_freehand(point) {
            self.tracing = false;
            let points = std::mem::take(&mut self.points);
            return match self.cloud_through(&points, true, false) {
                Some(entity) => self.commit(entity, true),
                None => self.invalid_shape(),
            };
        }
        if self.points.last().is_none_or(|last| last.distance(point) > 0.0) {
            self.points.push(point);
        }
        CmdResult::NeedPoint
    }
}

/// A cloud along a curve on its own plane, stored in that plane's OCS.
fn cloud_on_plane(source: &PlanarCurve, arc_length: f64, variance: bool, calligraphy: bool) -> Option<EntityType> {
    let mut cloud = cloud::cloud_along(&source.curve, arc_length, variance, calligraphy, false)?;
    let normal = Vec3::from(source.plane.normal()?);
    let elevation = Vec3::from(source.plane.origin).dot(normal);
    let normal = Vector3::new(normal.x, normal.y, normal.z);
    let storage_plane = ocs_plane(normal, elevation);
    for vertex in &mut cloud.vertices {
        let world = source.plane.point_at([vertex.location.x, vertex.location.y]);
        let local = storage_plane.project(world)?;
        vertex.location.x = local[0];
        vertex.location.y = local[1];
    }
    cloud.normal = normal;
    cloud.elevation = elevation;
    Some(EntityType::LwPolyline(cloud))
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "REVCLOUD",
        "REVCLOUD_RECTANGULAR",
        "REVCLOUD_POLYGONAL",
        "REVCLOUD_FREEHAND"
    ]
});
