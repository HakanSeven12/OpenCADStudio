// DIMJOGLINE — add or remove a jog on a linear/aligned dimension.

use acadrust::entities::Dimension;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_jog.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMJOGLINE",
        label: "Jog Line",
        icon: ICON,
        event: ModuleEvent::Command("DIMJOGLINE".to_string()),
    }
}

enum Step {
    PickDim,
    PickJogPos { handle: Handle },
}

pub struct DimJogLineCommand {
    step: Step,
    remove: bool,
    picked_entity: Option<EntityType>,
    dimension: Option<Dimension>,
}

impl DimJogLineCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickDim,
            remove: false,
            picked_entity: None,
            dimension: None,
        }
    }

    fn result(handle: Handle, point: Option<DVec3>) -> CmdResult {
        CmdResult::EditDimensionJog {
            dimension: handle,
            point,
        }
    }

    fn default_position(dimension: &Dimension) -> Option<DVec3> {
        let (first, second, definition, axis) = match dimension {
            Dimension::Linear(value) => (
                value.first_point,
                value.second_point,
                value.definition_point,
                DVec3::new(value.rotation.cos(), value.rotation.sin(), 0.0),
            ),
            Dimension::Aligned(value) => {
                let first = DVec3::new(
                    value.first_point.x,
                    value.first_point.y,
                    value.first_point.z,
                );
                let second = DVec3::new(
                    value.second_point.x,
                    value.second_point.y,
                    value.second_point.z,
                );
                (
                    value.first_point,
                    value.second_point,
                    value.definition_point,
                    (second - first).try_normalize().unwrap_or(DVec3::X),
                )
            }
            _ => return None,
        };
        let base = dimension.base();
        let text = base.text_middle_point;
        let normal = base.normal;
        let text = if base.text_user_positioned {
            [text.x, text.y, text.z]
        } else {
            [f64::NAN; 3]
        };
        cadkernel::space::default_dimension_jog_position(
            [first.x, first.y, first.z],
            [second.x, second.y, second.z],
            [definition.x, definition.y, definition.z],
            axis.to_array(),
            [normal.x, normal.y, normal.z],
            text,
        )
        .map(DVec3::from_array)
    }
}

impl CadCommand for DimJogLineCommand {
    fn name(&self) -> &'static str {
        "DIMJOGLINE"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::PickDim if self.remove => {
                t!("DIMJOGLINE  Select dimension to remove jog:").into_owned()
            }
            Step::PickDim => {
                t!("DIMJOGLINE  Select dimension to add jog or [Remove]:").into_owned()
            }
            Step::PickJogPos { .. } => {
                t!("DIMJOGLINE  Specify jog location (or press Enter):").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if matches!(self.step, Step::PickDim) && !self.remove {
            vec![CmdOption::new("Remove", "REMOVE")]
        } else {
            Vec::new()
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PickDim) && !self.remove
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        let Some(EntityType::Dimension(dimension @ (Dimension::Linear(_) | Dimension::Aligned(_)))) =
            self.picked_entity.take()
        else {
            return CmdResult::ReportError(
                t!("DIMJOGLINE: select a linear or aligned dimension.").into_owned(),
            );
        };
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if self.remove {
            return Self::result(handle, None);
        }
        self.dimension = Some(dimension);
        self.step = Step::PickJogPos { handle };
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if matches!(self.step, Step::PickDim)
            && matches!(text.trim().to_ascii_uppercase().as_str(), "R" | "REMOVE")
        {
            self.remove = true;
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::PickJogPos { handle } => Self::result(handle, Some(point)),
            Step::PickDim => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match (&self.step, &self.dimension) {
            (Step::PickJogPos { handle }, Some(dimension)) => Self::default_position(dimension)
                .map(|point| Self::result(*handle, Some(point)))
                .unwrap_or(CmdResult::Cancel),
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if !matches!(self.step, Step::PickJogPos { .. }) {
            return None;
        }
        let dimension = self.dimension.as_ref()?;
        let (segment, normal) = match dimension {
            Dimension::Linear(value) => {
                let axis = DVec3::new(value.rotation.cos(), value.rotation.sin(), 0.0);
                (
                    [(point - axis).to_array(), (point + axis).to_array()],
                    [
                        value.base.normal.x,
                        value.base.normal.y,
                        value.base.normal.z,
                    ],
                )
            }
            Dimension::Aligned(value) => (
                [
                    [
                        value.first_point.x,
                        value.first_point.y,
                        value.first_point.z,
                    ],
                    [
                        value.second_point.x,
                        value.second_point.y,
                        value.second_point.z,
                    ],
                ],
                [
                    value.base.normal.x,
                    value.base.normal.y,
                    value.base.normal.z,
                ],
            ),
            _ => return None,
        };
        let points = cadkernel::space::dimension_jog_points(
            segment,
            point.to_array(),
            normal,
            0.3,
            std::f64::consts::FRAC_PI_2,
        )?;
        let mut preview = WireModel::default();
        preview.name = "dimjog_preview".into();
        preview.points = points
            .into_iter()
            .map(|point| point.map(|value| value as f32))
            .collect();
        preview.color = WireModel::CYAN;
        preview.line_weight_px = 1.2;
        Some(preview)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMJOGLINE"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jog_result_keeps_the_selected_point() {
        let point = DVec3::new(1.0, 2.0, 3.0);
        let CmdResult::EditDimensionJog {
            dimension,
            point: actual,
        } = DimJogLineCommand::result(Handle::from(7), Some(point))
        else {
            panic!("expected dimension jog edit");
        };
        assert_eq!(dimension, Handle::from(7));
        assert_eq!(actual, Some(point));
    }
}
