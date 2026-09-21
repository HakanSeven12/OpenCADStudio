//! Plot system variables and the page-setup import command. `PLOTOFFSET`,
//! `PAPERUPDATE`, `PLOTROTMODE`, `PLOTTRANSPARENCYOVERRIDE` and
//! `BACKGROUNDPLOT` are plot preferences of the application, kept with the
//! Plot dialog's persisted settings; `CTAB` and `TILEMODE` say which layout
//! of the drawing is current. Each is typeable on its own (`PLOTOFFSET 1`,
//! `CTAB Layout1`) and through `SETVAR`, and a bare name prompts for the
//! value like the other variables. `PSETUPIN` brings named page setups in
//! from another drawing (see `app::update::page_setup_import`).

use crate::app::{Message, OpenCADStudio};
use iced::Task;

/// The variables this family answers for, for the command registry and the
/// `SETVAR ?` listing.
pub(super) const PLOT_SYSVARS: &[&str] = &[
    "PLOTOFFSET",
    "PAPERUPDATE",
    "PLOTROTMODE",
    "PLOTTRANSPARENCYOVERRIDE",
    "BACKGROUNDPLOT",
    "CTAB",
    "TILEMODE",
];

/// The `SETVAR ?` line for this family.
pub(super) fn setvar_listing() -> String {
    format!("SETVAR (plot): {}", PLOT_SYSVARS.join(" "))
}

/// What setting a variable did.
enum Outcome {
    /// The value was set (or was already that); the variable is part of the
    /// application's settings and persists with them.
    Preference(String),
    /// The value was set and the drawing changed accordingly, possibly with
    /// a follow-up task (a layout switch).
    Drawing(String, Task<Message>),
    /// No value given: the current one, for the prompt.
    Current(String),
}

impl OpenCADStudio {
    pub(super) fn dispatch_plotvars(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        // PSETUPIN [file [* | name,name…]] — the dash form is the same
        // command; both read the file and report on the command line.
        for name in ["PSETUPIN", "-PSETUPIN"] {
            if cmd == name {
                return Some(self.on_psetupin(""));
            }
            if let Some(args) = cmd.strip_prefix(name).filter(|rest| rest.starts_with(' ')) {
                return Some(self.on_psetupin(args));
            }
        }
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        if !PLOT_SYSVARS.contains(&name.as_str()) {
            return None;
        }
        let value = parts.next().map(str::trim).filter(|v| !v.is_empty());
        let task = match self.plot_sysvar(i, &name, value) {
            Ok(Outcome::Preference(shown)) => {
                self.persist_settings_if_changed();
                self.command_line.push_output(&format!("{name} = {shown}"));
                Task::none()
            }
            Ok(Outcome::Drawing(shown, task)) => {
                self.command_line.push_output(&format!("{name} = {shown}"));
                task
            }
            Ok(Outcome::Current(current)) => {
                self.command_line.push_output(
                    crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                );
                self.pending_setvar = Some(name);
                Task::none()
            }
            Err(error) => {
                self.command_line.push_error(&error);
                Task::none()
            }
        };
        Some(task)
    }

    fn plot_sysvar(
        &mut self,
        i: usize,
        name: &str,
        value: Option<&str>,
    ) -> Result<Outcome, String> {
        let parse_range = |value: &str, max: u8| -> Result<u8, String> {
            value
                .parse::<u8>()
                .ok()
                .filter(|v| *v <= max)
                .ok_or_else(|| format!("SETVAR: integer from 0 to {max} required."))
        };
        let d = &mut self.plot_dialog;
        match name {
            "PLOTOFFSET" => Ok(match value {
                Some(v) => {
                    d.plot_offset_from_edge = parse_range(v, 1)? == 1;
                    Outcome::Preference((d.plot_offset_from_edge as u8).to_string())
                }
                None => Outcome::Current((d.plot_offset_from_edge as u8).to_string()),
            }),
            "PAPERUPDATE" => Ok(match value {
                Some(v) => {
                    d.paper_update = parse_range(v, 1)?;
                    Outcome::Preference(d.paper_update.to_string())
                }
                None => Outcome::Current(d.paper_update.to_string()),
            }),
            "PLOTROTMODE" => Ok(match value {
                Some(v) => {
                    d.plot_rot_mode = parse_range(v, 2)?;
                    Outcome::Preference(d.plot_rot_mode.to_string())
                }
                None => Outcome::Current(d.plot_rot_mode.to_string()),
            }),
            "PLOTTRANSPARENCYOVERRIDE" => Ok(match value {
                Some(v) => {
                    d.transparency_override = parse_range(v, 2)?;
                    Outcome::Preference(d.transparency_override.to_string())
                }
                None => Outcome::Current(d.transparency_override.to_string()),
            }),
            // Bit 1: single plots run in the background; bit 2: batch plots.
            "BACKGROUNDPLOT" => {
                let current = d.background as u8 | (d.background_publish as u8) << 1;
                Ok(match value {
                    Some(v) => {
                        let bits = parse_range(v, 3)?;
                        d.background = bits & 1 != 0;
                        d.background_publish = bits & 2 != 0;
                        Outcome::Preference(bits.to_string())
                    }
                    None => Outcome::Current(current.to_string()),
                })
            }
            "CTAB" => {
                let current = self.tabs[i].scene.current_layout.clone();
                let Some(v) = value else {
                    return Ok(Outcome::Current(current));
                };
                let Some(target) = self.tabs[i]
                    .scene
                    .layout_names()
                    .into_iter()
                    .find(|layout| layout.eq_ignore_ascii_case(v))
                else {
                    return Err(format!("CTAB: no layout named \"{v}\"."));
                };
                let task = if target == current {
                    Task::none()
                } else {
                    self.on_layout_switch(target.clone())
                };
                Ok(Outcome::Drawing(target, task))
            }
            // 1: model space; 0: the paper layout that was last current (the
            // drawing's own `CTAB`), else the first one.
            "TILEMODE" => {
                let in_model = self.tabs[i].scene.current_layout == "Model";
                let Some(v) = value else {
                    return Ok(Outcome::Current((in_model as u8).to_string()));
                };
                let model = parse_range(v, 1)? == 1;
                if model == in_model {
                    return Ok(Outcome::Drawing((model as u8).to_string(), Task::none()));
                }
                let target = if model {
                    "Model".to_string()
                } else {
                    let scene = &self.tabs[i].scene;
                    let layouts = scene.layout_names();
                    crate::io::saved_active_layout(&scene.document)
                        .filter(|saved| saved != "Model" && layouts.contains(saved))
                        .or_else(|| layouts.into_iter().find(|layout| layout != "Model"))
                        .ok_or_else(|| "TILEMODE: the drawing has no paper layout.".to_string())?
                };
                let task = self.on_layout_switch(target);
                Ok(Outcome::Drawing((model as u8).to_string(), task))
            }
            _ => Err(format!("SETVAR: unknown variable \"{name}\".")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line.history.last().map(|line| line.text.clone()).unwrap_or_default()
    }

    #[test]
    fn plot_preferences_are_typeable_directly_and_through_setvar() {
        let mut app = app();
        let _ = app.run_command_line("PLOTOFFSET 1");
        assert!(app.plot_dialog.plot_offset_from_edge);
        assert_eq!(last_line(&app), "PLOTOFFSET = 1");
        let _ = app.run_command_line("SETVAR PAPERUPDATE 1");
        assert_eq!(app.plot_dialog.paper_update, 1);
        let _ = app.run_command_line("PLOTROTMODE 0");
        assert_eq!(app.plot_dialog.plot_rot_mode, 0);
        let _ = app.run_command_line("PLOTTRANSPARENCYOVERRIDE 2");
        assert_eq!(app.plot_dialog.transparency_override, 2);
        // Out-of-range values are refused and leave the setting alone.
        let _ = app.run_command_line("PLOTROTMODE 3");
        assert_eq!(app.plot_dialog.plot_rot_mode, 0);
        assert!(last_line(&app).contains("0 to 2"));
        // The preferences reach the persisted plot config.
        let config = app.current_config();
        assert!(config.plot.plot_offset_from_edge);
        assert_eq!(config.plot.transparency_override, 2);
    }

    #[test]
    fn backgroundplot_maps_to_the_two_background_switches() {
        let mut app = app();
        let _ = app.run_command_line("BACKGROUNDPLOT 2");
        assert!(!app.plot_dialog.background);
        assert!(app.plot_dialog.background_publish);
        let _ = app.run_command_line("BACKGROUNDPLOT 3");
        assert!(app.plot_dialog.background && app.plot_dialog.background_publish);
        let _ = app.run_command_line("BACKGROUNDPLOT");
        assert_eq!(app.pending_setvar.as_deref(), Some("BACKGROUNDPLOT"));
        assert!(last_line(&app).contains("<3>"));
    }

    #[test]
    fn ctab_and_tilemode_switch_the_current_layout() {
        let mut app = app();
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Model");
        let _ = app.run_command_line("CTAB layout1");
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Layout1");
        assert_eq!(last_line(&app), "CTAB = Layout1");
        let _ = app.run_command_line("TILEMODE 1");
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Model");
        // Back to paper space: the layout that was current last.
        let _ = app.run_command_line("SETVAR TILEMODE 0");
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Layout1");
        let _ = app.run_command_line("CTAB Nowhere");
        assert!(last_line(&app).contains("no layout named"));
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Layout1");
        let _ = app.run_command_line("CTAB");
        assert!(last_line(&app).contains("<Layout1>"));
        assert_eq!(app.pending_setvar.as_deref(), Some("CTAB"));
    }
}
