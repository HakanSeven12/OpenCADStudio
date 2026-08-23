// DIMJOGLINE command — add a jog (zigzag) symbol to a linear or aligned dimension.
//
// Workflow:
//   1. Pick the dimension
//   2. Click the position on the dimension line where the jog should appear

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_jog.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMJOGLINE",
        label: "Jog Line",
        icon: ICON,
        event: ModuleEvent::Command("DIMJOGLINE".to_string()),
    }
}

enum Step {
    PickDim,
    PickJogPos { handle: Handle },
}

pub struct DimJogLineCommand {
    step: Step,
}

impl DimJogLineCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickDim,
        }
    }
}

impl CadCommand for DimJogLineCommand {
    fn name(&self) -> &'static str {
        "DIMJOGLINE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::PickDim => t!("DIMJOGLINE  Select linear or aligned dimension:").into_owned(),
            Step::PickJogPos { .. } => t!("DIMJOGLINE  Specify jog location:").into_owned(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.step = Step::PickJogPos { handle };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Step::PickJogPos { handle } = &self.step {
            let h = *handle;
            // Emit sentinel for commands.rs to store the jog position
            use acadrust::entities::XLine;
            let mut xl = XLine::default();
            xl.common.layer = format!("__DIMJOG__{},{:.6},{:.6}", h.value(), pt.x, pt.z);
            return CmdResult::ReplaceEntity(h, vec![acadrust::EntityType::XLine(xl)]);
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        if !matches!(self.step, Step::PickJogPos { .. }) {
            return None;
        }
        let d = 0.3_f32;
        Some(WireModel {
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
            name: "dimjog_preview".into(),
            // Jog zigzag in the XY drawing plane (Z is elevation, ~0). The old
            // marker varied Z, so in the top-down view it collapsed to a flat
            // line instead of a zigzag. (#150)
            points: vec![
                [pt.x - d, pt.y, pt.z],
                [pt.x - d * 0.3, pt.y + d, pt.z],
                [pt.x + d * 0.3, pt.y - d, pt.z],
                [pt.x + d, pt.y, pt.z],
            ],
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.2,
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
inventory::submit!(crate::command::CommandRegistration { names: &["DIMJOGLINE"] });  // DimJogLineCommand
