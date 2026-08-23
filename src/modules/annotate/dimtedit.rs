// DIMTEDIT command — reposition the text of an existing dimension.
//
// Workflow:
//   1. Pick a dimension entity
//   2. Click the new text position (entity data is injected by update.rs after pick)

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_tedit.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMTEDIT",
        label: "Dim Text Edit",
        icon: ICON,
        event: ModuleEvent::Command("DIMTEDIT".to_string()),
    }
}

enum Step {
    PickDim,
    PickTextPos {
        handle: Handle,
        entity: Option<EntityType>,
    },
}

pub struct DimTeditCommand {
    step: Step,
}

impl DimTeditCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickDim,
        }
    }
}

impl CadCommand for DimTeditCommand {
    fn name(&self) -> &'static str {
        "DIMTEDIT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::PickDim => t!("DIMTEDIT  Select dimension:").into_owned(),
            Step::PickTextPos { .. } => {
                t!("DIMTEDIT  Specify new location for dimension text:").into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDim)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.step = Step::PickTextPos {
            handle,
            entity: None,
        };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Step::PickTextPos { handle, entity } = &mut self.step {
            let h = *handle;
            if let Some(mut ent) = entity.take() {
                if let EntityType::Dimension(ref mut d) = ent {
                    let new_pt = acadrust::types::Vector3::new(pt.x, pt.y, pt.z);
                    d.base_mut().text_middle_point = new_pt;
                    d.base_mut().insertion_point = new_pt;
                    // Pin the text to this location, else the renderer recomputes
                    // the style-default placement and the move is discarded.
                    d.base_mut().text_user_positioned = true;
                }
                return CmdResult::ReplaceEntity(h, vec![ent]);
            }
            // No entity yet — wait for inject
            CmdResult::NeedPoint
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        if !matches!(self.step, Step::PickTextPos { .. }) {
            return None;
        }
        let d = 0.2_f32;
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
            name: "dimtedit_preview".into(),
            // Marker box in the XY drawing plane (Z is elevation, ~0). The old
            // box varied Z, so in the top-down view it collapsed to a flat line
            // instead of a square. (#150)
            points: vec![
                [pt.x - d, pt.y - d, pt.z],
                [pt.x + d, pt.y - d, pt.z],
                [pt.x + d, pt.y + d, pt.z],
                [pt.x - d, pt.y + d, pt.z],
                [pt.x - d, pt.y - d, pt.z],
            ],
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

    fn inject_picked_entity(&mut self, entity: EntityType) {
        if let Step::PickTextPos { entity: slot, .. } = &mut self.step {
            *slot = Some(entity);
        }
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMTED", "DIMTEDIT"] });  // DimTeditCommand
