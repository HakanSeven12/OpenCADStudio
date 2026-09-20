// ID command — report coordinates of a picked point.

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};

pub struct IdCommand {
    plane: WorkingPlane,
}

impl IdCommand {
    pub fn new() -> Self {
        Self {
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for IdCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ID"
    }

    fn prompt(&self) -> String {
        t!("ID  Specify point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let local = self.plane.to_local(pt);
        let x = local.x;
        let y = local.y;
        let z = local.z;
        let x_s = format!("{x:.4}");
        let y_s = format!("{y:.4}");
        let z_s = format!("{z:.4}");
        let msg = t!("X = %{x},  Y = %{y},  Z = %{z}", x = x_s, y = y_s, z = z_s).into_owned();
        CmdResult::Measurement(msg)
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ID"] }); // IdCommand
