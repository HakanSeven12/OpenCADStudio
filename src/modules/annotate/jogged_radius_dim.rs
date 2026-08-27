use acadrust::entities::{Dimension, DimensionLargeRadial};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use glam::{DVec3, Vec3};

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, DimensionAssociationSource,
    WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::creation_style::DimensionCreationDefaults;
use crate::scene::dimension_assoc::RadialSourceGeometry;
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_jog.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMJOGGED",
        label: "Jogged Radius",
        icon: ICON,
        event: ModuleEvent::Command("DIMJOGGED".to_string()),
    }
}

#[derive(Clone, Copy)]
enum Step {
    SelectObject,
    OverrideCenter(RadialSourceGeometry),
    DimLine {
        source: RadialSourceGeometry,
        override_center: DVec3,
    },
    Jog {
        source: RadialSourceGeometry,
        override_center: DVec3,
        chord: DVec3,
        text_position: DVec3,
    },
}

pub struct JoggedRadiusDimensionCommand {
    step: Step,
    defaults: DimensionCreationDefaults,
    editor_height: f64,
    text_override: Option<String>,
    text_angle: Option<f64>,
    awaiting_text: bool,
    awaiting_angle: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
}

impl JoggedRadiusDimensionCommand {
    pub fn new(defaults: DimensionCreationDefaults, annotation_multiplier: f64) -> Self {
        let display_scale = if defaults.annotative {
            annotation_multiplier.max(f64::EPSILON)
        } else {
            defaults.scale
        };
        let editor_height = defaults.text_height * display_scale;
        Self {
            step: Step::SelectObject,
            defaults,
            editor_height,
            text_override: None,
            text_angle: None,
            awaiting_text: false,
            awaiting_angle: false,
            picked_entity: None,
            source_handle: None,
        }
    }

    fn editor_anchor(&self) -> DVec3 {
        match self.step {
            Step::SelectObject => DVec3::ZERO,
            Step::OverrideCenter(source) => dvec(source.point_at_angle(source.start_angle)),
            Step::DimLine {
                override_center, ..
            }
            | Step::Jog {
                override_center, ..
            } => override_center,
        }
    }

    fn commit_dimension(
        &self,
        source: RadialSourceGeometry,
        override_center_world: DVec3,
        chord_world: DVec3,
        text_world: DVec3,
        jog_world: DVec3,
    ) -> CmdResult {
        let plane = source_plane(source);
        let center = plane.to_local(dvec(source.center_world()));
        let chord = plane.to_local(chord_world);
        let override_center = plane.to_local(override_center_world);
        let jog = project_jog(override_center, chord, plane.to_local(jog_world));
        let text_position = plane.to_local(text_world);

        let mut dimension = DimensionLargeRadial::default();
        dimension.definition_point = v3(center);
        dimension.base.definition_point = v3(center);
        dimension.chord_point = v3(chord);
        dimension.override_center = v3(override_center);
        dimension.jog_point = v3(jog);
        dimension.jog_angle = self.defaults.jog_angle;
        dimension.base.style_name.clone_from(&self.defaults.style_name);
        dimension.base.text_middle_point = v3(text_position);
        dimension.base.insertion_point = v3(text_position);
        dimension.base.text_user_positioned = true;
        dimension.base.actual_measurement = dimension.measurement();
        crate::entities::dimension::set_dimension_text_override(
            &mut dimension.base,
            self.text_override.clone(),
        );
        if let Some(angle) = self.text_angle {
            dimension.base.text_rotation = angle;
        }

        let association = self.source_handle.map_or_else(
            || DimensionAssociationInput::Explicit(Vec::new()),
            |handle| {
                DimensionAssociationInput::Explicit(vec![Some(
                    DimensionAssociationSource::explicit(
                        handle,
                        source.marker,
                        source.angle_at(chord_world.to_array()),
                    ),
                )])
            },
        );

        CmdResult::CommitDimension {
            entity: plane.place_entity(EntityType::Dimension(Dimension::LargeRadial(
                dimension,
            ))),
            association,
        }
    }
}

impl CadCommand for JoggedRadiusDimensionCommand {
    fn set_working_plane(&mut self, _plane: WorkingPlane) {}

    fn name(&self) -> &'static str {
        "DIMJOGGED"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMJOGGED  Enter dimension text (blank = measured value):").into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMJOGGED  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::SelectObject => {
                t!("DIMJOGGED  Select arc, circle, or polyline arc:").into_owned()
            }
            Step::OverrideCenter(_) => {
                t!("DIMJOGGED  Specify center location override:").into_owned()
            }
            Step::DimLine { .. } => t!(
                "DIMJOGGED  Specify dimension line location  [Mtext/Text/Angle]:"
            )
            .into_owned(),
            Step::Jog { .. } => t!("DIMJOGGED  Specify jog location:").into_owned(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::SelectObject => CmdResult::NeedPoint,
            Step::OverrideCenter(source) => {
                self.step = Step::DimLine {
                    source,
                    override_center: point,
                };
                CmdResult::NeedPoint
            }
            Step::DimLine {
                source,
                override_center,
            } => {
                self.step = Step::Jog {
                    source,
                    override_center,
                    chord: dvec(source.chord_at(point.to_array())),
                    text_position: point,
                };
                CmdResult::NeedPoint
            }
            Step::Jog {
                source,
                override_center,
                chord,
                text_position,
            } => self.commit_dimension(
                source,
                override_center,
                chord,
                text_position,
                point,
            ),
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

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.awaiting_text && !self.awaiting_angle
    }

    fn wants_text_with_spaces(&self) -> bool {
        self.awaiting_text
    }

    fn options(&self) -> Vec<CmdOption> {
        if matches!(self.step, Step::DimLine { .. })
            && !self.awaiting_text
            && !self.awaiting_angle
        {
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
        if !matches!(self.step, Step::DimLine { .. }) {
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
                height: self.editor_height,
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
        self.step = Step::OverrideCenter(source);
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match self.step {
            Step::SelectObject => None,
            Step::OverrideCenter(source) => Some(preview_wire(vec![
                dvec(source.center_world()).as_vec3(),
                point.as_vec3(),
            ])),
            Step::DimLine {
                source,
                override_center,
            } => {
                let chord = dvec(source.chord_at(point.to_array()));
                Some(preview_wire(vec![
                    override_center.as_vec3(),
                    chord.as_vec3(),
                    Vec3::splat(f32::NAN),
                    chord.as_vec3(),
                    point.as_vec3(),
                ]))
            }
            Step::Jog {
                source,
                override_center,
                chord,
                ..
            } => {
                let plane = source_plane(source);
                let chord_local = plane.to_local(chord);
                let override_local = plane.to_local(override_center);
                let jog_local = project_jog(override_local, chord_local, plane.to_local(point));
                let (near, far) = jog_break(
                    chord_local,
                    jog_local,
                    override_local,
                    self.defaults.jog_angle,
                );
                Some(preview_wire(
                    [chord_local, near, far, override_local]
                        .into_iter()
                        .map(|position| plane.to_world(position).as_vec3())
                        .collect(),
                ))
            }
        }
    }
}

fn source_plane(source: RadialSourceGeometry) -> WorkingPlane {
    WorkingPlane::new(
        DVec3::from_array(source.plane.origin),
        DVec3::from_array(source.plane.x_axis),
        DVec3::from_array(source.plane.y_axis),
    )
}

fn project_jog(override_center: DVec3, chord: DVec3, point: DVec3) -> DVec3 {
    let line = chord - override_center;
    let length_squared = line.length_squared();
    if length_squared <= 1e-18 {
        return override_center;
    }
    let factor = ((point - override_center).dot(line) / length_squared).clamp(0.0, 1.0);
    override_center + line * factor
}

fn jog_break(
    chord: DVec3,
    jog: DVec3,
    override_center: DVec3,
    jog_angle: f64,
) -> (DVec3, DVec3) {
    let radial = (chord - override_center).normalize_or_zero();
    let (sin, cos) = jog_angle.sin_cos();
    let transverse = DVec3::new(
        radial.x * cos - radial.y * sin,
        radial.x * sin + radial.y * cos,
        0.0,
    )
    .normalize_or_zero();
    let half = ((chord - override_center).length() * 0.04).max(1e-3);
    let first = jog - transverse * half;
    let second = jog + transverse * half;
    if chord.distance_squared(first) <= chord.distance_squared(second) {
        (first, second)
    } else {
        (second, first)
    }
}

fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
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
        name: "dimjogged_preview".to_string(),
        points: points.into_iter().map(|point| point.to_array()).collect(),
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: Vec::new(),
        tangent_geoms: Vec::new(),
        aci: 0,
        key_vertices: Vec::new(),
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: Vec::new(),
        fill_tris_low: Vec::new(),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMJOGGED", "DIMJOG"]
});
