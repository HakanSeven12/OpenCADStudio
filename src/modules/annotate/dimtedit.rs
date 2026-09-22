// DIMTEDIT — reposition, justify, home or rotate existing dimension text.

use acadrust::entities::{AttachmentPointType, Dimension};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_tedit.svg"));

pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("DIMTEDIT")
}

enum Step {
    PickDim,
    PickTextPos {
        handle: Handle,
        entity: Option<EntityType>,
    },
    EnterAngle {
        handle: Handle,
        entity: Option<EntityType>,
    },
}

pub struct DimTeditCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

#[derive(Clone, Copy)]
enum Placement {
    Left,
    Right,
    Center,
    Home,
}

impl DimTeditCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickDim,
            picked_entity: None,
        }
    }

    fn finish_placement(
        handle: Handle,
        entity: &mut Option<EntityType>,
        placement: Placement,
    ) -> CmdResult {
        let Some(mut entity) = entity.take() else {
            return CmdResult::NeedPoint;
        };
        let EntityType::Dimension(dimension) = &mut entity else {
            return CmdResult::Cancel;
        };
        apply_placement(dimension, placement);
        CmdResult::UpdateEntityAndFinish { handle, entity }
    }
}

impl CadCommand for DimTeditCommand {
    fn name(&self) -> &'static str {
        "DIMTEDIT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::PickDim => t!("DIMTEDIT  Select dimension:").into_owned(),
            Step::PickTextPos { .. } => t!(
                "DIMTEDIT  Specify new location for dimension text or [Left/Right/Center/Home/Angle]:"
            )
            .into_owned(),
            Step::EnterAngle { .. } => {
                t!("DIMTEDIT  Specify angle for dimension text:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if !matches!(self.step, Step::PickTextPos { .. }) {
            return Vec::new();
        }
        vec![
            CmdOption::new("Left", "LEFT"),
            CmdOption::new("Right", "RIGHT"),
            CmdOption::new("Center", "CENTER"),
            CmdOption::new("Home", "HOME"),
            CmdOption::new("Angle", "ANGLE"),
        ]
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::EnterAngle { .. } => InputKind::SingleToken,
            Step::PickDim | Step::PickTextPos { .. } => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PickTextPos { .. })
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if !matches!(self.picked_entity.as_ref(), Some(EntityType::Dimension(_))) {
            self.picked_entity = None;
            return CmdResult::ReportError(
                t!("DIMTEDIT: select a dimension.").into_owned(),
            );
        }
        self.step = Step::PickTextPos {
            handle,
            entity: self.picked_entity.take(),
        };
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        match &mut self.step {
            Step::PickTextPos { handle, entity } => match keyword.as_str() {
                "L" | "LEFT" => Some(Self::finish_placement(*handle, entity, Placement::Left)),
                "R" | "RIGHT" => {
                    Some(Self::finish_placement(*handle, entity, Placement::Right))
                }
                "C" | "CENTER" => {
                    Some(Self::finish_placement(*handle, entity, Placement::Center))
                }
                "H" | "HOME" => Some(Self::finish_placement(*handle, entity, Placement::Home)),
                "A" | "ANGLE" => {
                    let handle = *handle;
                    let entity = entity.take();
                    self.step = Step::EnterAngle { handle, entity };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Step::EnterAngle { handle, entity } => {
                let degrees = text.trim().parse::<f64>().ok()?;
                let Some(mut entity) = entity.take() else {
                    return Some(CmdResult::NeedPoint);
                };
                let EntityType::Dimension(dimension) = &mut entity else {
                    return Some(CmdResult::Cancel);
                };
                dimension.base_mut().text_rotation = degrees.to_radians();
                Some(CmdResult::UpdateEntityAndFinish {
                    handle: *handle,
                    entity,
                })
            }
            Step::PickDim => None,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Step::PickTextPos { handle, entity } = &mut self.step else {
            return CmdResult::NeedPoint;
        };
        let Some(mut entity) = entity.take() else {
            return CmdResult::NeedPoint;
        };
        let EntityType::Dimension(dimension) = &mut entity else {
            return CmdResult::Cancel;
        };
        let point = acadrust::types::Vector3::new(point.x, point.y, point.z);
        let base = dimension.base_mut();
        base.text_middle_point = point;
        base.insertion_point = point;
        base.text_user_positioned = true;
        CmdResult::UpdateEntityAndFinish {
            handle: *handle,
            entity,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if !matches!(self.step, Step::PickTextPos { .. }) {
            return None;
        }
        let point = point.as_vec3();
        let d = 0.2_f32;
        Some(WireModel {
            name: "dimtedit_preview".into(),
            points: vec![
                [point.x - d, point.y - d, point.z],
                [point.x + d, point.y - d, point.z],
                [point.x + d, point.y + d, point.z],
                [point.x - d, point.y + d, point.z],
                [point.x - d, point.y - d, point.z],
            ],
            color: WireModel::CYAN,
            ..WireModel::default()
        })
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        match &mut self.step {
            Step::PickTextPos { entity: slot, .. } | Step::EnterAngle { entity: slot, .. } => {
                *slot = Some(entity);
            }
            Step::PickDim => self.picked_entity = Some(entity),
        }
    }
}

fn dimension_line_endpoints(dimension: &Dimension) -> Option<(DVec3, DVec3)> {
    let (first, second, definition, axis) = match dimension {
        Dimension::Linear(value) => (
            value.first_point,
            value.second_point,
            value.definition_point,
            DVec3::new(value.rotation.cos(), value.rotation.sin(), 0.0),
        ),
        Dimension::Aligned(value) => {
            let delta = DVec3::new(
                value.second_point.x - value.first_point.x,
                value.second_point.y - value.first_point.y,
                0.0,
            );
            if delta.length_squared() <= 1.0e-18 {
                return None;
            }
            (
                value.first_point,
                value.second_point,
                value.definition_point,
                delta.normalize(),
            )
        }
        _ => return None,
    };
    let first = DVec3::new(first.x, first.y, first.z);
    let second = DVec3::new(second.x, second.y, second.z);
    let definition = DVec3::new(definition.x, definition.y, definition.z);
    let perpendicular = DVec3::new(-axis.y, axis.x, 0.0);
    let project = |point: DVec3| {
        point + perpendicular * (definition - point).dot(perpendicular)
    };
    let first = project(first);
    let second = project(second);
    if first.dot(axis) <= second.dot(axis) {
        Some((first, second))
    } else {
        Some((second, first))
    }
}

fn apply_placement(dimension: &mut Dimension, placement: Placement) {
    if matches!(placement, Placement::Home) {
        let base = dimension.base_mut();
        base.text_middle_point = acadrust::types::Vector3::ZERO;
        base.insertion_point = acadrust::types::Vector3::ZERO;
        base.text_user_positioned = false;
        base.attachment_point = AttachmentPointType::MiddleCenter;
        return;
    }
    let Some((left, right)) = dimension_line_endpoints(dimension) else {
        return;
    };
    let (point, attachment) = match placement {
        Placement::Left => (left, AttachmentPointType::MiddleLeft),
        Placement::Right => (right, AttachmentPointType::MiddleRight),
        Placement::Center => ((left + right) * 0.5, AttachmentPointType::MiddleCenter),
        Placement::Home => unreachable!(),
    };
    let point = acadrust::types::Vector3::new(point.x, point.y, point.z);
    let base = dimension.base_mut();
    base.text_middle_point = point;
    base.insertion_point = point;
    base.text_user_positioned = true;
    base.attachment_point = attachment;
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMTED", "DIMTEDIT"] });
