//! MODEMACRO: a DIESEL string whose value the status bar shows at its left
//! end. Session only: not saved, it starts empty. Typed alone it asks for
//! the string; `SETVAR MODEMACRO $(getvar,clayer)` sets it, `.` clears it.

use crate::app::{Message, OpenCADStudio};
use iced::Task;

const NAME: &str = "MODEMACRO";

/// The value of DIESEL string `expr` in `document`; `$?` where it cannot be
/// evaluated (an unknown variable, a bad argument).
pub(in crate::app) fn diesel(document: &document::Document, expr: &str) -> String {
    crate::entities::field::evaluate(document, &format!("\\AcDiesel {expr}"), &[], None)
        .unwrap_or_else(|| "$?".to_string())
}

impl OpenCADStudio {
    pub(super) fn dispatch_modemacro_var(&mut self, cmd: &str) -> Option<Task<Message>> {
        let rest = cmd.trim_start();
        let rest = match rest.get(..7) {
            Some(head) if head.eq_ignore_ascii_case("SETVAR ") => rest[7..].trim_start(),
            _ => rest,
        };
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name
            .trim_start_matches(['_', '.'])
            .eq_ignore_ascii_case(NAME)
        {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            let current = format!("\"{}\"", self.modemacro);
            self.command_line.push_output(
                crate::tf!("Enter new value for {name} <{current}>:", name = NAME).as_ref(),
            );
            self.pending_setvar = Some(NAME.to_string());
            return Some(Task::none());
        }
        self.modemacro = if value == "." {
            String::new()
        } else {
            value.to_string()
        };
        Some(Task::none())
    }
}
