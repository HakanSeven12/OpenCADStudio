// DIMORDINATE command — ordinate (datum) dimension.
//
// Measures the X or Y distance from the UCS origin (datum) to a feature point.
// The user picks:
//   1. The feature location.
//   2. The leader endpoint (where the annotation line ends).
//
// If the leader moves mainly in Y → X-type ordinate (shows X coordinate).
// If the leader moves mainly in X → Y-type ordinate (shows Y coordinate).

use acadrust::entities::{Dimension, DimensionOrdinate};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Vec3};

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMORDINATE",
        label: "Ordinate",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/dim_ordinate.svg")),
        event: ModuleEvent::Command("DIMORDINATE".to_string()),
    }
}

enum Step {
    FeaturePoint,
    LeaderEndpoint { feature: DVec3 },
}

pub struct OrdinateDimCommand {
    step: Step,
    plane: WorkingPlane,
}

impl OrdinateDimCommand {
    pub fn new() -> Self {
        Self {
            step: Step::FeaturePoint,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for OrdinateDimCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMORDINATE"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::FeaturePoint => t!("DIMORDINATE  Specify feature location:").into_owned(),
            Step::LeaderEndpoint { .. } => t!("DIMORDINATE  Specify leader endpoint:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::FeaturePoint => {
                self.step = Step::LeaderEndpoint { feature: pt };
                CmdResult::NeedPoint
            }
            Step::LeaderEndpoint { feature } => {
                let feature = self.plane.to_local(feature);
                let pt = self.plane.to_local(pt);
                let is_x = is_x_type(feature, pt);
                let elbow = ordinate_elbow(feature, pt, is_x);
                let mut dim = DimensionOrdinate::new(v3(feature), v3(pt), is_x);
                // The leader is an orthogonal L from the feature to the
                // endpoint; store its elbow as the definition point. Without it
                // the renderer draws feature → (0,0,0) → endpoint, kinking the
                // leader through the world origin. The old code also worked in
                // the wrong (XZ) plane, dropping the Y coordinate. (#150)
                dim.definition_point = v3(elbow);
                dim.base.definition_point = v3(elbow);
                CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Dimension(
                    Dimension::Ordinate(dim),
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let feature = match self.step {
            Step::LeaderEndpoint { feature } => feature,
            _ => return None,
        };
        let feature = self.plane.to_local(feature);
        let pt = self.plane.to_local(pt);
        let is_x = is_x_type(feature, pt);
        let elbow = ordinate_elbow(feature, pt, is_x);
        let feature = self.plane.to_world(feature);
        let elbow = self.plane.to_world(elbow);
        let pt = self.plane.to_world(pt);
        // Screen-only rubber band: downcast to f32 at the preview boundary.
        Some(preview_wire(vec![
            feature.as_vec3(),
            elbow.as_vec3(),
            pt.as_vec3(),
        ]))
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

/// X-datum (labels the feature's X coordinate) when the leader runs more
/// vertically than horizontally; Y-datum otherwise. Mirrors the placement
/// decision so the preview and the committed entity agree.
fn is_x_type(feature: DVec3, leader: DVec3) -> bool {
    let dx = (leader.x - feature.x).abs();
    let dy = (leader.y - feature.y).abs();
    dy >= dx
}

/// Orthogonal elbow of the ordinate leader: an X-datum runs along Y from the
/// feature then jogs across in X; a Y-datum runs along X then jogs in Y.
fn ordinate_elbow(feature: DVec3, leader: DVec3, is_x: bool) -> DVec3 {
    if is_x {
        DVec3::new(feature.x, leader.y, feature.z)
    } else {
        DVec3::new(leader.x, feature.y, feature.z)
    }
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
        name: "dimordinate_preview".into(),
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


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMORDINATE"] });  // OrdinateDimCommand
