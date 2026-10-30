use std::f64::consts::PI;

use codec::entities::{
    Dimension, DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionBase,
    DimensionLinear, DimensionOrdinate,
};
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{
    arc_span, intersect, nearest_of, Arc, Curve, Line, Tolerance, Transform, Vec2, XLine,
};
use kernel::space::Plane;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, DimensionPreview,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_baseline.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMBASELINE",
        label: "Baseline",
        icon: ICON,
        event: ModuleEvent::Command("DIMBASELINE".to_string()),
    }
}

/// The dimensions DIM's Baseline and Continue take by a typed point on one
/// of their extension line origins, and how near the point has to be.
#[derive(Clone, Default)]
pub struct OriginPicks {
    dimensions: Vec<EntityType>,
    tolerance: f64,
}

impl OriginPicks {
    pub fn new(dimensions: Vec<EntityType>, tolerance: f64) -> Self {
        Self {
            dimensions,
            tolerance,
        }
    }

    /// The dimension with an extension line origin nearest `point`, within
    /// the tolerance; any other point (the dimension line, empty space)
    /// picks nothing.
    pub(super) fn at(&self, point: DVec3) -> Option<&EntityType> {
        let mut nearest: Option<(f64, &EntityType)> = None;
        for entity in &self.dimensions {
            let EntityType::Dimension(dimension) = entity else {
                continue;
            };
            let origins = match &**dimension {
                Dimension::Linear(d) => vec![d.first_point, d.second_point],
                Dimension::Aligned(d) => vec![d.first_point, d.second_point],
                Dimension::Angular2Ln(d) => vec![
                    d.first_point,
                    d.second_point,
                    d.angle_vertex,
                    d.definition_point,
                ],
                Dimension::Angular3Pt(d) => vec![d.first_point, d.second_point],
                Dimension::Ordinate(d) => vec![d.definition_point],
                _ => continue,
            };
            for origin in origins {
                let distance = DVec3::new(origin.x, origin.y, origin.z).distance(point);
                if distance <= self.tolerance && nearest.is_none_or(|(best, _)| distance < best) {
                    nearest = Some((distance, entity));
                }
            }
        }
        nearest.map(|(_, entity)| entity)
    }
}

/// What a baseline or continued dimension takes from its base dimension.
#[derive(Clone)]
pub(super) struct SourceStyle {
    layer: String,
    style_name: String,
    pub(super) normal: Vector3,
    text_rotation: f64,
    pub(super) horizontal_direction: f64,
    /// The base dimension's dimension variable overrides.
    overrides: Vec<(i16, codec::xdata::XDataValue)>,
}

impl SourceStyle {
    pub(super) fn from_base(base: &DimensionBase) -> Self {
        Self {
            layer: base.common.layer.clone(),
            style_name: base.style_name.clone(),
            normal: base.normal,
            text_rotation: base.text_rotation,
            horizontal_direction: base.horizontal_direction,
            overrides: crate::entities::dim_override::pairs(&base.common.extended_data),
        }
    }

    /// With DIMCONTINUEMODE 1 (`with_overrides`) the new dimension also
    /// carries every override of its base; with 0 it gets the drawing's like
    /// any new dimension.
    pub(super) fn apply(&self, base: &mut DimensionBase, with_overrides: bool) {
        base.common.layer = self.layer.clone();
        base.style_name = self.style_name.clone();
        base.normal = self.normal;
        base.text_rotation = self.text_rotation;
        base.horizontal_direction = self.horizontal_direction;
        if with_overrides {
            for (code, value) in &self.overrides {
                crate::entities::dim_override::set_on_common(
                    &mut base.common,
                    *code,
                    Some(value.clone()),
                );
            }
        }
    }
}

enum BaselineKind {
    Linear {
        fixed: Vec2,
        rotation: f64,
        aligned: bool,
        perpendicular: Vec2,
        next_offset: f64,
        increment: f64,
    },
    Angular2Ln {
        fixed_line: [Vec2; 2],
        fixed_is_first: bool,
        vertex: Vec2,
        fixed_ray: Vec2,
        next_radius: f64,
        increment: f64,
    },
    Angular3Pt {
        vertex: Vec2,
        fixed_point: Vec2,
        fixed_ray: Vec2,
        next_radius: f64,
        increment: f64,
    },
    Ordinate {
        definition: Vec2,
        leader: Vec2,
        is_x: bool,
    },
}

struct BaselineState {
    kind: BaselineKind,
    style: SourceStyle,
    plane: Plane,
}

pub struct DimBaselineCommand {
    base: Option<BaselineState>,
    injected: Option<EntityType>,
    preserve_base_style: bool,
    committed: usize,
    /// Run from DIM's Baseline option: its own wording and an Offset option.
    in_dim: bool,
    /// The Offset distance is being asked.
    offset_pending: bool,
    offset_override: Option<f64>,
    /// The drawing's DIMDLI: the spacing of the series.
    dimdli: f64,
    /// DIM's Baseline: dimensions a typed extension line origin picks.
    origin_picks: OriginPicks,
}

impl DimBaselineCommand {
    /// `dimdli` is the drawing's DIMDLI: baseline dimensions are spaced by
    /// it whatever the base dimension's own value is.
    pub fn new(recent: Option<EntityType>, dimdli: f64, preserve_base_style: bool) -> Self {
        let mut command = Self {
            base: None,
            injected: None,
            preserve_base_style,
            committed: 0,
            in_dim: false,
            offset_pending: false,
            offset_override: None,
            dimdli,
            origin_picks: OriginPicks::default(),
        };
        if let Some(entity) = recent {
            let _ = command.install_base(&entity, None);
        }
        command
    }

    /// The dimensions a typed point on an extension line origin picks as
    /// the base (DIM's Baseline).
    pub fn with_origin_picks(mut self, origin_picks: OriginPicks) -> Self {
        self.origin_picks = origin_picks;
        self
    }

    /// DIM's Baseline: the base is picked, and Offset sets the spacing.
    pub fn run_from_dim(&mut self, dimdli: f64) {
        self.in_dim = true;
        self.dimdli = dimdli;
        self.offset_override = Some(dimdli);
    }

    /// The spacing Offset typed, applied to the series under way as well.
    fn set_offset(&mut self, value: f64) {
        self.offset_override = Some(value);
        let Some(base) = self.base.as_mut() else {
            return;
        };
        match &mut base.kind {
            BaselineKind::Linear {
                next_offset,
                increment,
                ..
            } => {
                let signed = value.copysign(*increment);
                *next_offset += signed - *increment;
                *increment = signed;
            }
            BaselineKind::Angular2Ln {
                next_radius,
                increment,
                ..
            }
            | BaselineKind::Angular3Pt {
                next_radius,
                increment,
                ..
            } => {
                *next_radius = (*next_radius + value - *increment).max(1.0e-9);
                *increment = value;
            }
            BaselineKind::Ordinate { .. } => {}
        }
    }

    fn effective_increment(&self) -> f64 {
        self.offset_override.unwrap_or(self.dimdli).abs()
    }

    fn install_base(&mut self, entity: &EntityType, pick: Option<DVec3>) -> bool {
        let EntityType::Dimension(dimension) = entity else {
            return false;
        };
        let style = SourceStyle::from_base(dimension.base());
        let increment = self.effective_increment();
        let plane = plane_from_normal(dimension.base().definition_point, style.normal);
        let pick = pick.and_then(|point| plane.project(point.to_array()).map(Vec2::from));
        let kind = match &**dimension {
            Dimension::Linear(source) => {
                let first = project(&plane, source.first_point);
                let second = project(&plane, source.second_point);
                let (fixed, _) = nearest_end(first, second, pick);
                linear_base(
                    fixed,
                    source.rotation,
                    false,
                    project(&plane, source.base.definition_point),
                    increment,
                )
            }
            Dimension::Aligned(source) => {
                let first = project(&plane, source.first_point);
                let second = project(&plane, source.second_point);
                let (fixed, other) = nearest_end(first, second, pick);
                linear_base(
                    fixed,
                    (other - fixed).angle(),
                    true,
                    project(&plane, source.base.definition_point),
                    increment,
                )
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
                let fixed_index = nearest_line(lines, pick);
                let fixed_line = lines[fixed_index];
                let fixed_end = if fixed_index == 0 {
                    lines[0][1]
                } else {
                    lines[1][1]
                };
                let fixed_ray = fixed_end - vertex;
                let radius = project(&plane, source.dimension_arc)
                    .distance(vertex)
                    .max(1.0e-9);
                BaselineKind::Angular2Ln {
                    fixed_line,
                    fixed_is_first: fixed_index == 0,
                    vertex,
                    fixed_ray,
                    next_radius: radius + increment,
                    increment,
                }
            }
            Dimension::Angular3Pt(source) => {
                let vertex = project(&plane, source.angle_vertex);
                let points = [
                    project(&plane, source.first_point),
                    project(&plane, source.second_point),
                ];
                let fixed_index = nearest_line([[vertex, points[0]], [vertex, points[1]]], pick);
                let fixed_point = points[fixed_index];
                let radius = project(&plane, source.definition_point)
                    .distance(vertex)
                    .max(1.0e-9);
                BaselineKind::Angular3Pt {
                    vertex,
                    fixed_point,
                    fixed_ray: fixed_point - vertex,
                    next_radius: radius + increment,
                    increment,
                }
            }
            Dimension::Ordinate(source) => BaselineKind::Ordinate {
                definition: project(&plane, source.definition_point),
                leader: project(&plane, source.leader_endpoint),
                is_x: source.is_ordinate_type_x,
            },
            _ => return false,
        };
        self.base = Some(BaselineState { kind, style, plane });
        self.committed = 0;
        true
    }

    fn build_dimension(&self, point_world: DVec3) -> Option<Dimension> {
        let state = self.base.as_ref()?;
        let point = state
            .plane
            .project(point_world.to_array())
            .map(Vec2::from)?;
        let mut dimension = match &state.kind {
            BaselineKind::Linear {
                fixed,
                rotation,
                aligned,
                perpendicular,
                next_offset,
                ..
            } => build_linear(
                state.plane,
                *fixed,
                point,
                *rotation,
                *aligned,
                *perpendicular,
                *next_offset,
            )?,
            BaselineKind::Angular2Ln {
                fixed_line,
                fixed_is_first,
                vertex,
                fixed_ray,
                next_radius,
                ..
            } => {
                let moving = point - *vertex;
                let (definition, text) =
                    angular_definition_and_text(*vertex, *fixed_ray, moving, *next_radius)?;
                let mut result = DimensionAngular2Ln::default();
                if *fixed_is_first {
                    result.first_point = world(state.plane, fixed_line[0]);
                    result.second_point = world(state.plane, fixed_line[1]);
                    result.angle_vertex = world(state.plane, *vertex);
                    result.definition_point = world(state.plane, point);
                } else {
                    result.first_point = world(state.plane, *vertex);
                    result.second_point = world(state.plane, point);
                    result.angle_vertex = world(state.plane, fixed_line[0]);
                    result.definition_point = world(state.plane, fixed_line[1]);
                }
                result.dimension_arc = world(state.plane, definition);
                result.base.definition_point = result.dimension_arc;
                result.base.text_middle_point = world(state.plane, text);
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Angular2Ln(result)
            }
            BaselineKind::Angular3Pt {
                vertex,
                fixed_point,
                fixed_ray,
                next_radius,
                ..
            } => {
                let moving = point - *vertex;
                let (definition, text) =
                    angular_definition_and_text(*vertex, *fixed_ray, moving, *next_radius)?;
                let mut result = DimensionAngular3Pt::new(
                    world(state.plane, *vertex),
                    world(state.plane, *fixed_point),
                    world(state.plane, point),
                );
                result.definition_point = world(state.plane, definition);
                result.base.definition_point = result.definition_point;
                result.base.text_middle_point = world(state.plane, text);
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Angular3Pt(result)
            }
            BaselineKind::Ordinate {
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
        state
            .style
            .apply(dimension.base_mut(), self.preserve_base_style);
        refresh_measurement(&mut dimension, state.plane);
        dimension.base_mut().common.handle = Handle::NULL;
        dimension.base_mut().block_name.clear();
        Some(dimension)
    }

    fn advance(&mut self, direction: f64) {
        let Some(state) = self.base.as_mut() else {
            return;
        };
        match &mut state.kind {
            BaselineKind::Linear {
                next_offset,
                increment,
                ..
            } => *next_offset += *increment * direction,
            BaselineKind::Angular2Ln {
                next_radius,
                increment,
                ..
            }
            | BaselineKind::Angular3Pt {
                next_radius,
                increment,
                ..
            } => *next_radius = (*next_radius + *increment * direction).max(1.0e-9),
            BaselineKind::Ordinate { .. } => {}
        }
    }

    fn undo_one(&mut self) -> Option<CmdResult> {
        if self.committed == 0 {
            return Some(CmdResult::NeedPoint);
        }
        self.committed -= 1;
        self.advance(-1.0);
        Some(CmdResult::UndoDocument)
    }
}

impl CadCommand for DimBaselineCommand {
    fn name(&self) -> &'static str {
        "DIMBASELINE"
    }

    fn prompt(&self) -> String {
        if self.offset_pending {
            let current = self.offset_override.unwrap_or(self.dimdli);
            return crate::tr!("dim-cmd", "offset-distance", offset = format!("{current:.6}"));
        }
        if self.in_dim {
            return match &self.base {
                None => crate::tr!("dim-cmd", "baseline-first"),
                Some(base) if matches!(&base.kind, BaselineKind::Ordinate { .. }) => {
                    crate::tr!("dim-cmd", "baseline-ordinate")
                }
                Some(_) => crate::tr!("dim-cmd", "baseline-second"),
            };
        }
        if self.base.is_none() {
            crate::t!("DIMBASELINE  Select base dimension:").into_owned()
        } else if self
            .base
            .as_ref()
            .is_some_and(|base| matches!(&base.kind, BaselineKind::Ordinate { .. }))
        {
            crate::t!("DIMBASELINE  Specify feature location [Undo/Select] <Select>:").into_owned()
        } else {
            crate::t!("DIMBASELINE  Specify second extension line origin [Select/Undo] <Select>:")
                .into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.offset_pending {
            return Vec::new();
        }
        if self.in_dim {
            return if self.base.is_none() {
                vec![CmdOption::new("Offset", "O")]
            } else {
                vec![
                    CmdOption::new("Select", "S"),
                    CmdOption::new("Offset", "O"),
                    CmdOption::new("Undo", "U"),
                ]
            };
        }
        self.base.as_ref().map_or_else(Vec::new, |base| {
            if matches!(&base.kind, BaselineKind::Ordinate { .. }) {
                vec![CmdOption::new("Undo", "U"), CmdOption::new("Select", "S")]
            } else {
                vec![CmdOption::new("Select", "S"), CmdOption::new("Undo", "U")]
            }
        })
    }

    fn input_kind(&self) -> crate::command::InputKind {
        if self.offset_pending {
            crate::command::InputKind::SingleToken
        } else {
            crate::command::InputKind::Point
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.base.is_none() && !self.offset_pending
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.base.is_none() && !self.offset_pending
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

    fn entity_pick_accepts_points(&self) -> bool {
        self.in_dim
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.offset_pending {
            return CmdResult::NeedPoint;
        }
        // DIM's Baseline: a typed extension line origin picks its dimension
        // as the base; any other point asks again.
        if self.base.is_none() {
            if let Some(entity) = self.origin_picks.at(point).cloned() {
                let _ = self.install_base(&entity, Some(point));
            }
            return CmdResult::NeedPoint;
        }
        let Some(dimension) = self.build_dimension(point) else {
            return CmdResult::NeedPoint;
        };
        self.committed += 1;
        self.advance(1.0);
        CmdResult::CommitDimension {
            entity: EntityType::Dimension(Box::new(dimension)),
            association: DimensionAssociationInput::Infer(None),
            preserve_base_style: self.preserve_base_style,
            continue_command: true,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.offset_pending {
            self.offset_pending = false;
            return CmdResult::NeedPoint;
        }
        if self.base.take().is_some() {
            self.committed = 0;
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn wants_text_input(&self) -> bool {
        self.base.is_some() || self.in_dim
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.base.is_some() || self.in_dim
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.offset_pending {
            let Ok(value) = text.trim().parse::<f64>() else {
                return Some(CmdResult::ReportError(
                    crate::t!("Requires numeric distance or two points.").into_owned(),
                ));
            };
            if !value.is_finite() || value < 0.0 {
                return Some(CmdResult::ReportError(
                    crate::t!("Value must be positive.").into_owned(),
                ));
            }
            self.offset_pending = false;
            self.set_offset(value);
            self.dimdli = value;
            // The drawing's DIMDLI, kept with the drawing.
            return Some(CmdResult::Dispatch(format!("_DIM_SET_DIMDLI {value}")));
        }
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if self.in_dim && matches!(word.as_str(), "O" | "OFFSET") {
            self.offset_pending = true;
            return Some(CmdResult::NeedPoint);
        }
        if self.base.is_none() {
            return None;
        }
        match word.as_str() {
            "S" | "SELECT" => {
                self.base = None;
                self.committed = 0;
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => self.undo_one(),
            _ => Some(CmdResult::NeedPoint),
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        self.undo_one()
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        self.build_dimension(point)
            .map(|dimension| preview_for_dimension(&dimension))
    }

    fn dimension_preview(&self, cursor: DVec3) -> Option<Vec<DimensionPreview>> {
        self.build_dimension(cursor).map(|dimension| {
            vec![DimensionPreview {
                entity: EntityType::Dimension(Box::new(dimension)),
                preserve_base_style: self.preserve_base_style,
            }]
        })
    }
}

fn linear_base(
    fixed: Vec2,
    rotation: f64,
    aligned: bool,
    definition: Vec2,
    increment: f64,
) -> BaselineKind {
    let perpendicular = Vec2::new(-rotation.sin(), rotation.cos());
    let offset = (definition - fixed).dot(perpendicular);
    let increment = increment * if offset >= 0.0 { 1.0 } else { -1.0 };
    BaselineKind::Linear {
        fixed,
        rotation,
        aligned,
        perpendicular,
        next_offset: offset + increment,
        increment,
    }
}

fn build_linear(
    plane: Plane,
    fixed: Vec2,
    point: Vec2,
    rotation: f64,
    aligned: bool,
    source_perpendicular: Vec2,
    offset: f64,
) -> Option<Dimension> {
    let (first_line, second_line) = if aligned {
        let mut perpendicular = (point - fixed).normalize()?.perpendicular();
        let source_side = source_perpendicular * offset.signum();
        if perpendicular.dot(source_side) < 0.0 {
            perpendicular = -perpendicular;
        }
        let distance = offset.abs();
        (
            fixed + perpendicular * distance,
            point + perpendicular * distance,
        )
    } else {
        let target = fixed.dot(source_perpendicular) + offset;
        (
            project_to_line(fixed, source_perpendicular, target),
            project_to_line(point, source_perpendicular, target),
        )
    };
    if aligned {
        let mut result = DimensionAligned::new(world(plane, fixed), world(plane, point));
        result.definition_point = world(plane, second_line);
        result.base.definition_point = result.definition_point;
        result.base.text_middle_point = world(plane, first_line.lerp(second_line, 0.5));
        result.base.insertion_point = result.base.text_middle_point;
        crate::entities::dimension::reset_automatic_text_position(&mut result.base);
        Some(Dimension::Aligned(result))
    } else {
        let mut result = DimensionLinear::new(world(plane, fixed), world(plane, point));
        result.rotation = rotation;
        result.definition_point = world(plane, second_line);
        result.base.definition_point = result.definition_point;
        result.base.text_middle_point = world(plane, first_line.lerp(second_line, 0.5));
        result.base.insertion_point = result.base.text_middle_point;
        crate::entities::dimension::reset_automatic_text_position(&mut result.base);
        Some(Dimension::Linear(result))
    }
}

fn nearest_end(first: Vec2, second: Vec2, pick: Option<Vec2>) -> (Vec2, Vec2) {
    if pick.is_some_and(|point| point.distance_squared(second) < point.distance_squared(first)) {
        (second, first)
    } else {
        (first, second)
    }
}

fn nearest_line(lines: [[Vec2; 2]; 2], pick: Option<Vec2>) -> usize {
    let Some(pick) = pick else {
        return 0;
    };
    let curves = lines.map(|line| {
        Curve::Line(Line {
            start: line[0].into(),
            end: line[1].into(),
        })
    });
    nearest_of(curves.iter(), pick.into())
        .map(|(index, _)| index)
        .unwrap_or(0)
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
    let scale = lines
        .into_iter()
        .flatten()
        .map(Vec2::length)
        .fold(1.0, f64::max);
    intersect(&first, &second, Tolerance::new(scale * 1.0e-9))
        .first()
        .map(|crossing| Vec2::from(crossing.point))
}

fn angular_definition_and_text(
    vertex: Vec2,
    first: Vec2,
    second: Vec2,
    radius: f64,
) -> Option<(Vec2, Vec2)> {
    let curve = angular_arc(vertex, first, second, radius)?;
    Some((
        Vec2::from(curve.point_at(2.0 / 3.0)),
        Vec2::from(curve.point_at(0.5)),
    ))
}

fn angular_arc(vertex: Vec2, first: Vec2, second: Vec2, radius: f64) -> Option<Curve> {
    let first = first.normalize()?;
    let second = second.normalize()?;
    let first_angle = first.angle();
    let second_angle = second.angle();
    let (start_angle, end_angle) = if arc_span(first_angle, second_angle) <= PI {
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
        Dimension::Aligned(value) => value.base.actual_measurement = value.measurement(),
        Dimension::Angular2Ln(value) => value.base.actual_measurement = value.measurement_radians(),
        Dimension::Angular3Pt(value) => value.base.actual_measurement = value.measurement_radians(),
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
            );
        }
        Dimension::Aligned(value) => {
            let plane = plane_from_normal(value.first_point, value.base.normal);
            let first = project(&plane, value.first_point);
            let second = project(&plane, value.second_point);
            linear_preview(
                &mut points,
                plane,
                first,
                second,
                (second - first).angle(),
                project(&plane, value.definition_point),
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
            );
        }
        Dimension::Ordinate(value) => segment_world(
            &mut points,
            point(value.feature_location),
            point(value.leader_endpoint),
        ),
        _ => {}
    }
    // Preview geometry is f64; `solid_f64` keeps the residual so the preview
    // stays on the dimension points at UTM-scale coordinates.
    WireModel::solid_f64("dimbaseline_preview".into(), points, WireModel::CYAN, false)
}

fn linear_preview(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    first: Vec2,
    second: Vec2,
    rotation: f64,
    definition: Vec2,
) {
    let perpendicular = Vec2::new(-rotation.sin(), rotation.cos());
    let target = definition.dot(perpendicular);
    let first_line = project_to_line(first, perpendicular, target);
    let second_line = project_to_line(second, perpendicular, target);
    segment_local(points, plane, first, first_line);
    segment_local(points, plane, second, second_line);
    segment_local(points, plane, first_line, second_line);
    append_arrows(points, plane, first_line, second_line);
}

fn append_arrows(points: &mut Vec<[f64; 3]>, plane: Plane, first: Vec2, second: Vec2) {
    let Some(axis) = (second - first).normalize() else {
        return;
    };
    let normal = axis.perpendicular();
    let size = first.distance(second).clamp(1.0, 100.0) * 0.035;
    for (tip, inward) in [(first, axis), (second, -axis)] {
        let back = tip + inward * size;
        segment_local(points, plane, tip, back + normal * size * 0.4);
        segment_local(points, plane, tip, back - normal * size * 0.4);
    }
}

fn angular_preview(
    points: &mut Vec<[f64; 3]>,
    plane: Plane,
    vertex: Vec2,
    first: Vec2,
    second: Vec2,
    arc_point: Vec2,
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
    segment_local(points, plane, vertex, vertex + first_direction * radius);
    segment_local(points, plane, vertex, vertex + second_direction * radius);
    for index in 0..=24 {
        push_local(points, plane, Vec2::from(arc.point_at(index as f64 / 24.0)));
    }
}

fn project_to_line(point: Vec2, perpendicular: Vec2, offset: f64) -> Vec2 {
    point + perpendicular * (offset - point.dot(perpendicular))
}

fn plane_from_normal(origin: Vector3, normal: Vector3) -> Plane {
    let (x_axis, y_axis) = crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
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

fn segment_local(points: &mut Vec<[f64; 3]>, plane: Plane, first: Vec2, second: Vec2) {
    segment_world(
        points,
        plane.point_at(first.into()),
        plane.point_at(second.into()),
    );
}

fn segment_world(points: &mut Vec<[f64; 3]>, first: [f64; 3], second: [f64; 3]) {
    points.push(first);
    points.push(second);
    points.push([f64::NAN, 0.0, 0.0]);
}

fn push_local(points: &mut Vec<[f64; 3]>, plane: Plane, point: Vec2) {
    points.push(plane.point_at(point.into()));
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMBASELINE"]
});
