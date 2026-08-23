use acadrust::entities::{Dimension, DimensionRadius};
use acadrust::types::Vector3;
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
    CenterPoint,
    RadiusPoint(DVec3),
    TextPoint { center: DVec3, point: DVec3 },
}

pub struct RadiusDimensionCommand {
    step: Step,
    plane: WorkingPlane,
    /// Optional text that replaces the measured value (None = measurement).
    text_override: Option<String>,
    /// True while the next typed line is captured as the text override.
    awaiting_text: bool,
    /// Explicit text rotation in radians (None = follow the UCS/style).
    text_angle: Option<f64>,
    /// True while the next typed value is captured as the text angle.
    awaiting_angle: bool,
}

impl RadiusDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::CenterPoint,
            plane: WorkingPlane::default(),
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
        }
    }
}

impl CadCommand for RadiusDimensionCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMRADIUS"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMRADIUS  Enter dimension text (blank = measured value):").into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMRADIUS  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::CenterPoint => t!("DIMRADIUS  Specify center point:").into_owned(),
            Step::RadiusPoint(_) => t!("DIMRADIUS  Specify radius point:").into_owned(),
            Step::TextPoint { .. } => {
                t!("DIMRADIUS  Specify dimension line location  [Text/Angle]:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::CenterPoint => {
                self.step = Step::RadiusPoint(pt);
                CmdResult::NeedPoint
            }
            Step::RadiusPoint(center) => {
                self.step = Step::TextPoint { center, point: pt };
                CmdResult::NeedPoint
            }
            Step::TextPoint { center, point } => {
                let center = self.plane.to_local(center);
                let point = self.plane.to_local(point);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionRadius::new(v3(center), v3(point));
                dim.base.definition_point = v3(point);
                dim.base.text_middle_point = v3(pt);
                dim.base.insertion_point = v3(pt);
                dim.leader_length = point.distance(pt);
                dim.base.actual_measurement = dim.measurement();
                dim.base.user_text = self.text_override.clone();
                // An explicit text angle overrides the default rotation.
                if let Some(a) = self.text_angle {
                    dim.base.text_rotation = a;
                }
                CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Dimension(
                    Dimension::Radius(dim),
                )))
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
        match text.trim().to_uppercase().as_str() {
            "T" | "TEXT" | "M" | "MTEXT" => {
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

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let pt = pt.as_vec3();
        match self.step {
            Step::CenterPoint => None,
            Step::RadiusPoint(center) => Some(preview_wire(vec![center.as_vec3(), pt])),
            Step::TextPoint { center, point } => Some(preview_wire(vec![
                center.as_vec3(),
                point.as_vec3(),
                Vec3::new(f32::NAN, f32::NAN, f32::NAN),
                point.as_vec3(),
                pt,
            ])),
        }
    }
}

fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
}

fn preview_wire(points: Vec<Vec3>) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
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
