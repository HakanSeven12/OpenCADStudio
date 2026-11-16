//! MTEXTTOOLBAR: where the formatting tools of the in-place multiline text
//! editor show. 0 never on the floating toolbar, 1 always, 2 only while the
//! ribbon's Text Editor tab is not on show (the default). A profile setting.

use std::sync::atomic::{AtomicI16, Ordering};

use iced::Task;

use crate::app::{Message, OpenCADStudio};

static MODE: AtomicI16 = AtomicI16::new(2);

pub(crate) fn mtext_toolbar() -> i16 {
    MODE.load(Ordering::Relaxed)
}

pub(crate) fn set_mtext_toolbar(value: i16) {
    MODE.store(value.clamp(0, 2), Ordering::Relaxed);
}

impl OpenCADStudio {
    /// Whether the in-place editor shows its floating toolbar now.
    pub(in crate::app) fn mtext_toolbar_shown(&self) -> bool {
        match mtext_toolbar() {
            0 => false,
            1 => true,
            _ => !self.ribbon.text_editor_tab_shown(),
        }
    }

    pub(super) fn dispatch_mtext_toolbar(&mut self, cmd: &str) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.trim_start_matches(['_', '.']).to_ascii_uppercase();
        let rest = rest.trim();
        let value = if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if !name.eq_ignore_ascii_case("MTEXTTOOLBAR") {
                return None;
            }
            value.trim()
        } else if verb == "MTEXTTOOLBAR" {
            rest
        } else {
            return None;
        };
        let ask = |app: &mut Self| {
            app.command_line.push_output(&crate::ui::command_line::value_prompt(
                "MTEXTTOOLBAR",
                &mtext_toolbar().to_string(),
            ));
            app.pending_setvar = Some("MTEXTTOOLBAR".to_string());
        };
        if value.is_empty() {
            if verb != "SETVAR" {
                ask(self);
            }
            return Some(Task::none());
        }
        match value.parse::<i16>().ok().filter(|v| (0..=2).contains(v)) {
            Some(v) => {
                if v != mtext_toolbar() {
                    set_mtext_toolbar(v);
                    self.save_config();
                }
            }
            None => {
                self.command_line
                    .push_rejection("Requires an integer between 0 and 2.");
                ask(self);
            }
        }
        Some(Task::none())
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["MTEXTTOOLBAR"]
});
