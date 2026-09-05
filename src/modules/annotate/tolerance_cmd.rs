// TOLERANCE command — place a GD&T (geometric dimensioning & tolerancing) frame.
//
// The structured editor prepares the frame; this command places it.

use acadrust::entities::Tolerance;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/tolerance.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TOLERANCE",
        label: "Tolerance",
        icon: ICON,
        event: ModuleEvent::Command("TOLERANCE".to_string()),
    }
}

pub struct ToleranceCommand {
    text: String,
    preview_strokes: Vec<Vec<[f32; 2]>>,
    plane: WorkingPlane,
}

impl ToleranceCommand {
    pub fn with_text(text: String, preview_strokes: Vec<Vec<[f32; 2]>>) -> Self {
        Self {
            text,
            preview_strokes,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ToleranceCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TOLERANCE"
    }

    fn prompt(&self) -> String {
        t!("TOLERANCE  Specify insertion point:").into_owned()
    }

    fn wants_text_input(&self) -> bool {
        false
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let point = self.plane.to_local(pt);
        let ins = Vector3::new(point.x, point.y, point.z);
        let tol = Tolerance::with_text(ins, self.text.clone());
        CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Tolerance(tol)))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let mut points = Vec::new();
        for stroke in &self.preview_strokes {
            if stroke.len() < 2 {
                continue;
            }
            if !points.is_empty() {
                points.push(DVec3::splat(f64::NAN));
            }
            points.extend(stroke.iter().map(|[x, y]| {
                pt + self.plane.x * *x as f64 + self.plane.y * *y as f64
            }));
        }
        Some(WireModel {
            bg_adapt: None,
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
            name: "tolerance_preview".into(),
            points: points.iter().map(|point| point.as_vec3().to_array()).collect(),
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
        })
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TOLERANCE"] });  // ToleranceCommand
