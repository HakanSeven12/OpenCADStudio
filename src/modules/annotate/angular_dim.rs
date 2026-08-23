use acadrust::entities::{Dimension, DimensionAngular3Pt};
use acadrust::types::Vector3;
use acadrust::EntityType;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::{DVec3, Vec3};
use crate::t;

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
    SecondRay {
        vertex: DVec3,
        first: DVec3,
    },
    ArcPoint {
        vertex: DVec3,
        first: DVec3,
        second: DVec3,
    },
}

pub struct AngularDimensionCommand {
    step: Step,
    plane: WorkingPlane,
}

impl AngularDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Vertex,
            plane: WorkingPlane::default(),
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
        match self.step {
            Step::Vertex => t!("DIMANGULAR  Specify angle vertex:").into_owned(),
            Step::FirstRay(_) => {
                t!("DIMANGULAR  Specify first extension line point:").into_owned()
            }
            Step::SecondRay { .. } => {
                t!("DIMANGULAR  Specify second extension line point:").into_owned()
            }
            Step::ArcPoint { .. } => t!("DIMANGULAR  Specify dimension arc location:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Vertex => {
                self.step = Step::FirstRay(pt);
                CmdResult::NeedPoint
            }
            Step::FirstRay(vertex) => {
                self.step = Step::SecondRay { vertex, first: pt };
                CmdResult::NeedPoint
            }
            Step::SecondRay { vertex, first } => {
                self.step = Step::ArcPoint {
                    vertex,
                    first,
                    second: pt,
                };
                CmdResult::NeedPoint
            }
            Step::ArcPoint {
                vertex,
                first,
                second,
            } => {
                let vertex = self.plane.to_local(vertex);
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(second);
                let pt = self.plane.to_local(pt);
                let mut dim = DimensionAngular3Pt::new(v3(vertex), v3(first), v3(second));
                dim.definition_point = v3(pt);
                dim.base.definition_point = v3(pt);
                dim.base.text_middle_point = v3(pt);
                dim.base.insertion_point = v3(pt);
                dim.base.actual_measurement = dim.measurement_degrees();
                CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Dimension(
                    Dimension::Angular3Pt(dim),
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let pt = pt.as_vec3();
        match self.step {
            Step::Vertex => None,
            Step::FirstRay(vertex) => Some(preview_wire(vec![vertex.as_vec3(), pt])),
            Step::SecondRay { vertex, first } => Some(preview_wire(vec![
                vertex.as_vec3(),
                first.as_vec3(),
                Vec3::new(f32::NAN, f32::NAN, f32::NAN),
                vertex.as_vec3(),
                pt,
            ])),
            Step::ArcPoint {
                vertex,
                first,
                second,
            } => {
                let points = angular_preview(
                    self.plane.to_local(vertex).as_vec3(),
                    self.plane.to_local(first).as_vec3(),
                    self.plane.to_local(second).as_vec3(),
                    self.plane.to_local(pt.as_dvec3()).as_vec3(),
                )
                .into_iter()
                .map(|point| {
                    if point.x.is_nan() {
                        point
                    } else {
                        self.plane.to_world(point.as_dvec3()).as_vec3()
                    }
                })
                .collect();
                Some(preview_wire(points))
            }
        }
    }
}

fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
}

fn preview_wire(points: Vec<Vec3>) -> WireModel {
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
        name: "dimangular_preview".to_string(),
        points: points.into_iter().map(|p| [p.x, p.y, p.z]).collect(),
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

fn angular_preview(vertex: Vec3, first: Vec3, second: Vec3, arc_pt: Vec3) -> Vec<Vec3> {
    let mut points = vec![
        vertex,
        first,
        Vec3::new(f32::NAN, f32::NAN, f32::NAN),
        vertex,
        second,
        Vec3::new(f32::NAN, f32::NAN, f32::NAN),
    ];
    let r = vertex.distance(arc_pt);
    if r <= 1e-6 {
        return points;
    }
    let a0 = (first.y - vertex.y).atan2(first.x - vertex.x);
    let mut a1 = (second.y - vertex.y).atan2(second.x - vertex.x);
    let mut delta = a1 - a0;
    while delta <= 0.0 {
        delta += std::f32::consts::TAU;
    }
    if delta > std::f32::consts::PI {
        a1 -= std::f32::consts::TAU;
        delta = a1 - a0;
    }
    let steps = 24;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = a0 + delta * t;
        points.push(vertex + Vec3::new(a.cos() * r, a.sin() * r, 0.0));
    }
    points
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMANGULAR"] });  // AngularDimensionCommand
