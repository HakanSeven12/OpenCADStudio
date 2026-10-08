//! Block system variables. `BLOCKMRULIST`, `BLOCKREDEFINEMODE` and
//! `BLOCKNAVIGATE` are application preferences kept with the settings;
//! `EXPLMODE` lives in the drawing; `INSNAME` is the session's default block
//! name; `BLOCKSTATE` is read-only. Each answers to its bare name (prompting
//! for the value) and to `SETVAR NAME value`, with the reference's messages.

use crate::app::{Message, OpenCADStudio};
use iced::Task;

/// The variables this family answers for, for the command registry and the
/// `SETVAR ?` listing.
pub(super) const BLOCK_SYSVARS: &[&str] = &[
    "BLOCKMRULIST",
    "BLOCKREDEFINEMODE",
    "BLOCKNAVIGATE",
    "BLOCKSTATE",
    "EXPLMODE",
    "INSNAME",
];

/// The `SETVAR ?` line for this family.
pub(super) fn setvar_listing() -> String {
    format!("SETVAR (block): {}", BLOCK_SYSVARS.join(" "))
}

/// EXPLMODE of the drawing (default 1).
pub(crate) fn explmode(document: &codec::CadDocument) -> i16 {
    crate::io::drawing_variable(document, "EXPLMODE")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1)
}

impl OpenCADStudio {
    pub(super) fn dispatch_blockvars(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        if !BLOCK_SYSVARS.contains(&name.as_str()) {
            return None;
        }
        let value = parts.next().map(str::trim).filter(|v| !v.is_empty());
        match name.as_str() {
            "BLOCKSTATE" => {
                let state = self.tabs[i].refedit_session.is_some() as u8;
                self.command_line.push_output(&format!("BLOCKSTATE = {state} (read only)"));
            }
            "INSNAME" | "BLOCKNAVIGATE" => {
                let current = if name == "INSNAME" {
                    self.insname.clone()
                } else {
                    self.block_navigate.clone()
                };
                match value {
                    Some(v) => {
                        let v = if v == "." { String::new() } else { v.trim_matches('"').to_string() };
                        if name == "INSNAME" {
                            self.insname = v;
                        } else {
                            self.block_navigate = if v.is_empty() { ".".to_string() } else { v };
                            self.persist_settings_if_changed();
                        }
                    }
                    None => {
                        self.command_line.push_output(&format!(
                            "Enter new value for {name}, or . for none <\"{current}\">:"
                        ));
                        self.pending_setvar = Some(name);
                    }
                }
            }
            _ => {
                let (current, max) = match name.as_str() {
                    "BLOCKMRULIST" => (self.block_mru_list as i32, 100),
                    "BLOCKREDEFINEMODE" => (self.block_redefine_mode as i32, 2),
                    _ => (explmode(&self.tabs[i].scene.document) as i32, 1),
                };
                let ask = |app: &mut Self, name: String| {
                    app.command_line
                        .push_output(&format!("Enter new value for {name} <{current}>:"));
                    app.pending_setvar = Some(name);
                };
                match value {
                    None => ask(self, name),
                    Some(v) => match v.parse::<i32>() {
                        Err(_) => {
                            self.command_line.push_error("Requires an integer value.");
                            ask(self, name);
                        }
                        Ok(n) if !(0..=max).contains(&n) => {
                            self.command_line.push_error(&if max == 1 {
                                "Requires 0 or 1 only.".to_string()
                            } else {
                                format!("Requires an integer between 0 and {max}.")
                            });
                            ask(self, name);
                        }
                        Ok(n) => match name.as_str() {
                            "BLOCKMRULIST" => {
                                self.block_mru_list = n as u8;
                                self.trim_recent_blocks();
                                self.persist_settings_if_changed();
                            }
                            "BLOCKREDEFINEMODE" => {
                                self.block_redefine_mode = n as u8;
                                self.persist_settings_if_changed();
                            }
                            _ => {
                                if n != current {
                                    self.push_undo_snapshot(i, "EXPLMODE");
                                    crate::io::set_drawing_variable(
                                        &mut self.tabs[i].scene.document,
                                        "EXPLMODE",
                                        &n.to_string(),
                                    );
                                    self.tabs[i].dirty = true;
                                }
                            }
                        },
                    },
                }
            }
        }
        Some(Task::none())
    }
}
