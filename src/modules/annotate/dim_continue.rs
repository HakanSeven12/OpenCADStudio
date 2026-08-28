use acadrust::entities::{Dimension, DimensionBase, DimensionLinear};
use acadrust::types::Vector3;
use acadrust::EntityType;
use cadkernel::space::Plane;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, DimensionAssociationInput};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

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

struct ContinueState {
    chain_p1: [f64; 2],
    rotation: f64,
    dim_line_perp: f64,
    perpendicular: [f64; 2],
    plane: Plane,
    style: SourceStyle,
}

pub struct DimContinueCommand {
    state: Option<ContinueState>,
    preserve_base_style: bool,
}

impl DimContinueCommand {
    pub fn new(preserve_base_style: bool) -> Self {
        Self {
            state: None,
            preserve_base_style,
        }
    }

    pub fn from_dimension(dimension: &Dimension, preserve_base_style: bool) -> Self {
        let (first, second, definition, rotation) = match dimension {
            Dimension::Linear(source) => (
                source.first_point,
                source.second_point,
                source.base.definition_point,
                source.rotation,
            ),
            Dimension::Aligned(source) => {
                let plane = plane_from_normal(source.first_point, source.base.normal);
                let first = plane.project(point(source.first_point)).unwrap_or([0.0; 2]);
                let second = plane.project(point(source.second_point)).unwrap_or(first);
                let delta = [second[0] - first[0], second[1] - first[1]];
                (
                    source.first_point,
                    source.second_point,
                    source.base.definition_point,
                    delta[1].atan2(delta[0]),
                )
            }
            _ => return Self::new(preserve_base_style),
        };
        let plane = plane_from_normal(first, dimension.base().normal);
        let second = plane.project(point(second)).unwrap_or([0.0; 2]);
        let definition = plane.project(point(definition)).unwrap_or(second);
        let perpendicular = [-rotation.sin(), rotation.cos()];
        let dim_line_perp = dot(definition, perpendicular);
        Self {
            state: Some(ContinueState {
                chain_p1: second,
                rotation,
                dim_line_perp,
                perpendicular,
                plane,
                style: SourceStyle::from_base(dimension.base()),
            }),
            preserve_base_style,
        }
    }

    fn placement(&self, point_world: DVec3) -> Option<([f64; 2], [f64; 2], [f64; 2])> {
        let state = self.state.as_ref()?;
        let second = state.plane.project(point_world.to_array())?;
        let first_line = project_to_line(
            state.chain_p1,
            state.perpendicular,
            state.dim_line_perp,
        );
        let second_line = project_to_line(
            second,
            state.perpendicular,
            state.dim_line_perp,
        );
        Some((second, first_line, second_line))
    }
}

impl CadCommand for DimContinueCommand {
    fn name(&self) -> &'static str {
        "DIMCONTINUE"
    }

    fn prompt(&self) -> String {
        if self.state.is_none() {
            t!("DIMCONTINUE  No base dimension found. Place a dimension first.").into_owned()
        } else {
            t!("DIMCONTINUE  Specify a second extension line origin (Enter to exit):").into_owned()
        }
    }

    fn on_point(&mut self, point_world: DVec3) -> CmdResult {
        let Some((second, first_line, second_line)) = self.placement(point_world) else {
            return CmdResult::Cancel;
        };
        let state = self.state.as_mut().expect("continue state");
        let mut dimension = DimensionLinear::new(
            vector3(state.plane.point_at(state.chain_p1)),
            vector3(state.plane.point_at(second)),
        );
        dimension.rotation = state.rotation;
        state.style.apply(&mut dimension.base);
        dimension.definition_point = vector3(state.plane.point_at(first_line));
        dimension.base.definition_point = dimension.definition_point;
        dimension.base.text_middle_point = vector3(state.plane.point_at([
            (first_line[0] + second_line[0]) * 0.5,
            (first_line[1] + second_line[1]) * 0.5,
        ]));
        dimension.base.insertion_point = dimension.base.text_middle_point;
        let axis = [state.rotation.cos(), state.rotation.sin()];
        dimension.base.actual_measurement = dot(
            [
                second[0] - state.chain_p1[0],
                second[1] - state.chain_p1[1],
            ],
            axis,
        )
        .abs();
        state.chain_p1 = second;

        CmdResult::CommitDimension {
            entity: EntityType::Dimension(Dimension::Linear(dimension)),
            association: DimensionAssociationInput::Infer(None),
            preserve_base_style: self.preserve_base_style,
            continue_command: true,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, point_world: DVec3) -> Option<WireModel> {
        let (second, first_line, second_line) = self.placement(point_world)?;
        let state = self.state.as_ref()?;
        let first = state.plane.point_at(state.chain_p1);
        let second = state.plane.point_at(second);
        let first_line = state.plane.point_at(first_line);
        let second_line = state.plane.point_at(second_line);
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
            name: "dimcont_preview".into(),
            points: vec![
                float3(first),
                float3(first_line),
                [f32::NAN, 0.0, 0.0],
                float3(second),
                float3(second_line),
                [f32::NAN, 0.0, 0.0],
                float3(first_line),
                float3(second_line),
            ],
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
        })
    }
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

fn project_to_line(point: [f64; 2], perpendicular: [f64; 2], offset: f64) -> [f64; 2] {
    let distance = offset - dot(point, perpendicular);
    [
        point[0] + perpendicular[0] * distance,
        point[1] + perpendicular[1] * distance,
    ]
}

fn dot(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[0] + first[1] * second[1]
}

fn point(value: Vector3) -> [f64; 3] {
    [value.x, value.y, value.z]
}

fn vector3(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

fn float3(value: [f64; 3]) -> [f32; 3] {
    [value[0] as f32, value[1] as f32, value[2] as f32]
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMCONTINUE"] });
