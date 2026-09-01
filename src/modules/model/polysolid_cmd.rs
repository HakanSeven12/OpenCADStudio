use std::sync::{Mutex, OnceLock};

use acadrust::entities::{LwPolyline, LwVertex, Solid3D};
use acadrust::types::Vector2;
use acadrust::{EntityType, Handle};
use glam::{DVec2, DVec3};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::draw::draw::polyline::{
    arc_sample_points, compute_bulge, seg_exit_tangent,
};
use crate::scene::model::solid_model;
use crate::scene::model::sweep_model::{self, PolysolidJustification};
use crate::scene::model::wire_model::WireModel;
use crate::t;

#[derive(Clone, Copy)]
struct Defaults {
    height: f64,
    width: f64,
    justification: PolysolidJustification,
}

fn defaults() -> &'static Mutex<Defaults> {
    static DEFAULTS: OnceLock<Mutex<Defaults>> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        Mutex::new(Defaults {
            height: 80.0,
            width: 5.0,
            justification: PolysolidJustification::Center,
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Height,
    Width,
    Justify,
    Object,
    Line,
    Arc,
    ArcDirection,
    ArcSecond,
    ArcEnd,
}

pub struct PolysolidCommand {
    step: Step,
    return_step: Step,
    plane: WorkingPlane,
    vertices: Vec<DVec3>,
    bulges: Vec<f64>,
    height: f64,
    width: f64,
    justification: PolysolidJustification,
    pending_direction: Option<DVec2>,
    pending_second: Option<DVec3>,
    picked: Option<EntityType>,
    preselected: Option<(Handle, EntityType)>,
}

impl PolysolidCommand {
    pub fn new(preselected: Option<(Handle, EntityType)>) -> Self {
        let saved = defaults().lock().map(|value| *value).unwrap_or(Defaults {
            height: 80.0,
            width: 5.0,
            justification: PolysolidJustification::Center,
        });
        Self {
            step: Step::Start,
            return_step: Step::Start,
            plane: WorkingPlane::default(),
            vertices: Vec::new(),
            bulges: Vec::new(),
            height: saved.height,
            width: saved.width,
            justification: saved.justification,
            pending_direction: None,
            pending_second: None,
            picked: None,
            preselected,
        }
    }

    fn remember(&self) {
        if let Ok(mut saved) = defaults().lock() {
            *saved = Defaults {
                height: self.height,
                width: self.width,
                justification: self.justification,
            };
        }
    }

    fn supported(entity: &EntityType) -> bool {
        matches!(
            entity,
            EntityType::Line(_)
                | EntityType::Arc(_)
                | EntityType::Circle(_)
                | EntityType::Ellipse(_)
                | EntityType::LwPolyline(_)
                | EntityType::Spline(_)
        )
    }

    fn path_entity(&self, closed: bool) -> Option<EntityType> {
        self.path_entity_from(&self.vertices, &self.bulges, closed)
    }

    fn path_entity_from(
        &self,
        vertices: &[DVec3],
        bulges: &[f64],
        closed: bool,
    ) -> Option<EntityType> {
        if vertices.len() < 2 {
            return None;
        }
        let local = vertices
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect::<Vec<_>>();
        let mut path = LwPolyline::new();
        path.vertices = local
            .iter()
            .enumerate()
            .map(|(index, point)| {
                let mut vertex = LwVertex::new(Vector2::new(point.x, point.y));
                vertex.bulge = bulges.get(index).copied().unwrap_or(0.0);
                vertex
            })
            .collect();
        path.elevation = local[0].z;
        path.is_closed = closed;
        Some(self.plane.place_entity(EntityType::LwPolyline(path)))
    }

    fn commit(&self, path: EntityType, erase_source: Option<Handle>) -> CmdResult {
        let Some((solid, history)) = sweep_model::polysolid(
            &path,
            self.width,
            self.height,
            self.justification,
        ) else {
            return CmdResult::NeedPoint;
        };
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&solid) else {
            return CmdResult::Cancel;
        };
        let mut entity = Solid3D::new();
        entity.set_sat_document(&document);
        entity.wires = solid_model::edge_wires(&solid);
        CmdResult::CommitSolid {
            entity: EntityType::Solid3D(entity),
            solid: Box::new(solid),
            history,
            erase_source,
        }
    }

    fn finish(&self, closed: bool) -> CmdResult {
        self.path_entity(closed)
            .map(|path| self.commit(path, None))
            .unwrap_or(CmdResult::Cancel)
    }

    fn close(&mut self) -> CmdResult {
        if self.vertices.len() < 3 {
            return CmdResult::NeedPoint;
        }
        if self.step == Step::Arc {
            let last = *self.vertices.last().unwrap();
            let first = self.vertices[0];
            let bulge = compute_bulge(
                self.plane.to_local(last).truncate(),
                self.pending_direction.unwrap_or_else(|| self.last_tangent()),
                self.plane.to_local(first).truncate(),
            );
            if let Some(value) = self.bulges.last_mut() {
                *value = bulge;
            }
        }
        self.finish(true)
    }

    fn undo(&mut self) -> CmdResult {
        if self.vertices.pop().is_some() {
            self.bulges.pop();
            if let Some(last) = self.bulges.last_mut() {
                *last = 0.0;
            }
        }
        self.pending_direction = None;
        self.pending_second = None;
        if self.vertices.is_empty() {
            self.step = Step::Start;
        } else if matches!(self.step, Step::Arc | Step::ArcDirection | Step::ArcSecond | Step::ArcEnd)
        {
            self.step = Step::Arc;
        } else {
            self.step = Step::Line;
        }
        CmdResult::NeedPoint
    }

    fn last_tangent(&self) -> DVec2 {
        let count = self.vertices.len();
        if count < 2 {
            return DVec2::X;
        }
        let a = self.plane.to_local(self.vertices[count - 2]);
        let b = self.plane.to_local(self.vertices[count - 1]);
        seg_exit_tangent(a, b, self.bulges[count - 2])
            .map(|value| value.as_dvec2())
            .unwrap_or(DVec2::X)
    }

    fn add_segment(&mut self, point: DVec3, bulge: f64) -> CmdResult {
        if let Some(last) = self.bulges.last_mut() {
            *last = bulge;
        }
        self.vertices.push(point);
        self.bulges.push(0.0);
        self.pending_direction = None;
        self.pending_second = None;
        CmdResult::NeedPoint
    }

    fn three_point_bulge(&self, start: DVec3, middle: DVec3, end: DVec3) -> f64 {
        let a3 = self.plane.to_local(start);
        let m3 = self.plane.to_local(middle);
        let b3 = self.plane.to_local(end);
        let a = DVec2::new(a3.x, a3.y);
        let m = DVec2::new(m3.x, m3.y);
        let b = DVec2::new(b3.x, b3.y);
        let determinant = 2.0
            * (a.x * (m.y - b.y) + m.x * (b.y - a.y) + b.x * (a.y - m.y));
        if determinant.abs() <= 1e-12 {
            return 0.0;
        }
        let aa = a.length_squared();
        let mm = m.length_squared();
        let bb = b.length_squared();
        let center = DVec2::new(
            (aa * (m.y - b.y) + mm * (b.y - a.y) + bb * (a.y - m.y))
                / determinant,
            (aa * (b.x - m.x) + mm * (a.x - b.x) + bb * (m.x - a.x))
                / determinant,
        );
        let start_angle = (a - center).y.atan2((a - center).x);
        let middle_angle = (m - center).y.atan2((m - center).x);
        let end_angle = (b - center).y.atan2((b - center).x);
        let ccw = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
        let middle_ccw =
            (middle_angle - start_angle).rem_euclid(std::f64::consts::TAU);
        let sweep = if middle_ccw <= ccw + 1e-12 {
            ccw
        } else {
            ccw - std::f64::consts::TAU
        };
        (sweep / 4.0).tan()
    }

    fn value_step(&mut self, step: Step) {
        self.return_step = if self.vertices.is_empty() {
            Step::Start
        } else {
            self.step
        };
        self.step = step;
    }

    fn path_preview(&self, cursor: DVec3) -> Option<WireModel> {
        let start = *self.vertices.last()?;
        let start_local = self.plane.to_local(start);
        let cursor_local = self.plane.to_local(cursor);
        let points = match self.step {
            Step::Arc => {
                let bulge = compute_bulge(
                    start_local.truncate(),
                    self.pending_direction.unwrap_or_else(|| self.last_tangent()),
                    cursor_local.truncate(),
                );
                arc_sample_points(start_local.as_vec3(), bulge, cursor_local.as_vec3(), 24)
                    .into_iter()
                    .map(|point| self.plane.to_world(DVec3::new(
                        point[0] as f64,
                        point[1] as f64,
                        point[2] as f64,
                    )).to_array())
                    .collect()
            }
            Step::ArcEnd => {
                let middle = self.pending_second?;
                let bulge = self.three_point_bulge(start, middle, cursor);
                arc_sample_points(start_local.as_vec3(), bulge, cursor_local.as_vec3(), 24)
                    .into_iter()
                    .map(|point| self.plane.to_world(DVec3::new(
                        point[0] as f64,
                        point[1] as f64,
                        point[2] as f64,
                    )).to_array())
                    .collect()
            }
            Step::ArcDirection => vec![start.to_array(), cursor.to_array()],
            _ => vec![start.to_array(), cursor.to_array()],
        };
        Some(WireModel::solid_f64(
            "POLYSOLID_PREVIEW".to_string(),
            points,
            WireModel::CYAN,
            false,
        ))
    }

    fn solid_preview(&self, cursor: DVec3) -> Option<Vec<WireModel>> {
        if !matches!(self.step, Step::Line | Step::Arc | Step::ArcEnd) {
            return None;
        }
        let start = *self.vertices.last()?;
        if (cursor - start).length_squared() <= 1e-12 {
            return None;
        }

        let mut vertices = self.vertices.clone();
        let mut bulges = self.bulges.clone();
        let segment_bulge = match self.step {
            Step::Arc => compute_bulge(
                self.plane.to_local(start).truncate(),
                self.pending_direction.unwrap_or_else(|| self.last_tangent()),
                self.plane.to_local(cursor).truncate(),
            ),
            Step::ArcEnd => {
                self.three_point_bulge(start, self.pending_second?, cursor)
            }
            _ => 0.0,
        };
        if let Some(last) = bulges.last_mut() {
            *last = segment_bulge;
        }
        vertices.push(cursor);
        bulges.push(0.0);

        let path = self.path_entity_from(&vertices, &bulges, false)?;
        let (solid, _) = sweep_model::polysolid(
            &path,
            self.width,
            self.height,
            self.justification,
        )?;
        let previews = solid_model::edge_wires(&solid)
            .into_iter()
            .enumerate()
            .filter_map(|(index, wire)| {
                (wire.points.len() >= 2).then(|| {
                    WireModel::solid_f64(
                        format!("POLYSOLID_PREVIEW_{index}"),
                        wire.points
                            .into_iter()
                            .map(|point| [point.x, point.y, point.z])
                            .collect(),
                        WireModel::CYAN,
                        false,
                    )
                })
            })
            .collect::<Vec<_>>();
        (!previews.is_empty()).then_some(previews)
    }
}

impl CadCommand for PolysolidCommand {
    fn name(&self) -> &'static str {
        "POLYSOLID"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Start => format!(
                "POLYSOLID  Height = {:.4}, Width = {:.4}, Justification = {}\n{}",
                self.height,
                self.width,
                match self.justification {
                    PolysolidJustification::Left => "Left",
                    PolysolidJustification::Center => "Center",
                    PolysolidJustification::Right => "Right",
                },
                t!("Specify start point or [Object/Height/Width/Justify] <Object>:")
            ),
            Step::Height => format!("Specify height <{:.4}>:", self.height),
            Step::Width => format!("Specify width <{:.4}>:", self.width),
            Step::Justify => format!(
                "Enter justification [Left/Center/Right] <{}>:",
                match self.justification {
                    PolysolidJustification::Left => "Left",
                    PolysolidJustification::Center => "Center",
                    PolysolidJustification::Right => "Right",
                }
            ),
            Step::Object => t!("Select object:").into_owned(),
            Step::Line if self.vertices.len() >= 3 => {
                t!("Specify next point or [Arc/Close/Undo]:").into_owned()
            }
            Step::Line => t!("Specify next point or [Arc/Undo]:").into_owned(),
            Step::Arc if self.vertices.len() >= 3 => {
                t!("Specify endpoint of arc or [Close/Direction/Line/Second point/Undo]:")
                    .into_owned()
            }
            Step::Arc => {
                t!("Specify endpoint of arc or [Direction/Line/Second point/Undo]:").into_owned()
            }
            Step::ArcDirection => {
                t!("Specify the tangent direction for the start point of arc:").into_owned()
            }
            Step::ArcSecond => t!("Specify second point on arc:").into_owned(),
            Step::ArcEnd => t!("Specify end point of arc:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Start => vec![
                CmdOption::new(t!("Object").as_ref(), "O"),
                CmdOption::new(t!("Height").as_ref(), "H"),
                CmdOption::new(t!("Width").as_ref(), "W"),
                CmdOption::new(t!("Justify").as_ref(), "J"),
            ],
            Step::Line => {
                let mut options = vec![CmdOption::new(t!("Arc").as_ref(), "A")];
                if self.vertices.len() >= 3 {
                    options.push(CmdOption::new(t!("Close").as_ref(), "C"));
                }
                options.push(CmdOption::new(t!("Undo").as_ref(), "U"));
                options
            }
            Step::Arc => {
                let mut options = Vec::new();
                if self.vertices.len() >= 3 {
                    options.push(CmdOption::new(t!("Close").as_ref(), "C"));
                }
                options.extend([
                    CmdOption::new(t!("Direction").as_ref(), "D"),
                    CmdOption::new(t!("Line").as_ref(), "L"),
                    CmdOption::new(t!("Second point").as_ref(), "S"),
                    CmdOption::new(t!("Undo").as_ref(), "U"),
                ]);
                options
            }
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Start => {
                self.vertices.push(point);
                self.bulges.push(0.0);
                self.step = Step::Line;
                CmdResult::NeedPoint
            }
            Step::Line => self.add_segment(point, 0.0),
            Step::Arc => {
                let start = *self.vertices.last().unwrap_or(&point);
                let a = self.plane.to_local(start).truncate();
                let b = self.plane.to_local(point).truncate();
                let tangent = self.pending_direction.unwrap_or_else(|| self.last_tangent());
                let bulge = compute_bulge(a, tangent, b);
                self.add_segment(point, bulge)
            }
            Step::ArcDirection => {
                let start = *self.vertices.last().unwrap_or(&point);
                let direction = self.plane.to_local(point) - self.plane.to_local(start);
                if direction.truncate().length_squared() > 1e-12 {
                    self.pending_direction = Some(direction.truncate().normalize());
                    self.step = Step::Arc;
                }
                CmdResult::NeedPoint
            }
            Step::ArcSecond => {
                self.pending_second = Some(point);
                self.step = Step::ArcEnd;
                CmdResult::NeedPoint
            }
            Step::ArcEnd => {
                let start = *self.vertices.last().unwrap_or(&point);
                let middle = self.pending_second.unwrap_or(point);
                let bulge = self.three_point_bulge(start, middle, point);
                self.step = Step::Arc;
                self.add_segment(point, bulge)
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Start => {
                if let Some((handle, entity)) = self.preselected.take() {
                    return self.commit(entity, Some(handle));
                }
                self.step = Step::Object;
                CmdResult::NeedPoint
            }
            Step::Line | Step::Arc if self.vertices.len() >= 2 => self.finish(false),
            Step::Height | Step::Width | Step::Justify => {
                self.remember();
                self.step = self.return_step;
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Object | Step::ArcDirection | Step::ArcSecond | Step::ArcEnd)
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start | Step::Line | Step::Arc)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.to_uppercase();
        match self.step {
            Step::Height => {
                let number = crate::entities::common::parse_length(value)?;
                if number > 0.0 && number.is_finite() {
                    self.height = number;
                    self.remember();
                    self.step = self.return_step;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::Width => {
                let number = crate::entities::common::parse_length(value)?;
                if number > 0.0 && number.is_finite() {
                    self.width = number;
                    self.remember();
                    self.step = self.return_step;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::Justify => {
                self.justification = match keyword.as_str() {
                    "L" | "LEFT" => PolysolidJustification::Left,
                    "C" | "CENTER" => PolysolidJustification::Center,
                    "R" | "RIGHT" => PolysolidJustification::Right,
                    _ => return None,
                };
                self.remember();
                self.step = self.return_step;
                Some(CmdResult::NeedPoint)
            }
            Step::Start => match keyword.as_str() {
                "O" | "OBJECT" => {
                    self.step = Step::Object;
                    Some(CmdResult::NeedPoint)
                }
                "H" | "HEIGHT" => {
                    self.value_step(Step::Height);
                    Some(CmdResult::NeedPoint)
                }
                "W" | "WIDTH" => {
                    self.value_step(Step::Width);
                    Some(CmdResult::NeedPoint)
                }
                "J" | "JUSTIFY" => {
                    self.value_step(Step::Justify);
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Step::Line => match keyword.as_str() {
                "A" | "ARC" => {
                    self.step = Step::Arc;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.vertices.len() >= 3 => Some(self.close()),
                "U" | "UNDO" => Some(self.undo()),
                _ => None,
            },
            Step::Arc => match keyword.as_str() {
                "D" | "DIRECTION" => {
                    self.step = Step::ArcDirection;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" => {
                    self.step = Step::Line;
                    Some(CmdResult::NeedPoint)
                }
                "S" | "SECOND" | "SECOND POINT" => {
                    self.step = Step::ArcSecond;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.vertices.len() >= 3 => Some(self.close()),
                "U" | "UNDO" => Some(self.undo()),
                _ => None,
            },
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        (!self.vertices.is_empty()).then(|| self.undo())
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Object
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.step == Step::Object
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        let Some(entity) = self.picked.take() else {
            return CmdResult::NeedPoint;
        };
        if !Self::supported(&entity) {
            return CmdResult::NeedPoint;
        }
        self.commit(entity, Some(handle))
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        self.path_preview(point)
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        self.solid_preview(point)
            .unwrap_or_else(|| self.path_preview(point).into_iter().collect())
    }
}
