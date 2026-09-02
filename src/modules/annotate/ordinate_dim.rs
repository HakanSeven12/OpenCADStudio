use acadrust::entities::{Dimension, DimensionOrdinate};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Vec3};

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, InputKind, WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMORDINATE",
        label: "Ordinate",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/dim_ordinate.svg")),
        event: ModuleEvent::Command("DIMORDINATE".to_string()),
    }
}

enum Step {
    FeaturePoint,
    LeaderEndpoint { feature: DVec3 },
}

#[derive(Clone, Copy)]
enum DatumMode {
    Automatic,
    X,
    Y,
}

pub struct OrdinateDimCommand {
    step: Step,
    plane: WorkingPlane,
    datum_mode: DatumMode,
    text_override: Option<String>,
    awaiting_text: bool,
    text_angle: Option<f64>,
    awaiting_angle: bool,
}

impl OrdinateDimCommand {
    pub fn new() -> Self {
        Self {
            step: Step::FeaturePoint,
            plane: WorkingPlane::default(),
            datum_mode: DatumMode::Automatic,
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
        }
    }

    fn editor_anchor(&self) -> DVec3 {
        match self.step {
            Step::FeaturePoint => DVec3::ZERO,
            Step::LeaderEndpoint { feature } => feature,
        }
    }
}

impl CadCommand for OrdinateDimCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMORDINATE"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMORDINATE  Enter dimension text (blank = measured value):")
                .into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMORDINATE  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::FeaturePoint => t!("DIMORDINATE  Specify feature location:").into_owned(),
            Step::LeaderEndpoint { .. } => t!(
                "DIMORDINATE  Specify leader endpoint [Xdatum/Ydatum/Mtext/Text/Angle]:"
            )
            .into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::FeaturePoint => {
                self.step = Step::LeaderEndpoint { feature: pt };
                CmdResult::NeedPoint
            }
            Step::LeaderEndpoint { feature } => {
                let feature = self.plane.to_local(feature);
                let pt = self.plane.to_local(pt);
                let is_x = match self.datum_mode {
                    DatumMode::Automatic => is_x_type(feature, pt),
                    DatumMode::X => true,
                    DatumMode::Y => false,
                };
                let mut dim = DimensionOrdinate::new(v3(feature), v3(pt), is_x);
                crate::entities::dimension::set_dimension_text_override(
                    &mut dim.base,
                    self.text_override.clone(),
                );
                if let Some(angle) = self.text_angle {
                    dim.base.text_rotation = angle;
                }
                dim.refresh_measurement();
                CmdResult::CommitDimension {
                    entity: self.plane.place_entity(EntityType::Dimension(
                        Dimension::Ordinate(dim),
                    )),
                    association: DimensionAssociationInput::Infer(None),
                    preserve_base_style: false,
                    continue_command: false,
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.awaiting_text {
            self.awaiting_text = false;
            return CmdResult::NeedPoint;
        }
        if self.awaiting_angle {
            self.awaiting_angle = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn input_kind(&self) -> InputKind {
        if self.awaiting_text {
            InputKind::FreeText
        } else {
            InputKind::SingleToken
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.awaiting_text && !self.awaiting_angle
    }

    fn options(&self) -> Vec<CmdOption> {
        if matches!(self.step, Step::LeaderEndpoint { .. })
            && !self.awaiting_text
            && !self.awaiting_angle
        {
            vec![
                CmdOption::new("Xdatum", "XDATUM"),
                CmdOption::new("Ydatum", "YDATUM"),
                CmdOption::new("MText", "MTEXT"),
                CmdOption::new("Text", "TEXT"),
                CmdOption::new("Angle", "ANGLE"),
            ]
        } else {
            Vec::new()
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.awaiting_text {
            let value = text.trim();
            self.text_override = if value.is_empty() || value == "<>" {
                None
            } else {
                Some(value.to_string())
            };
            self.awaiting_text = false;
            return Some(CmdResult::NeedPoint);
        }
        if self.awaiting_angle {
            let value = text.trim();
            self.text_angle = if value.is_empty() {
                None
            } else {
                crate::entities::common::parse_typed_angle(value)
            };
            self.awaiting_angle = false;
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.step, Step::LeaderEndpoint { .. }) {
            return None;
        }
        match text.trim().to_ascii_uppercase().as_str() {
            "X" | "XDATUM" => {
                self.datum_mode = DatumMode::X;
                Some(CmdResult::NeedPoint)
            }
            "Y" | "YDATUM" => {
                self.datum_mode = DatumMode::Y;
                Some(CmdResult::NeedPoint)
            }
            "T" | "TEXT" => {
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "M" | "MTEXT" => Some(CmdResult::SuspendForMTextInput {
                pos: self.editor_anchor(),
                initial: self.text_override.clone().unwrap_or_default(),
                height: 2.5,
            }),
            "A" | "ANGLE" => {
                self.awaiting_angle = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_editor_text(&mut self, value: String) {
        let value = value.trim();
        self.text_override = if value.is_empty() || value == "<>" {
            None
        } else {
            Some(value.to_string())
        };
    }

    fn on_editor_closed(&mut self, _committed: bool) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let feature = match self.step {
            Step::LeaderEndpoint { feature } => feature,
            _ => return None,
        };
        let feature = self.plane.to_local(feature);
        let pt = self.plane.to_local(pt);
        let is_x = match self.datum_mode {
            DatumMode::Automatic => is_x_type(feature, pt),
            DatumMode::X => true,
            DatumMode::Y => false,
        };
        let dim = DimensionOrdinate::new(v3(feature), v3(pt), is_x);
        let points = dim.leader_polyline(0.44, 0.0, None);
        Some(preview_wire(
            points
                .into_iter()
                .map(|point| {
                    self.plane
                        .to_world(DVec3::new(point.x, point.y, point.z))
                        .as_vec3()
                })
                .collect(),
        ))
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn is_x_type(feature: DVec3, leader: DVec3) -> bool {
    let dx = (leader.x - feature.x).abs();
    let dy = (leader.y - feature.y).abs();
    dy > dx
}

fn preview_wire(points: Vec<Vec3>) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name: "dimordinate_preview".into(),
        points: points.into_iter().map(|p| [p.x, p.y, p.z]).collect(),
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        aci: 0,
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMORDINATE"] });  // OrdinateDimCommand
