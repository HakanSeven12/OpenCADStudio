//! CTABLESTYLE: the table style new tables take. Typed alone it asks for the
//! name; `SETVAR CTABLESTYLE name` sets it. The name must be a table style
//! of the drawing (any case); it is kept spelled as the style is.

use crate::app::{Message, OpenCADStudio};
use crate::modules::StyleKey;
use iced::Task;

const NAME: &str = "CTABLESTYLE";

impl OpenCADStudio {
    pub(super) fn dispatch_table_style_var(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name.trim_start_matches(['_', '.']).eq_ignore_ascii_case(NAME) {
            return None;
        }
        let ask = |app: &mut Self| {
            let current = app.tabs[i].scene.document.header.current_table_style_name.clone();
            app.command_line.push_output(&crate::t!(
                "Enter new value for CTABLESTYLE <\"%{name}\">:",
                name = current
            ));
            app.pending_setvar = Some(NAME.to_string());
        };
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() {
            ask(self);
            return Some(Task::none());
        }
        let style = self.tabs[i]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                codec::objects::ObjectType::TableStyle(style)
                    if style.name.eq_ignore_ascii_case(value) =>
                {
                    Some(style.name.clone())
                }
                _ => None,
            });
        match style {
            Some(style) => Some(self.on_ribbon_style_changed(StyleKey::TableStyle, style)),
            None => {
                self.command_line
                    .push_error(crate::t!("Cannot set CTABLESTYLE to that value.").as_ref());
                self.command_line.push_error("*Invalid*");
                Some(Task::none())
            }
        }
    }
}
