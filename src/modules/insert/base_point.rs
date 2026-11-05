use crate::command::{CadCommand, CmdResult};
use glam::DVec3;

// ── Command implementation ────────────────────────────────────────────────

/// BASE (`Enter base point <x,y,z>:`) and INSBASE
/// (`Enter new value for INSBASE <x,y,z>:`): a point sets the active space's
/// insertion base through the inline `BASE <x> <y> <z>` handler; Enter keeps
/// the current one.
pub struct BaseCommand {
    current: DVec3,
    variable: bool,
}

impl BaseCommand {
    pub fn new(current: DVec3) -> Self {
        Self {
            current,
            variable: false,
        }
    }

    /// The INSBASE system variable's prompt.
    pub fn insbase(current: DVec3) -> Self {
        Self {
            current,
            variable: true,
        }
    }
}

fn point_text(p: DVec3) -> String {
    format!("{:.4},{:.4},{:.4}", p.x, p.y, p.z)
}

impl CadCommand for BaseCommand {
    fn name(&self) -> &'static str {
        if self.variable {
            "INSBASE"
        } else {
            "BASE"
        }
    }

    fn prompt(&self) -> String {
        if self.variable {
            crate::ui::command_line::value_prompt("INSBASE", &point_text(self.current)
            )
        } else {
            format!("Enter base point <{}>:", point_text(self.current))
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("BASE {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Measurement(String::new())
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["BASE"] }); // BaseCommand
