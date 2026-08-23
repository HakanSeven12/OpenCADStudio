// ZOOM WINDOW command — pick two corners to define the zoom area.

use glam::{DVec3, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/zoom_window.svg"));

/// Ribbon button: zoom into a rectangle picked by two corners.
pub fn tool() -> ToolDef {
    ToolDef {
        id: "ZOOM_WINDOW",
        label: "Zoom Window",
        icon: ICON,
        event: ModuleEvent::Command("ZOOM WINDOW".to_string()),
    }
}

pub struct ZoomWindowCommand {
    first: Option<Vec3>,
    scale_prompt: bool,
}

impl ZoomWindowCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            scale_prompt: false,
        }
    }
}

impl CadCommand for ZoomWindowCommand {
    fn name(&self) -> &'static str {
        "ZOOM WINDOW"
    }

    fn prompt(&self) -> String {
        if self.scale_prompt {
            t!("ZOOM  scale factor (e.g. 2 or 0.5):").into_owned()
        } else if self.first.is_none() {
            t!("ZOOM WINDOW  Specify first corner:").into_owned()
        } else {
            t!("ZOOM WINDOW  Specify opposite corner:").into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.first.is_some() || self.scale_prompt {
            return Vec::new();
        }
        vec![
            CmdOption::new("Window", "W"),
            CmdOption::new("Extents", "E"),
            CmdOption::new("Previous", "P"),
            CmdOption::new("Object", "O"),
            CmdOption::new("All", "A"),
            CmdOption::new("Dynamic", "D"),
            CmdOption::new("Extents All", "EA"),
            CmdOption::new("In", "I"),
            CmdOption::new("Out", "OUT"),
            CmdOption::new("Scale", "S"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if self.scale_prompt {
            return Some(CmdResult::Dispatch(format!("ZOOM SCALE {text}")));
        }
        let command = match text.to_uppercase().as_str() {
            "W" | "WINDOW" => {
                self.first = None;
                return Some(CmdResult::NeedPoint);
            }
            "E" | "EXTENTS" => "ZOOM EXTENTS",
            "P" | "PREVIOUS" => "ZOOM PREVIOUS",
            "O" | "OBJECT" => "ZOOM OBJECT",
            "A" | "ALL" => "ZOOM ALL",
            "D" | "DYNAMIC" => "ZOOM DYNAMIC",
            "EA" | "EXTENTS ALL" => "ZOOM EXTENTS ALL",
            "I" | "IN" => "ZOOM IN",
            "OUT" => "ZOOM OUT",
            "S" | "SCALE" => {
                self.first = None;
                self.scale_prompt = true;
                return Some(CmdResult::NeedPoint);
            }
            _ => return None,
        };
        Some(CmdResult::Dispatch(command.to_string()))
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.scale_prompt {
            return CmdResult::NeedPoint;
        }
        let pt = pt.as_vec3();
        if let Some(p1) = self.first {
            CmdResult::ZoomToWindow { p1: p1.as_dvec3(), p2: pt.as_dvec3() }
        } else {
            self.first = Some(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn window_corner_pick(&self) -> bool {
        // Both zoom-window corners are free points; Ortho/Polar must not pin
        // the opposite corner to an axis or the window collapses to a line
        // (#363, same class as #291).
        true
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        let p1 = self.first?;
        let min = p1.min(pt);
        let max = p1.max(pt);
        // Draw a rectangle preview
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
            name: "zoom_window_preview".into(),
            points: vec![
                [min.x, min.y, 0.0],
                [max.x, min.y, 0.0],
                [max.x, max.y, 0.0],
                [min.x, max.y, 0.0],
                [min.x, min.y, 0.0],
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
}
