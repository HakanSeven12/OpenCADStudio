use acadrust::entities::{Dimension, DimensionLinear};
use acadrust::types::Vector3;
use acadrust::EntityType;
use cadkernel::geom2d::{
    closest_point, Circle as KernelCircle, Curve, Line as KernelLine,
};

use crate::command::{CadCommand, CmdResult, DimensionAssociationInput, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;
use crate::t;

/// Select the measured axis from where the dimension line clears the points.
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

#[derive(Clone, Copy)]
enum AxisMode {
    Automatic,
    Horizontal,
    Vertical,
    Rotated(f64),
}

impl AxisMode {
    fn axis(self, first: DVec3, second: DVec3, point: DVec3) -> DVec3 {
        match self {
            Self::Automatic => measure_axis(first, second, point),
            Self::Horizontal => DVec3::X,
            Self::Vertical => DVec3::Y,
            Self::Rotated(angle) => DVec3::new(angle.cos(), angle.sin(), 0.0),
        }
    }
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
    /// True while a rotation value for the Rotated option is being entered.
    awaiting_rotation: bool,
    axis_mode: AxisMode,
    selecting_object: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<acadrust::Handle>,
    mtext_override: bool,
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
            awaiting_rotation: false,
            axis_mode: AxisMode::Automatic,
            selecting_object: false,
            picked_entity: None,
            source_handle: None,
            mtext_override: false,
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
            return if self.mtext_override {
                t!("DIMLINEAR  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMLINEAR  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return t!("DIMLINEAR  Specify text angle (degrees):").into_owned();
        }
        if self.awaiting_rotation {
            return t!("DIMLINEAR  Specify dimension line angle (degrees):").into_owned();
        }
        if self.selecting_object {
            return t!("DIMLINEAR  Select object to dimension:").into_owned();
        }
        match self.step {
            Step::FirstPoint => {
                t!("DIMLINEAR  Specify first extension line origin or press Enter to select object:")
                    .into_owned()
            }
            Step::SecondPoint(_) => {
                t!("DIMLINEAR  Specify second extension line origin:").into_owned()
            }
            Step::DimensionLine { .. } => {
                t!("DIMLINEAR  Specify dimension line location  [Mtext/Text/Angle/Horizontal/Vertical/Rotated]:").into_owned()
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
                if pt.distance_squared(first) <= 1e-24 {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::DimensionLine { first, second: pt };
                CmdResult::NeedPoint
            }
            Step::DimensionLine { first, second } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionLinear::new(v3(first), v3(second));
                let axis = self.axis_mode.axis(first, second, pt);
                dim.rotation = axis.y.atan2(axis.x);
                dim.set_offset(dimension_line_offset(second, pt, axis));
                dim.base.definition_point = dim.definition_point;
                dim.base.text_middle_point = v3(linear_text_pos(first, second, pt, axis));
                dim.base.insertion_point = dim.base.text_middle_point;
                dim.base.actual_measurement = dim.measurement();
                crate::entities::dimension::set_dimension_text_override(
                    &mut dim.base,
                    self.text_override.clone(),
                );
                // An explicit text angle overrides the UCS-derived rotation.
                if let Some(a) = self.text_angle {
                    dim.base.text_rotation = a;
                }
                let entity = self.plane.place_entity(EntityType::Dimension(
                    Dimension::Linear(dim),
                ));
                CmdResult::CommitDimension {
                    entity,
                    association: DimensionAssociationInput::Infer(self.source_handle),
                    preserve_base_style: false,
                    keep_active: false,
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
        if self.awaiting_rotation {
            self.awaiting_rotation = false;
            return CmdResult::NeedPoint;
        }
        if matches!(self.step, Step::FirstPoint) {
            self.selecting_object = true;
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
        !self.awaiting_text && !self.awaiting_angle && !self.awaiting_rotation
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
        if self.awaiting_rotation {
            if let Some(angle) = crate::entities::common::parse_typed_angle(text.trim()) {
                self.axis_mode = AxisMode::Rotated(angle);
            }
            self.awaiting_rotation = false;
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.step, Step::DimensionLine { .. }) {
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
            "H" | "HORIZONTAL" => {
                self.axis_mode = AxisMode::Horizontal;
                Some(CmdResult::NeedPoint)
            }
            "V" | "VERTICAL" => {
                self.axis_mode = AxisMode::Vertical;
                Some(CmdResult::NeedPoint)
            }
            "R" | "ROTATED" => {
                self.awaiting_rotation = true;
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

    fn on_entity_pick(&mut self, handle: acadrust::Handle, point: DVec3) -> CmdResult {
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some((first, second)) = dimension_source_points(&entity, point) else {
            return CmdResult::NeedPoint;
        };
        if first.distance_squared(second) <= 1e-24 {
            return CmdResult::NeedPoint;
        }
        self.source_handle = Some(handle);
        self.selecting_object = false;
        self.step = Step::DimensionLine { first, second };
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            Step::FirstPoint => None,
            Step::SecondPoint(first) => Some(preview_wire(vec![first, pt])),
            Step::DimensionLine { first, second } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let pt = self.plane.to_local(pt);
                let axis = self.axis_mode.axis(first, second, pt);
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

fn dimension_line_offset(second: DVec3, point: DVec3, axis: DVec3) -> f64 {
    let perpendicular = DVec3::new(-axis.y, axis.x, 0.0);
    (point - second).dot(perpendicular)
}

pub(crate) fn dimension_source_points(
    entity: &EntityType,
    click: DVec3,
) -> Option<(DVec3, DVec3)> {
    let point = |p: Vector3| DVec3::new(p.x, p.y, p.z);
    match entity {
        EntityType::Line(line) => Some((point(line.start), point(line.end))),
        EntityType::Arc(arc) => Some((point(arc.start_point_wcs()), point(arc.end_point_wcs()))),
        EntityType::Circle(circle) => {
            let click = crate::scene::view::transform::wcs_point_to_ocs(
                (click.x, click.y, click.z),
                (circle.normal.x, circle.normal.y, circle.normal.z),
            );
            let curve = Curve::Circle(KernelCircle {
                centre: [circle.center.x, circle.center.y],
                radius: circle.radius,
            });
            let first = closest_point(&curve, [click.0, click.1]).point;
            let parameter = curve.parameter_at(first);
            let second = curve.point_at((parameter + 0.5).rem_euclid(1.0));
            Some((
                ocs_point(first, circle.center.z, circle.normal),
                ocs_point(second, circle.center.z, circle.normal),
            ))
        }
        EntityType::LwPolyline(polyline) => nearest_planar_source(
            polyline
                .vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y]),
            polyline.is_closed,
            polyline.elevation,
            polyline.normal,
            click,
        ),
        EntityType::Polyline2D(polyline) => {
            let vertices = crate::entities::polyline::drawn_vertices2d(polyline)
                .unwrap_or_else(|| polyline.vertices.clone());
            nearest_planar_source(
                vertices
                    .iter()
                    .map(|vertex| [vertex.location.x, vertex.location.y]),
                polyline.is_closed(),
                polyline.elevation,
                polyline.normal,
                click,
            )
        }
        _ => None,
    }
}

fn nearest_planar_source(
    points: impl IntoIterator<Item = [f64; 2]>,
    closed: bool,
    elevation: f64,
    normal: Vector3,
    click: DVec3,
) -> Option<(DVec3, DVec3)> {
    let click = crate::scene::view::transform::wcs_point_to_ocs(
        (click.x, click.y, click.z),
        (normal.x, normal.y, normal.z),
    );
    let (first, second) = nearest_segment(points, closed, [click.0, click.1])?;
    Some((
        ocs_point(first, elevation, normal),
        ocs_point(second, elevation, normal),
    ))
}

fn nearest_segment(
    points: impl IntoIterator<Item = [f64; 2]>,
    closed: bool,
    click: [f64; 2],
) -> Option<([f64; 2], [f64; 2])> {
    let points: Vec<_> = points.into_iter().collect();
    if points.len() < 2 {
        return None;
    }
    let count = if closed { points.len() } else { points.len() - 1 };
    (0..count)
        .map(|index| {
            let first = points[index];
            let second = points[(index + 1) % points.len()];
            let distance = closest_point(
                &Curve::Line(KernelLine {
                    start: first,
                    end: second,
                }),
                click,
            )
            .distance;
            (distance, first, second)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, first, second)| (first, second))
}

fn ocs_point(point: [f64; 2], elevation: f64, normal: Vector3) -> DVec3 {
    let point = crate::scene::view::transform::ocs_point_to_wcs(
        (point[0], point[1], elevation),
        (normal.x, normal.y, normal.z),
    );
    DVec3::new(point.0, point.1, point.2)
}

fn preview_wire(points: Vec<DVec3>) -> WireModel {
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
    let arrow = 0.22;
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let text = linear_text_pos(first, second, def, axis);
    let half_width = ((second - first).length().log10().max(0.0) + 1.0) * 0.18;
    let half_height = 0.16;
    vec![
        first, d1, nan, second, d2, nan, d1, d2, nan,
        d1, d1 + axis * arrow + perp * arrow * 0.45, nan,
        d1, d1 + axis * arrow - perp * arrow * 0.45, nan,
        d2, d2 - axis * arrow + perp * arrow * 0.45, nan,
        d2, d2 - axis * arrow - perp * arrow * 0.45, nan,
        text - axis * half_width - perp * half_height,
        text + axis * half_width - perp * half_height,
        text + axis * half_width + perp * half_height,
        text - axis * half_width + perp * half_height,
        text - axis * half_width - perp * half_height,
    ]
}

fn linear_text_pos(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> DVec3 {
    let (d1, d2) = dim_line_endpoints(first, second, def, axis);
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    (d1 + d2) * 0.5 + perp * 0.15
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMLINEAR"] });  // LinearDimensionCommand
