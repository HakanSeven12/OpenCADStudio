//! HPNAME: the pattern a new hatch starts with. Session only: not saved and
//! not part of the profile; it starts as `_SOLID`. Typed alone it asks for
//! the name; `SETVAR HPNAME name` sets it. `.` is the pattern file's first
//! pattern; a name the pattern file does not have is refused.

use crate::app::{Message, OpenCADStudio};
use crate::modules::draw::draw::hatch::{hpname, hpname_value, set_hpname};
use iced::Task;

const NAME: &str = "HPNAME";

impl OpenCADStudio {
    pub(super) fn dispatch_hatch_var(&mut self, cmd: &str) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name.trim_start_matches(['_', '.']).eq_ignore_ascii_case(NAME) {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            let current = format!("\"{}\"", hpname());
            self.command_line.push_output(
                crate::tf!("Enter new value for {name} <{current}>:", name = NAME).as_ref(),
            );
            self.pending_setvar = Some(NAME.to_string());
            return Some(Task::none());
        }
        match hpname_value(value) {
            Some(name) => set_hpname(&name),
            None => {
                self.command_line
                    .push_output(crate::t!("Cannot set HPNAME to that value.").as_ref());
                self.command_line.push_output("*Invalid*");
            }
        }
        Some(Task::none())
    }
}
