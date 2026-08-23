// WIPEOUT command — draw a polygonal mask or derive one from a closed polyline.

use acadrust::entities::{Wipeout, WipeoutClipType};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use cadkernel::geom2d::{polygon_frame, Tolerance};
use cadkernel::space::Plane;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/wipeout.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "WIPEOUT",
        label: "Wipeout",
        icon: ICON,
        event: ModuleEvent::Command("WIPEOUT".to_string()),
    }
}

pub struct WipeoutCommand {
    mode: WipeoutMode,
    first: Option<DVec3>,
    points: Vec<DVec3>,
    plane: WorkingPlane,
    selected_polyline: Option<Handle>,
    frame_mode: i16,
}

#[derive(Clone, Copy, PartialEq)]
enum WipeoutMode {
    Draw,
    Polyline,
    Rectangular,
    Frames,
    ErasePolyline,
}

impl WipeoutCommand {
    pub fn new_polygonal(frame_mode: i16) -> Self {
        Self {
            mode: WipeoutMode::Draw,
            first: None,
            points: Vec::new(),
            plane: WorkingPlane::default(),
            selected_polyline: None,
            frame_mode: frame_mode.clamp(0, 2),
        }
    }

    pub fn new_polyline() -> Self {
        Self {
            mode: WipeoutMode::Polyline,
            first: None,
            points: Vec::new(),
            plane: WorkingPlane::default(),
            selected_polyline: None,
            frame_mode: 1,
        }
    }

    /// Kept for the legacy `WIPEOUT RECTANGULAR` command-line form.
    pub fn new_rectangular() -> Self {
        Self {
            mode: WipeoutMode::Rectangular,
            first: None,
            points: Vec::new(),
            plane: WorkingPlane::default(),
            selected_polyline: None,
            frame_mode: 1,
        }
    }

    fn finish_draw(&self) -> CmdResult {
        let local: Vec<[f64; 2]> = self
            .points
            .iter()
            .map(|point| {
                let point = self.plane.to_local(*point);
                [point.x, point.y]
            })
            .collect();
        let plane = Plane::from_axes(
            self.plane.origin.to_array(),
            self.plane.x.to_array(),
            self.plane.y.to_array(),
        );
        match make_poly_wipeout(&local, plane) {
            Some(entity) => CmdResult::CommitAndExit(entity),
            None => CmdResult::NeedPoint,
        }
    }

    fn undo_point(&mut self) -> CmdResult {
        self.points.pop();
        CmdResult::NeedPoint
    }
}

impl CadCommand for WipeoutCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "WIPEOUT"
    }

    fn prompt(&self) -> String {
        match self.mode {
            WipeoutMode::Draw if self.points.is_empty() => {
                t!("WIPEOUT  Specify first point or [Frames/Polyline] <Polyline>:").into_owned()
            }
            WipeoutMode::Draw => {
                let n = self.points.len();
                t!(
                    "WIPEOUT  Specify next point or [Undo/Close] (%{n} points):",
                    n = n
                )
                .into_owned()
            }
            WipeoutMode::Polyline => {
                t!("WIPEOUT Polyline  Select a closed planar polyline:").into_owned()
            }
            WipeoutMode::Rectangular if self.first.is_none() => {
                t!("WIPEOUT Rectangular  Specify first corner:").into_owned()
            }
            WipeoutMode::Rectangular => {
                t!("WIPEOUT Rectangular  Specify opposite corner:").into_owned()
            }
            WipeoutMode::Frames => t!(
                "WIPEOUT Frames  Enter frame mode [Off/On/DisplayButNotPlot] <%{mode}>:",
                mode = self.frame_mode
            )
            .into_owned(),
            WipeoutMode::ErasePolyline => {
                t!("WIPEOUT Polyline  Erase source polyline? [Yes/No] <No>:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.mode {
            WipeoutMode::Draw if self.points.is_empty() => {
                vec![
                    CmdOption::new(t!("Frames").as_ref(), "F"),
                    CmdOption::new(t!("Polyline").as_ref(), "P"),
                ]
            }
            WipeoutMode::Draw => vec![
                CmdOption::new(t!("Undo").as_ref(), "U"),
                CmdOption::new(t!("Close").as_ref(), "C"),
            ],
            WipeoutMode::Frames => vec![
                CmdOption::new(t!("Off").as_ref(), "OFF"),
                CmdOption::new(t!("On").as_ref(), "ON"),
                CmdOption::new(t!("Display but not plot").as_ref(), "D"),
            ],
            WipeoutMode::ErasePolyline => vec![
                CmdOption::new(t!("Yes").as_ref(), "Y"),
                CmdOption::new(t!("No").as_ref(), "N"),
            ],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.mode {
            WipeoutMode::Draw => {
                if let Some(first) = self.points.first() {
                    let first = self.plane.to_local(*first);
                    let point = self.plane.to_local(point);
                    let distance = cadkernel::geom2d::Vec2::new(point.x, point.y)
                        .distance(cadkernel::geom2d::Vec2::new(first.x, first.y));
                    if self.points.len() >= 3
                        && distance <= Tolerance::default().linear()
                    {
                        return self.finish_draw();
                    }
                }
                self.points.push(point);
                CmdResult::NeedPoint
            }
            WipeoutMode::Rectangular => {
                if let Some(first) = self.first {
                    let first = self.plane.to_local(first);
                    let point = self.plane.to_local(point);
                    let plane = Plane::from_axes(
                        self.plane.origin.to_array(),
                        self.plane.x.to_array(),
                        self.plane.y.to_array(),
                    );
                    make_rect_wipeout(first, point, plane)
                        .map_or(CmdResult::NeedPoint, CmdResult::CommitAndExit)
                } else {
                    self.first = Some(point);
                    CmdResult::NeedPoint
                }
            }
            WipeoutMode::Polyline | WipeoutMode::Frames | WipeoutMode::ErasePolyline => {
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.mode {
            WipeoutMode::Draw if self.points.is_empty() => {
                self.mode = WipeoutMode::Polyline;
                CmdResult::NeedPoint
            }
            WipeoutMode::Draw if self.points.len() >= 3 => self.finish_draw(),
            WipeoutMode::Frames => {
                CmdResult::Dispatch(format!("WIPEOUTFRAME {}", self.frame_mode))
            }
            WipeoutMode::ErasePolyline => self.finish_polyline(false),
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn needs_entity_pick(&self) -> bool {
        self.mode == WipeoutMode::Polyline
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        if handle.is_null() {
            CmdResult::NeedPoint
        } else {
            self.selected_polyline = Some(handle);
            self.mode = WipeoutMode::ErasePolyline;
            CmdResult::NeedPoint
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.mode,
            WipeoutMode::Draw | WipeoutMode::Frames | WipeoutMode::ErasePolyline
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.mode,
            WipeoutMode::Draw | WipeoutMode::Frames | WipeoutMode::ErasePolyline
        )
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim().to_ascii_uppercase();
        match self.mode {
            WipeoutMode::Draw => match text.as_str() {
                "F" | "FRAMES" if self.points.is_empty() => {
                    self.mode = WipeoutMode::Frames;
                    Some(CmdResult::NeedPoint)
                }
                "P" | "POLYLINE" if self.points.is_empty() => {
                    self.mode = WipeoutMode::Polyline;
                    Some(CmdResult::NeedPoint)
                }
                "U" | "UNDO" if !self.points.is_empty() => Some(self.undo_point()),
                "C" | "CLOSE" if self.points.len() >= 3 => Some(self.finish_draw()),
                _ => None,
            },
            WipeoutMode::Frames => match text.as_str() {
                "0" | "OFF" => Some(CmdResult::Dispatch("WIPEOUTFRAME 0".into())),
                "1" | "ON" => Some(CmdResult::Dispatch("WIPEOUTFRAME 1".into())),
                "2" | "D" | "DISPLAYBUTNOTPLOT" => {
                    Some(CmdResult::Dispatch("WIPEOUTFRAME 2".into()))
                }
                _ => None,
            },
            WipeoutMode::ErasePolyline => match text.as_str() {
                "Y" | "YES" => Some(self.finish_polyline(true)),
                "N" | "NO" => Some(self.finish_polyline(false)),
                _ => None,
            },
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.mode == WipeoutMode::Draw && !self.points.is_empty() {
            Some(self.undo_point())
        } else {
            None
        }
    }

    fn window_corner_pick(&self) -> bool {
        self.mode == WipeoutMode::Rectangular
    }

    fn window_first_corner(&self) -> Option<DVec3> {
        (self.mode == WipeoutMode::Rectangular)
            .then_some(self.first)
            .flatten()
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match self.mode {
            WipeoutMode::Draw => {
                let first = *self.points.first()?;
                let mut preview = self.points.clone();
                preview.push(point);
                preview.push(first);
                Some(WireModel::solid_f64(
                    "wipeout_preview".into(),
                    preview
                        .iter()
                        .map(|point| [point.x, point.y, point.z])
                        .collect(),
                    WireModel::CYAN,
                    false,
                ))
            }
            WipeoutMode::Rectangular => {
                let first = self.first?;
                let corners = {
                    let first_local = self.plane.to_local(first);
                    let point_local = self.plane.to_local(point);
                    [
                        first_local,
                        DVec3::new(point_local.x, first_local.y, first_local.z),
                        DVec3::new(point_local.x, point_local.y, first_local.z),
                        DVec3::new(first_local.x, point_local.y, first_local.z),
                        first_local,
                    ]
                    .map(|corner| self.plane.to_world(corner))
                };
                Some(WireModel::solid_f64(
                    "wipeout_preview".into(),
                    corners.iter().map(|p| [p.x, p.y, p.z]).collect(),
                    WireModel::CYAN,
                    false,
                ))
            }
            WipeoutMode::Polyline | WipeoutMode::Frames | WipeoutMode::ErasePolyline => None,
        }
    }
}

impl WipeoutCommand {
    fn finish_polyline(&self, erase_source: bool) -> CmdResult {
        self.selected_polyline.map_or(CmdResult::Cancel, |handle| {
            CmdResult::WipeoutFromPolyline {
                handle,
                erase_source,
            }
        })
    }
}

pub(crate) fn wipeout_frame_mode(document: &CadDocument) -> i16 {
    crate::scene::frame::mode(document, crate::scene::frame::FrameKind::Wipeout)
}

fn vector(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

fn make_rect_wipeout(first: DVec3, second: DVec3, plane: Plane) -> Option<EntityType> {
    plane.normal()?;
    let points = [
        [first.x, first.y],
        [second.x, first.y],
        [second.x, second.y],
        [first.x, second.y],
    ];
    let frame = polygon_frame(&points, Tolerance::default())?;
    let mut wipeout = Wipeout::new();
    wipeout.insertion_point = vector(plane.point_at(frame.origin));
    wipeout.u_vector = vector(plane.x_axis) * frame.size[0];
    wipeout.v_vector = vector(plane.y_axis) * frame.size[1];
    Some(EntityType::Wipeout(wipeout))
}

fn make_poly_wipeout(points: &[[f64; 2]], plane: Plane) -> Option<EntityType> {
    plane.normal()?;
    let frame = polygon_frame(points, Tolerance::default())?;
    let [width, height] = frame.size;
    let mut wipeout = Wipeout::new();
    wipeout.insertion_point = vector(plane.point_at(frame.origin));
    wipeout.u_vector = vector(plane.x_axis) * width;
    wipeout.v_vector = vector(plane.y_axis) * height;
    wipeout.size = Vector2::new(1.0, 1.0);
    wipeout.clip_type = WipeoutClipType::Polygonal;
    wipeout.clip_boundary_vertices = frame
        .points
        .iter()
        .map(|point| {
            Vector2::new(
                (point[0] - frame.origin[0]) / width - 0.5,
                0.5 - (point[1] - frame.origin[1]) / height,
            )
        })
        .collect();
    wipeout.clipping_enabled = true;
    Some(EntityType::Wipeout(wipeout))
}

fn explicitly_closed(points: &[[f64; 2]]) -> bool {
    points.len() >= 4
        && cadkernel::geom2d::Vec2::from(points[0])
            .distance(cadkernel::geom2d::Vec2::from(*points.last().unwrap()))
            <= Tolerance::default().linear()
}

/// Build a wipeout boundary from a picked closed polyline without consuming
/// the source entity. Only straight, closed 2D polygonal boundaries qualify.
pub(crate) fn wipeout_from_polyline(entity: &EntityType) -> Option<EntityType> {
    fn from_ocs(points: &[[f64; 2]], normal: Vector3, elevation: f64) -> Option<EntityType> {
        make_poly_wipeout(points, crate::entities::curve::ocs_plane(normal, elevation))
    }

    match entity {
        EntityType::LwPolyline(polyline) => {
            if polyline.vertices.iter().any(|vertex| vertex.bulge != 0.0) {
                return None;
            }
            let raw: Vec<[f64; 2]> = polyline
                .vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y])
                .collect();
            if !polyline.is_closed && !explicitly_closed(&raw) {
                return None;
            }
            from_ocs(&raw, polyline.normal, polyline.elevation)
        }
        EntityType::Polyline2D(polyline) => {
            if polyline.vertices.iter().any(|vertex| vertex.bulge != 0.0) {
                return None;
            }
            let raw: Vec<[f64; 2]> = polyline
                .vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y])
                .collect();
            if !polyline.is_closed() && !explicitly_closed(&raw) {
                return None;
            }
            from_ocs(&raw, polyline.normal, polyline.elevation)
        }
        _ => return None,
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["WIPEOUT"] });
