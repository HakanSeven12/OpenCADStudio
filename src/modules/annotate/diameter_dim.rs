use acadrust::entities::{Dimension, DimensionDiameter};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use glam::{DVec3, Vec3};

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, DimensionPreview, InputKind,
    WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_diameter.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMDIAMETER",
        label: "Diameter",
        icon: ICON,
        event: ModuleEvent::Command("DIMDIAMETER".to_string()),
    }
}

enum Step {
    SelectObject,
    DimLine(crate::scene::dimension_assoc::RadialSourceGeometry),
}

pub struct DiameterDimensionCommand {
    step: Step,
    text_override: Option<String>,
    awaiting_text: bool,
    text_angle: Option<f64>,
    awaiting_angle: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
}

impl DiameterDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::SelectObject,
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
            picked_entity: None,
            source_handle: None,
        }
    }

    fn editor_anchor(&self) -> DVec3 {
        match self.step {
            Step::SelectObject => DVec3::ZERO,
            Step::DimLine(source) => dvec(source.point_at_angle(source.start_angle)),
        }
    }

    fn build_dimension(
        &self,
        source: crate::scene::dimension_assoc::RadialSourceGeometry,
        world: DVec3,
    ) -> EntityType {
        let plane = WorkingPlane::new(
            DVec3::from_array(source.plane.origin),
            DVec3::from_array(source.plane.x_axis),
            DVec3::from_array(source.plane.y_axis),
        );
        let chord = plane.to_local(dvec(source.chord_at(world.to_array())));
        let far_chord = plane.to_local(dvec(source.opposite_chord_at(world.to_array())));
        let point = plane.to_local(world);
        let mut dim = DimensionDiameter::new(v3(chord), v3(far_chord));
        dim.base.text_middle_point = v3(point);
        dim.base.insertion_point = v3(point);
        dim.base.text_user_positioned = true;
        dim.leader_length = chord.distance(point);
        dim.base.actual_measurement = dim.measurement();
        crate::entities::dimension::set_dimension_text_override(
            &mut dim.base,
            self.text_override.clone(),
        );
        if let Some(angle) = self.text_angle {
            dim.base.text_rotation = angle;
        }
        plane.place_entity(EntityType::Dimension(Dimension::Diameter(dim)))
    }
}

impl CadCommand for DiameterDimensionCommand {
    fn set_working_plane(&mut self, _plane: WorkingPlane) {
    }

    fn name(&self) -> &'static str {
        "DIMDIAMETER"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMDIAMETER  Enter dimension text (blank = measured value):")
                .into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMDIAMETER  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::SelectObject => {
                t!("DIMDIAMETER  Select arc, circle, or polyline arc:").into_owned()
            }
            Step::DimLine(_) => {
                t!("DIMDIAMETER  Specify dimension line location  [Mtext/Text/Angle]:")
                    .into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::SelectObject => CmdResult::NeedPoint,
            Step::DimLine(source) => {
                CmdResult::CommitDimension {
                    entity: self.build_dimension(source, pt),
                    association: DimensionAssociationInput::Infer(self.source_handle),
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

    /// Points and object picks may come through a paper-space viewport;
    /// the committed dimension then reports the model measurement.
    fn measures_through_viewports(&self) -> bool {
        true
    }

    fn dimension_acquired_points(&self) -> Vec<DVec3> {
        match self.step {
            Step::SelectObject => vec![],
            Step::DimLine(source) => vec![dvec(source.point_at_angle(source.start_angle))],
        }
    }

    fn dimension_placement_pending(&self) -> bool {
        matches!(self.step, Step::DimLine(_))
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn input_kind(&self) -> InputKind {
        if self.awaiting_text {
            InputKind::FreeText
        } else if self.awaiting_angle {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.awaiting_text && !self.awaiting_angle
    }

    fn options(&self) -> Vec<CmdOption> {
        if matches!(self.step, Step::DimLine(_)) && !self.awaiting_text && !self.awaiting_angle {
            vec![
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
            let t = text.trim();
            self.text_override = if t.is_empty() || t == "<>" {
                None
            } else {
                Some(t.to_string())
            };
            self.awaiting_text = false;
            return Some(CmdResult::NeedPoint);
        }
        if self.awaiting_angle {
            let t = text.trim();
            self.text_angle = if t.is_empty() {
                None
            } else {
                crate::entities::common::parse_typed_angle(t)
            };
            self.awaiting_angle = false;
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.step, Step::DimLine(_)) {
            return None;
        }
        match text.trim().to_ascii_uppercase().as_str() {
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

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::SelectObject)
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(source) = crate::scene::dimension_assoc::radial_source_at(
            &entity,
            Vector3::new(point.x, point.y, point.z),
        ) else {
            return CmdResult::NeedPoint;
        };
        if !source.radius.is_finite() || source.radius <= 1e-12 {
            return CmdResult::NeedPoint;
        }
        self.source_handle = Some(handle);
        self.step = Step::DimLine(source);
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            Step::SelectObject => None,
            Step::DimLine(source) => {
                let chord = dvec(source.chord_at(pt.to_array())).as_vec3();
                let far_chord = dvec(source.opposite_chord_at(pt.to_array())).as_vec3();
                Some(preview_line(far_chord, chord, pt.as_vec3()))
            }
        }
    }

    fn dimension_preview(&self, cursor: DVec3) -> Option<Vec<DimensionPreview>> {
        let Step::DimLine(source) = self.step else {
            return None;
        };
        Some(vec![DimensionPreview::current_style(self.build_dimension(source, cursor))])
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn preview_line(far_chord: Vec3, chord: Vec3, text: Vec3) -> WireModel {
    let separator = [f32::NAN; 3];
    WireModel {
        bg_adapt: None,
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
        name: "dimdia_preview".into(),
        points: vec![
            far_chord.to_array(),
            chord.to_array(),
            separator,
            chord.to_array(),
            text.to_array(),
        ],
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

inventory::submit!(crate::command::CommandRegistration { names: &["DIMDIAMETER"] });
