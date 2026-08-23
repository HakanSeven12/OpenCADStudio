// REVCLOUD command — create or modify arc-bumped lightweight polylines.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::{distance_to, Curve, Polyline, PolylineVertex, Vec2};
use cadkernel::space::{PlanarCurve, Plane, Vec3};
use glam::DVec3;
use rustc_hash::FxHashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::entities::curve::{curve_points, entity_curve, ocs_plane};
use crate::entities::lwpolyline::{is_revision_cloud, revision_cloud_from_curve};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/revcloud.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "REVCLOUD",
        label: "Rev Cloud",
        icon: ICON,
        event: ModuleEvent::Command("REVCLOUD".to_string()),
    }
}

const DEFAULT_ARC_LENGTH: f64 = 1.0;

static LAST_ARC_LENGTH: AtomicU64 = AtomicU64::new(0);
static LAST_CREATION: AtomicU8 = AtomicU8::new(1);
static LAST_STYLE: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationMode {
    Freehand,
    Rectangular,
    Polygonal,
}

impl CreationMode {
    fn remembered() -> Self {
        match LAST_CREATION.load(Ordering::Relaxed) {
            0 => Self::Freehand,
            2 => Self::Polygonal,
            _ => Self::Rectangular,
        }
    }

    fn remember(self) {
        let value = match self {
            Self::Freehand => 0,
            Self::Rectangular => 1,
            Self::Polygonal => 2,
        };
        LAST_CREATION.store(value, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloudStyle {
    Normal,
    Calligraphy,
}

impl CloudStyle {
    fn remembered() -> Self {
        if LAST_STYLE.load(Ordering::Relaxed) == 1 {
            Self::Calligraphy
        } else {
            Self::Normal
        }
    }

    fn remember(self) {
        LAST_STYLE.store((self == Self::Calligraphy) as u8, Ordering::Relaxed);
    }

    fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Calligraphy => "Calligraphy",
        }
    }
}

struct PendingCloud {
    entity: EntityType,
    replacement: Option<Handle>,
}

struct ModifyState {
    handle: Handle,
    source: EntityType,
    plane: Plane,
    segments: Vec<Curve>,
    vertices: Vec<DVec3>,
    start: usize,
    end: Option<usize>,
    replacement: Vec<DVec3>,
}

enum Stage {
    Create,
    ArcLength,
    Style,
    Object,
    Reverse(PendingCloud),
    ModifySelect,
    ModifyDraw(ModifyState),
    ModifyErase(ModifyState),
}

pub struct RevCloudCommand {
    points: Vec<DVec3>,
    arc_length: f64,
    creation: CreationMode,
    style: CloudStyle,
    stage: Stage,
    tracing: bool,
    sources: FxHashMap<Handle, EntityType>,
    plane: WorkingPlane,
    message: Option<&'static str>,
}

impl RevCloudCommand {
    pub fn new(default_arc_length: f64, sources: FxHashMap<Handle, EntityType>) -> Self {
        let remembered = f64::from_bits(LAST_ARC_LENGTH.load(Ordering::Relaxed));
        let arc_length = if remembered.is_finite() && remembered > 0.0 {
            remembered
        } else if default_arc_length.is_finite() && default_arc_length > 0.0 {
            default_arc_length
        } else {
            DEFAULT_ARC_LENGTH
        };
        LAST_ARC_LENGTH.store(arc_length.to_bits(), Ordering::Relaxed);
        Self {
            points: Vec::new(),
            arc_length,
            creation: CreationMode::remembered(),
            style: CloudStyle::remembered(),
            stage: Stage::Create,
            tracing: false,
            sources,
            plane: WorkingPlane::default(),
            message: None,
        }
    }

    fn set_creation(&mut self, mode: CreationMode) {
        self.creation = mode;
        self.creation.remember();
        self.stage = Stage::Create;
        self.points.clear();
        self.tracing = false;
        self.message = None;
    }

    fn local_points(&self, points: &[DVec3]) -> Vec<DVec3> {
        points
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect()
    }

    fn guide_curve(&self, points: &[DVec3]) -> Curve {
        let vertices = self
            .local_points(points)
            .into_iter()
            .map(|point| PolylineVertex::straight([point.x, point.y]))
            .collect();
        Curve::Polyline(Polyline {
            vertices,
            closed: true,
        })
    }

    fn cloud_from_world_points(&self, points: &[DVec3], reverse: bool) -> Option<EntityType> {
        let curve = self.guide_curve(points);
        let cloud = revision_cloud_from_curve(
            &curve,
            self.arc_length,
            reverse,
            style_width_ratios(self.style),
        )?;
        Some(self.plane.place_entity(EntityType::LwPolyline(cloud)))
    }

    fn prepare_cloud(&mut self, points: &[DVec3], replacement: Option<Handle>) -> CmdResult {
        let Some(mut entity) = self.cloud_from_world_points(points, false) else {
            self.message = Some("The selected path cannot form a revision cloud.");
            return CmdResult::NeedPoint;
        };
        if let Some(handle) = replacement {
            if let Some(source) = self.sources.get(&handle) {
                *entity.common_mut() = source.common().clone();
                entity.common_mut().handle = Handle::NULL;
            }
        }
        self.stage = Stage::Reverse(PendingCloud {
            entity,
            replacement,
        });
        self.message = None;
        CmdResult::NeedPoint
    }

    fn prepare_planar_cloud(
        &mut self,
        curve: &PlanarCurve,
        replacement: Handle,
    ) -> CmdResult {
        let Some(mut entity) =
            cloud_entity_on_plane(curve, self.arc_length, self.style, false)
        else {
            self.message = Some("The selected path cannot form a revision cloud.");
            return CmdResult::NeedPoint;
        };
        if let Some(source) = self.sources.get(&replacement) {
            *entity.common_mut() = source.common().clone();
            entity.common_mut().handle = Handle::NULL;
        }
        self.stage = Stage::Reverse(PendingCloud {
            entity,
            replacement: Some(replacement),
        });
        self.message = None;
        CmdResult::NeedPoint
    }

    fn finish_pending(pending: PendingCloud, reverse: bool) -> CmdResult {
        let entity = if reverse {
            reverse_cloud_entity(pending.entity)
        } else {
            pending.entity
        };
        if let Some(handle) = pending.replacement {
            CmdResult::ReplaceEntity(handle, vec![entity])
        } else {
            CmdResult::CommitAndExit(entity)
        }
    }

    fn object_curve(entity: &EntityType) -> Option<PlanarCurve> {
        let closed = match entity {
            EntityType::Circle(_) => true,
            EntityType::Ellipse(ellipse) => ellipse.is_full(),
            EntityType::LwPolyline(polyline) => polyline.is_closed,
            EntityType::Polyline2D(polyline) => polyline.is_closed(),
            EntityType::Spline(spline) => spline.flags.closed || spline.flags.periodic,
            _ => false,
        };
        if !closed {
            return None;
        }
        let curve = entity_curve(entity)?;
        curve.is_closed().then_some(curve)
    }

    fn rectangle_points(&self, first: DVec3, opposite: DVec3) -> Option<Vec<DVec3>> {
        let first = self.plane.to_local(first);
        let opposite = self.plane.to_local(opposite);
        if (first.x - opposite.x).abs() <= f64::EPSILON
            || (first.y - opposite.y).abs() <= f64::EPSILON
        {
            return None;
        }
        Some(
            [
                DVec3::new(first.x, first.y, first.z),
                DVec3::new(opposite.x, first.y, first.z),
                DVec3::new(opposite.x, opposite.y, first.z),
                DVec3::new(first.x, opposite.y, first.z),
            ]
            .into_iter()
            .map(|point| self.plane.to_world(point))
            .collect(),
        )
    }

    fn preview(entity: &EntityType, name: &str) -> Option<WireModel> {
        let curve = entity_curve(entity)?;
        let points = curve_points(&curve);
        (points.len() >= 2).then(|| {
            WireModel::solid_f64(name.to_string(), points, WireModel::CYAN, false)
        })
    }

    fn preview_world_points(&self, points: &[DVec3], name: &str) -> Option<WireModel> {
        let entity = self.cloud_from_world_points(points, false)?;
        Self::preview(&entity, name)
    }

    fn start_modify(&mut self, handle: Handle, picked: DVec3) -> CmdResult {
        let Some(source) = self.sources.get(&handle).cloned() else {
            self.message = Some("Select a closed revision-cloud polyline.");
            return CmdResult::NeedPoint;
        };
        let EntityType::LwPolyline(polyline) = &source else {
            self.message = Some("Select a closed revision-cloud polyline.");
            return CmdResult::NeedPoint;
        };
        if !is_revision_cloud(polyline) {
            self.message = Some("Select a closed revision-cloud polyline.");
            return CmdResult::NeedPoint;
        }
        let Some(curve) = entity_curve(&source) else {
            self.message = Some("The selected polyline is not planar.");
            return CmdResult::NeedPoint;
        };
        let segments = curve.curve.segments();
        if segments.len() != polyline.vertices.len() {
            self.message = Some("The selected polyline is not planar.");
            return CmdResult::NeedPoint;
        }
        let vertices: Vec<DVec3> = polyline
            .vertices
            .iter()
            .map(|vertex| {
                DVec3::from_array(curve.plane.point_at([
                    vertex.location.x,
                    vertex.location.y,
                ]))
            })
            .collect();
        let start = vertices
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                left.distance_squared(picked)
                    .total_cmp(&right.distance_squared(picked))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.stage = Stage::ModifyDraw(ModifyState {
            handle,
            source,
            plane: curve.plane,
            segments,
            vertices: vertices.clone(),
            start,
            end: None,
            replacement: vec![vertices[start]],
        });
        self.message = None;
        CmdResult::NeedPoint
    }

    fn modify_endpoint(state: &ModifyState, point: DVec3, tolerance: f64) -> Option<usize> {
        state
            .vertices
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != state.start)
            .filter_map(|(index, vertex)| {
                let distance = vertex.distance(point);
                (distance <= tolerance).then_some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    }

    fn path_indices(count: usize, from: usize, to: usize) -> Vec<usize> {
        let mut indices = vec![from];
        let mut current = from;
        while current != to && indices.len() <= count {
            current = (current + 1) % count;
            indices.push(current);
        }
        indices
    }

    fn path_distance(state: &ModifyState, indices: &[usize], point: DVec3) -> f64 {
        let Some(point) = state.plane.project(point.to_array()) else {
            return f64::INFINITY;
        };
        indices
            .windows(2)
            .map(|pair| distance_to(&state.segments[pair[0]], point))
            .fold(f64::INFINITY, f64::min)
    }

    fn modified_cloud(&self, state: ModifyState, erase_point: DVec3) -> Option<PendingCloud> {
        let end = state.end?;
        let count = state.vertices.len();
        let forward = Self::path_indices(count, state.start, end);
        let backward = Self::path_indices(count, end, state.start);
        let erase_forward = Self::path_distance(&state, &forward, erase_point)
            <= Self::path_distance(&state, &backward, erase_point);

        let mut guide = Vec::new();
        if erase_forward {
            guide.extend(backward.iter().map(|index| state.vertices[*index]));
            guide.extend(state.replacement.iter().skip(1).copied());
        } else {
            guide.extend(forward.iter().map(|index| state.vertices[*index]));
            guide.extend(state.replacement.iter().rev().skip(1).copied());
        }
        let style = style_from_entity(&state.source).unwrap_or(self.style);
        let curve = guide_curve_on_plane(
            &state.plane,
            &guide,
            self.arc_length * 1.0e-6,
        )?;
        let mut entity = cloud_entity_on_plane(
            &PlanarCurve::new(state.plane, curve),
            self.arc_length,
            style,
            false,
        )?;
        let common = state.source.common().clone();
        *entity.common_mut() = common;
        entity.common_mut().handle = Handle::NULL;
        Some(PendingCloud {
            entity,
            replacement: Some(state.handle),
        })
    }

    fn common_options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Arc length", "A"),
            CmdOption::new("Object", "O"),
            CmdOption::new("Rectangular", "R"),
            CmdOption::new("Polygonal", "P"),
            CmdOption::new("Freehand", "F"),
            CmdOption::new("Style", "S"),
            CmdOption::new("Modify", "M"),
        ]
    }
}

impl CadCommand for RevCloudCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "REVCLOUD"
    }

    fn prompt(&self) -> String {
        let message = self
            .message
            .map(|message| t!(message).into_owned())
            .unwrap_or_default();
        let style = t!(self.style.label()).into_owned();
        match &self.stage {
            Stage::Create => match self.creation {
                CreationMode::Rectangular if self.points.is_empty() => t!(
                    "REVCLOUD  First corner (%{style}, arc length %{length}): %{message}",
                    style = &style,
                    length = format!("{:.4}", self.arc_length),
                    message = message
                )
                .into_owned(),
                CreationMode::Rectangular => {
                    t!("REVCLOUD  Opposite corner: %{message}", message = message).into_owned()
                }
                CreationMode::Polygonal if self.points.is_empty() => t!(
                    "REVCLOUD  Polygonal start point (%{style}, arc length %{length}): %{message}",
                    style = &style,
                    length = format!("{:.4}", self.arc_length),
                    message = message
                )
                .into_owned(),
                CreationMode::Polygonal => t!(
                    "REVCLOUD  Next polygonal point (%{count} points, Enter to close): %{message}",
                    count = self.points.len(),
                    message = message
                )
                .into_owned(),
                CreationMode::Freehand if !self.tracing => t!(
                    "REVCLOUD  Freehand first point (%{style}, arc length %{length}): %{message}",
                    style = &style,
                    length = format!("{:.4}", self.arc_length),
                    message = message
                )
                .into_owned(),
                CreationMode::Freehand => t!(
                    "REVCLOUD  Guide the cursor, then click or press Enter to close: %{message}",
                    message = message
                )
                .into_owned(),
            },
            Stage::ArcLength => t!(
                "REVCLOUD  Approximate arc chord length <%{length}>:",
                length = format!("{:.4}", self.arc_length)
            )
            .into_owned(),
            Stage::Style => t!(
                "REVCLOUD  Style [Normal/Calligraphy] <%{style}>:",
                style = &style
            )
            .into_owned(),
            Stage::Object => t!(
                "REVCLOUD  Select a closed circle, ellipse, polyline, or spline: %{message}",
                message = message
            )
            .into_owned(),
            Stage::Reverse(_) => {
                t!("REVCLOUD  Reverse arc direction [Yes/No] <No>:").into_owned()
            }
            Stage::ModifySelect => t!(
                "REVCLOUD  Select a closed revision-cloud polyline near the replacement start: %{message}",
                message = message
            )
            .into_owned(),
            Stage::ModifyDraw(state) => t!(
                "REVCLOUD  Replacement point %{count}; finish on another cloud vertex:",
                count = state.replacement.len()
            )
            .into_owned(),
            Stage::ModifyErase(_) => {
                t!("REVCLOUD  Pick the side of the original cloud to erase:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.stage {
            Stage::Create if self.points.is_empty() => self.common_options(),
            Stage::Create if self.creation == CreationMode::Polygonal => {
                let mut options = vec![CmdOption::new("Undo", "U")];
                if self.points.len() >= 3 {
                    options.push(CmdOption::enter("Close"));
                }
                options
            }
            Stage::Create if self.creation == CreationMode::Freehand && self.tracing => {
                vec![CmdOption::enter("Close")]
            }
            Stage::Create => Vec::new(),
            Stage::ArcLength => Vec::new(),
            Stage::Style => vec![
                CmdOption::new("Normal", "N"),
                CmdOption::new("Calligraphy", "C"),
            ],
            Stage::Object | Stage::ModifySelect => vec![CmdOption::new("Back", "B")],
            Stage::Reverse(_) => vec![
                CmdOption::new("Yes", "Y"),
                CmdOption::new("No", "N"),
            ],
            Stage::ModifyDraw(state) => {
                if state.replacement.len() > 1 {
                    vec![CmdOption::new("Undo", "U"), CmdOption::new("First point", "F")]
                } else {
                    Vec::new()
                }
            }
            Stage::ModifyErase(_) => Vec::new(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.message = None;
        match &mut self.stage {
            Stage::Create => match self.creation {
                CreationMode::Rectangular => {
                    if self.points.is_empty() {
                        self.points.push(point);
                        CmdResult::NeedPoint
                    } else {
                        let first = self.points[0];
                        let Some(points) = self.rectangle_points(first, point) else {
                            self.message = Some("The two corners must define a non-zero area.");
                            return CmdResult::NeedPoint;
                        };
                        self.points.clear();
                        self.prepare_cloud(&points, None)
                    }
                }
                CreationMode::Polygonal => {
                    self.points.push(point);
                    CmdResult::NeedPoint
                }
                CreationMode::Freehand => {
                    if !self.tracing {
                        self.points.clear();
                        self.points.push(point);
                        self.tracing = true;
                        CmdResult::NeedPoint
                    } else if self.points.len() >= 3 {
                        self.tracing = false;
                        let points = self.points.clone();
                        self.points.clear();
                        self.prepare_cloud(&points, None)
                    } else {
                        CmdResult::NeedPoint
                    }
                }
            },
            Stage::ModifyDraw(state) => {
                let tolerance = self.arc_length.max(1.0e-6) * 0.75;
                if state.replacement.len() >= 2 {
                    if let Some(end) = Self::modify_endpoint(state, point, tolerance) {
                        state.end = Some(end);
                        state.replacement.push(state.vertices[end]);
                        let state = match std::mem::replace(&mut self.stage, Stage::Create) {
                            Stage::ModifyDraw(state) => state,
                            _ => unreachable!(),
                        };
                        self.stage = Stage::ModifyErase(state);
                        return CmdResult::NeedPoint;
                    }
                }
                state.replacement.push(point);
                CmdResult::NeedPoint
            }
            Stage::ModifyErase(_) => {
                let state = match std::mem::replace(&mut self.stage, Stage::Create) {
                    Stage::ModifyErase(state) => state,
                    _ => unreachable!(),
                };
                let Some(pending) = self.modified_cloud(state, point) else {
                    return CmdResult::Cancel;
                };
                self.stage = Stage::Reverse(pending);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.stage {
            Stage::Create if self.creation == CreationMode::Polygonal && self.points.len() >= 3 => {
                let points = self.points.clone();
                self.points.clear();
                self.prepare_cloud(&points, None)
            }
            Stage::Create if self.creation == CreationMode::Freehand && self.points.len() >= 3 => {
                self.tracing = false;
                let points = self.points.clone();
                self.points.clear();
                self.prepare_cloud(&points, None)
            }
            Stage::ArcLength | Stage::Style => {
                self.stage = Stage::Create;
                CmdResult::NeedPoint
            }
            Stage::Reverse(_) => {
                let stage = std::mem::replace(&mut self.stage, Stage::Create);
                match stage {
                    Stage::Reverse(pending) => Self::finish_pending(pending, false),
                    _ => unreachable!(),
                }
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

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.stage, Stage::Create | Stage::ModifyDraw(_))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        if matches!(self.stage, Stage::ArcLength) {
            if let Ok(value) = input.parse::<f64>() {
                if value.is_finite() && value > 0.0 {
                    self.arc_length = value;
                    LAST_ARC_LENGTH.store(value.to_bits(), Ordering::Relaxed);
                    self.stage = Stage::Create;
                    self.message = None;
                } else {
                    self.message = Some("Arc length must be greater than zero.");
                }
            } else {
                self.message = Some("Enter a valid arc length.");
            }
            return Some(CmdResult::NeedPoint);
        }
        let keyword = input.to_ascii_uppercase();
        if matches!(self.stage, Stage::Style) {
            self.style = match keyword.as_str() {
                "N" | "NORMAL" => CloudStyle::Normal,
                "C" | "CALLIGRAPHY" => CloudStyle::Calligraphy,
                _ => return None,
            };
            self.style.remember();
            self.stage = Stage::Create;
            return Some(CmdResult::NeedPoint);
        }
        if matches!(keyword.as_str(), "F" | "FIRST") {
            if let Stage::ModifyDraw(state) = &mut self.stage {
                state.replacement.truncate(1);
                return Some(CmdResult::NeedPoint);
            }
        }
        match &self.stage {
            Stage::Reverse(_) => {
                let reverse = match keyword.as_str() {
                    "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return None,
                };
                let stage = std::mem::replace(&mut self.stage, Stage::Create);
                return Some(match stage {
                    Stage::Reverse(pending) => Self::finish_pending(pending, reverse),
                    _ => unreachable!(),
                });
            }
            Stage::Object | Stage::ModifySelect if keyword == "B" || keyword == "BACK" => {
                self.stage = Stage::Create;
                return Some(CmdResult::NeedPoint);
            }
            Stage::ModifyDraw(_) if keyword == "U" || keyword == "UNDO" => {
                return self.on_undo_step();
            }
            Stage::Create => match keyword.as_str() {
                "A" | "ARC" | "ARCLENGTH" => self.stage = Stage::ArcLength,
                "O" | "OBJECT" => {
                    self.stage = Stage::Object;
                    self.points.clear();
                }
                "R" | "RECTANGULAR" | "RECTANGLE" => {
                    self.set_creation(CreationMode::Rectangular)
                }
                "P" | "POLYGONAL" | "POLYGON" => {
                    self.set_creation(CreationMode::Polygonal)
                }
                "F" | "FREEHAND" => self.set_creation(CreationMode::Freehand),
                "S" | "STYLE" => self.stage = Stage::Style,
                "M" | "MODIFY" => {
                    self.stage = Stage::ModifySelect;
                    self.points.clear();
                }
                "U" | "UNDO" => return self.on_undo_step(),
                _ => return None,
            },
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

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        match self.stage {
            Stage::Object => {
                let Some(entity) = self.sources.get(&handle) else {
                    self.message = Some("Select a supported closed curve.");
                    return CmdResult::NeedPoint;
                };
                let Some(curve) = Self::object_curve(entity) else {
                    self.message = Some("Select a closed circle, ellipse, polyline, or spline.");
                    return CmdResult::NeedPoint;
                };
                self.prepare_planar_cloud(&curve, handle)
            }
            Stage::ModifySelect => self.start_modify(handle, point),
            _ => CmdResult::Cancel,
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, _point: DVec3) -> Vec<WireModel> {
        if !matches!(self.stage, Stage::Object) {
            return Vec::new();
        }
        let Some(curve) = self
            .sources
            .get(&handle)
            .and_then(Self::object_curve)
        else {
            return Vec::new();
        };
        cloud_entity_on_plane(&curve, self.arc_length, self.style, false)
            .as_ref()
            .and_then(|entity| Self::preview(entity, "revcloud_object_preview"))
            .into_iter()
            .collect()
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        match &mut self.stage {
            Stage::Create if !self.points.is_empty() => {
                self.points.pop();
                if self.points.is_empty() {
                    self.tracing = false;
                }
                Some(CmdResult::NeedPoint)
            }
            Stage::ModifyDraw(state) if state.replacement.len() > 1 => {
                state.replacement.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match &mut self.stage {
            Stage::Create if self.creation == CreationMode::Freehand && self.tracing => {
                let spacing = (self.arc_length * 0.25).max(1.0e-6);
                if self.points.last().is_none_or(|last| last.distance(point) >= spacing) {
                    self.points.push(point);
                }
                let points = self.points.clone();
                self.preview_world_points(&points, "revcloud_freehand_preview")
            }
            Stage::Create if self.creation == CreationMode::Rectangular => {
                let first = *self.points.first()?;
                let points = self.rectangle_points(first, point)?;
                self.preview_world_points(&points, "revcloud_rectangular_preview")
            }
            Stage::Create if self.creation == CreationMode::Polygonal && !self.points.is_empty() => {
                let mut points = self.points.clone();
                points.push(point);
                self.preview_world_points(&points, "revcloud_polygonal_preview")
            }
            Stage::Reverse(pending) => Self::preview(&pending.entity, "revcloud_reverse_preview"),
            Stage::ModifyDraw(state) => {
                let mut points = state.replacement.clone();
                points.push(point);
                let curve = guide_curve_on_plane(
                    &state.plane,
                    &points,
                    self.arc_length * 1.0e-6,
                )?;
                let entity = cloud_entity_on_plane(
                    &PlanarCurve::new(state.plane, curve),
                    self.arc_length,
                    style_from_entity(&state.source).unwrap_or(self.style),
                    false,
                )?;
                Self::preview(&entity, "revcloud_modify_preview")
            }
            _ => None,
        }
    }
}

fn style_width_ratios(style: CloudStyle) -> Option<(f64, f64)> {
    (style == CloudStyle::Calligraphy).then_some((0.04, 0.16))
}

fn cloud_entity_on_plane(
    source: &PlanarCurve,
    arc_length: f64,
    style: CloudStyle,
    reverse: bool,
) -> Option<EntityType> {
    let cloud = revision_cloud_from_curve(
        &source.curve,
        arc_length,
        reverse,
        style_width_ratios(style),
    )?;
    let normal = Vec3::from(source.plane.normal()?);
    let elevation = Vec3::from(source.plane.origin).dot(normal);
    let normal = Vector3::new(normal.x, normal.y, normal.z);
    let storage_plane = ocs_plane(normal, elevation);
    let mut cloud = cloud;
    for vertex in &mut cloud.vertices {
        let world = source
            .plane
            .point_at([vertex.location.x, vertex.location.y]);
        let local = storage_plane.project(world)?;
        vertex.location.x = local[0];
        vertex.location.y = local[1];
    }
    cloud.normal = normal;
    cloud.elevation = elevation;
    Some(EntityType::LwPolyline(cloud))
}

fn guide_curve_on_plane(
    plane: &Plane,
    points: &[DVec3],
    tolerance: f64,
) -> Option<Curve> {
    let mut points: Vec<[f64; 2]> = points
        .iter()
        .map(|point| plane.project(point.to_array()))
        .collect::<Option<_>>()?;
    points.dedup_by(|right, left| {
        Vec2::from(*left).distance(Vec2::from(*right)) <= tolerance
    });
    if points.len() >= 2
        && Vec2::from(points[0]).distance(Vec2::from(*points.last()?)) <= tolerance
    {
        points.pop();
    }
    (points.len() >= 3).then(|| {
        Curve::Polyline(Polyline {
            vertices: points
                .into_iter()
                .map(PolylineVertex::straight)
                .collect(),
            closed: true,
        })
    })
}

fn reverse_cloud_entity(mut entity: EntityType) -> EntityType {
    if let EntityType::LwPolyline(polyline) = &mut entity {
        for vertex in &mut polyline.vertices {
            vertex.bulge = -vertex.bulge;
        }
    }
    entity
}

fn style_from_entity(entity: &EntityType) -> Option<CloudStyle> {
    let EntityType::LwPolyline(polyline) = entity else {
        return None;
    };
    Some(if polyline
        .vertices
        .iter()
        .any(|vertex| vertex.start_width > 0.0 || vertex.end_width > 0.0)
    {
        CloudStyle::Calligraphy
    } else {
        CloudStyle::Normal
    })
}

inventory::submit!(crate::command::CommandRegistration { names: &["REVCLOUD"] });
