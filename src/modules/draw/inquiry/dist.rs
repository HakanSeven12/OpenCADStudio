// DIST command — measure distance and angle between two picked points.

use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

pub struct DistCommand {
    // Keep the picked point in full f64 precision. Downcasting to f32 here
    // loses ~0.03–0.06 units at survey-scale coordinates (e.g. eastings near
    // 5e5), which made snapped-endpoint measurements read off by that much.
    first: Option<DVec3>,
    plane: WorkingPlane,
}

impl DistCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for DistCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIST"
    }

    fn prompt(&self) -> String {
        if self.first.is_none() {
            t!("DIST  Specify first point:").into_owned()
        } else {
            t!("DIST  Specify second point:").into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(p1) = self.first {
            let delta = self.plane.vector_to_local(pt - p1);
            let dist = delta.length();
            let dx = delta.x;
            let dy = delta.y;
            let dz = delta.z;

            // Angle in XY plane — degrees from +X
            let angle_xy = dy.atan2(dx).to_degrees();
            // Angle from XY plane toward Z (elevation angle)
            let dist_xy = dx.hypot(dy);
            let angle_z = dz.atan2(dist_xy).to_degrees();

            let dist_s = format!("{dist:.4}");
            let angle_xy_s = format!("{angle_xy:.4}");
            let angle_z_s = format!("{angle_z:.4}");
            let dx_s = format!("{dx:.4}");
            let dy_s = format!("{dy:.4}");
            let dz_s = format!("{dz:.4}");
            let msg = t!(
                "Distance = %{dist},  Angle in XY Plane = %{angle_xy}°,  Angle from XY Plane = %{angle_z}°\n  Delta X = %{dx},  Delta Y = %{dy},  Delta Z = %{dz}",
                dist = dist_s,
                angle_xy = angle_xy_s,
                angle_z = angle_z_s,
                dx = dx_s,
                dy = dy_s,
                dz = dz_s,
            )
            .into_owned();
            CmdResult::Measurement(msg)
        } else {
            self.first = Some(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let p1 = self.first?;
        // The preview wire is purely visual, so f32 vertices are fine here.
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
            name: "dist_preview".into(),
            points: vec![
                [p1.x as f32, p1.y as f32, p1.z as f32],
                [pt.x as f32, pt.y as f32, pt.z as f32],
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
inventory::submit!(crate::command::CommandRegistration { names: &["DIST"] });  // DistCommand
