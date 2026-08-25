use acadrust::entities::{Dimension, DimensionAngular2Ln, DimensionAngular3Pt};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use glam::DVec3;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_angular.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMANGULAR",
        label: "Angular",
        icon: ICON,
        event: ModuleEvent::Command("DIMANGULAR".to_string()),
    }
}

enum Step {
    Vertex,
    FirstRay(DVec3),
    SecondRay { vertex: DVec3, first: DVec3 },
    CircleSecondRay {
        vertex: DVec3,
        first: DVec3,
        radius: f64,
        source: Handle,
    },
    SecondLine {
        first_start: DVec3,
        first_end: DVec3,
        first_source: Handle,
    },
    ArcPoint3 {
        vertex: DVec3,
        first: DVec3,
        second: DVec3,
    },
    ArcPoint2 {
        first_start: DVec3,
        first_end: DVec3,
        second_start: DVec3,
        second_end: DVec3,
    },
}

enum PickedCurve {
    Line(DVec3, DVec3),
    Arc {
        center: DVec3,
        first: DVec3,
        second: DVec3,
    },
    Circle {
        center: DVec3,
        radius: f64,
    },
}

pub struct AngularDimensionCommand {
    step: Step,
    plane: WorkingPlane,
    selecting_object: bool,
    picked_entity: Option<EntityType>,
    source_handles: Vec<Handle>,
    text_override: Option<String>,
    awaiting_text: bool,
    mtext_override: bool,
    text_angle: Option<f64>,
    awaiting_angle: bool,
}

impl AngularDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Vertex,
            plane: WorkingPlane::default(),
            selecting_object: false,
            picked_entity: None,
            source_handles: Vec::new(),
            text_override: None,
            awaiting_text: false,
            mtext_override: false,
            text_angle: None,
            awaiting_angle: false,
        }
    }

    fn finish_three_point(
        &self,
        vertex: DVec3,
        first: DVec3,
        second: DVec3,
        arc_point: DVec3,
    ) -> CmdResult {
        let vertex = self.plane.to_local(vertex);
        let first = self.plane.to_local(first);
        let second = self.plane.to_local(second);
        let arc_point = self.plane.to_local(arc_point);
        let mut dim = DimensionAngular3Pt::new(v3(vertex), v3(first), v3(second));
        dim.definition_point = v3(arc_point);
        dim.base.definition_point = dim.definition_point;
        dim.base.text_middle_point = dim.definition_point;
        dim.base.insertion_point = dim.definition_point;
        dim.base.actual_measurement = dim.measurement_degrees();
        crate::entities::dimension::set_dimension_text_override(
            &mut dim.base,
            self.text_override.clone(),
        );
        if let Some(angle) = self.text_angle {
            dim.base.text_rotation = angle;
        }
        self.commit(Dimension::Angular3Pt(dim))
    }

    fn finish_two_line(
        &self,
        first_start: DVec3,
        first_end: DVec3,
        second_start: DVec3,
        second_end: DVec3,
        arc_point: DVec3,
    ) -> CmdResult {
        let first_start = self.plane.to_local(first_start);
        let first_end = self.plane.to_local(first_end);
        let second_start = self.plane.to_local(second_start);
        let second_end = self.plane.to_local(second_end);
        let arc_point = self.plane.to_local(arc_point);
        let mut dim = DimensionAngular2Ln::default();
        dim.first_point = v3(first_start);
        dim.second_point = v3(first_end);
        dim.angle_vertex = v3(second_start);
        dim.definition_point = v3(second_end);
        dim.dimension_arc = v3(arc_point);
        dim.base.definition_point = dim.dimension_arc;
        dim.base.text_middle_point = dim.dimension_arc;
        dim.base.insertion_point = dim.dimension_arc;
        dim.base.actual_measurement = dim.measurement_degrees();
        crate::entities::dimension::set_dimension_text_override(
            &mut dim.base,
            self.text_override.clone(),
        );
        if let Some(angle) = self.text_angle {
            dim.base.text_rotation = angle;
        }
        self.commit(Dimension::Angular2Ln(dim))
    }

    fn commit(&self, dimension: Dimension) -> CmdResult {
        let entity = self.plane.place_entity(EntityType::Dimension(dimension));
        if self.source_handles.is_empty() {
            CmdResult::CommitAndExit(entity)
        } else {
            CmdResult::CommitAssociativeDimension {
                entity,
                sources: self.source_handles.clone(),
            }
        }
    }

    fn placement_step(&self) -> bool {
        matches!(self.step, Step::ArcPoint3 { .. } | Step::ArcPoint2 { .. })
    }
}

impl CadCommand for AngularDimensionCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMANGULAR"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return if self.mtext_override {
                t!("DIMANGULAR  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMANGULAR  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return t!("DIMANGULAR  Specify text angle (degrees):").into_owned();
        }
        if self.selecting_object {
            return match self.step {
                Step::SecondLine { .. } => t!("DIMANGULAR  Select second line:").into_owned(),
                _ => t!("DIMANGULAR  Select arc, circle, line, or polyline arc:").into_owned(),
            };
        }
        match self.step {
            Step::Vertex => t!(
                "DIMANGULAR  Specify angle vertex or press Enter to select an object:"
            )
            .into_owned(),
            Step::FirstRay(_) => {
                t!("DIMANGULAR  Specify first extension line point:").into_owned()
            }
            Step::SecondRay { .. } | Step::CircleSecondRay { .. } => {
                t!("DIMANGULAR  Specify second extension line point:").into_owned()
            }
            Step::SecondLine { .. } => t!("DIMANGULAR  Select second line:").into_owned(),
            Step::ArcPoint3 { .. } | Step::ArcPoint2 { .. } => t!(
                "DIMANGULAR  Specify dimension arc location [Mtext/Text/Angle/Quadrant]:"
            )
            .into_owned(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Vertex => {
                self.step = Step::FirstRay(point);
                CmdResult::NeedPoint
            }
            Step::FirstRay(vertex) => {
                if point.distance_squared(vertex) <= 1.0e-24 {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::SecondRay { vertex, first: point };
                CmdResult::NeedPoint
            }
            Step::SecondRay { vertex, first } => {
                if point.distance_squared(vertex) <= 1.0e-24 {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::ArcPoint3 { vertex, first, second: point };
                CmdResult::NeedPoint
            }
            Step::CircleSecondRay { vertex, first, radius, source } => {
                let Some(second) = project_to_radius(vertex, point, radius) else {
                    return CmdResult::NeedPoint;
                };
                self.source_handles = vec![source, source, source];
                self.step = Step::ArcPoint3 { vertex, first, second };
                CmdResult::NeedPoint
            }
            Step::SecondLine { .. } => CmdResult::NeedPoint,
            Step::ArcPoint3 { vertex, first, second } => {
                self.finish_three_point(vertex, first, second, point)
            }
            Step::ArcPoint2 {
                first_start,
                first_end,
                second_start,
                second_end,
            } => self.finish_two_line(
                first_start,
                first_end,
                second_start,
                second_end,
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
        if matches!(self.step, Step::Vertex) {
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

    fn wants_text_with_spaces(&self) -> bool {
        self.awaiting_text
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.placement_step() && !self.awaiting_text && !self.awaiting_angle
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
            self.text_angle = if text.trim().is_empty() {
                None
            } else {
                crate::entities::common::parse_typed_angle(text.trim())
            };
            self.awaiting_angle = false;
            return Some(CmdResult::NeedPoint);
        }
        if !self.placement_step() {
            return None;
        }
        match text.trim().to_ascii_uppercase().as_str() {
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
            "Q" | "QUADRANT" => Some(CmdResult::NeedPoint),
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
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(curve) = picked_curve(&entity, point) else {
            return CmdResult::NeedPoint;
        };

        match (&self.step, curve) {
            (
                Step::SecondLine { first_start, first_end, first_source },
                PickedCurve::Line(second_start, second_end),
            ) if *first_source != handle => {
                self.source_handles = vec![*first_source, *first_source, handle, handle];
                self.selecting_object = false;
                self.step = Step::ArcPoint2 {
                    first_start: *first_start,
                    first_end: *first_end,
                    second_start,
                    second_end,
                };
                CmdResult::NeedPoint
            }
            (Step::Vertex, PickedCurve::Line(first_start, first_end)) => {
                self.step = Step::SecondLine {
                    first_start,
                    first_end,
                    first_source: handle,
                };
                CmdResult::NeedPoint
            }
            (Step::Vertex, PickedCurve::Arc { center, first, second }) => {
                self.source_handles = vec![handle, handle, handle];
                self.selecting_object = false;
                self.step = Step::ArcPoint3 { vertex: center, first, second };
                CmdResult::NeedPoint
            }
            (Step::Vertex, PickedCurve::Circle { center, radius }) => {
                let Some(first) = project_to_radius(center, point, radius) else {
                    return CmdResult::NeedPoint;
                };
                self.selecting_object = false;
                self.step = Step::CircleSecondRay {
                    vertex: center,
                    first,
                    radius,
                    source: handle,
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        let points = match self.step {
            Step::Vertex | Step::SecondLine { .. } => return None,
            Step::FirstRay(vertex) => vec![vertex, point],
            Step::SecondRay { vertex, first } => vec![vertex, first, nan(), vertex, point],
            Step::CircleSecondRay { vertex, first, radius, .. } => {
                let second = project_to_radius(vertex, point, radius).unwrap_or(point);
                vec![vertex, first, nan(), vertex, second]
            }
            Step::ArcPoint3 { vertex, first, second } => {
                angular_preview(vertex, first, second, point)
            }
            Step::ArcPoint2 {
                first_start,
                first_end,
                second_start,
                second_end,
            } => two_line_preview(
                first_start,
                first_end,
                second_start,
                second_end,
                point,
            ),
        };
        Some(preview_wire(points))
    }
}

fn picked_curve(entity: &EntityType, click: DVec3) -> Option<PickedCurve> {
    let point = |value: Vector3| DVec3::new(value.x, value.y, value.z);
    match entity {
        EntityType::Line(line) => Some(PickedCurve::Line(point(line.start), point(line.end))),
        EntityType::Arc(arc) => Some(PickedCurve::Arc {
            center: point(arc.center_wcs()),
            first: point(arc.start_point_wcs()),
            second: point(arc.end_point_wcs()),
        }),
        EntityType::Circle(circle) => Some(PickedCurve::Circle {
            center: point(circle.center_wcs()),
            radius: circle.radius,
        }),
        EntityType::LwPolyline(polyline) => {
            let click = crate::scene::view::transform::wcs_point_to_ocs(
                (click.x, click.y, click.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let segment = nearest_segment_index(
                polyline.vertices.iter().map(|vertex| [vertex.location.x, vertex.location.y]).collect(),
                polyline.is_closed,
                [click.0, click.1],
            )?;
            let first = polyline.vertices[segment];
            let second = polyline.vertices[(segment + 1) % polyline.vertices.len()];
            polyline_segment(
                [first.location.x, first.location.y],
                [second.location.x, second.location.y],
                first.bulge,
                polyline.elevation,
                polyline.normal,
            )
        }
        EntityType::Polyline2D(polyline) => {
            let vertices = crate::entities::polyline::drawn_vertices2d(polyline)
                .unwrap_or_else(|| polyline.vertices.clone());
            let click = crate::scene::view::transform::wcs_point_to_ocs(
                (click.x, click.y, click.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let segment = nearest_segment_index(
                vertices.iter().map(|vertex| [vertex.location.x, vertex.location.y]).collect(),
                polyline.is_closed(),
                [click.0, click.1],
            )?;
            let first = &vertices[segment];
            let second = &vertices[(segment + 1) % vertices.len()];
            polyline_segment(
                [first.location.x, first.location.y],
                [second.location.x, second.location.y],
                first.bulge,
                polyline.elevation,
                polyline.normal,
            )
        }
        _ => None,
    }
}

fn nearest_segment_index(points: Vec<[f64; 2]>, closed: bool, click: [f64; 2]) -> Option<usize> {
    if points.len() < 2 {
        return None;
    }
    let count = if closed { points.len() } else { points.len() - 1 };
    (0..count).min_by(|first, second| {
        segment_distance_squared(points[*first], points[(*first + 1) % points.len()], click)
            .total_cmp(&segment_distance_squared(
                points[*second],
                points[(*second + 1) % points.len()],
                click,
            ))
    })
}

fn segment_distance_squared(first: [f64; 2], second: [f64; 2], point: [f64; 2]) -> f64 {
    let dx = second[0] - first[0];
    let dy = second[1] - first[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 1.0e-24 {
        return (point[0] - first[0]).powi(2) + (point[1] - first[1]).powi(2);
    }
    let t = (((point[0] - first[0]) * dx + (point[1] - first[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    let x = first[0] + dx * t;
    let y = first[1] + dy * t;
    (point[0] - x).powi(2) + (point[1] - y).powi(2)
}

fn polyline_segment(
    first: [f64; 2],
    second: [f64; 2],
    bulge: f64,
    elevation: f64,
    normal: Vector3,
) -> Option<PickedCurve> {
    let to_world = |point: [f64; 2]| {
        let value = crate::scene::view::transform::ocs_point_to_wcs(
            (point[0], point[1], elevation),
            (normal.x, normal.y, normal.z),
        );
        DVec3::new(value.0, value.1, value.2)
    };
    let first_world = to_world(first);
    let second_world = to_world(second);
    if bulge.abs() <= 1.0e-12 {
        return Some(PickedCurve::Line(first_world, second_world));
    }
    let dx = second[0] - first[0];
    let dy = second[1] - first[1];
    let chord = (dx * dx + dy * dy).sqrt();
    if chord <= 1.0e-12 {
        return None;
    }
    let midpoint = [(first[0] + second[0]) * 0.5, (first[1] + second[1]) * 0.5];
    let center_offset = chord * (1.0 - bulge * bulge) / (4.0 * bulge);
    let center = [
        midpoint[0] - dy / chord * center_offset,
        midpoint[1] + dx / chord * center_offset,
    ];
    Some(PickedCurve::Arc {
        center: to_world(center),
        first: first_world,
        second: second_world,
    })
}

fn project_to_radius(center: DVec3, point: DVec3, radius: f64) -> Option<DVec3> {
    let direction = (point - center).normalize_or_zero();
    (direction.length_squared() > 0.0).then_some(center + direction * radius)
}

fn two_line_frame(
    first_start: DVec3,
    first_end: DVec3,
    second_start: DVec3,
    second_end: DVec3,
    arc_point: DVec3,
) -> Option<(DVec3, f64, f64)> {
    let first_direction = first_end - first_start;
    let second_direction = second_end - second_start;
    let denominator = first_direction.x * second_direction.y - first_direction.y * second_direction.x;
    if denominator.abs()
        <= 1.0e-12 * first_direction.length().max(second_direction.length()).max(1.0)
    {
        return None;
    }
    let offset = second_start - first_start;
    let t = (offset.x * second_direction.y - offset.y * second_direction.x) / denominator;
    let vertex = first_start + first_direction * t;
    let target = (arc_point.y - vertex.y).atan2(arc_point.x - vertex.x);
    let mut best: Option<(f64, f64, f64)> = None;
    for first_sign in [1.0, -1.0] {
        for second_sign in [1.0, -1.0] {
            let first = first_direction * first_sign;
            let second = second_direction * second_sign;
            for (start, end) in [
                (first.y.atan2(first.x), second.y.atan2(second.x)),
                (second.y.atan2(second.x), first.y.atan2(first.x)),
            ] {
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sweep <= 1.0e-9 || sweep > std::f64::consts::PI {
                    continue;
                }
                let selected = (target - start).rem_euclid(std::f64::consts::TAU);
                if selected <= sweep + 1.0e-9 && best.is_none_or(|known| sweep < known.2) {
                    best = Some((start, start + sweep, sweep));
                }
            }
        }
    }
    best.map(|(start, end, _)| (vertex, start, end))
}

fn angular_preview(vertex: DVec3, first: DVec3, second: DVec3, arc_point: DVec3) -> Vec<DVec3> {
    let Some((_, start, end)) = two_line_frame(vertex, first, vertex, second, arc_point) else {
        return vec![vertex, first, nan(), vertex, second];
    };
    angular_preview_with_frame(vertex, first, second, arc_point, start, end)
}

fn two_line_preview(
    first_start: DVec3,
    first_end: DVec3,
    second_start: DVec3,
    second_end: DVec3,
    arc_point: DVec3,
) -> Vec<DVec3> {
    let Some((vertex, start, end)) = two_line_frame(
        first_start,
        first_end,
        second_start,
        second_end,
        arc_point,
    ) else {
        return vec![first_start, first_end, nan(), second_start, second_end];
    };
    angular_preview_with_frame(vertex, first_start, second_start, arc_point, start, end)
}

fn angular_preview_with_frame(
    vertex: DVec3,
    first: DVec3,
    second: DVec3,
    arc_point: DVec3,
    start: f64,
    end: f64,
) -> Vec<DVec3> {
    let radius = vertex.distance(arc_point);
    let first_end = vertex + DVec3::new(start.cos(), start.sin(), 0.0) * radius;
    let second_end = vertex + DVec3::new(end.cos(), end.sin(), 0.0) * radius;
    let mut points = vec![first, first_end, nan(), second, second_end, nan()];
    let sweep = end - start;
    let steps = ((sweep.abs() / std::f64::consts::PI * 90.0).ceil() as usize).clamp(8, 720);
    for index in 0..=steps {
        let angle = start + sweep * index as f64 / steps as f64;
        points.push(vertex + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0));
    }
    points
}

fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn nan() -> DVec3 {
    DVec3::splat(f64::NAN)
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
        name: "dimangular_preview".to_string(),
        points: points.into_iter().map(|point| [point.x as f32, point.y as f32, point.z as f32]).collect(),
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

inventory::submit!(crate::command::CommandRegistration { names: &["DIMANGULAR"] });
