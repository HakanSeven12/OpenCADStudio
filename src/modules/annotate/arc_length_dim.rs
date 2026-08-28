use acadrust::entities::{Dimension, DimensionArc};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use cadkernel::geom2d::tessellate::{arc, DEFAULT_SEGMENTS_PER_RADIAN};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput,
    DimensionAssociationSource, WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::dimension_assoc::{
    polyline_arc_point_marker, RadialSourceGeometry,
    ARC_DIMENSION_POINT_MARKER, POLYLINE_ARC_CENTER_MARKER,
};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/dim_angular.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMARC",
        label: "Arc Length",
        icon: ICON,
        event: ModuleEvent::Command("DIMARC".to_string()),
    }
}

#[derive(Clone, Copy)]
enum SourceBinding {
    Arc,
    PolylineSegment(i32),
}

#[derive(Clone, Copy)]
struct ArcSelection {
    source: RadialSourceGeometry,
    binding: SourceBinding,
    start_angle: f64,
    end_angle: f64,
    is_partial: bool,
}

impl ArcSelection {
    fn new(source: RadialSourceGeometry, binding: SourceBinding) -> Option<Self> {
        let sweep = positive_sweep(source.start_angle, source.end_angle);
        (source.limited && source.radius > 1.0e-12 && sweep > 1.0e-12).then_some(Self {
            source,
            binding,
            start_angle: source.start_angle,
            end_angle: source.start_angle + sweep,
            is_partial: false,
        })
    }

    fn sweep(self) -> f64 {
        positive_sweep(self.start_angle, self.end_angle)
    }

    fn point_at(self, angle: f64) -> DVec3 {
        dvec(self.source.point_at_angle(angle))
    }

    fn center(self) -> DVec3 {
        dvec(self.source.center_world())
    }

    fn project(self, point: DVec3) -> Option<DVec3> {
        let point = self.source.plane.project(point.to_array())?;
        Some(DVec3::from_array(self.source.plane.point_at(point)))
    }

    fn plane(self) -> WorkingPlane {
        WorkingPlane::new(
            DVec3::from_array(self.source.plane.origin),
            DVec3::from_array(self.source.plane.x_axis),
            DVec3::from_array(self.source.plane.y_axis),
        )
    }

    fn clamped_angle(self, point: DVec3) -> f64 {
        let raw = self.source.angle_at(point.to_array());
        let relative = (raw - self.start_angle).rem_euclid(std::f64::consts::TAU);
        let sweep = self.sweep();
        if relative <= sweep + 1.0e-10 {
            return self.start_angle + relative.min(sweep);
        }
        let start_distance = relative.min(std::f64::consts::TAU - relative);
        let end_relative = (raw - self.end_angle).rem_euclid(std::f64::consts::TAU);
        let end_distance = end_relative.min(std::f64::consts::TAU - end_relative);
        if start_distance <= end_distance {
            self.start_angle
        } else {
            self.end_angle
        }
    }

    fn with_partial(self, first: f64, second: f64) -> Option<Self> {
        let first = (first - self.start_angle)
            .rem_euclid(std::f64::consts::TAU)
            .clamp(0.0, self.sweep());
        let second = (second - self.start_angle)
            .rem_euclid(std::f64::consts::TAU)
            .clamp(0.0, self.sweep());
        let (start, end) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        (end - start > 1.0e-12).then_some(Self {
            start_angle: self.start_angle + start,
            end_angle: self.start_angle + end,
            is_partial: true,
            ..self
        })
    }

    fn association_sources(self, handle: Handle) -> Vec<Option<DimensionAssociationSource>> {
        let source_sweep = positive_sweep(self.source.start_angle, self.source.end_angle);
        let parameter = |angle: f64| {
            ((angle - self.source.start_angle) / source_sweep).clamp(0.0, 1.0)
        };
        match self.binding {
            SourceBinding::Arc => vec![
                Some(DimensionAssociationSource::explicit(handle, -3, 0.0)),
                Some(DimensionAssociationSource::explicit(
                    handle,
                    ARC_DIMENSION_POINT_MARKER,
                    parameter(self.start_angle),
                )),
                Some(DimensionAssociationSource::explicit(
                    handle,
                    ARC_DIMENSION_POINT_MARKER,
                    parameter(self.end_angle),
                )),
            ],
            SourceBinding::PolylineSegment(segment) => {
                let point_marker = polyline_arc_point_marker(segment);
                vec![
                    Some(DimensionAssociationSource::explicit(
                        handle,
                        POLYLINE_ARC_CENTER_MARKER,
                        segment as f64,
                    )),
                    Some(DimensionAssociationSource::explicit(
                        handle,
                        point_marker,
                        parameter(self.start_angle),
                    )),
                    Some(DimensionAssociationSource::explicit(
                        handle,
                        point_marker,
                        parameter(self.end_angle),
                    )),
                ]
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    SelectObject,
    DimLine(ArcSelection),
    PartialFirst(ArcSelection),
    PartialSecond {
        selection: ArcSelection,
        first_angle: f64,
    },
}

pub struct ArcLengthDimensionCommand {
    step: Step,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
    text_override: Option<String>,
    awaiting_text: bool,
    text_angle: Option<f64>,
    awaiting_angle: bool,
    leader_enabled: bool,
}

impl ArcLengthDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::SelectObject,
            picked_entity: None,
            source_handle: None,
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
            leader_enabled: false,
        }
    }

    fn editor_anchor(&self) -> DVec3 {
        match self.step {
            Step::SelectObject => DVec3::ZERO,
            Step::DimLine(selection)
            | Step::PartialFirst(selection)
            | Step::PartialSecond { selection, .. } => {
                selection.point_at(selection.start_angle + selection.sweep() * 0.5)
            }
        }
    }

    fn commit_dimension(&self, selection: ArcSelection, point: DVec3) -> CmdResult {
        let plane = selection.plane();
        let center = plane.to_local(selection.center());
        let first = plane.to_local(selection.point_at(selection.start_angle));
        let second = plane.to_local(selection.point_at(selection.end_angle));
        let Some(point) = selection.project(point) else {
            return CmdResult::NeedPoint;
        };
        let picked = plane.to_local(point);
        let picked_radius = (picked - center).truncate().length();
        if !picked_radius.is_finite() || picked_radius <= 1.0e-12 {
            return CmdResult::NeedPoint;
        }

        let mut dimension = DimensionArc::default();
        dimension.center_point = v3(center);
        dimension.first_extension_point = v3(first);
        dimension.second_extension_point = v3(second);
        dimension.definition_point = v3(picked);
        dimension.is_partial = selection.is_partial;
        dimension.arc_start_parameter = selection.start_angle;
        dimension.arc_end_parameter = selection.end_angle;
        dimension.base.definition_point = dimension.definition_point;
        dimension.base.text_middle_point = dimension.definition_point;
        dimension.base.insertion_point = dimension.definition_point;
        dimension.has_leader = self.leader_enabled && selection.sweep() > std::f64::consts::FRAC_PI_2;
        if dimension.has_leader {
            let middle = selection.start_angle + selection.sweep() * 0.5;
            let anchor = DVec3::new(
                center.x + picked_radius * middle.cos(),
                center.y + picked_radius * middle.sin(),
                picked.z,
            );
            dimension.first_leader_point = v3(anchor);
            dimension.second_leader_point = v3(picked);
            dimension.definition_point = v3(anchor);
            dimension.base.definition_point = dimension.definition_point;
            dimension.base.text_user_positioned = true;
        }
        dimension.base.actual_measurement = dimension.measurement();
        crate::entities::dimension::set_dimension_text_override(
            &mut dimension.base,
            self.text_override.clone(),
        );
        if let Some(angle) = self.text_angle {
            dimension.base.text_rotation = angle;
        }

        let association = self.source_handle.map_or_else(
            || DimensionAssociationInput::Explicit(Vec::new()),
            |handle| {
                DimensionAssociationInput::Explicit(
                    selection.association_sources(handle),
                )
            },
        );
        CmdResult::CommitDimension {
            entity: plane.place_entity(EntityType::Dimension(Dimension::Arc(dimension))),
            association,
            preserve_base_style: false,
            continue_command: false,
        }
    }
}

impl CadCommand for ArcLengthDimensionCommand {
    fn set_working_plane(&mut self, _plane: WorkingPlane) {}

    fn name(&self) -> &'static str {
        "DIMARC"
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return "DIMARC  Enter dimension text (blank = measured value):".to_string();
        }
        if self.awaiting_angle {
            return "DIMARC  Specify text angle (degrees):".to_string();
        }
        match self.step {
            Step::SelectObject => {
                "DIMARC  Select arc or polyline arc segment:".to_string()
            }
            Step::DimLine(selection) => {
                let leader_option = if selection.sweep() > std::f64::consts::FRAC_PI_2 {
                    if self.leader_enabled { "/No Leader" } else { "/Leader" }
                } else {
                    ""
                };
                format!(
                    "DIMARC  Specify arc length dimension location [Mtext/Text/Angle/Partial{leader_option}]:"
                )
            }
            Step::PartialFirst(_) => {
                "DIMARC  Specify first point of partial arc:".to_string()
            }
            Step::PartialSecond { .. } => {
                "DIMARC  Specify second point of partial arc:".to_string()
            }
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::SelectObject => CmdResult::NeedPoint,
            Step::DimLine(selection) => self.commit_dimension(selection, point),
            Step::PartialFirst(selection) => {
                self.step = Step::PartialSecond {
                    selection,
                    first_angle: selection.clamped_angle(point),
                };
                CmdResult::NeedPoint
            }
            Step::PartialSecond {
                selection,
                first_angle,
            } => {
                let second_angle = selection.clamped_angle(point);
                let Some(partial) = selection.with_partial(first_angle, second_angle) else {
                    return CmdResult::NeedPoint;
                };
                self.leader_enabled = false;
                self.step = Step::DimLine(partial);
                CmdResult::NeedPoint
            }
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
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.awaiting_text && !self.awaiting_angle
    }

    fn wants_text_with_spaces(&self) -> bool {
        self.awaiting_text
    }

    fn options(&self) -> Vec<CmdOption> {
        let Step::DimLine(selection) = self.step else {
            return Vec::new();
        };
        if self.awaiting_text || self.awaiting_angle {
            return Vec::new();
        }
        let mut options = vec![
            CmdOption::new("MText", "MTEXT"),
            CmdOption::new("Text", "TEXT"),
            CmdOption::new("Angle", "ANGLE"),
            CmdOption::new("Partial", "PARTIAL"),
        ];
        if selection.sweep() > std::f64::consts::FRAC_PI_2 {
            options.push(if self.leader_enabled {
                CmdOption::new("No Leader", "NOLEADER")
            } else {
                CmdOption::new("Leader", "LEADER")
            });
        }
        options
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
            let value = text.trim();
            self.text_angle = if value.is_empty() {
                None
            } else {
                crate::entities::common::parse_typed_angle(value)
            };
            self.awaiting_angle = false;
            return Some(CmdResult::NeedPoint);
        }
        let Step::DimLine(selection) = self.step else {
            return None;
        };
        match text.trim().to_ascii_uppercase().as_str() {
            "M" | "MTEXT" => Some(CmdResult::SuspendForMTextInput {
                pos: self.editor_anchor(),
                initial: self.text_override.clone().unwrap_or_default(),
                height: 2.5,
            }),
            "T" | "TEXT" => {
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "A" | "ANGLE" => {
                self.awaiting_angle = true;
                Some(CmdResult::NeedPoint)
            }
            "P" | "PARTIAL" => {
                self.step = Step::PartialFirst(selection);
                Some(CmdResult::NeedPoint)
            }
            "L" | "LEADER" if selection.sweep() > std::f64::consts::FRAC_PI_2 => {
                self.leader_enabled = true;
                Some(CmdResult::NeedPoint)
            }
            "N" | "NOLEADER" => {
                self.leader_enabled = false;
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
        if !source.limited {
            return CmdResult::NeedPoint;
        }
        let binding = match entity {
            EntityType::Arc(_) => SourceBinding::Arc,
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                SourceBinding::PolylineSegment(source.marker)
            }
            _ => return CmdResult::NeedPoint,
        };
        let Some(selection) = ArcSelection::new(source, binding) else {
            return CmdResult::NeedPoint;
        };
        self.source_handle = Some(handle);
        self.step = Step::DimLine(selection);
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match self.step {
            Step::SelectObject | Step::PartialFirst(_) => None,
            Step::DimLine(selection) => Some(dimension_preview(
                selection,
                point,
                self.leader_enabled,
            )),
            Step::PartialSecond {
                selection,
                first_angle,
            } => {
                let second_angle = selection.clamped_angle(point);
                let partial = selection.with_partial(first_angle, second_angle)?;
                Some(partial_preview(partial))
            }
        }
    }
}

fn positive_sweep(start: f64, end: f64) -> f64 {
    let raw = end - start;
    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
        sweep = std::f64::consts::TAU;
    }
    sweep
}

fn dimension_preview(
    selection: ArcSelection,
    point: DVec3,
    leader: bool,
) -> WireModel {
    let plane = selection.plane();
    let center = plane.to_local(selection.center());
    let first = plane.to_local(selection.point_at(selection.start_angle));
    let second = plane.to_local(selection.point_at(selection.end_angle));
    let Some(point) = selection.project(point) else {
        return preview_wire(Vec::new(), "dimarc_preview");
    };
    let point = plane.to_local(point);
    let radius = (point - center).truncate().length();
    if radius <= 1.0e-12 {
        return preview_wire(Vec::new(), "dimarc_preview");
    }
    let start_land = DVec3::new(
        center.x + radius * selection.start_angle.cos(),
        center.y + radius * selection.start_angle.sin(),
        point.z,
    );
    let end_land = DVec3::new(
        center.x + radius * selection.end_angle.cos(),
        center.y + radius * selection.end_angle.sin(),
        point.z,
    );
    let mut points = vec![first, start_land, nan(), second, end_land, nan()];
    points.extend(
        arc(
            [center.x, center.y],
            radius,
            selection.start_angle,
            selection.end_angle,
            point.z,
            DEFAULT_SEGMENTS_PER_RADIAN,
        )
        .into_iter()
        .map(DVec3::from_array),
    );
    if leader && selection.sweep() > std::f64::consts::FRAC_PI_2 {
        let middle = selection.start_angle + selection.sweep() * 0.5;
        points.extend([
            nan(),
            DVec3::new(
                center.x + radius * middle.cos(),
                center.y + radius * middle.sin(),
                point.z,
            ),
            point,
        ]);
    }
    preview_wire(
        points
            .into_iter()
            .map(|value| if value.is_nan() { value } else { plane.to_world(value) })
            .collect(),
        "dimarc_preview",
    )
}

fn partial_preview(selection: ArcSelection) -> WireModel {
    let points = arc(
        selection.source.center,
        selection.source.radius,
        selection.start_angle,
        selection.end_angle,
        0.0,
        DEFAULT_SEGMENTS_PER_RADIAN,
    )
    .into_iter()
    .map(|point| DVec3::from_array(selection.source.plane.point_at([point[0], point[1]])))
    .collect();
    preview_wire(points, "dimarc_partial_preview")
}

fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn nan() -> DVec3 {
    DVec3::splat(f64::NAN)
}

fn preview_wire(points: Vec<DVec3>, name: &str) -> WireModel {
    WireModel::solid_f64(
        name.to_string(),
        points.into_iter().map(|point| point.to_array()).collect(),
        WireModel::CYAN,
        false,
    )
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMARC"] });
