use acadrust::entities::{Dimension, DimensionAngular2Ln, DimensionAngular3Pt};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use cadkernel::geom2d::{
    closest_point, line_line, Arc as KernelArc, BulgeArc, Curve as KernelCurve,
    Line as KernelLine,
};

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput,
    DimensionAssociationSource, WorkingPlane,
};
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
        source: Handle,
    },
    SecondLine {
        first_start: DVec3,
        first_end: DVec3,
        first_source: Handle,
        first_refs: [SourceLocator; 2],
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceLocator {
    marker: Option<i32>,
    parameter: f64,
}

impl SourceLocator {
    const INFERRED: Self = Self { marker: None, parameter: 0.0 };

    const fn explicit(marker: i32, parameter: f64) -> Self {
        Self { marker: Some(marker), parameter }
    }

    fn bind(self, handle: Handle) -> DimensionAssociationSource {
        match self.marker {
            Some(marker) => DimensionAssociationSource::explicit(handle, marker, self.parameter),
            None => DimensionAssociationSource::inferred(handle),
        }
    }
}

enum PickedCurve {
    Line {
        start: DVec3,
        end: DVec3,
        refs: [SourceLocator; 2],
        normal: Option<DVec3>,
    },
    Arc {
        center: DVec3,
        first: DVec3,
        second: DVec3,
        refs: [SourceLocator; 3],
        normal: DVec3,
    },
    Circle {
        center: DVec3,
        normal: DVec3,
    },
}

pub struct AngularDimensionCommand {
    step: Step,
    plane: WorkingPlane,
    selecting_object: bool,
    picked_entity: Option<EntityType>,
    source_refs: Vec<Option<DimensionAssociationSource>>,
    text_override: Option<String>,
    awaiting_text: bool,
    mtext_override: bool,
    text_angle: Option<f64>,
    awaiting_angle: bool,
    angle_origin: Option<DVec3>,
    awaiting_quadrant: bool,
    quadrant_lock: Option<(f64, f64)>,
}

impl AngularDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Vertex,
            plane: WorkingPlane::default(),
            selecting_object: true,
            picked_entity: None,
            source_refs: Vec::new(),
            text_override: None,
            awaiting_text: false,
            mtext_override: false,
            text_angle: None,
            awaiting_angle: false,
            angle_origin: None,
            awaiting_quadrant: false,
            quadrant_lock: None,
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
        let picked_text = self.plane.to_local(arc_point);
        let arc_point = locked_arc_point(vertex, picked_text, self.quadrant_lock);
        if two_line_frame(vertex, first, vertex, second, arc_point).is_none() {
            return CmdResult::NeedPoint;
        }
        let mut dim = DimensionAngular3Pt::new(v3(vertex), v3(first), v3(second));
        dim.definition_point = v3(arc_point);
        dim.base.definition_point = dim.definition_point;
        dim.base.text_middle_point = dim.definition_point;
        dim.base.insertion_point = dim.definition_point;
        if self.quadrant_lock.is_some_and(|frame| {
            !point_angle_in_frame(vertex, picked_text, frame)
        }) {
            dim.base.text_middle_point = v3(picked_text);
            dim.base.insertion_point = dim.base.text_middle_point;
            dim.base.text_user_positioned = true;
        }
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
        let picked_text = self.plane.to_local(arc_point);
        let Some((vertex, _, _)) = two_line_frame(
            first_start,
            first_end,
            second_start,
            second_end,
            picked_text,
        ) else {
            return CmdResult::NeedPoint;
        };
        let arc_point = locked_arc_point(vertex, picked_text, self.quadrant_lock);
        if two_line_frame(
            first_start,
            first_end,
            second_start,
            second_end,
            arc_point,
        )
        .is_none()
        {
            return CmdResult::NeedPoint;
        }
        let mut dim = DimensionAngular2Ln::default();
        dim.first_point = v3(first_start);
        dim.second_point = v3(first_end);
        dim.angle_vertex = v3(second_start);
        dim.definition_point = v3(second_end);
        dim.dimension_arc = v3(arc_point);
        dim.base.definition_point = dim.definition_point;
        dim.base.text_middle_point = dim.dimension_arc;
        dim.base.insertion_point = dim.dimension_arc;
        if self.quadrant_lock.is_some_and(|frame| {
            !point_angle_in_frame(vertex, picked_text, frame)
        }) {
            dim.base.text_middle_point = v3(picked_text);
            dim.base.insertion_point = dim.base.text_middle_point;
            dim.base.text_user_positioned = true;
        }
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
        CmdResult::CommitDimension {
            entity,
            association: DimensionAssociationInput::Explicit(self.source_refs.clone()),
            preserve_base_style: false,
            continue_command: false,
        }
    }

    fn placement_step(&self) -> bool {
        matches!(self.step, Step::ArcPoint3 { .. } | Step::ArcPoint2 { .. })
    }

    fn editor_anchor(&self) -> DVec3 {
        match self.step {
            Step::ArcPoint3 { vertex, first, second } => {
                let radius = vertex.distance(first).max(vertex.distance(second)).max(1.0);
                let first_angle = self.plane.angle(vertex, first).unwrap_or(0.0);
                let second_angle = self.plane.angle(vertex, second).unwrap_or(first_angle);
                let sweep = (second_angle - first_angle).rem_euclid(std::f64::consts::TAU);
                let angle = first_angle + sweep * 0.5;
                self.plane.to_world(
                    self.plane.to_local(vertex)
                        + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0),
                )
            }
            Step::ArcPoint2 {
                first_start,
                first_end,
                second_start,
                second_end,
            } => {
                let first_start = self.plane.to_local(first_start);
                let first_end = self.plane.to_local(first_end);
                let second_start = self.plane.to_local(second_start);
                let second_end = self.plane.to_local(second_end);
                two_line_frame(
                    first_start,
                    first_end,
                    second_start,
                    second_end,
                    (first_start + second_start) * 0.5,
                )
                .map(|(vertex, start, end)| {
                    self.plane.to_world(vertex + DVec3::new(
                        ((start + end) * 0.5).cos(),
                        ((start + end) * 0.5).sin(),
                        0.0,
                    ))
                })
                .unwrap_or_else(|| self.plane.to_world(first_start))
            }
            _ => self.plane.origin,
        }
    }

    fn set_quadrant_from_point(&mut self, point: DVec3) -> bool {
        let point = self.plane.to_local(point);
        let frame = match self.step {
            Step::ArcPoint3 { vertex, first, second } => two_line_frame(
                self.plane.to_local(vertex),
                self.plane.to_local(first),
                self.plane.to_local(vertex),
                self.plane.to_local(second),
                point,
            ),
            Step::ArcPoint2 {
                first_start,
                first_end,
                second_start,
                second_end,
            } => two_line_frame(
                self.plane.to_local(first_start),
                self.plane.to_local(first_end),
                self.plane.to_local(second_start),
                self.plane.to_local(second_end),
                point,
            ),
            _ => None,
        };
        if let Some((_, start, end)) = frame {
            self.quadrant_lock = Some((start, end));
            true
        } else {
            false
        }
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
        if self.awaiting_quadrant {
            return t!("DIMANGULAR  Specify quadrant:").into_owned();
        }
        if self.awaiting_text {
            return if self.mtext_override {
                t!("DIMANGULAR  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMANGULAR  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return if self.angle_origin.is_some() {
                t!("DIMANGULAR  Specify second point for text angle:").into_owned()
            } else {
                t!("DIMANGULAR  Specify text angle or first point:").into_owned()
            };
        }
        if self.selecting_object {
            return match self.step {
                Step::SecondLine { .. } => t!("DIMANGULAR  Select second line:").into_owned(),
                _ => t!(
                    "DIMANGULAR  Select arc, circle, line, or specify an angle vertex:"
                )
                .into_owned(),
            };
        }
        match self.step {
            Step::Vertex => t!(
                "DIMANGULAR  Specify angle vertex:"
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
        if self.awaiting_quadrant {
            if self.set_quadrant_from_point(point) {
                self.awaiting_quadrant = false;
            }
            return CmdResult::NeedPoint;
        }
        if self.awaiting_angle {
            if let Some(origin) = self.angle_origin {
                if let Some(angle) = self.plane.angle(origin, point) {
                    self.text_angle = Some(angle);
                    self.awaiting_angle = false;
                    self.angle_origin = None;
                }
            } else {
                self.angle_origin = Some(point);
            }
            return CmdResult::NeedPoint;
        }
        if self.selecting_object && matches!(self.step, Step::Vertex) {
            self.selecting_object = false;
            self.step = Step::FirstRay(point);
            return CmdResult::NeedPoint;
        }
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
                let first_local = self.plane.vector_to_local(first - vertex);
                let second_local = self.plane.vector_to_local(point - vertex);
                if line_line(
                    [0.0, 0.0],
                    [first_local.x, first_local.y],
                    [0.0, 0.0],
                    [second_local.x, second_local.y],
                )
                .is_none()
                {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::ArcPoint3 { vertex, first, second: point };
                CmdResult::NeedPoint
            }
            Step::CircleSecondRay { vertex, first, source } => {
                if point.distance_squared(vertex) <= 1.0e-24 {
                    return CmdResult::NeedPoint;
                }
                self.source_refs = vec![
                    Some(DimensionAssociationSource::inferred(source)),
                    Some(DimensionAssociationSource::inferred(source)),
                    None,
                ];
                self.step = Step::ArcPoint3 { vertex, first, second: point };
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
        if self.awaiting_quadrant {
            self.awaiting_quadrant = false;
            return CmdResult::NeedPoint;
        }
        if self.awaiting_text {
            self.awaiting_text = false;
            return CmdResult::NeedPoint;
        }
        if self.awaiting_angle {
            self.awaiting_angle = false;
            self.angle_origin = None;
            return CmdResult::NeedPoint;
        }
        if matches!(self.step, Step::Vertex) {
            self.selecting_object = !self.selecting_object;
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

    fn options(&self) -> Vec<CmdOption> {
        if self.placement_step() && !self.awaiting_text && !self.awaiting_angle && !self.awaiting_quadrant {
            vec![
                CmdOption::new("MText", "MTEXT"),
                CmdOption::new("Text", "TEXT"),
                CmdOption::new("Angle", "ANGLE"),
                CmdOption::new("Quadrant", "QUADRANT"),
            ]
        } else {
            Vec::new()
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.placement_step()
            && !self.awaiting_text
            && !self.awaiting_angle
            && !self.awaiting_quadrant
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
            self.angle_origin = None;
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
                Some(CmdResult::SuspendForMTextInput {
                    pos: self.editor_anchor(),
                    initial: self.text_override.clone().unwrap_or_default(),
                    height: 2.5,
                })
            }
            "A" | "ANGLE" => {
                self.awaiting_angle = true;
                self.angle_origin = None;
                Some(CmdResult::NeedPoint)
            }
            "Q" | "QUADRANT" => {
                self.awaiting_quadrant = true;
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
        self.selecting_object
    }

    fn entity_pick_accepts_points(&self) -> bool {
        self.selecting_object && matches!(self.step, Step::Vertex)
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
            if matches!(self.step, Step::Vertex) {
                self.selecting_object = false;
                self.step = Step::FirstRay(point);
            }
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(curve) = picked_curve(&entity, point) else {
            return CmdResult::NeedPoint;
        };
        if !curve_is_coplanar(&curve, self.plane) {
            return CmdResult::NeedPoint;
        }

        match (&self.step, curve) {
            (
                Step::SecondLine {
                    first_start,
                    first_end,
                    first_source,
                    first_refs,
                },
                PickedCurve::Line {
                    start: second_start,
                    end: second_end,
                    refs: second_refs,
                    ..
                },
            ) => {
                if (*first_source == handle && *first_refs == second_refs)
                    || lines_parallel_in_plane(
                        self.plane,
                        *first_start,
                        *first_end,
                        second_start,
                        second_end,
                    )
                {
                    return CmdResult::NeedPoint;
                }
                self.source_refs = vec![
                    Some(first_refs[0].bind(*first_source)),
                    Some(first_refs[1].bind(*first_source)),
                    Some(second_refs[0].bind(handle)),
                    Some(second_refs[1].bind(handle)),
                ];
                self.selecting_object = false;
                self.step = Step::ArcPoint2 {
                    first_start: *first_start,
                    first_end: *first_end,
                    second_start,
                    second_end,
                };
                CmdResult::NeedPoint
            }
            (
                Step::Vertex,
                PickedCurve::Line {
                    start: first_start,
                    end: first_end,
                    refs,
                    ..
                },
            ) => {
                self.step = Step::SecondLine {
                    first_start,
                    first_end,
                    first_source: handle,
                    first_refs: refs,
                };
                CmdResult::NeedPoint
            }
            (
                Step::Vertex,
                PickedCurve::Arc {
                    center,
                    first,
                    second,
                    refs,
                    ..
                },
            ) => {
                self.source_refs = refs
                    .into_iter()
                    .map(|reference| Some(reference.bind(handle)))
                    .collect();
                self.selecting_object = false;
                self.step = Step::ArcPoint3 { vertex: center, first, second };
                CmdResult::NeedPoint
            }
            (Step::Vertex, PickedCurve::Circle { center, .. }) => {
                if point.distance_squared(center) <= 1.0e-24 {
                    return CmdResult::NeedPoint;
                }
                self.selecting_object = false;
                self.step = Step::CircleSecondRay {
                    vertex: center,
                    first: point,
                    source: handle,
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if self.awaiting_angle {
            return self
                .angle_origin
                .map(|origin| preview_wire(vec![origin, point]));
        }
        let points = match self.step {
            Step::Vertex | Step::SecondLine { .. } => return None,
            Step::FirstRay(vertex) => vec![vertex, point],
            Step::SecondRay { vertex, first } => vec![vertex, first, nan(), vertex, point],
            Step::CircleSecondRay { vertex, first, .. } => {
                vec![vertex, first, nan(), vertex, point]
            }
            Step::ArcPoint3 { vertex, first, second } => {
                let vertex = self.plane.to_local(vertex);
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let point = locked_arc_point(
                    vertex,
                    self.plane.to_local(point),
                    self.quadrant_lock,
                );
                angular_preview(vertex, first, second, point)
                    .into_iter()
                    .map(|value| if value.is_nan() { value } else { self.plane.to_world(value) })
                    .collect()
            }
            Step::ArcPoint2 {
                first_start,
                first_end,
                second_start,
                second_end,
            } => {
                let first_start = self.plane.to_local(first_start);
                let first_end = self.plane.to_local(first_end);
                let second_start = self.plane.to_local(second_start);
                let second_end = self.plane.to_local(second_end);
                let point = self.plane.to_local(point);
                let point = two_line_frame(
                    first_start,
                    first_end,
                    second_start,
                    second_end,
                    point,
                )
                .map(|(vertex, _, _)| locked_arc_point(vertex, point, self.quadrant_lock))
                .unwrap_or(point);
                two_line_preview(
                    first_start,
                    first_end,
                    second_start,
                    second_end,
                    point,
                )
                .into_iter()
                .map(|value| if value.is_nan() { value } else { self.plane.to_world(value) })
                .collect()
            }
        };
        Some(preview_wire(points))
    }
}

fn picked_curve(entity: &EntityType, click: DVec3) -> Option<PickedCurve> {
    let point = |value: Vector3| DVec3::new(value.x, value.y, value.z);
    let normal = |value: Vector3| DVec3::new(value.x, value.y, value.z);
    match entity {
        EntityType::Line(line) => {
            let start = point(line.start);
            let end = point(line.end);
            (start.distance_squared(end) > 1.0e-24).then_some(PickedCurve::Line {
                start,
                end,
                refs: [SourceLocator::INFERRED; 2],
                normal: None,
            })
        }
        EntityType::Arc(arc) => Some(PickedCurve::Arc {
            center: point(arc.center_wcs()),
            first: point(arc.start_point_wcs()),
            second: point(arc.end_point_wcs()),
            refs: [SourceLocator::INFERRED; 3],
            normal: normal(arc.normal),
        }),
        EntityType::Circle(circle) => Some(PickedCurve::Circle {
            center: point(circle.center_wcs()),
            normal: normal(circle.normal),
        }),
        EntityType::LwPolyline(polyline) => {
            let click = crate::scene::view::transform::wcs_point_to_ocs(
                (click.x, click.y, click.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let points: Vec<_> = polyline
                .vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y])
                .collect();
            let bulges: Vec<_> = polyline.vertices.iter().map(|vertex| vertex.bulge).collect();
            let segment = nearest_segment_index(
                &points,
                &bulges,
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
                segment,
                polyline.vertices.len(),
            )
        }
        EntityType::Polyline2D(polyline) => {
            let vertices = &polyline.vertices;
            let click = crate::scene::view::transform::wcs_point_to_ocs(
                (click.x, click.y, click.z),
                (polyline.normal.x, polyline.normal.y, polyline.normal.z),
            );
            let points: Vec<_> = vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y])
                .collect();
            let bulges: Vec<_> = vertices.iter().map(|vertex| vertex.bulge).collect();
            let segment = nearest_segment_index(
                &points,
                &bulges,
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
                segment,
                vertices.len(),
            )
        }
        _ => None,
    }
}

fn nearest_segment_index(
    points: &[[f64; 2]],
    bulges: &[f64],
    closed: bool,
    click: [f64; 2],
) -> Option<usize> {
    if points.len() < 2 {
        return None;
    }
    let count = if closed { points.len() } else { points.len() - 1 };
    (0..count).min_by(|first, second| {
        polyline_segment_distance_squared(
            points[*first],
            points[(*first + 1) % points.len()],
            bulges.get(*first).copied().unwrap_or(0.0),
            click,
        )
        .total_cmp(&polyline_segment_distance_squared(
                points[*second],
                points[(*second + 1) % points.len()],
                bulges.get(*second).copied().unwrap_or(0.0),
                click,
            ))
    })
}

fn polyline_segment_curve(first: [f64; 2], second: [f64; 2], bulge: f64) -> KernelCurve {
    if let Some(arc) = BulgeArc::from_bulge(first, second, bulge) {
        let (start_angle, end_angle) = if arc.sweep >= 0.0 {
            (arc.start_angle, arc.end_angle)
        } else {
            (arc.end_angle, arc.start_angle)
        };
        KernelCurve::Arc(KernelArc {
            centre: arc.center,
            radius: arc.radius,
            start_angle,
            end_angle,
        })
    } else {
        KernelCurve::Line(KernelLine { start: first, end: second })
    }
}

fn polyline_segment_distance_squared(
    first: [f64; 2],
    second: [f64; 2],
    bulge: f64,
    point: [f64; 2],
) -> f64 {
    closest_point(&polyline_segment_curve(first, second, bulge), point)
        .distance
        .powi(2)
}

fn polyline_segment(
    first: [f64; 2],
    second: [f64; 2],
    bulge: f64,
    elevation: f64,
    normal: Vector3,
    segment: usize,
    vertex_count: usize,
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
    if first_world.distance_squared(second_world) <= 1.0e-24 {
        return None;
    }
    let refs = [
        SourceLocator::explicit(segment as i32, 0.0),
        SourceLocator::explicit(((segment + 1) % vertex_count) as i32, 0.0),
    ];
    let normal_world = DVec3::new(normal.x, normal.y, normal.z);
    if bulge.abs() <= 1.0e-12 {
        return Some(PickedCurve::Line {
            start: first_world,
            end: second_world,
            refs,
            normal: Some(normal_world),
        });
    }
    let center = BulgeArc::from_bulge(first, second, bulge)?.center;
    Some(PickedCurve::Arc {
        center: to_world(center),
        first: first_world,
        second: second_world,
        refs: [
            SourceLocator::explicit(-4, segment as f64),
            refs[0],
            refs[1],
        ],
        normal: normal_world,
    })
}

fn curve_is_coplanar(curve: &PickedCurve, plane: WorkingPlane) -> bool {
    let (points, normal): (Vec<DVec3>, Option<DVec3>) = match curve {
        PickedCurve::Line { start, end, normal, .. } => (vec![*start, *end], *normal),
        PickedCurve::Arc { center, first, second, normal, .. } => {
            (vec![*center, *first, *second], Some(*normal))
        }
        PickedCurve::Circle { center, normal } => (vec![*center], Some(*normal)),
    };
    if let Some(normal) = normal {
        let alignment = normal.normalize_or_zero().dot(plane.z).abs();
        if alignment < 1.0 - 1.0e-7 {
            return false;
        }
    }
    let local: Vec<_> = points.into_iter().map(|point| plane.to_local(point)).collect();
    let scale = local
        .iter()
        .map(|point| point.x.abs().max(point.y.abs()).max(1.0))
        .fold(1.0, f64::max);
    local.iter().all(|point| point.z.abs() <= 1.0e-9 * scale)
}

fn lines_parallel_in_plane(
    plane: WorkingPlane,
    first_start: DVec3,
    first_end: DVec3,
    second_start: DVec3,
    second_end: DVec3,
) -> bool {
    let first_start = plane.to_local(first_start);
    let first_end = plane.to_local(first_end);
    let second_start = plane.to_local(second_start);
    let second_end = plane.to_local(second_end);
    let first = first_end - first_start;
    let second = second_end - second_start;
    line_line(
        [first_start.x, first_start.y],
        [first.x, first.y],
        [second_start.x, second_start.y],
        [second.x, second.y],
    )
    .is_none()
}

fn locked_arc_point(
    vertex: DVec3,
    point: DVec3,
    frame: Option<(f64, f64)>,
) -> DVec3 {
    let Some((start, end)) = frame else { return point };
    let radius = ((point.x - vertex.x).powi(2) + (point.y - vertex.y).powi(2)).sqrt();
    if radius <= 1.0e-12 {
        return point;
    }
    let angle = (start + end) * 0.5;
    DVec3::new(
        vertex.x + angle.cos() * radius,
        vertex.y + angle.sin() * radius,
        vertex.z,
    )
}

fn point_angle_in_frame(vertex: DVec3, point: DVec3, frame: (f64, f64)) -> bool {
    let angle = (point.y - vertex.y).atan2(point.x - vertex.x);
    let sweep = frame.1 - frame.0;
    (angle - frame.0).rem_euclid(std::f64::consts::TAU) <= sweep + 1.0e-9
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
    let (t, _) = line_line(
        [first_start.x, first_start.y],
        [first_direction.x, first_direction.y],
        [second_start.x, second_start.y],
        [second_direction.x, second_direction.y],
    )?;
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
    points.extend(
        cadkernel::geom2d::tessellate::arc(
            [vertex.x, vertex.y],
            radius,
            start,
            end,
            vertex.z,
            cadkernel::geom2d::tessellate::DEFAULT_SEGMENTS_PER_RADIAN,
        )
        .into_iter()
        .map(DVec3::from_array),
    );
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
