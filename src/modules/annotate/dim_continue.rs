use acadrust::entities::{
    Dimension, DimensionAngular2Ln, DimensionAngular3Pt, DimensionBase, DimensionLinear,
    DimensionOrdinate,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::{
    arc_span, closest_point, intersect, nearest_of, Arc, Curve, Ray, Tolerance, Transform, Vec2,
    XLine,
};
use cadkernel::space::Plane;
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
        current: Vec2,
        rotation: f64,
        perpendicular: Vec2,
        line_coordinate: f64,
    },
    Angular2Ln {
        vertex: Vec2,
        current: Vec2,
        radius: f64,
    },
    Angular3Pt {
        vertex: Vec2,
        current: Vec2,
        radius: f64,
    },
    Ordinate {
        definition: Vec2,
        leader: Vec2,
        is_x: bool,
    },
}

#[derive(Clone)]
struct ContinueState {
    kind: ContinueKind,
    style: SourceStyle,
    plane: Plane,
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

    fn install_base(&mut self, entity: &EntityType, pick_world: Option<DVec3>) -> bool {
        let EntityType::Dimension(dimension) = entity else {
            return false;
        };
        let style = SourceStyle::from_base(dimension.base());
        let plane = plane_from_normal(dimension.base().definition_point, style.normal);
        let pick = pick_world.and_then(|point| plane.project(point.to_array()).map(Vec2::from));
        let kind = match dimension {
            Dimension::Linear(source) => {
                let rotation = source.rotation;
                let perpendicular = Vec2::new(-rotation.sin(), rotation.cos());
                ContinueKind::Linear {
                    current: selected_linear_end(
                        project(&plane, source.first_point),
                        project(&plane, source.second_point),
                        pick,
                    ),
                    rotation,
                    perpendicular,
                    line_coordinate: project(&plane, source.base.definition_point)
                        .dot(perpendicular),
                }
            }
            Dimension::Aligned(source) => {
                let first = project(&plane, source.first_point);
                let second = project(&plane, source.second_point);
                let Some(direction) = (second - first).normalize() else {
                    return false;
                };
                let rotation = direction.angle();
                let perpendicular = direction.perpendicular();
                ContinueKind::Linear {
                    current: selected_linear_end(first, second, pick),
                    rotation,
                    perpendicular,
                    line_coordinate: project(&plane, source.base.definition_point)
                        .dot(perpendicular),
                }
            }
            Dimension::Angular2Ln(source) => {
                let lines = [
                    [
                        project(&plane, source.first_point),
                        project(&plane, source.second_point),
                    ],
                    [
                        project(&plane, source.angle_vertex),
                        project(&plane, source.definition_point),
                    ],
                ];
                let vertex = angular2_vertex(lines).unwrap_or(lines[1][0]);
                ContinueKind::Angular2Ln {
                    vertex,
                    current: selected_angular_end(vertex, [lines[0][1], lines[1][1]], pick),
                    radius: project(&plane, source.dimension_arc)
                        .distance(vertex)
                        .max(1.0e-9),
                }
            }
            Dimension::Angular3Pt(source) => {
                let vertex = project(&plane, source.angle_vertex);
                ContinueKind::Angular3Pt {
                    vertex,
                    current: selected_angular_end(
                        vertex,
                        [
                            project(&plane, source.first_point),
                            project(&plane, source.second_point),
                        ],
                        pick,
                    ),
                    radius: project(&plane, source.definition_point)
                        .distance(vertex)
                        .max(1.0e-9),
                }
            }
            Dimension::Ordinate(source) => ContinueKind::Ordinate {
                definition: project(&plane, source.definition_point),
                leader: project(&plane, source.leader_endpoint),
                is_x: source.is_ordinate_type_x,
            },
            _ => return false,
        };
        self.base = Some(ContinueState { kind, style, plane });
        true
    }

    fn build_dimension(&self, point_world: DVec3) -> Option<Dimension> {
        let state = self.base.as_ref()?;
        let point = state.plane.project(point_world.to_array()).map(Vec2::from)?;
        let mut dimension = match &state.kind {
            ContinueKind::Linear {
                current,
                rotation,
                perpendicular,
                line_coordinate,
            } => build_linear(
                state.plane,
                *current,
                point,
                *rotation,
                *perpendicular,
                *line_coordinate,
            ),
            ContinueKind::Angular2Ln {
                vertex,
                current,
                radius,
            } => {
                let previous = *current - *vertex;
                let moving = point - *vertex;
                let (definition, text) =
                    angular_definition_and_text(*vertex, previous, moving, *radius)?;
                let mut result = DimensionAngular2Ln::default();
                result.first_point = world(state.plane, *vertex);
                result.second_point = world(state.plane, *current);
                result.angle_vertex = world(state.plane, *vertex);
                result.definition_point = world(state.plane, point);
                result.dimension_arc = world(state.plane, definition);
                result.base.definition_point = result.dimension_arc;
                result.base.text_middle_point = world(state.plane, text);
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Angular2Ln(result)
            }
            ContinueKind::Angular3Pt {
                vertex,
                current,
                radius,
            } => {
                let previous = *current - *vertex;
                let moving = point - *vertex;
                let (definition, text) =
                    angular_definition_and_text(*vertex, previous, moving, *radius)?;
                let mut result = DimensionAngular3Pt::new(
                    world(state.plane, *vertex),
                    world(state.plane, *current),
                    world(state.plane, point),
                );
                result.definition_point = world(state.plane, definition);
                result.base.definition_point = result.definition_point;
                result.base.text_middle_point = world(state.plane, text);
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Angular3Pt(result)
            }
            ContinueKind::Ordinate {
                definition,
                leader,
                is_x,
            } => {
                let to_dimension = Transform::rotation(-state.style.horizontal_direction);
                let from_dimension = Transform::rotation(state.style.horizontal_direction);
                let feature = Vec2::from(to_dimension.apply_point(point.into()));
                let source_leader = Vec2::from(to_dimension.apply_point((*leader).into()));
                let leader = if *is_x {
                    Vec2::new(feature.x, source_leader.y)
                } else {
                    Vec2::new(source_leader.x, feature.y)
                };
                let leader = Vec2::from(from_dimension.apply_point(leader.into()));
                let mut result = DimensionOrdinate::new(
                    world(state.plane, point),
                    world(state.plane, leader),
                    *is_x,
                );
                result.definition_point = world(state.plane, *definition);
                result.base.definition_point = result.definition_point;
                result.base.text_middle_point = result.leader_endpoint;
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Ordinate(result)
            }
        };
        state.style.apply(dimension.base_mut());
        refresh_measurement(&mut dimension, state.plane);
        dimension.base_mut().common.handle = Handle::NULL;
        dimension.base_mut().block_name.clear();
        Some(dimension)
    }

    fn advance(&mut self, point_world: DVec3) {
        let Some(point) = self
            .base
            .as_ref()
            .and_then(|state| state.plane.project(point_world.to_array()))
            .map(Vec2::from)
        else {
            return;
        };
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
            continue_command: true,
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

fn build_linear(
    plane: Plane,
    first: Vec2,
    second: Vec2,
    rotation: f64,
    perpendicular: Vec2,
    line_coordinate: f64,
) -> Dimension {
    let first_line = project_to_line(first, perpendicular, line_coordinate);
    let second_line = project_to_line(second, perpendicular, line_coordinate);
    let mut result = DimensionLinear::new(world(plane, first), world(plane, second));
    result.rotation = rotation;
    result.definition_point = world(plane, second_line);
    result.base.definition_point = result.definition_point;
    result.base.text_middle_point = world(plane, first_line.lerp(second_line, 0.5));
    result.base.insertion_point = result.base.text_middle_point;
    Dimension::Linear(result)
}

fn selected_linear_end(first: Vec2, second: Vec2, pick: Option<Vec2>) -> Vec2 {
    pick.map_or(second, |point| {
        if point.distance_squared(first) <= point.distance_squared(second) {
            first
        } else {
            second
        }
    })
}

fn selected_angular_end(vertex: Vec2, ends: [Vec2; 2], pick: Option<Vec2>) -> Vec2 {
    let Some(pick) = pick else {
        return ends[0];
    };
    let rays = ends.map(|end| {
        Curve::Ray(Ray {
            origin: vertex.into(),
            direction: (end - vertex).into(),
        })
    });
    nearest_of(rays.iter(), pick.into())
        .map(|(index, _)| ends[index])
        .unwrap_or(ends[0])
}

fn angular2_vertex(lines: [[Vec2; 2]; 2]) -> Option<Vec2> {
    let first = Curve::XLine(XLine {
        base: lines[0][0].into(),
        direction: (lines[0][1] - lines[0][0]).into(),
    });
    let second = Curve::XLine(XLine {
        base: lines[1][0].into(),
        direction: (lines[1][1] - lines[1][0]).into(),
    });
    let scale = (lines[0][1] - lines[0][0])
        .length()
        .max((lines[1][1] - lines[1][0]).length())
        .max(1.0);
    intersect(&first, &second, Tolerance::new(scale * 1.0e-9))
        .first()
        .map(|crossing| Vec2::from(crossing.point))
}

fn angular_definition_and_text(
    vertex: Vec2,
    previous: Vec2,
    moving: Vec2,
    radius: f64,
) -> Option<(Vec2, Vec2)> {
    let arc = angular_arc(vertex, previous, moving, radius)?;
    Some((
        Vec2::from(arc.point_at(2.0 / 3.0)),
        Vec2::from(arc.point_at(0.5)),
    ))
}

fn angular_arc(vertex: Vec2, first: Vec2, second: Vec2, radius: f64) -> Option<Curve> {
    let first = first.normalize()?;
    let second = second.normalize()?;
    if first.distance_squared(second) <= 1.0e-18 {
        return None;
    }
    let first_angle = first.angle();
    let second_angle = second.angle();
    let (start_angle, end_angle) =
        if arc_span(first_angle, second_angle) <= std::f64::consts::PI {
            (first_angle, second_angle)
        } else {
            (second_angle, first_angle)
        };
    Some(Curve::Arc(Arc {
        centre: vertex.into(),
        radius,
        start_angle,
        end_angle,
    }))
}

fn refresh_measurement(dimension: &mut Dimension, plane: Plane) {
    match dimension {
        Dimension::Linear(value) => {
            let first = project(&plane, value.first_point);
            let second = project(&plane, value.second_point);
            let axis = Vec2::new(value.rotation.cos(), value.rotation.sin());
            value.base.actual_measurement = (second - first).dot(axis).abs();
        }
        Dimension::Angular2Ln(value) => {
            value.base.actual_measurement = value.measurement_degrees()
        }
        Dimension::Angular3Pt(value) => {
            value.base.actual_measurement = value.measurement_degrees()
        }
        Dimension::Ordinate(value) => value.refresh_measurement(),
        _ => {}
    }
}

fn preview_for_dimension(dimension: &Dimension) -> WireModel {
    let mut points = Vec::<[f64; 3]>::new();
    match dimension {
        Dimension::Linear(value) => {
            let plane = plane_from_normal(value.first_point, value.base.normal);
            linear_preview(
                &mut points,
                plane,
                project(&plane, value.first_point),
                project(&plane, value.second_point),
                value.rotation,
                project(&plane, value.definition_point),
                project(&plane, value.base.text_middle_point),
                value.base.actual_measurement,
            );
        }
        Dimension::Angular2Ln(value) => {
            let plane = plane_from_normal(value.first_point, value.base.normal);
            let lines = [
                [
                    project(&plane, value.first_point),
                    project(&plane, value.second_point),
                ],
                [
                    project(&plane, value.angle_vertex),
                    project(&plane, value.definition_point),
                ],
            ];
            let vertex = angular2_vertex(lines).unwrap_or(lines[1][0]);
            angular_preview(
                &mut points,
                plane,
                vertex,
                lines[0][1] - vertex,
                lines[1][1] - vertex,
                project(&plane, value.dimension_arc),
                project(&plane, value.base.text_middle_point),
                value.base.actual_measurement,
            );
        }
        Dimension::Angular3Pt(value) => {
            let plane = plane_from_normal(value.angle_vertex, value.base.normal);
            let vertex = project(&plane, value.angle_vertex);
            angular_preview(
                &mut points,
                plane,
                vertex,
                project(&plane, value.first_point) - vertex,
                project(&plane, value.second_point) - vertex,
                project(&plane, value.definition_point),
                project(&plane, value.base.text_middle_point),
                value.base.actual_measurement,
            );
        }
        Dimension::Ordinate(value) => {
            let plane = plane_from_normal(value.definition_point, value.base.normal);
            ordinate_preview(
                &mut points,
                plane,
                project(&plane, value.feature_location),
                project(&plane, value.leader_endpoint),
                value.base.actual_measurement,
            );
        }
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
    plane: Plane,
    first: Vec2,
    second: Vec2,
    rotation: f64,
    definition: Vec2,
    text: Vec2,
    measurement: f64,
) {
    let axis = Vec2::new(rotation.cos(), rotation.sin());
    let perpendicular = axis.perpendicular();
    let coordinate = definition.dot(perpendicular);
    let first_line = project_to_line(first, perpendicular, coordinate);
    let second_line = project_to_line(second, perpendicular, coordinate);
    segment(points, plane, first, first_line);
    segment(points, plane, second, second_line);
    segment(points, plane, first_line, second_line);
    arrow(
        points,
        plane,
        first_line,
        axis,
        perpendicular,
        first_line.distance(second_line),
    );
    arrow(
        points,
        plane,
        second_line,
        -axis,
        perpendicular,
        first_line.distance(second_line),
    );
    text_box(points, plane, text, axis, perpendicular, measurement);
}

fn angular_preview(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    vertex: Vec2,
    first: Vec2,
    second: Vec2,
    arc_point: Vec2,
    text: Vec2,
    measurement: f64,
) {
    let radius = arc_point.distance(vertex).max(1.0e-9);
    let Some(first_direction) = first.normalize() else {
        return;
    };
    let Some(second_direction) = second.normalize() else {
        return;
    };
    let Some(arc) = angular_arc(vertex, first, second, radius) else {
        return;
    };
    segment(points, plane, vertex, vertex + first_direction * radius);
    segment(points, plane, vertex, vertex + second_direction * radius);
    for index in 0..=24 {
        push_local(points, plane, Vec2::from(arc.point_at(index as f64 / 24.0)));
    }
    points.push([f64::NAN; 3]);
    let start = Vec2::from(arc.point_at(0.0));
    let start_next = Vec2::from(arc.point_at(1.0 / 1024.0));
    let end = Vec2::from(arc.point_at(1.0));
    let end_previous = Vec2::from(arc.point_at(1.0 - 1.0 / 1024.0));
    arrow(
        points,
        plane,
        start,
        (start_next - start).normalize().unwrap_or(first_direction),
        first_direction,
        radius,
    );
    arrow(
        points,
        plane,
        end,
        (end_previous - end).normalize().unwrap_or(-second_direction),
        second_direction,
        radius,
    );
    let before_middle = Vec2::from(arc.point_at(0.5 - 1.0 / 1024.0));
    let after_middle = Vec2::from(arc.point_at(0.5 + 1.0 / 1024.0));
    let text_axis = (after_middle - before_middle)
        .normalize()
        .unwrap_or(Vec2::new(1.0, 0.0));
    text_box(
        points,
        plane,
        text,
        text_axis,
        text_axis.perpendicular(),
        measurement,
    );
}

fn ordinate_preview(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    feature: Vec2,
    leader: Vec2,
    measurement: f64,
) {
    segment(points, plane, feature, leader);
    let axis = (leader - feature)
        .normalize()
        .unwrap_or(Vec2::new(0.0, 1.0));
    let perpendicular = axis.perpendicular();
    arrow(
        points,
        plane,
        feature,
        axis,
        perpendicular,
        feature.distance(leader),
    );
    text_box(
        points,
        plane,
        leader,
        perpendicular,
        axis,
        measurement,
    );
}

fn arrow(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    tip: Vec2,
    inward: Vec2,
    perpendicular: Vec2,
    span: f64,
) {
    let inward = inward.normalize().unwrap_or(Vec2::new(1.0, 0.0));
    let perpendicular = perpendicular
        .normalize()
        .unwrap_or(Vec2::new(0.0, 1.0));
    let size = span.clamp(1.0, 100.0) * 0.035;
    let back = tip + inward * size;
    segment(
        points,
        plane,
        tip,
        back + perpendicular * size * 0.4,
    );
    segment(
        points,
        plane,
        tip,
        back - perpendicular * size * 0.4,
    );
}

fn text_box(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    center: Vec2,
    axis: Vec2,
    perpendicular: Vec2,
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
    for point in [a, b, c, d, a] {
        push_local(points, plane, point);
    }
    points.push([f64::NAN; 3]);
}

fn project_to_line(point: Vec2, perpendicular: Vec2, coordinate: f64) -> Vec2 {
    let line = Curve::XLine(XLine {
        base: (perpendicular * coordinate).into(),
        direction: perpendicular.perpendicular().into(),
    });
    Vec2::from(closest_point(&line, point.into()).point)
}

fn plane_from_normal(origin: Vector3, normal: Vector3) -> Plane {
    let (x_axis, y_axis) =
        crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    Plane::from_axes(
        point(origin),
        [x_axis.0, x_axis.1, x_axis.2],
        [y_axis.0, y_axis.1, y_axis.2],
    )
}

fn project(plane: &Plane, value: Vector3) -> Vec2 {
    Vec2::from(plane.project(point(value)).unwrap_or([0.0; 2]))
}

fn world(plane: Plane, value: Vec2) -> Vector3 {
    let value = plane.point_at(value.into());
    Vector3::new(value[0], value[1], value[2])
}

fn point(value: Vector3) -> [f64; 3] {
    [value.x, value.y, value.z]
}

fn segment(points: &mut Vec<[f64; 3]>, plane: Plane, first: Vec2, second: Vec2) {
    push_local(points, plane, first);
    push_local(points, plane, second);
    points.push([f64::NAN; 3]);
}

fn push_local(points: &mut Vec<[f64; 3]>, plane: Plane, point: Vec2) {
    points.push(plane.point_at(point.into()));
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMCONTINUE"] });
