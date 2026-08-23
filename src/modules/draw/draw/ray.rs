// Ray / XLine draw commands.
//
//  RAY   — semi-infinite line: click base point, then click direction point.
//          Produces a Ray entity; repeats until Enter/Esc.
//  XLINE — infinite construction line: same two-click pattern, yields XLine.

use acadrust::entities::{Ray as RayEnt, XLine as XLineEnt};
use acadrust::types::Vector3;
use acadrust::EntityType;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

const DISPLAY_EXTENT: f32 = 1_000_000.0;

// ── RAY ───────────────────────────────────────────────────────────────────

pub struct RayCommand {
    base: Option<DVec3>,
}

impl RayCommand {
    pub fn new() -> Self {
        Self { base: None }
    }
}

impl CadCommand for RayCommand {
    fn name(&self) -> &'static str {
        "RAY"
    }

    fn prompt(&self) -> String {
        if self.base.is_none() {
            crate::t!("RAY  Specify start point:").into_owned()
        } else {
            crate::t!("RAY  Specify through point:").into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.base.is_some() {
            vec![CmdOption::enter(t!("Done").as_ref())]
        } else {
            vec![]
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(base) = self.base {
            let dir = pt - base;
            let len = dir.length();
            if len < 1e-6 {
                return CmdResult::NeedPoint;
            }
            let dir_n = dir / len;
            let ray = RayEnt::new(
                Vector3::new(base.x, base.y, base.z),
                Vector3::new(dir_n.x, dir_n.y, dir_n.z),
            );
            // Stay active: new base = same base (can keep clicking through points)
            // Actually AutoCAD prompts for new start after each ray — reset base.
            self.base = None;
            CmdResult::CommitEntity(EntityType::Ray(ray))
        } else {
            self.base = Some(pt);
            CmdResult::NeedPoint
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
        let base = self.base?.as_vec3();
        let dir = (pt - base).normalize_or_zero();
        let far = base + dir * DISPLAY_EXTENT;
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
            name: "ray_preview".into(),
            points: vec![[base.x, base.y, base.z], [far.x, far.y, far.z]],
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

// ── XLINE ─────────────────────────────────────────────────────────────────

pub struct XLineCommand {
    base: Option<DVec3>,
}

impl XLineCommand {
    pub fn new() -> Self {
        Self { base: None }
    }
}

impl CadCommand for XLineCommand {
    fn name(&self) -> &'static str {
        "XLINE"
    }

    fn prompt(&self) -> String {
        if self.base.is_none() {
            t!("XLINE  Specify a point:").into_owned()
        } else {
            t!("XLINE  Specify through point:").into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.base.is_some() {
            vec![CmdOption::enter(t!("Done").as_ref())]
        } else {
            vec![]
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(base) = self.base {
            let dir = pt - base;
            let len = dir.length();
            if len < 1e-6 {
                return CmdResult::NeedPoint;
            }
            let dir_n = dir / len;
            let xline = XLineEnt::new(
                Vector3::new(base.x, base.y, base.z),
                Vector3::new(dir_n.x, dir_n.y, dir_n.z),
            );
            self.base = None;
            CmdResult::CommitEntity(EntityType::XLine(xline))
        } else {
            self.base = Some(pt);
            CmdResult::NeedPoint
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
        let base = self.base?.as_vec3();
        let dir = (pt - base).normalize_or_zero();
        let far_pos = base + dir * DISPLAY_EXTENT;
        let far_neg = base - dir * DISPLAY_EXTENT;
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
            name: "xline_preview".into(),
            points: vec![
                [far_neg.x, far_neg.y, far_neg.z],
                [far_pos.x, far_pos.y, far_pos.z],
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


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["RAY"] });  // RayCommand
inventory::submit!(crate::command::CommandRegistration { names: &["CONSTRUCTIONLINE", "XLINE"] });  // XLineCommand
