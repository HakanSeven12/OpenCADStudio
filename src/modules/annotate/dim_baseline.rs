//! Repeating baseline dimensions from a session base or an explicitly picked base.

use std::collections::HashMap;

use acadrust::entities::{
    Dimension, DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionBase,
    DimensionLinear, DimensionOrdinate,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, DimensionAssociationInput};
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

enum BaselineKind {
    Linear {
        fixed: DVec3,
        rotation: f64,
        aligned: bool,
        perpendicular: DVec3,
        next_offset: f64,
        increment: f64,
    },
    Angular2Ln {
        source: DimensionAngular2Ln,
        vertex: DVec3,
        fixed_ray: DVec3,
        next_radius: f64,
        increment: f64,
    },
    Angular3Pt {
        source: DimensionAngular3Pt,
        vertex: DVec3,
        fixed_ray: DVec3,
        next_radius: f64,
        increment: f64,
    },
    Ordinate {
        source: DimensionOrdinate,
    },
}

struct BaselineState {
    kind: BaselineKind,
    style: SourceStyle,
}

pub struct DimBaselineCommand {
    base: Option<BaselineState>,
    injected: Option<EntityType>,
    dimdli_by_style: HashMap<String, f64>,
    current_style_name: String,
    fallback_dimdli: f64,
    preserve_base_style: bool,
    committed: usize,
}

impl DimBaselineCommand {
    pub fn new(
        recent: Option<EntityType>,
        dimdli_by_style: HashMap<String, f64>,
        current_style_name: String,
        fallback_dimdli: f64,
        preserve_base_style: bool,
    ) -> Self {
        let mut command = Self {
            base: None,
            injected: None,
            dimdli_by_style,
            current_style_name,
            fallback_dimdli,
            preserve_base_style,
            committed: 0,
        };
        if let Some(entity) = recent {
            let _ = command.install_base(&entity, None);
        }
        command
    }

    fn effective_increment(&self, style_name: &str) -> f64 {
        let style_name = if self.preserve_base_style {
            style_name
        } else {
            &self.current_style_name
        };
        self.dimdli_by_style
            .get(&style_name.to_ascii_lowercase())
            .copied()
            .filter(|value| value.is_finite() && value.abs() > 1.0e-9)
            .unwrap_or(self.fallback_dimdli)
            .abs()
    }

    fn install_base(&mut self, entity: &EntityType, pick: Option<DVec3>) -> bool {
        let EntityType::Dimension(dimension) = entity else {
            return false;
        };
        let style = SourceStyle::from_base(dimension.base());
        let increment = self.effective_increment(&style.style_name);
        let kind = match dimension {
            Dimension::Linear(source) => {
                let (fixed, _) = nearest_end(source.first_point, source.second_point, pick);
                linear_base(
                    fixed,
                    source.rotation,
                    false,
                    dv(source.base.definition_point),
                    increment,
                )
            }
            Dimension::Aligned(source) => {
                let (fixed, other) = nearest_end(source.first_point, source.second_point, pick);
                linear_base(
                    fixed,
                    (other - fixed).y.atan2((other - fixed).x),
                    true,
                    dv(source.base.definition_point),
                    increment,
                )
            }
            Dimension::Angular2Ln(source) => {
                let vertex = angular2_vertex(source);
                let first = dv(source.second_point) - vertex;
                let second = dv(source.definition_point) - vertex;
                let fixed_ray = nearest_direction(first, second, vertex, pick);
                let radius = (dv(source.dimension_arc) - vertex).length().max(1.0e-9);
                BaselineKind::Angular2Ln {
                    source: source.clone(),
                    vertex,
                    fixed_ray,
                    next_radius: radius + increment,
                    increment,
                }
            }
            Dimension::Angular3Pt(source) => {
                let vertex = dv(source.angle_vertex);
                let first = dv(source.first_point) - vertex;
                let second = dv(source.second_point) - vertex;
                let fixed_ray = nearest_direction(first, second, vertex, pick);
                let radius = (dv(source.definition_point) - vertex).length().max(1.0e-9);
                BaselineKind::Angular3Pt {
                    source: source.clone(),
                    vertex,
                    fixed_ray,
                    next_radius: radius + increment,
                    increment,
                }
            }
            Dimension::Ordinate(source) => BaselineKind::Ordinate {
                source: source.clone(),
            },
            _ => return false,
        };
        self.base = Some(BaselineState { kind, style });
        self.committed = 0;
        true
    }

    fn build_dimension(&self, point: DVec3) -> Option<Dimension> {
        let state = self.base.as_ref()?;
        let mut dimension = match &state.kind {
            BaselineKind::Linear {
                fixed,
                rotation,
                aligned,
                perpendicular,
                next_offset,
                ..
            } => build_linear(
                *fixed,
                point,
                *rotation,
                *aligned,
                *perpendicular,
                *next_offset,
            ),
            BaselineKind::Angular2Ln {
                source,
                vertex,
                fixed_ray,
                next_radius,
                ..
            } => {
                let moving = point - *vertex;
                if moving.length_squared() <= 1.0e-18 {
                    return None;
                }
                let mut result = source.clone();
                result.first_point = v3(*vertex);
                result.second_point = v3(*vertex + normalized_or(*fixed_ray, DVec3::X));
                result.angle_vertex = v3(*vertex);
                result.definition_point = v3(point);
                let (definition, text) =
                    angular_definition_and_text(*vertex, *fixed_ray, moving, *next_radius);
                result.dimension_arc = v3(definition);
                result.base.definition_point = result.dimension_arc;
                result.base.text_middle_point = v3(text);
                result.base.insertion_point = result.base.text_middle_point;
                result.base.actual_measurement = result.measurement_degrees();
                Dimension::Angular2Ln(result)
            }
            BaselineKind::Angular3Pt {
                source,
                vertex,
                fixed_ray,
                next_radius,
                ..
            } => {
                let moving = point - *vertex;
                if moving.length_squared() <= 1.0e-18 {
                    return None;
                }
                let mut result = source.clone();
                result.angle_vertex = v3(*vertex);
                result.first_point = v3(*vertex + normalized_or(*fixed_ray, DVec3::X));
                result.second_point = v3(point);
                let (definition, text) =
                    angular_definition_and_text(*vertex, *fixed_ray, moving, *next_radius);
                result.definition_point = v3(definition);
                result.base.definition_point = result.definition_point;
                result.base.text_middle_point = v3(text);
                result.base.insertion_point = result.base.text_middle_point;
                result.base.actual_measurement = result.measurement_degrees();
                Dimension::Angular3Pt(result)
            }
            BaselineKind::Ordinate { source } => {
                let source_leader = dv(source.leader_endpoint);
                let leader = if source.is_ordinate_type_x {
                    DVec3::new(point.x, source_leader.y, source_leader.z)
                } else {
                    DVec3::new(source_leader.x, point.y, source_leader.z)
                };
                let mut result = DimensionOrdinate::new(
                    v3(point),
                    v3(leader),
                    source.is_ordinate_type_x,
                );
                result.definition_point = source.definition_point;
                result.base.definition_point = source.base.definition_point;
                result.base.text_middle_point = v3(leader);
                result.base.insertion_point = result.base.text_middle_point;
                Dimension::Ordinate(result)
            }
        };
        state.style.apply(dimension.base_mut());
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
        if self.base.is_none() {
            "DIMBASELINE  Select base dimension:".to_string()
        } else if self
            .base
            .as_ref()
            .is_some_and(|base| matches!(&base.kind, BaselineKind::Ordinate { .. }))
        {
            "DIMBASELINE  Specify feature location [Undo/Select] <Select>:".to_string()
        } else {
            "DIMBASELINE  Specify second extension line origin [Select/Undo] <Select>:"
                .to_string()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        self.base.as_ref().map_or_else(Vec::new, |base| {
            if matches!(&base.kind, BaselineKind::Ordinate { .. }) {
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
        self.committed += 1;
        self.advance(1.0);
        CmdResult::CommitDimension {
            entity: EntityType::Dimension(dimension),
            association: DimensionAssociationInput::Infer(None),
            preserve_base_style: self.preserve_base_style,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.base.take().is_some() {
            self.committed = 0;
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
        self.build_dimension(point).map(|dimension| preview_for_dimension(&dimension))
    }
}

fn linear_base(
    fixed: DVec3,
    rotation: f64,
    aligned: bool,
    definition: DVec3,
    increment: f64,
) -> BaselineKind {
    let perpendicular = DVec3::new(-rotation.sin(), rotation.cos(), 0.0);
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
    fixed: DVec3,
    point: DVec3,
    rotation: f64,
    aligned: bool,
    perpendicular: DVec3,
    offset: f64,
) -> Dimension {
    let target = fixed.dot(perpendicular) + offset;
    let first_line = fixed + perpendicular * (target - fixed.dot(perpendicular));
    let second_line = point + perpendicular * (target - point.dot(perpendicular));
    if aligned {
        let mut result = DimensionAligned::new(v3(fixed), v3(point));
        result.definition_point = v3(second_line);
        result.base.definition_point = v3(second_line);
        result.base.text_middle_point = v3((first_line + second_line) * 0.5);
        result.base.insertion_point = result.base.text_middle_point;
        result.base.actual_measurement = result.measurement();
        Dimension::Aligned(result)
    } else {
        let mut result = DimensionLinear::new(v3(fixed), v3(point));
        result.rotation = rotation;
        result.definition_point = v3(second_line);
        result.base.definition_point = v3(second_line);
        result.base.text_middle_point = v3((first_line + second_line) * 0.5);
        result.base.insertion_point = result.base.text_middle_point;
        result.base.actual_measurement = result.measurement();
        Dimension::Linear(result)
    }
}

fn nearest_end(first: Vector3, second: Vector3, pick: Option<DVec3>) -> (DVec3, DVec3) {
    let first = dv(first);
    let second = dv(second);
    if pick.is_some_and(|point| point.distance_squared(second) < point.distance_squared(first)) {
        (second, first)
    } else {
        (first, second)
    }
}

fn nearest_direction(first: DVec3, second: DVec3, vertex: DVec3, pick: Option<DVec3>) -> DVec3 {
    if let Some(point) = pick {
        let direction = normalized_or(point - vertex, DVec3::X);
        if normalized_or(second, DVec3::Y).dot(direction)
            > normalized_or(first, DVec3::X).dot(direction)
        {
            return second;
        }
    }
    first
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
    first: DVec3,
    second: DVec3,
    radius: f64,
) -> (DVec3, DVec3) {
    let start = first.y.atan2(first.x);
    let mut sweep = second.y.atan2(second.x) - start;
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

fn normalized_or(value: DVec3, fallback: DVec3) -> DVec3 {
    if value.length_squared() > 1.0e-18 {
        value.normalize()
    } else {
        fallback
    }
}

fn preview_for_dimension(dimension: &Dimension) -> WireModel {
    let mut points = Vec::<[f32; 3]>::new();
    match dimension {
        Dimension::Linear(dimension) => linear_preview(
            &mut points,
            dv(dimension.first_point),
            dv(dimension.second_point),
            dimension.rotation,
            dv(dimension.definition_point),
        ),
        Dimension::Aligned(dimension) => {
            let first = dv(dimension.first_point);
            let second = dv(dimension.second_point);
            let delta = second - first;
            linear_preview(
                &mut points,
                first,
                second,
                delta.y.atan2(delta.x),
                dv(dimension.definition_point),
            );
        }
        Dimension::Angular2Ln(dimension) => angular_preview(
            &mut points,
            angular2_vertex(dimension),
            dv(dimension.second_point) - angular2_vertex(dimension),
            dv(dimension.definition_point) - angular2_vertex(dimension),
            dv(dimension.dimension_arc),
        ),
        Dimension::Angular3Pt(dimension) => angular_preview(
            &mut points,
            dv(dimension.angle_vertex),
            dv(dimension.first_point) - dv(dimension.angle_vertex),
            dv(dimension.second_point) - dv(dimension.angle_vertex),
            dv(dimension.definition_point),
        ),
        Dimension::Ordinate(dimension) => segment(
            &mut points,
            dv(dimension.feature_location),
            dv(dimension.leader_endpoint),
        ),
        _ => {}
    }
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
        name: "dimbaseline_preview".into(),
        points,
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: Vec::new(),
        tangent_geoms: Vec::new(),
        aci: 0,
        key_vertices: Vec::new(),
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: Vec::new(),
        fill_tris_low: Vec::new(),
    }
}

fn linear_preview(
    points: &mut Vec<[f32; 3]>,
    first: DVec3,
    second: DVec3,
    rotation: f64,
    definition: DVec3,
) {
    let perpendicular = DVec3::new(-rotation.sin(), rotation.cos(), 0.0);
    let target = definition.dot(perpendicular);
    let first_line = first + perpendicular * (target - first.dot(perpendicular));
    let second_line = second + perpendicular * (target - second.dot(perpendicular));
    segment(points, first, first_line);
    segment(points, second, second_line);
    segment(points, first_line, second_line);
    append_arrows(points, first_line, second_line);
}

fn segment(points: &mut Vec<[f32; 3]>, first: DVec3, second: DVec3) {
    points.push(first.as_vec3().to_array());
    points.push(second.as_vec3().to_array());
    points.push([f32::NAN, 0.0, 0.0]);
}

fn append_arrows(points: &mut Vec<[f32; 3]>, first: DVec3, second: DVec3) {
    let axis = normalized_or(second - first, DVec3::X);
    let normal = DVec3::new(-axis.y, axis.x, 0.0);
    let size = first.distance(second).clamp(1.0, 100.0) * 0.035;
    for (tip, inward) in [(first, axis), (second, -axis)] {
        let back = tip + inward * size;
        segment(points, tip, back + normal * size * 0.4);
        segment(points, tip, back - normal * size * 0.4);
    }
}

fn angular_preview(
    points: &mut Vec<[f32; 3]>,
    vertex: DVec3,
    first: DVec3,
    second: DVec3,
    arc_point: DVec3,
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
    segment(points, vertex, vertex + normalized_or(first, DVec3::X) * radius);
    segment(points, vertex, vertex + normalized_or(second, DVec3::Y) * radius);
    for index in 0..=24 {
        let angle = start + sweep * index as f64 / 24.0;
        points.push(
            (vertex + DVec3::new(angle.cos(), angle.sin(), 0.0) * radius)
                .as_vec3()
                .to_array(),
        );
    }
}

fn dv(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMBASELINE"] });
