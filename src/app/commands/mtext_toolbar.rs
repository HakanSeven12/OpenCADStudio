//! MTEXTTOOLBAR: where the formatting tools of the in-place multiline text
//! editor show. 0 never on the floating toolbar, 1 always, 2 only while the
//! ribbon's Text Editor tab is not on show (the default). A profile setting.
//! MTEXTFIXED: 2 shows a text too small to read at a readable size and
//! level while it is edited, 0 and 1 as it is (Always Display as WYSIWYG).
//! MTEXTAUTOSTACK: AutoStack on (1) or off (0).

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
        const NAMES: [&str; 3] = ["MTEXTTOOLBAR", "MTEXTFIXED", "MTEXTAUTOSTACK"];
        let (name, value) = if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let name = name.to_ascii_uppercase();
            (NAMES.into_iter().find(|n| *n == name)?, value.trim())
        } else {
            (NAMES.into_iter().find(|n| *n == verb)?, rest)
        };
        let current = |name: &str| -> i16 {
            let p = crate::app::mtext_prefs::prefs();
            match name {
                "MTEXTFIXED" => {
                    if p.wysiwyg {
                        0
                    } else {
                        2
                    }
                }
                "MTEXTAUTOSTACK" => i16::from(p.auto_stack),
                _ => mtext_toolbar(),
            }
        };
        let top: i16 = if name == "MTEXTAUTOSTACK" { 1 } else { 2 };
        let ask = |app: &mut Self| {
            app.command_line.push_output(&crate::ui::command_line::value_prompt(
                name,
                &current(name).to_string(),
            ));
            app.pending_setvar = Some(name.to_string());
        };
        if value.is_empty() {
            if verb != "SETVAR" {
                ask(self);
            }
            return Some(Task::none());
        }
        match value.parse::<i16>().ok().filter(|v| (0..=top).contains(v)) {
            Some(v) => {
                if v != current(name) {
                    match name {
                        "MTEXTFIXED" => {
                            crate::app::mtext_prefs::update_prefs(|p| p.wysiwyg = v != 2)
                        }
                        "MTEXTAUTOSTACK" => {
                            crate::app::mtext_prefs::update_prefs(|p| p.auto_stack = v == 1)
                        }
                        _ => set_mtext_toolbar(v),
                    }
                    self.save_config();
                }
            }
            None => {
                self.command_line.push_rejection(if top == 1 {
                    "Requires 0 or 1 only."
                } else {
                    "Requires an integer between 0 and 2."
                });
                ask(self);
            }
        }
        Some(Task::none())
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["MTEXTTOOLBAR", "MTEXTFIXED", "MTEXTAUTOSTACK"]
});
