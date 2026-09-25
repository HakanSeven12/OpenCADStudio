// MVIEW — interactive paper-space viewport creation.
//
// VPCLIP (and CLIP on a layout viewport) reuses the polygon and object
// steps to clip an existing viewport:
//   Select viewport to clip:
//   Select clipping object or [Polygonal] <Polygonal>:   (Delete also taken
//                                                         when clipped)
//   Specify start point:
//   Specify next point or [Arc/Length/Undo]:
//   Specify next point or [Arc/Close/Length/Undo]:
//   Enter an arc boundary option
//   [Angle/CEnter/CLose/Direction/Line/Radius/Second pt/Undo/Endpoint of arc] <Endpoint>:
//   Specify length of line:
// and the arc options' prompts (Angle, CEnter, Direction, Radius, Second pt)
// as the reference words them; CLose in arc mode closes with a tangent arc.

use codec::entities::{LwPolyline, LwVertex, Viewport};
use codec::tables::View;
use codec::types::{Vector2, Vector3};
use codec::{EntityType, Handle};
use crate::t;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::draw::draw::polyline::{
    arc_for, arc_sample_points, compute_bulge, seg_exit_tangent, update_tangent_after_arc, Sub,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::{DVec2, DVec3, Vec2};

// ── Ribbon definition ─────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MVIEW",
        label: "Viewport",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/viewport.svg")),
        event: ModuleEvent::Command("MVIEW".to_string()),
    }
}

// ── Command ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Step {
    RectangleFirst,
    RectangleSecond,
    Polygon,
    Object,
    ChooseView,
    DefineNewFirst,
    DefineNewSecond,
    PlaceView,
    /// VPCLIP: the viewport to clip.
    ClipSelect,
    /// VPCLIP: a clipping object, or Polygonal / Delete.
    ClipChoice,
    /// VPCLIP Length: a line of that length along the last direction.
    ClipLength,
}

#[derive(Clone, Copy, PartialEq)]
enum PolygonMode {
    Line,
    Arc,
}

pub struct MviewCommand {
    step: Step,
    first: Option<DVec3>,
    polygon: Vec<DVec3>,
    polygon_bulges: Vec<f64>,
    polygon_mode: PolygonMode,
    polygon_last_tangent: Option<Vec2>,
    view: Option<View>,
    views: Vec<View>,
    paper_bounds: ((f64, f64), (f64, f64)),
    original_layout: String,
    /// VPCLIP: the viewport being clipped (NULL until chosen) and whether it
    /// already has a boundary.
    clip: Option<(Handle, bool)>,
    picked: Option<EntityType>,
    /// VPCLIP arc mode: the arc option being answered.
    arc_sub: Sub,
}

impl MviewCommand {
    pub fn new(
        original_layout: String,
        paper_bounds: ((f64, f64), (f64, f64)),
        views: Vec<View>,
    ) -> Self {
        Self {
            step: Step::RectangleFirst,
            first: None,
            polygon: Vec::new(),
            polygon_bulges: Vec::new(),
            polygon_mode: PolygonMode::Line,
            polygon_last_tangent: None,
            view: None,
            views,
            paper_bounds,
            original_layout,
            clip: None,
            picked: None,
            arc_sub: Sub::None,
        }
    }

    /// VPCLIP asking for the viewport.
    pub fn vpclip_select() -> Self {
        let mut command = Self::new(String::new(), ((0.0, 0.0), (0.0, 0.0)), Vec::new());
        command.clip = Some((Handle::NULL, false));
        command.step = Step::ClipSelect;
        command
    }

    /// VPCLIP on a chosen viewport.
    pub fn vpclip(viewport: Handle, clipped: bool) -> Self {
        let mut command = Self::vpclip_select();
        command.clip = Some((viewport, clipped));
        command.step = Step::ClipChoice;
        command
    }

    fn clip_target(&self) -> Handle {
        self.clip.map_or(Handle::NULL, |(handle, _)| handle)
    }

    /// VPCLIP: an arc segment from the last vertex to `e`.
    fn push_arc(&mut self, e: DVec2, bulge: f64) -> CmdResult {
        let Some(last) = self.polygon.last().copied() else {
            return CmdResult::NeedPoint;
        };
        let end = DVec3::new(e.x, e.y, last.z);
        let index = self.polygon.len() - 1;
        self.polygon_bulges[index] = bulge;
        self.polygon_last_tangent = seg_exit_tangent(last, end, bulge);
        self.arc_sub = Sub::None;
        self.polygon.push(end);
        self.polygon_bulges.push(0.0);
        CmdResult::NeedPoint
    }

    fn last2(&self) -> Option<DVec2> {
        self.polygon.last().map(|p| DVec2::new(p.x, p.y))
    }

    /// VPCLIP arc option answered with a typed value.
    fn arc_value(&mut self, value: f64) -> Option<CmdResult> {
        let a = self.last2()?;
        let tangent = self.polygon_last_tangent;
        let angle_ok = |r: f64| r.abs() > 1e-9 && r.abs() < std::f64::consts::TAU;
        let next = match self.arc_sub {
            Sub::ArcAngle if angle_ok(value.to_radians()) => Sub::ArcAngleEnd { angle: value.to_radians() },
            Sub::ArcAngleRadius { angle } if value > 0.0 => Sub::ArcAngleRadiusDir { angle, r: value },
            Sub::ArcRadius if value > 0.0 => Sub::ArcRadiusEnd { r: value },
            Sub::ArcRadiusAngle { r } if angle_ok(value.to_radians()) => {
                Sub::ArcRadiusAngleDir { r, angle: value.to_radians() }
            }
            sub @ Sub::ArcCenterAngle { .. } => {
                let (e, bulge) = arc_for(sub, a, DVec2::new(value.to_radians(), 0.0), tangent)?;
                return Some(self.push_arc(e, bulge));
            }
            sub @ Sub::ArcCenterLength { .. } => {
                let (e, bulge) = arc_for(sub, a, DVec2::new(value, 0.0), tangent)?;
                return Some(self.push_arc(e, bulge));
            }
            _ => return None,
        };
        self.arc_sub = next;
        Some(CmdResult::NeedPoint)
    }

    /// VPCLIP arc option answered with a point.
    fn arc_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(a) = self.last2() else {
            return CmdResult::NeedPoint;
        };
        let p = DVec2::new(pt.x, pt.y);
        let sub = self.arc_sub;
        if sub.is_scalar() {
            let value = if sub.is_angle() { (p - a).y.atan2((p - a).x).to_degrees() } else { (p - a).length() };
            return self.arc_value(value).unwrap_or(CmdResult::NeedPoint);
        }
        match sub {
            Sub::ArcCenter if (p - a).length_squared() > 1e-12 => self.arc_sub = Sub::ArcCenterEnd { c: p },
            Sub::ArcDirection => {
                if let Some(dir) = (p - a).try_normalize() {
                    self.arc_sub = Sub::ArcDirectionEnd { dir };
                }
            }
            Sub::ArcSecond if (p - a).length_squared() > 1e-12 => self.arc_sub = Sub::ArcSecondEnd { s: p },
            _ => {
                if let Some((e, bulge)) = arc_for(sub, a, p, self.polygon_last_tangent) {
                    return self.push_arc(e, bulge);
                }
            }
        }
        CmdResult::NeedPoint
    }

    /// VPCLIP CLose in arc mode: a tangent arc back to the start.
    fn close_with_arc(&mut self) -> CmdResult {
        let (Some(last), Some(first)) = (self.polygon.last().copied(), self.polygon.first().copied()) else {
            return CmdResult::NeedPoint;
        };
        let tangent = self.polygon_last_tangent.map_or(DVec2::X, |t| t.as_dvec2());
        let index = self.polygon.len() - 1;
        self.polygon_bulges[index] = compute_bulge(DVec2::new(last.x, last.y), tangent, DVec2::new(first.x, first.y));
        self.finish_polygon()
    }

    fn arc_prompt(&self) -> Option<&'static str> {
        Some(match self.arc_sub {
            Sub::ArcAngle | Sub::ArcCenterAngle { .. } | Sub::ArcRadiusAngle { .. } => "Specify included angle:",
            Sub::ArcAngleEnd { .. } => "Specify endpoint of arc (hold Ctrl to switch direction) or [CEnter/Radius]:",
            Sub::ArcCenter | Sub::ArcAngleCenter { .. } => "Specify center point of arc:",
            Sub::ArcCenterEnd { .. } => "Specify endpoint of arc (hold Ctrl to switch direction) or [Angle/Length]:",
            Sub::ArcCenterLength { .. } => "Specify length of chord:",
            Sub::ArcDirection => "Specify the tangent direction for the start point of arc:",
            Sub::ArcDirectionEnd { .. } => "Specify endpoint of the arc (hold Ctrl to switch direction):",
            Sub::ArcRadius | Sub::ArcAngleRadius { .. } => "Specify radius of arc:",
            Sub::ArcRadiusEnd { .. } => "Specify endpoint of arc (hold Ctrl to switch direction) or [Angle]:",
            Sub::ArcAngleRadiusDir { .. } | Sub::ArcRadiusAngleDir { .. } => "Specify direction of chord for arc:",
            Sub::ArcSecond => "Specify second point on arc:",
            Sub::ArcSecondEnd { .. } => "Specify end point of arc:",
            _ => return None,
        })
    }

    fn clip_text(&mut self, upper: &str) -> Option<CmdResult> {
        if self.step == Step::Polygon && self.arc_sub != Sub::None {
            let next = match (self.arc_sub, upper) {
                (Sub::ArcAngleEnd { angle }, "CE" | "CENTER" | "CENTRE") => Sub::ArcAngleCenter { angle },
                (Sub::ArcAngleEnd { angle }, "R" | "RADIUS") => Sub::ArcAngleRadius { angle },
                (Sub::ArcCenterEnd { c }, "A" | "ANGLE") => Sub::ArcCenterAngle { c },
                (Sub::ArcCenterEnd { c }, "L" | "LENGTH") => Sub::ArcCenterLength { c },
                (Sub::ArcRadiusEnd { r }, "A" | "ANGLE") => Sub::ArcRadiusAngle { r },
                (sub, text) if sub.is_scalar() => {
                    let value = if sub.is_angle() {
                        crate::entities::common::parse_typed_angle(text).map(f64::to_degrees)
                    } else {
                        crate::entities::common::parse_typed_length(text)
                    };
                    return value.and_then(|v| self.arc_value(v));
                }
                _ => return None,
            };
            self.arc_sub = next;
            return Some(CmdResult::NeedPoint);
        }
        match self.step {
            Step::ClipChoice => match upper {
                "" | "P" | "POLYGONAL" => {
                    self.step = Step::Polygon;
                    Some(CmdResult::NeedPoint)
                }
                "D" | "DELETE" if self.clip.is_some_and(|(_, clipped)| clipped) => {
                    Some(CmdResult::MviewCreateClipped {
                        boundary: None,
                        boundary_handle: Handle::NULL,
                        target: self.clip_target(),
                    })
                }
                _ => None,
            },
            Step::ClipLength => {
                let length = upper.parse::<f64>().ok()?;
                let last = *self.polygon.last()?;
                let direction = self.polygon_last_tangent.map_or(DVec2::X, |t| t.as_dvec2());
                self.step = Step::Polygon;
                Some(self.on_point(last + DVec3::new(direction.x, direction.y, 0.0) * length))
            }
            Step::Polygon if self.polygon_mode == PolygonMode::Arc => match upper {
                "CL" | "CLOSE" if self.polygon.len() >= 2 => Some(self.close_with_arc()),
                "A" | "ANGLE" | "CE" | "CENTER" | "CENTRE" | "D" | "DIRECTION" | "R" | "RADIUS" | "S"
                | "SECOND" | "SECOND PT" => {
                    self.arc_sub = match upper {
                        "A" | "ANGLE" => Sub::ArcAngle,
                        "CE" | "CENTER" | "CENTRE" => Sub::ArcCenter,
                        "D" | "DIRECTION" => Sub::ArcDirection,
                        "R" | "RADIUS" => Sub::ArcRadius,
                        _ => Sub::ArcSecond,
                    };
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" => {
                    self.polygon_mode = PolygonMode::Line;
                    Some(CmdResult::NeedPoint)
                }
                "U" | "UNDO" if !self.polygon.is_empty() => Some(self.undo_polygon()),
                _ => None,
            },
            Step::Polygon => match upper {
                "A" | "ARC" if !self.polygon.is_empty() => {
                    self.polygon_mode = PolygonMode::Arc;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.polygon.len() >= 3 => Some(self.finish_polygon()),
                "L" | "LENGTH" if !self.polygon.is_empty() => {
                    self.step = Step::ClipLength;
                    Some(CmdResult::NeedPoint)
                }
                "U" | "UNDO" if !self.polygon.is_empty() => Some(self.undo_polygon()),
                _ => None,
            },
            _ => None,
        }
    }

    fn viewport_from_corners(a: DVec3, b: DVec3) -> Option<Viewport> {
        let width = (b.x - a.x).abs();
        let height = (b.y - a.y).abs();
        if width < 1e-6 || height < 1e-6 {
            return None;
        }
        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(
            (a.x + b.x) / 2.0,
            (a.y + b.y) / 2.0,
            a.z,
        );
        viewport.width = width;
        viewport.height = height;
        viewport.id = 2;
        Some(viewport)
    }

    fn fit_viewport(&self) -> Option<Viewport> {
        let ((x0, y0), (x1, y1)) = self.paper_bounds;
        Self::viewport_from_corners(
            DVec3::new(x0, y0, 0.0),
            DVec3::new(x1, y1, 0.0),
        )
    }

    fn placed_viewport(&self, center: DVec3) -> Option<Viewport> {
        let view = self.view.as_ref()?;
        let source_width = view.width.abs().max(1e-6);
        let source_height = view.height.abs().max(1e-6);
        let ((x0, y0), (x1, y1)) = self.paper_bounds;
        let max_width = ((x1 - x0).abs() * 0.5).max(1e-6);
        let max_height = ((y1 - y0).abs() * 0.5).max(1e-6);
        let aspect = source_width / source_height;
        let (width, height) = if max_width / max_height > aspect {
            (max_height * aspect, max_height)
        } else {
            (max_width, max_width / aspect)
        };

        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(center.x, center.y, center.z);
        viewport.width = width;
        viewport.height = height;
        viewport.id = 2;
        viewport.view_target = view.target.clone();
        viewport.view_direction = view.direction.clone();
        viewport.view_height = source_height;
        viewport.custom_scale = height / source_height;
        viewport.lens_length = view.lens_length;
        viewport.twist_angle = view.twist_angle;
        viewport.status.perspective = view.perspective;
        Some(viewport)
    }

    fn polygon_boundary(&self) -> Option<EntityType> {
        if self.polygon.len() < 3 {
            return None;
        }
        let mut polyline = LwPolyline::new();
        polyline.is_closed = true;
        polyline.elevation = self.polygon[0].z;
        polyline.vertices = self
            .polygon
            .iter()
            .zip(self.polygon_bulges.iter())
            .map(|(point, bulge)| {
                let mut vertex = LwVertex::new(Vector2::new(point.x, point.y));
                vertex.bulge = *bulge;
                vertex
            })
            .collect();
        Some(EntityType::LwPolyline(polyline))
    }

    fn finish_polygon(&self) -> CmdResult {
        match self.polygon_boundary() {
            Some(boundary) => CmdResult::MviewCreateClipped {
                boundary: Some(boundary),
                boundary_handle: Handle::NULL,
                target: self.clip_target(),
            },
            None => CmdResult::Cancel,
        }
    }

    fn undo_polygon(&mut self) -> CmdResult {
        self.polygon.pop();
        self.polygon_bulges.pop();
        let count = self.polygon.len();
        self.polygon_last_tangent = if count >= 2 {
            seg_exit_tangent(
                self.polygon[count - 2],
                self.polygon[count - 1],
                self.polygon_bulges[count - 2],
            )
        } else {
            None
        };
        CmdResult::NeedPoint
    }

    fn polygon_preview(&self, cursor: DVec3) -> Option<WireModel> {
        let last = self.polygon.last()?.as_vec3();
        let mut points: Vec<[f32; 3]> = Vec::new();

        for index in 0..self.polygon.len().saturating_sub(1) {
            let start = self.polygon[index].as_vec3();
            let end = self.polygon[index + 1].as_vec3();
            let bulge = self.polygon_bulges[index];
            if bulge.abs() < 1e-10 {
                if points.is_empty() {
                    points.push([start.x, start.y, start.z]);
                }
                points.push([end.x, end.y, end.z]);
            } else {
                let sampled = arc_sample_points(start, bulge, end, 16);
                if points.is_empty() {
                    points.extend_from_slice(&sampled);
                } else {
                    points.extend_from_slice(&sampled[1..]);
                }
            }
        }

        let cursor = cursor.as_vec3();
        match self.polygon_mode {
            PolygonMode::Line => {
                if points.is_empty() {
                    points.push([last.x, last.y, last.z]);
                }
                points.push([cursor.x, cursor.y, cursor.z]);
            }
            PolygonMode::Arc => {
                let tangent = self
                    .polygon_last_tangent
                    .map(|value| value.as_dvec2())
                    .unwrap_or(DVec2::new(1.0, 0.0));
                let bulge = compute_bulge(
                    DVec2::new(last.x as f64, last.y as f64),
                    tangent,
                    DVec2::new(cursor.x as f64, cursor.y as f64),
                );
                let sampled = arc_sample_points(last, bulge, cursor, 16);
                if points.is_empty() {
                    points.extend_from_slice(&sampled);
                } else {
                    points.extend_from_slice(&sampled[1..]);
                }
            }
        }

        Some(WireModel::solid(
            "mview_preview".to_string(),
            points,
            WireModel::CYAN,
            false,
        ))
    }

    fn select_view(&mut self, name: &str) -> Option<CmdResult> {
        let view = self
            .views
            .iter()
            .find(|view| view.name.eq_ignore_ascii_case(name.trim()))?
            .clone();
        self.view = Some(view);
        self.step = Step::PlaceView;
        Some(CmdResult::NeedPoint)
    }

    fn preview(points: Vec<DVec3>) -> Option<WireModel> {
        if points.len() < 2 {
            return None;
        }
        Some(WireModel::solid_f64(
            "mview_preview".to_string(),
            points.iter().map(|point| [point.x, point.y, point.z]).collect(),
            WireModel::CYAN,
            false,
        ))
    }
}

impl CadCommand for MviewCommand {
    fn name(&self) -> &'static str {
        if self.clip.is_some() { "VPCLIP" } else { "MVIEW" }
    }

    fn prompt(&self) -> String {
        if self.clip.is_some() {
            return match self.step {
                Step::ClipSelect => "Select viewport to clip:",
                Step::ClipChoice => "Select clipping object or [Polygonal] <Polygonal>:",
                Step::ClipLength => "Specify length of line:",
                Step::Polygon if self.polygon.is_empty() => "Specify start point:",
                Step::Polygon if self.arc_prompt().is_some() => self.arc_prompt().unwrap_or_default(),
                Step::Polygon if self.polygon_mode == PolygonMode::Arc => {
                    "Enter an arc boundary option\n[Angle/CEnter/CLose/Direction/Line/Radius/Second pt/Undo/Endpoint of arc] <Endpoint>:"
                }
                Step::Polygon if self.polygon.len() < 3 => "Specify next point or [Arc/Length/Undo]:",
                _ => "Specify next point or [Arc/Close/Length/Undo]:",
            }
            .to_string();
        }
        match self.step {
            Step::RectangleFirst => t!(
                "MVIEW  Specify corner of viewport or [Polygonal/Object/Fit/Insert view]:"
            )
            .into_owned(),
            Step::RectangleSecond => t!("MVIEW  Specify opposite corner:").into_owned(),
            Step::Polygon if self.polygon.is_empty() => {
                t!("MVIEW Polygonal  Specify start point:").into_owned()
            }
            Step::Polygon => {
                let mode = match self.polygon_mode {
                    PolygonMode::Line => t!("Line"),
                    PolygonMode::Arc => t!("Arc"),
                };
                t!(
                    "MVIEW Polygonal [%{mode}]  Specify next point or [Arc/Line/Close/Undo] (%{count} points):",
                    mode = mode,
                    count = self.polygon.len()
                )
                .into_owned()
            }
            Step::Object => {
                t!("MVIEW Object  Select a circle, full ellipse, or closed polyline:").into_owned()
            }
            Step::ChooseView if self.views.is_empty() => {
                t!("MVIEW Insert view  No named views; choose [New]:").into_owned()
            }
            Step::ChooseView => t!("MVIEW Insert view  Choose a named view or [New]:").into_owned(),
            Step::DefineNewFirst => {
                t!("MVIEW New view  Specify first model-space corner:").into_owned()
            }
            Step::DefineNewSecond => {
                t!("MVIEW New view  Specify opposite model-space corner:").into_owned()
            }
            Step::PlaceView => t!("MVIEW Insert view  Specify placement point:").into_owned(),
            Step::ClipSelect | Step::ClipChoice | Step::ClipLength => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.clip.is_some() {
            return match self.step {
                Step::ClipChoice => vec![CmdOption::new("Polygonal", "P")],
                Step::Polygon if self.arc_sub != Sub::None => match self.arc_sub {
                    Sub::ArcAngleEnd { .. } => vec![CmdOption::new("CEnter", "CE"), CmdOption::new("Radius", "R")],
                    Sub::ArcCenterEnd { .. } => vec![CmdOption::new("Angle", "A"), CmdOption::new("Length", "L")],
                    Sub::ArcRadiusEnd { .. } => vec![CmdOption::new("Angle", "A")],
                    _ => Vec::new(),
                },
                Step::Polygon if self.polygon_mode == PolygonMode::Arc => vec![
                    CmdOption::new("Angle", "A"),
                    CmdOption::new("CEnter", "CE"),
                    CmdOption::new("CLose", "CL"),
                    CmdOption::new("Direction", "D"),
                    CmdOption::new("Line", "L"),
                    CmdOption::new("Radius", "R"),
                    CmdOption::new("Second pt", "S"),
                    CmdOption::new("Undo", "U"),
                ],
                Step::Polygon if !self.polygon.is_empty() => {
                    let mut options = vec![CmdOption::new("Arc", "A")];
                    if self.polygon.len() >= 3 {
                        options.push(CmdOption::new("Close", "C"));
                    }
                    options.push(CmdOption::new("Length", "L"));
                    options.push(CmdOption::new("Undo", "U"));
                    options
                }
                _ => Vec::new(),
            };
        }
        match self.step {
            Step::RectangleFirst => vec![
                CmdOption::new(t!("Polygonal").as_ref(), "POLYGONAL"),
                CmdOption::new(t!("Object").as_ref(), "OBJECT"),
                CmdOption::new(t!("Fit").as_ref(), "FIT"),
                CmdOption::new(t!("Insert view").as_ref(), "INSERT"),
            ],
            Step::Polygon if !self.polygon.is_empty() => vec![
                CmdOption::new(t!("Arc").as_ref(), "ARC"),
                CmdOption::new(t!("Line").as_ref(), "LINE"),
                CmdOption::new(t!("Close").as_ref(), "CLOSE"),
                CmdOption::new(t!("Undo").as_ref(), "UNDO"),
                CmdOption::enter(t!("Done").as_ref()),
            ],
            Step::ChooseView => {
                let mut options = vec![CmdOption::new(t!("New").as_ref(), "NEW")];
                options.extend(
                    self.views
                        .iter()
                        .map(|view| CmdOption::new(&view.name, &view.name)),
                );
                options
            }
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::RectangleFirst => {
                self.first = Some(pt);
                self.step = Step::RectangleSecond;
                CmdResult::NeedPoint
            }
            Step::RectangleSecond => match self
                .first
                .and_then(|first| Self::viewport_from_corners(first, pt))
            {
                Some(viewport) => CmdResult::MviewCreate {
                    viewport,
                    preserve_view: false,
                },
                None => CmdResult::NeedPoint,
            },
            Step::Polygon if self.arc_sub != Sub::None => self.arc_point(pt),
            Step::Polygon => {
                if let Some(last) = self.polygon.last().copied() {
                    let last_index = self.polygon.len() - 1;
                    let bulge = match self.polygon_mode {
                        PolygonMode::Line => {
                            let direction = DVec2::new(pt.x - last.x, pt.y - last.y);
                            if direction.length_squared() > 1e-10 {
                                self.polygon_last_tangent =
                                    Some(direction.normalize().as_vec2());
                            }
                            0.0
                        }
                        PolygonMode::Arc => {
                            let tangent = self
                                .polygon_last_tangent
                                .map(|value| value.as_dvec2())
                                .unwrap_or(DVec2::new(1.0, 0.0));
                            let bulge = compute_bulge(
                                DVec2::new(last.x, last.y),
                                tangent,
                                DVec2::new(pt.x, pt.y),
                            );
                            update_tangent_after_arc(
                                &mut self.polygon_last_tangent,
                                bulge,
                            );
                            bulge
                        }
                    };
                    self.polygon_bulges[last_index] = bulge;
                }

                if let Some(first) = self.polygon.first() {
                    let distance_squared =
                        (pt.x - first.x).powi(2) + (pt.y - first.y).powi(2);
                    if self.polygon.len() >= 3 && distance_squared < 1e-12 {
                        return self.finish_polygon();
                    }
                }

                self.polygon.push(pt);
                self.polygon_bulges.push(0.0);
                CmdResult::NeedPoint
            }
            Step::DefineNewFirst => {
                self.first = Some(pt);
                self.step = Step::DefineNewSecond;
                CmdResult::NeedPoint
            }
            Step::DefineNewSecond => {
                let Some(first) = self.first else {
                    return CmdResult::NeedPoint;
                };
                let width = (pt.x - first.x).abs();
                let height = (pt.y - first.y).abs();
                if width < 1e-6 || height < 1e-6 {
                    return CmdResult::NeedPoint;
                }
                let mut view = View::new("");
                view.width = width;
                view.height = height;
                view.target = Vector3::new(
                    (first.x + pt.x) / 2.0,
                    (first.y + pt.y) / 2.0,
                    (first.z + pt.z) / 2.0,
                );
                self.view = Some(view);
                self.step = Step::PlaceView;
                CmdResult::MviewSwitchLayout(self.original_layout.clone())
            }
            Step::PlaceView => match self.placed_viewport(pt) {
                Some(viewport) => CmdResult::MviewCreate {
                    viewport,
                    preserve_view: true,
                },
                None => CmdResult::Cancel,
            },
            Step::Object | Step::ChooseView | Step::ClipSelect | Step::ClipChoice => {
                CmdResult::NeedPoint
            }
            Step::ClipLength => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::ClipChoice => {
                self.step = Step::Polygon;
                CmdResult::NeedPoint
            }
            Step::Polygon if self.polygon.len() >= 3 => self.finish_polygon(),
            Step::DefineNewFirst | Step::DefineNewSecond => {
                CmdResult::MviewCancelToLayout(self.original_layout.clone())
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        match self.step {
            Step::DefineNewFirst | Step::DefineNewSecond => {
                CmdResult::MviewCancelToLayout(self.original_layout.clone())
            }
            _ => CmdResult::Cancel,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Object | Step::ClipSelect | Step::ClipChoice)
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.clip.is_some()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if self.step == Step::ClipSelect {
            return match self.picked.take() {
                Some(EntityType::Viewport(vp)) if vp.id != 1 => {
                    self.clip = Some((handle, !vp.clip_boundary_handle.is_null()));
                    self.step = Step::ClipChoice;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Object selected was not a viewport\n.".to_string()),
            };
        }
        CmdResult::MviewCreateClipped {
            boundary: None,
            boundary_handle: handle,
            target: self.clip_target(),
        }
    }

    fn input_kind(&self) -> InputKind {
        if self.step == Step::ClipLength || (self.step == Step::Polygon && self.arc_sub.is_scalar()) {
            InputKind::FreeText
        } else if self.step == Step::ChooseView {
            InputKind::FreeText
        } else if self.step == Step::RectangleFirst
            || self.step == Step::ClipChoice
            || (self.step == Step::Polygon && !self.polygon.is_empty())
        {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::RectangleFirst
            || self.step == Step::ClipChoice
            || (self.step == Step::Polygon && !self.polygon.is_empty())
    }

    fn window_corner_pick(&self) -> bool {
        matches!(self.step, Step::RectangleSecond | Step::DefineNewSecond)
    }

    fn window_first_corner(&self) -> Option<DVec3> {
        self.window_corner_pick().then_some(self.first).flatten()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim();
        let upper = keyword.to_ascii_uppercase();
        if self.clip.is_some() {
            return self.clip_text(&upper);
        }
        match self.step {
            Step::RectangleFirst => match upper.as_str() {
                "P" | "POLYGONAL" => {
                    self.step = Step::Polygon;
                    Some(CmdResult::NeedPoint)
                }
                "O" | "OBJECT" => {
                    self.step = Step::Object;
                    Some(CmdResult::NeedPoint)
                }
                "F" | "FIT" => self.fit_viewport().map(|viewport| CmdResult::MviewCreate {
                    viewport,
                    preserve_view: false,
                }),
                "I" | "INSERT" | "INSERTVIEW" | "INSERT VIEW" => {
                    self.step = Step::ChooseView;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Step::Polygon => match upper.as_str() {
                "A" | "ARC" if !self.polygon.is_empty() => {
                    self.polygon_mode = PolygonMode::Arc;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" if !self.polygon.is_empty() => {
                    self.polygon_mode = PolygonMode::Line;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.polygon.len() >= 3 => Some(self.finish_polygon()),
                "U" | "UNDO" if !self.polygon.is_empty() => {
                    Some(self.undo_polygon())
                }
                _ => None,
            },
            Step::ChooseView => {
                if matches!(upper.as_str(), "N" | "NEW") {
                    self.first = None;
                    self.step = Step::DefineNewFirst;
                    Some(CmdResult::MviewSwitchLayout("Model".to_string()))
                } else {
                    self.select_view(keyword)
                }
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.clip.is_some() && self.step == Step::ClipLength {
            self.step = Step::Polygon;
            return Some(CmdResult::NeedPoint);
        }
        if self.arc_sub != Sub::None {
            self.arc_sub = Sub::None;
            return Some(CmdResult::NeedPoint);
        }
        if self.step == Step::Polygon && !self.polygon.is_empty() {
            Some(self.undo_polygon())
        } else {
            None
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            Step::RectangleSecond => {
                let first = self.first?;
                Self::preview(vec![
                    first,
                    DVec3::new(pt.x, first.y, first.z),
                    DVec3::new(pt.x, pt.y, first.z),
                    DVec3::new(first.x, pt.y, first.z),
                    first,
                ])
            }
            Step::Polygon => self.polygon_preview(pt),
            Step::PlaceView => {
                let viewport = self.placed_viewport(pt)?;
                let half_width = viewport.width / 2.0;
                let half_height = viewport.height / 2.0;
                Self::preview(vec![
                    DVec3::new(pt.x - half_width, pt.y - half_height, pt.z),
                    DVec3::new(pt.x + half_width, pt.y - half_height, pt.z),
                    DVec3::new(pt.x + half_width, pt.y + half_height, pt.z),
                    DVec3::new(pt.x - half_width, pt.y + half_height, pt.z),
                    DVec3::new(pt.x - half_width, pt.y - half_height, pt.z),
                ])
            }
            _ => None,
        }
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["MVIEW", "VPCLIP"] });  // MviewCommand
