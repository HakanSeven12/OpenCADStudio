//! Repeating continued dimensions from the latest session dimension or a picked base.

use acadrust::entities::{
    Dimension, DimensionAngular2Ln, DimensionAngular3Pt, DimensionBase, DimensionLinear,
    DimensionOrdinate,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, DimensionAssociationInput};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_continue.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMCONTINUE",
        label: "Continue",
        icon: ICON,
        event: ModuleEvent::Command("DIMCONTINUE".to_string()),
    }
}

#[derive(Clone)]
struct SourceStyle {
    layer: String,
    style_name: String,
    normal: Vector3,
    text_rotation: f64,
    horizontal_direction: f64,
}

impl SourceStyle {
    fn from_base(base: &DimensionBase) -> Self {
        Self {
            layer: base.common.layer.clone(),
            style_name: base.style_name.clone(),
            normal: base.normal,
            text_rotation: base.text_rotation,
            horizontal_direction: base.horizontal_direction,
        }
    }

    fn apply(&self, base: &mut DimensionBase) {
        base.common.layer = self.layer.clone();
        base.style_name = self.style_name.clone();
        base.normal = self.normal;
        base.text_rotation = self.text_rotation;
        base.horizontal_direction = self.horizontal_direction;
    }
}

#[derive(Clone)]
enum ContinueKind {
    Linear {
        current: DVec3,
        rotation: f64,
        perpendicular: DVec3,
        line_coordinate: f64,
    },
    Angular2Ln {
        source: DimensionAngular2Ln,
        vertex: DVec3,
        current: DVec3,
        radius: f64,
    },
    Angular3Pt {
        source: DimensionAngular3Pt,
        vertex: DVec3,
        current: DVec3,
        radius: f64,
    },
    Ordinate {
        source: DimensionOrdinate,
    },
}

#[derive(Clone)]
struct ContinueState {
    kind: ContinueKind,
    style: SourceStyle,
}

pub struct DimContinueCommand {
    base: Option<ContinueState>,
    injected: Option<EntityType>,
    history: Vec<ContinueState>,
    preserve_base_style: bool,
}

impl DimContinueCommand {
    pub fn new(recent: Option<EntityType>, preserve_base_style: bool) -> Self {
        let mut command = Self {
            base: None,
            injected: None,
            history: Vec::new(),
            preserve_base_style,
        };
        if let Some(entity) = recent {
            let _ = command.install_base(&entity, None);
        }
        command
    }

    fn install_base(&mut self, entity: &EntityType, pick: Option<DVec3>) -> bool {
        let EntityType::Dimension(dimension) = entity else {
            return false;
        };
        let style = SourceStyle::from_base(dimension.base());
        let kind = match dimension {
            Dimension::Linear(source) => {
                let rotation = source.rotation;
                let perpendicular = DVec3::new(-rotation.sin(), rotation.cos(), 0.0);
                let current = selected_linear_end(
                    dv(source.first_point),
                    dv(source.second_point),
                    pick,
                );
                ContinueKind::Linear {
                    current,
                    rotation,
                    perpendicular,
                    line_coordinate: dv(source.base.definition_point).dot(perpendicular),
                }
            }
            Dimension::Aligned(source) => {
                let first = dv(source.first_point);
                let second = dv(source.second_point);
                let delta = second - first;
                if delta.length_squared() <= 1.0e-18 {
                    return false;
                }
                let rotation = delta.y.atan2(delta.x);
                let perpendicular = DVec3::new(-rotation.sin(), rotation.cos(), 0.0);
                ContinueKind::Linear {
                    current: selected_linear_end(first, second, pick),
                    rotation,
                    perpendicular,
                    line_coordinate: dv(source.base.definition_point).dot(perpendicular),
                }
            }
            Dimension::Angular2Ln(source) => {
                let vertex = angular2_vertex(source);
                let first = dv(source.second_point);
                let second = dv(source.definition_point);
                ContinueKind::Angular2Ln {
                    source: source.clone(),
                    vertex,
                    current: selected_angular_end(first, second, pick),
                    radius: (dv(source.dimension_arc) - vertex).length().max(1.0e-9),
                }
            }
            Dimension::Angular3Pt(source) => {
                let vertex = dv(source.angle_vertex);
                ContinueKind::Angular3Pt {
                    source: source.clone(),
                    vertex,
                    current: selected_angular_end(
                        dv(source.first_point),
                        dv(source.second_point),
                        pick,
                    ),
                    radius: (dv(source.definition_point) - vertex).length().max(1.0e-9),
                }
            }
            Dimension::Ordinate(source) => ContinueKind::Ordinate {
                source: source.clone(),
            },
            _ => return false,
        };
        self.base = Some(ContinueState { kind, style });
        true
    }

    fn build_dimension(&self, point: DVec3) -> Option<Dimension> {
        let state = self.base.as_ref()?;
        let mut dimension = match &state.kind {
            ContinueKind::Linear {
                current,
                rotation,
                perpendicular,
                line_coordinate,
            } => build_linear(
                *current,
                point,
                *rotation,
                *perpendicular,
                *line_coordinate,
            ),
            ContinueKind::Angular2Ln {
                source,
                vertex,
                current,
                radius,
            } => {
                let moving = point - *vertex;
                let previous = *current - *vertex;
                if moving.length_squared() <= 1.0e-18 || previous.length_squared() <= 1.0e-18 {
                    return None;
                }
                let mut result = source.clone();
                result.first_point = v3(*vertex);
                result.second_point = v3(point);
                result.angle_vertex = v3(*vertex);
                result.definition_point = v3(*current);
                let (definition, text) =
                    angular_definition_and_text(*vertex, previous, moving, *radius);
                result.dimension_arc = v3(definition);
                result.base.definition_point = result.dimension_arc;
                result.base.text_middle_point = v3(text);
                result.base.insertion_point = result.base.text_middle_point;
                result.base.actual_measurement = result.measurement_degrees();
                Dimension::Angular2Ln(result)
            }
            ContinueKind::Angular3Pt {
                source,
                vertex,
                current,
                radius,
            } => {
                let moving = point - *vertex;
                let previous = *current - *vertex;
                if moving.length_squared() <= 1.0e-18 || previous.length_squared() <= 1.0e-18 {
                    return None;
                }
                let mut result = source.clone();
                result.angle_vertex = v3(*vertex);
                result.first_point = v3(point);
                result.second_point = v3(*current);
                let (definition, text) =
                    angular_definition_and_text(*vertex, previous, moving, *radius);
                result.definition_point = v3(definition);
                result.base.definition_point = result.definition_point;
                result.base.text_middle_point = v3(text);
                result.base.insertion_point = result.base.text_middle_point;
                result.base.actual_measurement = result.measurement_degrees();
                Dimension::Angular3Pt(result)
            }
            ContinueKind::Ordinate { source } => {
                let source_leader = dv(source.leader_endpoint);
                let leader = if source.is_ordinate_type_x {
                    DVec3::new(point.x, source_leader.y, source_leader.z)
                } else {
                    DVec3::new(source_leader.x, point.y, source_leader.z)
                };
                let mut result =
                    DimensionOrdinate::new(v3(point), v3(leader), source.is_ordinate_type_x);
                result.definition_point = source.definition_point;
                result.base.definition_point = source.base.definition_point;
                result.base.text_middle_point = v3(leader);
                result.base.insertion_point = result.base.text_middle_point;
                result.base.actual_measurement = result.measurement();
                Dimension::Ordinate(result)
            }
        };
        state.style.apply(dimension.base_mut());
        dimension.base_mut().common.handle = Handle::NULL;
        dimension.base_mut().block_name.clear();
        Some(dimension)
    }

    fn advance(&mut self, point: DVec3) {
        let Some(state) = self.base.as_mut() else {
            return;
        };
        match &mut state.kind {
            ContinueKind::Linear { current, .. }
            | ContinueKind::Angular2Ln { current, .. }
            | ContinueKind::Angular3Pt { current, .. } => *current = point,
            ContinueKind::Ordinate { .. } => {}
        }
    }

    fn undo_one(&mut self) -> CmdResult {
        let Some(previous) = self.history.pop() else {
            return CmdResult::NeedPoint;
        };
        self.base = Some(previous);
        CmdResult::UndoDocument
    }
}

impl CadCommand for DimContinueCommand {
    fn name(&self) -> &'static str {
        "DIMCONTINUE"
    }

    fn prompt(&self) -> String {
        if self.base.is_none() {
            "DIMCONTINUE  Select continued dimension:".to_string()
        } else if self
            .base
            .as_ref()
            .is_some_and(|base| matches!(&base.kind, ContinueKind::Ordinate { .. }))
        {
            "DIMCONTINUE  Specify feature location [Undo/Select] <Select>:".to_string()
        } else {
            "DIMCONTINUE  Specify second extension line origin [Select/Undo] <Select>:"
                .to_string()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        self.base.as_ref().map_or_else(Vec::new, |base| {
            if matches!(&base.kind, ContinueKind::Ordinate { .. }) {
                vec![CmdOption::new("Undo", "U"), CmdOption::new("Select", "S")]
            } else {
                vec![CmdOption::new("Select", "S"), CmdOption::new("Undo", "U")]
            }
        })
    }

    fn needs_entity_pick(&self) -> bool {
        self.base.is_none()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.base.is_none()
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.injected = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: Handle, point: DVec3) -> CmdResult {
        let Some(entity) = self.injected.take() else {
            return CmdResult::NeedPoint;
        };
        let _ = self.install_base(&entity, Some(point));
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some(dimension) = self.build_dimension(point) else {
            return CmdResult::NeedPoint;
        };
        if let Some(base) = self.base.clone() {
            self.history.push(base);
        }
        self.advance(point);
        CmdResult::CommitDimension {
            entity: EntityType::Dimension(dimension),
            association: DimensionAssociationInput::Infer(None),
            preserve_base_style: self.preserve_base_style,
            keep_active: true,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.base.take().is_some() {
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn wants_text_input(&self) -> bool {
        self.base.is_some()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.base.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_ascii_uppercase().as_str() {
            "S" | "SELECT" => {
                self.base = None;
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => Some(self.undo_one()),
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.history.is_empty() {
            None
        } else {
            Some(self.undo_one())
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        self.build_dimension(point)
            .map(|dimension| preview_for_dimension(&dimension))
    }
}

fn selected_linear_end(first: DVec3, second: DVec3, pick: Option<DVec3>) -> DVec3 {
    pick.map_or(second, |point| {
        if point.distance_squared(first) <= point.distance_squared(second) {
            first
        } else {
            second
        }
    })
}

fn selected_angular_end(first: DVec3, second: DVec3, pick: Option<DVec3>) -> DVec3 {
    pick.map_or(first, |point| {
        if point.distance_squared(first) <= point.distance_squared(second) {
            first
        } else {
            second
        }
    })
}

fn build_linear(
    first: DVec3,
    second: DVec3,
    rotation: f64,
    perpendicular: DVec3,
    line_coordinate: f64,
) -> Dimension {
    let first_line = first + perpendicular * (line_coordinate - first.dot(perpendicular));
    let second_line = second + perpendicular * (line_coordinate - second.dot(perpendicular));
    let mut result = DimensionLinear::new(v3(first), v3(second));
    result.rotation = rotation;
    result.definition_point = v3(second_line);
    result.base.definition_point = v3(second_line);
    result.base.text_middle_point = v3((first_line + second_line) * 0.5);
    result.base.insertion_point = result.base.text_middle_point;
    result.base.actual_measurement = result.measurement();
    Dimension::Linear(result)
}

fn angular2_vertex(source: &DimensionAngular2Ln) -> DVec3 {
    let first = dv(source.first_point);
    let first_direction = dv(source.second_point) - first;
    let second = dv(source.angle_vertex);
    let second_direction = dv(source.definition_point) - second;
    line_intersection(first, first_direction, second, second_direction).unwrap_or(second)
}

fn line_intersection(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> Option<DVec3> {
    let cross = b.x * d.y - b.y * d.x;
    if cross.abs() <= 1.0e-12 {
        return None;
    }
    let delta = c - a;
    Some(a + b * ((delta.x * d.y - delta.y * d.x) / cross))
}

fn angular_definition_and_text(
    vertex: DVec3,
    previous: DVec3,
    moving: DVec3,
    radius: f64,
) -> (DVec3, DVec3) {
    let start = previous.y.atan2(previous.x);
    let mut sweep = moving.y.atan2(moving.x) - start;
    while sweep <= -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    let point_at = |fraction: f64| {
        let angle = start + sweep * fraction;
        vertex + DVec3::new(angle.cos(), angle.sin(), 0.0) * radius
    };
    (point_at(2.0 / 3.0), point_at(0.5))
}

fn preview_for_dimension(dimension: &Dimension) -> WireModel {
    let mut points = Vec::<[f64; 3]>::new();
    match dimension {
        Dimension::Linear(dimension) => linear_preview(
            &mut points,
            dv(dimension.first_point),
            dv(dimension.second_point),
            dimension.rotation,
            dv(dimension.definition_point),
            dv(dimension.base.text_middle_point),
            dimension.base.actual_measurement,
        ),
        Dimension::Angular2Ln(dimension) => {
            let vertex = angular2_vertex(dimension);
            angular_preview(
                &mut points,
                vertex,
                dv(dimension.second_point) - vertex,
                dv(dimension.definition_point) - vertex,
                dv(dimension.dimension_arc),
                dv(dimension.base.text_middle_point),
                dimension.base.actual_measurement,
            );
        }
        Dimension::Angular3Pt(dimension) => angular_preview(
            &mut points,
            dv(dimension.angle_vertex),
            dv(dimension.first_point) - dv(dimension.angle_vertex),
            dv(dimension.second_point) - dv(dimension.angle_vertex),
            dv(dimension.definition_point),
            dv(dimension.base.text_middle_point),
            dimension.base.actual_measurement,
        ),
        Dimension::Ordinate(dimension) => ordinate_preview(
            &mut points,
            dv(dimension.feature_location),
            dv(dimension.leader_endpoint),
            dimension.base.actual_measurement,
        ),
        _ => {}
    }
    WireModel::solid_f64(
        "dimcontinue_preview".to_string(),
        points,
        WireModel::CYAN,
        false,
    )
}

fn linear_preview(
    points: &mut Vec<[f64; 3]>,
    first: DVec3,
    second: DVec3,
    rotation: f64,
    definition: DVec3,
    text: DVec3,
    measurement: f64,
) {
    let axis = DVec3::new(rotation.cos(), rotation.sin(), 0.0);
    let perpendicular = DVec3::new(-axis.y, axis.x, 0.0);
    let coordinate = definition.dot(perpendicular);
    let first_line = first + perpendicular * (coordinate - first.dot(perpendicular));
    let second_line = second + perpendicular * (coordinate - second.dot(perpendicular));
    segment(points, first, first_line);
    segment(points, second, second_line);
    segment(points, first_line, second_line);
    arrow(points, first_line, axis, perpendicular, first_line.distance(second_line));
    arrow(points, second_line, -axis, perpendicular, first_line.distance(second_line));
    text_box(points, text, axis, perpendicular, measurement);
}

fn angular_preview(
    points: &mut Vec<[f64; 3]>,
    vertex: DVec3,
    first: DVec3,
    second: DVec3,
    arc_point: DVec3,
    text: DVec3,
    measurement: f64,
) {
    let radius = (arc_point - vertex).length().max(1.0e-9);
    let start = first.y.atan2(first.x);
    let mut sweep = second.y.atan2(second.x) - start;
    while sweep <= -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    let first_end = vertex + normalized_or(first, DVec3::X) * radius;
    let second_end = vertex + normalized_or(second, DVec3::Y) * radius;
    segment(points, vertex, first_end);
    segment(points, vertex, second_end);
    let arc = cadkernel::geom2d::tessellate::arc(
        [vertex.x, vertex.y],
        radius,
        start,
        start + sweep,
        vertex.z,
        cadkernel::geom2d::tessellate::DEFAULT_SEGMENTS_PER_RADIAN,
    );
    points.extend(arc);
    points.push([f64::NAN, f64::NAN, f64::NAN]);
    let sign = sweep.signum();
    let start_tangent = DVec3::new(-start.sin(), start.cos(), 0.0) * sign;
    let end_angle = start + sweep;
    let end_tangent = DVec3::new(end_angle.sin(), -end_angle.cos(), 0.0) * sign;
    arrow(points, first_end, start_tangent, normalized_or(first, DVec3::X), radius);
    arrow(points, second_end, end_tangent, normalized_or(second, DVec3::Y), radius);
    let middle = start + sweep * 0.5;
    let text_axis = normalized_or(DVec3::new(-middle.sin(), middle.cos(), 0.0), DVec3::X);
    let text_perpendicular = DVec3::new(-text_axis.y, text_axis.x, 0.0);
    text_box(points, text, text_axis, text_perpendicular, measurement);
}

fn ordinate_preview(
    points: &mut Vec<[f64; 3]>,
    feature: DVec3,
    leader: DVec3,
    measurement: f64,
) {
    segment(points, feature, leader);
    let axis = normalized_or(leader - feature, DVec3::Y);
    let perpendicular = DVec3::new(-axis.y, axis.x, 0.0);
    arrow(points, feature, axis, perpendicular, feature.distance(leader));
    text_box(points, leader, perpendicular, axis, measurement);
}

fn arrow(
    points: &mut Vec<[f64; 3]>,
    tip: DVec3,
    inward: DVec3,
    perpendicular: DVec3,
    span: f64,
) {
    let inward = normalized_or(inward, DVec3::X);
    let perpendicular = normalized_or(perpendicular, DVec3::Y);
    let size = span.clamp(1.0, 100.0) * 0.035;
    let back = tip + inward * size;
    segment(points, tip, back + perpendicular * size * 0.4);
    segment(points, tip, back - perpendicular * size * 0.4);
}

fn text_box(
    points: &mut Vec<[f64; 3]>,
    center: DVec3,
    axis: DVec3,
    perpendicular: DVec3,
    measurement: f64,
) {
    let digits = if measurement.abs() < 1.0 {
        1.0
    } else {
        measurement.abs().log10().floor() + 1.0
    };
    let half_width = (digits + 2.0) * 0.12;
    let half_height = 0.16;
    let a = center - axis * half_width - perpendicular * half_height;
    let b = center + axis * half_width - perpendicular * half_height;
    let c = center + axis * half_width + perpendicular * half_height;
    let d = center - axis * half_width + perpendicular * half_height;
    points.extend([a.to_array(), b.to_array(), c.to_array(), d.to_array(), a.to_array()]);
    points.push([f64::NAN, f64::NAN, f64::NAN]);
}

fn segment(points: &mut Vec<[f64; 3]>, first: DVec3, second: DVec3) {
    points.push(first.to_array());
    points.push(second.to_array());
    points.push([f64::NAN, f64::NAN, f64::NAN]);
}

fn normalized_or(value: DVec3, fallback: DVec3) -> DVec3 {
    if value.length_squared() > 1.0e-18 {
        value.normalize()
    } else {
        fallback
    }
}

fn dv(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMCONTINUE"] });
