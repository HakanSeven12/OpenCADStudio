// DIMALIGNED command — aligned dimension (measures true distance between two points).

use acadrust::entities::{Dimension, DimensionAligned};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, DimensionAssociationInput, WorkingPlane};
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
    selecting_object: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
    mtext_override: bool,
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
            selecting_object: false,
            picked_entity: None,
            source_handle: None,
            mtext_override: false,
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
            return if self.mtext_override {
                t!("DIMALIGNED  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMALIGNED  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return t!("DIMALIGNED  Specify text angle (degrees):").into_owned();
        }
        if self.selecting_object {
            return t!("DIMALIGNED  Select object to dimension:").into_owned();
        }
        match self.step {
            Step::First => t!(
                "DIMALIGNED  Specify first extension line origin or press Enter to select object:"
            )
            .into_owned(),
            Step::Second(_) => {
                t!("DIMALIGNED  Specify second extension line origin:").into_owned()
            }
            Step::DimLine { .. } => {
                t!("DIMALIGNED  Specify dimension line location  [Mtext/Text/Angle]:").into_owned()
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
                if pt.distance_squared(p1) <= 1e-24 {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::DimLine { p1, p2: pt };
                CmdResult::NeedPoint
            }
            Step::DimLine { p1, p2 } => {
                let p1 = self.plane.to_local(p1);
                let p2 = self.plane.to_local(p2);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionAligned::new(v3(p1), v3(p2));
                // Store the cursor position so commit matches the preview.
                dim.definition_point = v3(pt);
                dim.base.definition_point = v3(pt);
                let (d1, d2) = dim_line_endpoints(p1, p2, pt);
                dim.base.text_middle_point = v3((d1 + d2) * 0.5);
                dim.base.insertion_point = dim.base.text_middle_point;
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
                    entity: self.plane.place_entity(EntityType::Dimension(Dimension::Aligned(dim))),
                    association: DimensionAssociationInput::Infer(self.source_handle),
                    preserve_base_style: false,
                }
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
        if matches!(self.step, Step::First) {
            self.selecting_object = true;
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
        if !matches!(self.step, Step::DimLine { .. }) {
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
        self.selecting_object
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
        let Some((p1, p2)) = super::linear_dim::dimension_source_points(&entity, point) else {
            return CmdResult::NeedPoint;
        };
        if p1.distance_squared(p2) <= 1e-24 {
            return CmdResult::NeedPoint;
        }
        self.source_handle = Some(handle);
        self.selecting_object = false;
        self.step = Step::DimLine { p1, p2 };
        CmdResult::NeedPoint
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
    let (d1, d2) = dim_line_endpoints(p1, p2, dim_pt);
    let axis = (d2 - d1).normalize_or_zero();
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let nan = [f32::NAN, 0.0, 0.0];
    let arrow = 0.22;
    let text = (d1 + d2) * 0.5 + perp * 0.15;
    let half_width = ((p2 - p1).length().log10().max(0.0) + 1.0) * 0.18;
    let half_height = 0.16;
    let to_point = |point: DVec3| point.as_vec3().to_array();
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
        name: "dimaligned_preview".into(),
        points: vec![
            to_point(p1),
            to_point(d1),
            nan,
            to_point(p2),
            to_point(d2),
            nan,
            to_point(d1),
            to_point(d2),
            nan,
            to_point(d1 + axis * arrow + perp * arrow * 0.45),
            to_point(d1),
            to_point(d1 + axis * arrow - perp * arrow * 0.45),
            nan,
            to_point(d2 - axis * arrow + perp * arrow * 0.45),
            to_point(d2),
            to_point(d2 - axis * arrow - perp * arrow * 0.45),
            nan,
            to_point(text - axis * half_width - perp * half_height),
            to_point(text + axis * half_width - perp * half_height),
            to_point(text + axis * half_width + perp * half_height),
            to_point(text - axis * half_width + perp * half_height),
            to_point(text - axis * half_width - perp * half_height),
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
