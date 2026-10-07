// POINTCLOUDCROP — crop a point cloud to a rectangle, polygon or circle.
// POINTCLOUDUNCROP — `Select point cloud:` and all its crops go.
//
//   Select point cloud:
//   Specify first corner point or [Polygon/Circular]:
//     (with crops: [Polygon/Circular/Invert/Remove last/ON/OFF])
//   Specify other corner point:
//   Specify first point:  /  Specify next point or [Undo]:      (Polygon)
//   Specify center point:  /  Specify radius:                   (Circular)
//   Keep points inside or outside? [Inside/Outside] <Inside>:
//
// Crops are kept in the cloud's own coordinates, on the view plane (its
// right and up directions) and running along the view: a rectangle as its
// four corners, a polygon as its points, a circle as its centre and a point
// on it. They add up; Invert flips them,
// Remove last drops the newest, ON / OFF shows or hides them.

use std::sync::Mutex;

use codec::entities::{ExtendedEntity, ExtendedEntityData, PointCloudExCrop, PointCloudExData};
use codec::types::{Handle, Vector3};
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    First,
    RectSecond,
    PolyFirst,
    PolyNext,
    Center,
    Radius,
    Keep,
}

/// The crop being drawn: its type and points in the drawing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Rectangle = 1,
    Polygon = 2,
    Circle = 3,
}

/// Inside or outside, as last chosen: the next crop offers it.
static LAST_INSIDE: Mutex<bool> = Mutex::new(true);

fn last_inside() -> bool {
    LAST_INSIDE.lock().map(|v| *v).unwrap_or(true)
}

const REQUIRED: &str = "Point or option keyword required.";

pub struct PointCloudCropCommand {
    step: Step,
    handle: Handle,
    cloud: Option<ExtendedEntity>,
    picked: Option<EntityType>,
    shape: Shape,
    points: Vec<DVec3>,
    /// POINTCLOUDUNCROP: the picked cloud loses its crops.
    uncrop: bool,
    /// The view's right and up directions in the drawing.
    view: (DVec3, DVec3),
}

impl PointCloudCropCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Select,
            handle: Handle::NULL,
            cloud: None,
            picked: None,
            shape: Shape::Rectangle,
            points: Vec::new(),
            uncrop: false,
            view: (DVec3::X, DVec3::Y),
        }
    }

    pub fn uncrop() -> Self {
        Self { uncrop: true, ..Self::new() }
    }

    /// The toolbar's crops: the cloud already chosen, the shape too
    /// ("P" polygon, "C" circle, "" rectangle).
    pub fn for_cloud(handle: Handle, cloud: ExtendedEntity, option: &str) -> Self {
        let mut command = Self::new();
        command.handle = handle;
        command.cloud = Some(cloud);
        command.step = Step::First;
        if !option.is_empty() {
            command.option(option);
        }
        command
    }

    fn data(&self) -> Option<&PointCloudExData> {
        match &self.cloud.as_ref()?.data {
            ExtendedEntityData::PointCloudEx(data) => Some(data),
            _ => None,
        }
    }

    fn has_crops(&self) -> bool {
        self.data().is_some_and(|data| !data.croppings.is_empty())
    }

    /// The cloud with `change` made, and the command done.
    fn finish(&self, change: impl FnOnce(&mut PointCloudExData)) -> CmdResult {
        let Some(mut cloud) = self.cloud.clone() else {
            return CmdResult::Cancel;
        };
        if let ExtendedEntityData::PointCloudEx(data) = &mut cloud.data {
            change(data);
        }
        CmdResult::UpdateEntityAndFinish {
            handle: self.handle,
            entity: EntityType::Extended(Box::new(cloud)),
        }
    }

    fn axes(data: &PointCloudExData) -> glam::DMat3 {
        let v = |a: Vector3| DVec3::new(a.x, a.y, a.z);
        glam::DMat3::from_cols(v(data.ucs_x_direction), v(data.ucs_y_direction), v(data.ucs_z_direction))
    }

    /// A point of the drawing in the cloud's own coordinates.
    fn to_local(data: &PointCloudExData, p: DVec3) -> DVec3 {
        let origin = data.ucs_origin;
        Self::axes(data).inverse() * (p - DVec3::new(origin.x, origin.y, origin.z))
    }

    /// The view's right and up directions in the cloud's own coordinates.
    fn view_local(&self, data: &PointCloudExData) -> (DVec3, DVec3) {
        let inverse = Self::axes(data).inverse();
        ((inverse * self.view.0).normalize_or(DVec3::X), (inverse * self.view.1).normalize_or(DVec3::Y))
    }

    fn option(&mut self, text: &str) -> CmdResult {
        let crops = self.has_crops();
        match text.trim().to_ascii_uppercase().as_str() {
            "P" | "POLYGON" => {
                self.shape = Shape::Polygon;
                self.step = Step::PolyFirst;
                CmdResult::NeedPoint
            }
            "C" | "CIRCULAR" => {
                self.shape = Shape::Circle;
                self.step = Step::Center;
                CmdResult::NeedPoint
            }
            "I" | "INVERT" if crops => self.finish(|data| {
                for crop in &mut data.croppings {
                    crop.inverted = !crop.inverted;
                }
            }),
            "R" | "REMOVE" | "REMOVE LAST" if crops => self.finish(|data| {
                data.croppings.pop();
            }),
            "ON" if crops => self.finish(|data| data.show_cropping = true),
            "OFF" if crops => self.finish(|data| data.show_cropping = false),
            _ => CmdResult::ReportError(REQUIRED.to_string()),
        }
    }

    fn keep(&mut self, text: &str) -> CmdResult {
        let inside = match text.trim().to_ascii_uppercase().as_str() {
            "" => last_inside(),
            "I" | "INSIDE" => true,
            "O" | "OUTSIDE" => false,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        if let Ok(mut last) = LAST_INSIDE.lock() {
            *last = inside;
        }
        let Some(data) = self.data() else {
            return CmdResult::Cancel;
        };
        let (right, up) = self.view_local(data);
        let local: Vec<DVec3> = self.points.iter().map(|p| Self::to_local(data, *p)).collect();
        let local = match self.shape {
            // The corners along the view's right and up directions.
            Shape::Rectangle => {
                let (a, d) = (local[0], local[1] - local[0]);
                let (dx, dy) = (right * d.dot(right), up * d.dot(up));
                vec![a, a + dx, a + dx + dy, a + dy]
            }
            _ => local,
        };
        let vector = |v: DVec3| Vector3::new(v.x, v.y, v.z);
        let crop = PointCloudExCrop {
            crop_type: self.shape as i16,
            inside,
            inverted: false,
            plane: Vector3::ZERO,
            x_direction: vector(right),
            y_direction: vector(up),
            points: local.into_iter().map(vector).collect(),
        };
        self.finish(move |data| {
            data.croppings.push(crop);
            data.show_cropping = true;
        })
    }

    fn to_keep(&mut self) -> CmdResult {
        self.step = Step::Keep;
        CmdResult::NeedPoint
    }
}

impl CadCommand for PointCloudCropCommand {
    fn name(&self) -> &'static str {
        if self.uncrop { "POINTCLOUDUNCROP" } else { "POINTCLOUDCROP" }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select point cloud:".to_string(),
            Step::First if self.has_crops() => {
                "Specify first corner point or [Polygon/Circular/Invert/Remove last/ON/OFF]:".to_string()
            }
            Step::First => "Specify first corner point or [Polygon/Circular]:".to_string(),
            Step::RectSecond => "Specify other corner point:".to_string(),
            Step::PolyFirst => "Specify first point:".to_string(),
            Step::PolyNext => "Specify next point or [Undo]:".to_string(),
            Step::Center => "Specify center point:".to_string(),
            Step::Radius => "Specify radius:".to_string(),
            Step::Keep => format!(
                "Keep points inside or outside? [Inside/Outside] <{}>:",
                if last_inside() { "Inside" } else { "Outside" }
            ),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::First => {
                let mut options = vec![CmdOption::new("Polygon", "P"), CmdOption::new("Circular", "C")];
                if self.has_crops() {
                    options.extend([
                        CmdOption::new("Invert", "I"),
                        CmdOption::new("Remove last", "R"),
                        CmdOption::new("ON", "ON"),
                        CmdOption::new("OFF", "OFF"),
                    ]);
                }
                options
            }
            Step::PolyNext => vec![CmdOption::new("Undo", "U")],
            Step::Keep => vec![CmdOption::new("Inside", "I"), CmdOption::new("Outside", "O")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Keep => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::First | Step::PolyNext | Step::Radius)
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Select
    }

    fn wants_point_pick_context(&self) -> bool {
        !matches!(self.step, Step::Select | Step::Keep)
    }

    fn set_point_pick_context(&mut self, context: Option<crate::command::PointPickContext>) {
        // The projection's first two rows run along the view's right and up.
        if let Some(context) = context {
            let row = |i: usize| context.view.row(i).truncate().as_dvec3().normalize_or_zero();
            let (right, up) = (row(0), row(1));
            if right != DVec3::ZERO && up != DVec3::ZERO {
                self.view = (right, up);
            }
        }
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::Extended(cloud)) if matches!(cloud.data, ExtendedEntityData::PointCloudEx(_)) => {
                self.handle = handle;
                self.cloud = Some(*cloud);
                if self.uncrop {
                    return self.finish(|data| data.croppings.clear());
                }
                self.step = Step::First;
                CmdResult::NeedPoint
            }
            // Anything else: the prompt again.
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::First => {
                self.points = vec![pt];
                self.step = Step::RectSecond;
                CmdResult::NeedPoint
            }
            Step::RectSecond => {
                self.points.push(pt);
                self.to_keep()
            }
            Step::PolyFirst => {
                self.points = vec![pt];
                self.step = Step::PolyNext;
                CmdResult::NeedPoint
            }
            Step::PolyNext => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            Step::Center => {
                self.points = vec![pt];
                self.step = Step::Radius;
                CmdResult::NeedPoint
            }
            Step::Radius => {
                self.points.push(pt);
                self.to_keep()
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::First => self.option(text),
            Step::PolyNext => match text.trim().to_ascii_uppercase().as_str() {
                "U" | "UNDO" if self.points.len() > 1 => {
                    self.points.pop();
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError(REQUIRED.to_string()),
            },
            // A radius typed: a point that far along the X axis.
            Step::Radius => match crate::entities::common::parse_f64(text).filter(|r| *r > 0.0) {
                Some(radius) => {
                    let center = self.points[0];
                    self.points.push(center + DVec3::new(radius, 0.0, 0.0));
                    self.to_keep()
                }
                None => CmdResult::ReportError(REQUIRED.to_string()),
            },
            Step::Keep => self.keep(text),
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // A polygon of three points or more is closed by Enter.
            Step::PolyNext if self.points.len() >= 3 => self.to_keep(),
            Step::Keep => self.keep(""),
            Step::Select => CmdResult::Cancel,
            _ => CmdResult::ReportError(REQUIRED.to_string()),
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let outline: Vec<[f64; 3]> = match self.step {
            Step::RectSecond => {
                let a = self.points[0];
                [a, DVec3::new(pt.x, a.y, a.z), DVec3::new(pt.x, pt.y, a.z), DVec3::new(a.x, pt.y, a.z), a]
                    .map(|p| p.to_array())
                    .to_vec()
            }
            Step::PolyNext => self
                .points
                .iter()
                .copied()
                .chain([pt, self.points[0]])
                .map(|p| p.to_array())
                .collect(),
            Step::Radius => {
                let c = self.points[0];
                let r = (pt - c).truncate().length();
                (0..=64)
                    .map(|k| {
                        let a = k as f64 / 64.0 * std::f64::consts::TAU;
                        (c + DVec3::new(r * a.cos(), r * a.sin(), 0.0)).to_array()
                    })
                    .collect()
            }
            _ => return None,
        };
        Some(WireModel::solid_f64("pc_crop_boundary".into(), outline, WireModel::CYAN, false))
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["POINTCLOUDCROP", "POINTCLOUDUNCROP"] });
