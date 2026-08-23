use acadrust::entities::{Dimension, DimensionLinear};
use acadrust::types::Vector3;
use acadrust::EntityType;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;
use crate::t;

/// Which axis a linear dimension measures along.
///
/// Not the segment's own direction. A linear dimension reports the run or the
/// rise, and which of the two is wanted is said by where the dimension line is
/// put: pulled clear of the points above or below, the horizontal distance is
/// what is left to show; pulled out to the side, the vertical one. Taking the
/// answer from the two origins alone made every shallow tilt horizontal
/// forever, whatever the cursor did. (#669)
///
/// "Clear of the points" is measured against their extent rather than their
/// midpoint: a dimension line dragged straight up from near one end of a long
/// pair is still above them, though it is a long way from the middle.
fn measure_axis(first: DVec3, second: DVec3, def: DVec3) -> DVec3 {
    let outside = |value: f64, a: f64, b: f64| {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        (low - value).max(value - high).max(0.0)
    };
    let out_x = outside(def.x, first.x, second.x);
    let out_y = outside(def.y, first.y, second.y);
    if out_x > 0.0 || out_y > 0.0 {
        return if out_y >= out_x { DVec3::X } else { DVec3::Y };
    }
    // Still between the origins on both axes — the location has not said
    // anything yet, so fall back to the longer side of the pair.
    let delta = second - first;
    if delta.x.abs() >= delta.y.abs() {
        DVec3::X
    } else {
        DVec3::Y
    }
}

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_linear.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMLINEAR",
        label: "Linear",
        icon: ICON,
        event: ModuleEvent::Command("DIMLINEAR".to_string()),
    }
}

enum Step {
    FirstPoint,
    SecondPoint(DVec3),
    DimensionLine { first: DVec3, second: DVec3 },
}

pub struct LinearDimensionCommand {
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

impl LinearDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::FirstPoint,
            plane: WorkingPlane::default(),
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
        }
    }
}

impl CadCommand for LinearDimensionCommand {
    fn name(&self) -> &'static str {
        "DIMLINEAR"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return t!("DIMLINEAR  Enter dimension text (blank = measured value):").into_owned();
        }
        if self.awaiting_angle {
            return t!("DIMLINEAR  Specify text angle (degrees):").into_owned();
        }
        match self.step {
            Step::FirstPoint => t!("DIMLINEAR  Specify first extension line origin:").into_owned(),
            Step::SecondPoint(_) => {
                t!("DIMLINEAR  Specify second extension line origin  [Text/Angle]:").into_owned()
            }
            Step::DimensionLine { .. } => {
                t!("DIMLINEAR  Specify dimension line location  [Text/Angle]:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::FirstPoint => {
                self.step = Step::SecondPoint(pt);
                CmdResult::NeedPoint
            }
            Step::SecondPoint(first) => {
                self.step = Step::DimensionLine { first, second: pt };
                CmdResult::NeedPoint
            }
            Step::DimensionLine { first, second } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionLinear::new(v3(first), v3(second));
                let axis = measure_axis(first, second, pt);
                dim.rotation = axis.y.atan2(axis.x);
                dim.definition_point = v3(pt);
                dim.base.definition_point = v3(pt);
                dim.base.text_middle_point = v3(linear_text_pos(first, second, pt, axis));
                dim.base.insertion_point = dim.base.text_middle_point;
                dim.base.actual_measurement = dim.measurement();
                dim.base.user_text = self.text_override.clone();
                // An explicit text angle overrides the UCS-derived rotation.
                if let Some(a) = self.text_angle {
                    dim.base.text_rotation = a;
                }
                CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Dimension(
                    Dimension::Linear(dim),
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

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // While typing the override text or angle, route input as a value, not
        // a point pick / keyword.
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
        match self.step {
            Step::FirstPoint => None,
            Step::SecondPoint(first) => Some(preview_wire(vec![first, pt])),
            Step::DimensionLine { first, second } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let pt = self.plane.to_local(pt);
                let axis = measure_axis(first, second, pt);
                let points = linear_dimension_preview(first, second, pt, axis)
                    .into_iter()
                    .map(|point| self.plane.to_world(point))
                    .collect();
                Some(preview_wire(points))
            }
        }
    }
}

fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
}

fn preview_wire(points: Vec<DVec3>) -> WireModel {
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
        name: "dimlinear_preview".to_string(),
        points: points
            .into_iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect(),
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

/// Project the two extension origins onto the dimension line, which passes
/// through `def` along `axis`. Each origin is projected *independently*: a
/// single shared offset only lands both on the line when they are level, and
/// tilts the dimension line when they are not (e.g. measuring across sloped
/// points). See #181.
fn dim_line_endpoints(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> (DVec3, DVec3) {
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let dperp = def.dot(perp);
    let d1 = first + perp * (dperp - first.dot(perp));
    let d2 = second + perp * (dperp - second.dot(perp));
    (d1, d2)
}

fn linear_dimension_preview(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> Vec<DVec3> {
    let (d1, d2) = dim_line_endpoints(first, second, def, axis);
    let nan = DVec3::new(f64::NAN, f64::NAN, f64::NAN);
    vec![first, d1, nan, second, d2, nan, d1, d2]
}

fn linear_text_pos(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> DVec3 {
    let (d1, d2) = dim_line_endpoints(first, second, def, axis);
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    (d1 + d2) * 0.5 + perp * 0.15
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMLINEAR"] });  // LinearDimensionCommand
