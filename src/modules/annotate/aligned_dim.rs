// DIMALIGNED command — aligned dimension (measures true distance between two points).

use acadrust::entities::{Dimension, DimensionAligned};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_aligned.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMALIGNED",
        label: "Aligned",
        icon: ICON,
        event: ModuleEvent::Command("DIMALIGNED".to_string()),
    }
}

enum Step {
    First,
    Second(DVec3),
    DimLine { p1: DVec3, p2: DVec3 },
}

pub struct AlignedDimensionCommand {
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

impl AlignedDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::First,
            plane: WorkingPlane::default(),
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
        }
    }
}

impl CadCommand for AlignedDimensionCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMALIGNED"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMALIGNED  Enter dimension text (blank = measured value):").into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMALIGNED  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::First => t!("DIMALIGNED  Specify first extension line origin:").into_owned(),
            Step::Second(_) => {
                t!("DIMALIGNED  Specify second extension line origin  [Text/Angle]:").into_owned()
            }
            Step::DimLine { .. } => {
                t!("DIMALIGNED  Specify dimension line location  [Text/Angle]:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::First => {
                self.step = Step::Second(pt);
                CmdResult::NeedPoint
            }
            Step::Second(p1) => {
                self.step = Step::DimLine { p1, p2: pt };
                CmdResult::NeedPoint
            }
            Step::DimLine { p1, p2 } => {
                let p1 = self.plane.to_local(p1);
                let p2 = self.plane.to_local(p2);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionAligned::new(v3(p1), v3(p2));
                // The dimension line runs through the cursor: store it as the
                // definition point and let the renderer project it onto the
                // line perpendicular (same as the preview and DIMLINEAR).
                //
                // The old path called `set_offset` with the straight-line
                // p2→cursor *distance*, which `set_offset` then re-applies along
                // the line perpendicular — placing the line at the wrong
                // perpendicular distance and always on the +perp side, so it
                // never matched the preview the user was dragging. (#150)
                dim.definition_point = v3(pt);
                dim.base.definition_point = v3(pt);
                let (d1, d2) = dim_line_endpoints(p1, p2, pt);
                dim.base.text_middle_point = v3((d1 + d2) * 0.5);
                dim.base.insertion_point = dim.base.text_middle_point;
                dim.base.actual_measurement = dim.measurement();
                dim.base.user_text = self.text_override.clone();
                // An explicit text angle overrides the default rotation.
                if let Some(a) = self.text_angle {
                    dim.base.text_rotation = a;
                }
                CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Dimension(
                    Dimension::Aligned(dim),
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // A bare Enter while entering override text/angle accepts the default.
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

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // While entering the override text or angle it is a value, not a point
        // step.
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
            // Blank clears any explicit angle (follow the default again).
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
        let (p1, p2) = match self.step {
            Step::First => return None,
            Step::Second(p1) => (p1, pt),
            Step::DimLine { p1, p2 } => {
                let p1 = self.plane.to_local(p1);
                let p2 = self.plane.to_local(p2);
                let pt = self.plane.to_local(pt);
                let mut preview = preview_aligned(p1, p2, pt);
                preview.points = preview
                    .points
                    .iter()
                    .map(|point| {
                        if point[0].is_nan() {
                            *point
                        } else {
                            self.plane
                                .to_world(DVec3::new(
                                    point[0] as f64,
                                    point[1] as f64,
                                    point[2] as f64,
                                ))
                                .as_vec3()
                                .to_array()
                        }
                    })
                    .collect();
                return Some(preview);
            }
        };
        // Preview WireModel points are screen/GPU-side: downcast to f32.
        let p1 = p1.as_vec3();
        let p2 = p2.as_vec3();
        Some(WireModel {
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
            name: "dimaligned_preview".into(),
            points: vec![[p1.x, p1.y, p1.z], [p2.x, p2.y, p2.z]],
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
        })
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

/// Dimension-line endpoints: the baseline `p1`–`p2` shifted to pass through
/// the cursor's perpendicular projection. Uses the XY-plane perpendicular so
/// it matches the committed entity's renderer (and DIMLINEAR). The old
/// preview used an XZ-plane perpendicular, drawing the offset in the wrong
/// spatial direction. (#150)
fn dim_line_endpoints(p1: DVec3, p2: DVec3, dim_pt: DVec3) -> (DVec3, DVec3) {
    let axis = (p2 - p1).normalize_or_zero();
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let offset = (dim_pt - p1).dot(perp);
    (p1 + perp * offset, p2 + perp * offset)
}

fn preview_aligned(p1: DVec3, p2: DVec3, dim_pt: DVec3) -> WireModel {
    // Show ext lines + dim line.
    let (d1, d2) = dim_line_endpoints(p1, p2, dim_pt);
    // Preview WireModel points are screen/GPU-side: downcast to f32.
    let p1 = p1.as_vec3();
    let p2 = p2.as_vec3();
    let d1 = d1.as_vec3();
    let d2 = d2.as_vec3();
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
        name: "dimaligned_preview".into(),
        points: vec![
            [p1.x, p1.y, p1.z],
            [d1.x, d1.y, d1.z],
            [f32::NAN, 0.0, 0.0],
            [p2.x, p2.y, p2.z],
            [d2.x, d2.y, d2.z],
            [f32::NAN, 0.0, 0.0],
            [d1.x, d1.y, d1.z],
            [d2.x, d2.y, d2.z],
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


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMALIGNED"] });  // AlignedDimensionCommand
