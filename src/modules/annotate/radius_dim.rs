use acadrust::entities::{Dimension, DimensionRadius};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::{DVec3, Vec3};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_radius.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMRADIUS",
        label: "Radius",
        icon: ICON,
        event: ModuleEvent::Command("DIMRADIUS".to_string()),
    }
}

enum Step {
    SelectObject,
    DimLine(crate::scene::dimension_assoc::RadialSourceGeometry),
}

pub struct RadiusDimensionCommand {
    step: Step,
    /// Optional text that replaces the measured value (None = measurement).
    text_override: Option<String>,
    /// True while the next typed line is captured as the text override.
    awaiting_text: bool,
    /// Explicit text rotation in radians (None = follow the UCS/style).
    text_angle: Option<f64>,
    /// True while the next typed value is captured as the text angle.
    awaiting_angle: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
    mtext_override: bool,
}

impl RadiusDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::SelectObject,
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
            picked_entity: None,
            source_handle: None,
            mtext_override: false,
        }
    }
}

impl CadCommand for RadiusDimensionCommand {
    fn set_working_plane(&mut self, _plane: WorkingPlane) {
    }

    fn name(&self) -> &'static str {
        "DIMRADIUS"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return if self.mtext_override {
                t!("DIMRADIUS  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMRADIUS  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return t!("DIMRADIUS  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::SelectObject => t!("DIMRADIUS  Select arc, circle, or polyline arc:").into_owned(),
            Step::DimLine(_) => {
                t!("DIMRADIUS  Specify dimension line location  [Mtext/Text/Angle]:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::SelectObject => CmdResult::NeedPoint,
            Step::DimLine(source) => {
                let source_plane = WorkingPlane::new(
                    DVec3::from_array(source.plane.origin),
                    DVec3::from_array(source.plane.x_axis),
                    DVec3::from_array(source.plane.y_axis),
                );
                let center_world = dvec(source.center_world());
                let point_world = dvec(source.chord_at(pt.to_array()));
                let center = source_plane.to_local(center_world);
                let point = source_plane.to_local(point_world);
                let pt = source_plane.to_local(pt);
                let mut dim = DimensionRadius::new(v3(center), v3(point));
                dim.base.definition_point = v3(point);
                dim.base.text_middle_point = v3(pt);
                dim.base.insertion_point = v3(pt);
                dim.base.text_user_positioned = true;
                dim.leader_length = point.distance(pt);
                dim.base.actual_measurement = dim.measurement();
                crate::entities::dimension::set_dimension_text_override(
                    &mut dim.base,
                    self.text_override.clone(),
                );
                // An explicit text angle overrides the default rotation.
                if let Some(a) = self.text_angle {
                    dim.base.text_rotation = a;
                }
                CmdResult::CommitDimension {
                    entity: source_plane.place_entity(EntityType::Dimension(Dimension::Radius(dim))),
                    source: self.source_handle,
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // A bare Enter while entering override text accepts the measured value.
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
        // While entering the override text or angle it is a value, not a point step.
        !self.awaiting_text && !self.awaiting_angle
    }

    fn wants_text_with_spaces(&self) -> bool {
        // The override text may contain spaces.
        self.awaiting_text
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.awaiting_text {
            let t = text.trim();
            // Blank (or the "<>" placeholder) keeps the measured value.
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
            // Blank clears any explicit angle (follow the UCS/style again).
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
        match text.trim().to_uppercase().as_str() {
            "T" | "TEXT" => {
                self.mtext_override = false;
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "M" | "MTEXT" => {
                self.mtext_override = true;
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "A" | "ANGLE" => {
                self.awaiting_angle = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
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
                let center = dvec(source.center_world()).as_vec3();
                let point = dvec(source.chord_at(pt.to_array())).as_vec3();
                Some(preview_wire(vec![
                center,
                point,
                Vec3::new(f32::NAN, f32::NAN, f32::NAN),
                point,
                pt.as_vec3(),
            ]))
            }
        }
    }
}

fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
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
        name: "dimradius_preview".to_string(),
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
inventory::submit!(crate::command::CommandRegistration { names: &["DIMRADIUS"] });  // RadiusDimensionCommand
