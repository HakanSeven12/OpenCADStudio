//! -VIEW: the command-line form of VIEW.
//!
//! The prompts follow the reference command line word for word. The command
//! only gathers input and checks the names it is given against the drawing
//! (a snapshot taken when it starts); whatever changes the drawing or the
//! camera is handed to the inline `-VIEW >…` handler through
//! [`CmdResult::Dispatch`], which re-enters the settings loop where the
//! reference does.

use crate::command::{CadCommand, CmdResult};
use glam::DVec3;
use std::sync::atomic::{AtomicBool, Ordering};

static UCS_ORTHO: AtomicBool = AtomicBool::new(true);
static UCS_VIEW: AtomicBool = AtomicBool::new(true);

/// UCSORTHO: an orthographic view preset also sets the matching orthographic UCS.
pub fn ucs_ortho() -> bool {
    UCS_ORTHO.load(Ordering::Relaxed)
}

pub fn set_ucs_ortho(on: bool) {
    UCS_ORTHO.store(on, Ordering::Relaxed);
}

/// UCSVIEW: a saved named view keeps the current UCS.
pub fn ucs_view() -> bool {
    UCS_VIEW.load(Ordering::Relaxed)
}

pub fn set_ucs_view(on: bool) {
    UCS_VIEW.store(on, Ordering::Relaxed);
}

/// The standard orthographic and isometric view presets.
pub const PRESETS: [&str; 10] =
    ["TOP", "BOTTOM", "FRONT", "BACK", "LEFT", "RIGHT", "SWISO", "SEISO", "NEISO", "NWISO"];

/// The orthographic UCS an orthographic preset sets while UCSORTHO is on:
/// its name, X and Y axes and ORTHOGRAPHIC type, as the reference sets them.
pub fn orthographic_ucs(preset: &str) -> Option<(&'static str, [f64; 3], [f64; 3], i16)> {
    Some(match preset {
        "TOP" => ("*TOP*", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1),
        "BOTTOM" => ("*BOTTOM*", [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2),
        "FRONT" => ("*FRONT*", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 3),
        "BACK" => ("*BACK*", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 4),
        "LEFT" => ("*LEFT*", [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], 5),
        "RIGHT" => ("*RIGHT*", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 6),
        _ => return None,
    })
}

/// One named view, as -VIEW's settings need to know it.
#[derive(Clone, Default)]
pub struct ViewInfo {
    pub name: String,
    pub category: String,
    pub visual_style: Option<String>,
    pub live_section: Option<String>,
}

/// What -VIEW checks typed names against.
#[derive(Clone, Default)]
pub struct ViewNames {
    pub views: Vec<ViewInfo>,
    pub visual_styles: Vec<String>,
    pub live_sections: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    Background,
    Categorize,
    LayerSnapshot,
    LiveSection,
    VisualStyle,
}

impl Setting {
    /// The word the inline handler takes.
    pub fn verb(self) -> &'static str {
        match self {
            Setting::Background => "BACKGROUND",
            Setting::Categorize => "CATEGORIZE",
            Setting::LayerSnapshot => "LAYER",
            Setting::LiveSection => "SECTION",
            Setting::VisualStyle => "VISUALSTYLE",
        }
    }

    fn view_prompt(self) -> &'static str {
        match self {
            Setting::Background => "Enter view name to edit Background or [?]:",
            Setting::Categorize => "Enter view name to Categorize or [?]:",
            Setting::LayerSnapshot => "Enter view name to edit Layer snapshot or [?]:",
            Setting::LiveSection => "Enter view name to edit live section or [?]:",
            Setting::VisualStyle => "Enter view name to edit visual style or [?]:",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Step {
    Main,
    Orthographic,
    List,
    Delete,
    Restore,
    Save,
    WindowName,
    WindowFirst(String),
    WindowSecond(String, DVec3),
    Settings,
    SettingView(Setting),
    SettingViewList(Setting),
    SettingValue(Setting, String),
    SettingValueList(Setting, String),
    Ucs,
}

pub struct DashViewCommand {
    names: ViewNames,
    step: Step,
}

/// Typed `input` matched against the options of a prompt. An option's
/// keyword is its word holding capitals (`live Section` → `Section`); the
/// capitals alone are its abbreviation (`sEttings` → `E`), and any start of
/// the keyword that reaches its last capital is taken too (`SE`, `SETTINGS`).
/// A leading `_` is the international form scripts use.
fn keyword(input: &str, options: &[&'static str]) -> Option<&'static str> {
    let input = input.trim().trim_start_matches('_').to_ascii_uppercase();
    if input.is_empty() {
        return None;
    }
    options.iter().copied().find(|option| {
        if *option == "?" {
            return input == "?";
        }
        let word = option
            .split(' ')
            .find(|word| word.chars().any(|c| c.is_ascii_uppercase()))
            .unwrap_or(option);
        let capitals: String = word.chars().filter(char::is_ascii_uppercase).collect();
        let reach = word.rfind(|c: char| c.is_ascii_uppercase()).map_or(1, |at| at + 1);
        input == capitals || (word.to_ascii_uppercase().starts_with(&input) && input.len() >= reach)
    })
}

/// The names in `names` that one of the comma-separated wildcard patterns matches.
pub fn matching<'a>(names: impl IntoIterator<Item = &'a str>, patterns: &str) -> Vec<&'a str> {
    let patterns: Vec<&str> = patterns.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    names
        .into_iter()
        .filter(|name| patterns.iter().any(|pattern| crate::io::xref_model::wildcard_match(name, pattern)))
        .collect()
}

/// The reference's listing of saved views: `"NAME"` padded to 35 columns
/// followed by the space letter.
pub fn view_listing(views: &[(String, bool)]) -> String {
    let mut out = String::from("Saved views:");
    if views.is_empty() {
        out.push_str("\n  No matching views found.");
        return out;
    }
    out.push_str("\nView name                        Space");
    for (name, paper) in views {
        out.push_str(&format!("\n{:<35}{}", format!("\"{name}\""), if *paper { "P" } else { "M" }));
    }
    out
}

impl DashViewCommand {
    pub fn new(names: ViewNames) -> Self {
        Self { names, step: Step::Main }
    }

    /// The command back at its settings prompt, after a setting was applied.
    pub fn settings(names: ViewNames) -> Self {
        Self { names, step: Step::Settings }
    }

    fn view(&self, name: &str) -> Option<&ViewInfo> {
        self.names.views.iter().find(|view| view.name.eq_ignore_ascii_case(name))
    }

    fn end(message: &str) -> CmdResult {
        CmdResult::Dispatch(format!("-VIEW >END {message}"))
    }

    fn list_views(&self, pattern: &str) -> String {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern };
        let names: Vec<&str> = self.names.views.iter().map(|view| view.name.as_str()).collect();
        let found: Vec<(String, bool)> =
            matching(names, pattern).into_iter().map(|name| (name.to_string(), false)).collect();
        view_listing(&found)
    }

    fn value_prompt(&self, setting: Setting, view: &str) -> String {
        let info = self.view(view).cloned().unwrap_or_default();
        match setting {
            Setting::Background => "Specify background type [Color/Gradient/Image/None]<None>:".into(),
            Setting::Categorize => {
                format!("Enter category name or * for none, or [?]: <\"{}\">", info.category)
            }
            Setting::LayerSnapshot => "Enter an option [Save/Delete] <Cancel>:".into(),
            Setting::LiveSection => format!(
                "Enter live section name or * for none, or [?]: <{}>:",
                info.live_section.as_deref().unwrap_or("*")
            ),
            Setting::VisualStyle => format!(
                "Enter visual style name or * for none, or [?]: <{}>:",
                info.visual_style.as_deref().unwrap_or("2D Wireframe")
            ),
        }
    }

    /// Hand a setting to the inline handler, which applies it and starts the
    /// settings prompt again.
    fn apply(setting: Setting, view: &str, value: &str) -> CmdResult {
        CmdResult::Dispatch(format!("-VIEW >SET {}\u{1f}{view}\u{1f}{value}", setting.verb()))
    }

    fn on_setting_value(&mut self, setting: Setting, view: String, text: &str) -> CmdResult {
        let t = text.trim();
        match setting {
            Setting::Background => match keyword(t, &["Color", "Gradient", "Image", "None"]) {
                // The other background types are chosen in their own dialogs.
                Some("None") => Self::apply(setting, &view, "NONE"),
                Some(_) => CmdResult::Dispatch("-VIEW >SETTINGS".into()),
                None => {
                    self.step = Step::SettingValue(setting, view);
                    CmdResult::ReportError("Invalid option keyword.".into())
                }
            },
            Setting::LayerSnapshot => match keyword(t, &["Save", "Delete"]) {
                Some("Save") => Self::apply(setting, &view, "SAVE"),
                Some(_) => Self::apply(setting, &view, "DELETE"),
                None => {
                    self.step = Step::SettingValue(setting, view);
                    CmdResult::ReportError("Invalid option keyword.".into())
                }
            },
            Setting::Categorize => {
                if t == "?" {
                    self.step = Step::SettingValueList(setting, view);
                    return CmdResult::NeedPoint;
                }
                let value = if t == "*" { "" } else { t };
                Self::apply(setting, &view, value)
            }
            Setting::LiveSection | Setting::VisualStyle => {
                if t == "?" {
                    self.step = Step::SettingValueList(setting, view);
                    return CmdResult::NeedPoint;
                }
                if t == "*" {
                    return Self::apply(setting, &view, "");
                }
                let known = if setting == Setting::LiveSection {
                    &self.names.live_sections
                } else {
                    &self.names.visual_styles
                };
                match known.iter().find(|name| name.eq_ignore_ascii_case(t)) {
                    Some(name) => {
                        let name = name.clone();
                        Self::apply(setting, &view, &name)
                    }
                    None => {
                        self.step = Step::SettingValue(setting, view);
                        CmdResult::ReportError(
                            if setting == Setting::LiveSection {
                                "Invalid live section name."
                            } else {
                                "Invalid visual style name."
                            }
                            .into(),
                        )
                    }
                }
            }
        }
    }

    fn value_listing(&self, setting: Setting, pattern: &str) -> String {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern };
        match setting {
            Setting::Categorize => {
                let mut categories: Vec<&str> = self
                    .names
                    .views
                    .iter()
                    .map(|view| view.category.as_str())
                    .filter(|category| !category.is_empty())
                    .collect();
                categories.sort_unstable_by_key(|name| name.to_ascii_uppercase());
                categories.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
                let found = matching(categories, pattern);
                let mut out = String::from("Saved categories:");
                if !found.is_empty() {
                    out.push_str("\nCategory name");
                    for name in found {
                        out.push('\n');
                        out.push_str(name);
                    }
                }
                out
            }
            Setting::VisualStyle | Setting::LiveSection => {
                let known = if setting == Setting::VisualStyle {
                    &self.names.visual_styles
                } else {
                    &self.names.live_sections
                };
                let mut found = matching(known.iter().map(String::as_str), pattern);
                found.sort_unstable_by_key(|name| name.to_ascii_uppercase());
                if found.is_empty() {
                    if setting == Setting::VisualStyle {
                        "  No matching visual styles found.".into()
                    } else {
                        "  No matching live sections found.".into()
                    }
                } else {
                    found.join("\n")
                }
            }
            _ => String::new(),
        }
    }
}

impl CadCommand for DashViewCommand {
    fn name(&self) -> &'static str {
        "-VIEW"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Main => "Enter an option [?/Delete/Orthographic/Restore/Save/sEttings/Window]:".into(),
            Step::Orthographic => "Enter an option [Top/Bottom/Front/BAck/Left/Right]<Top>:".into(),
            Step::List | Step::SettingViewList(_) => "Enter view name(s) to list <*>:".into(),
            Step::Delete => "Enter view name(s) to delete:".into(),
            Step::Restore => "Enter view name to restore:".into(),
            Step::Save | Step::WindowName => "Enter view name to save:".into(),
            Step::WindowFirst(_) => "Specify first corner:".into(),
            Step::WindowSecond(..) => "Specify opposite corner:".into(),
            Step::Settings => {
                "Enter an option [Background/Categorize/Layer snapshot/live Section/Ucs/Visual style]:".into()
            }
            Step::SettingView(setting) => setting.view_prompt().into(),
            Step::SettingValue(setting, view) => self.value_prompt(*setting, view),
            Step::SettingValueList(Setting::Categorize, _) => "Enter category name(s) to list <*>:".into(),
            Step::SettingValueList(Setting::LiveSection, _) => "Enter live section name(s) to list <*>:".into(),
            Step::SettingValueList(_, _) => "Enter visual style name(s) to list <*>:".into(),
            Step::Ucs => format!(
                "Save current UCS with named views?[Yes/No]<{}>:",
                if ucs_view() { "Yes" } else { "No" }
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        // The window corners take typed coordinates as points.
        if matches!(self.step, Step::WindowFirst(_) | Step::WindowSecond(..))
            && crate::app::helpers::parse_coord(t).is_some()
        {
            return None;
        }
        if t.is_empty() {
            return Some(self.on_enter());
        }
        let step = std::mem::replace(&mut self.step, Step::Main);
        Some(match step {
            Step::Main => {
                let word = t.trim_start_matches('_').to_ascii_uppercase();
                if let Some(preset) = PRESETS.iter().find(|preset| **preset == word) {
                    return Some(CmdResult::Dispatch(format!("-VIEW >PRESET {preset}")));
                }
                match keyword(t, &["?", "Delete", "Orthographic", "Restore", "Save", "sEttings", "Window"]) {
                    Some("?") => self.step = Step::List,
                    Some("Delete") => self.step = Step::Delete,
                    Some("Orthographic") => self.step = Step::Orthographic,
                    Some("Restore") => self.step = Step::Restore,
                    Some("Save") => self.step = Step::Save,
                    Some("sEttings") => self.step = Step::Settings,
                    Some(_) => self.step = Step::WindowName,
                    None => return Some(CmdResult::ReportError("Invalid option keyword.".into())),
                }
                CmdResult::NeedPoint
            }
            Step::Orthographic => {
                match keyword(t, &["Top", "Bottom", "Front", "BAck", "Left", "Right"]) {
                    Some(face) => CmdResult::Dispatch(format!("-VIEW >PRESET {}", face.to_ascii_uppercase())),
                    None => {
                        self.step = Step::Orthographic;
                        CmdResult::ReportError("Invalid option keyword.".into())
                    }
                }
            }
            Step::List => CmdResult::Measurement(self.list_views(t)),
            Step::Delete => {
                let names: Vec<&str> = self.names.views.iter().map(|view| view.name.as_str()).collect();
                if matching(names, t).is_empty() {
                    Self::end("No matching view names found.")
                } else {
                    CmdResult::Dispatch(format!("-VIEW >DELETE {t}"))
                }
            }
            Step::Restore => match self.view(t) {
                Some(view) => CmdResult::Dispatch(format!("-VIEW >RESTORE {}", view.name)),
                None => Self::end(&format!("Cannot find view \"{t}\".")),
            },
            Step::Save => CmdResult::Dispatch(format!("-VIEW >SAVE {t}")),
            Step::WindowName => {
                self.step = Step::WindowFirst(t.to_string());
                CmdResult::NeedPoint
            }
            Step::WindowFirst(_) | Step::WindowSecond(..) => Self::end("Invalid window specification."),
            Step::Settings => {
                match keyword(
                    t,
                    &["Background", "Categorize", "Layer snapshot", "live Section", "Ucs", "Visual style"],
                ) {
                    Some("Background") => self.step = Step::SettingView(Setting::Background),
                    Some("Categorize") => self.step = Step::SettingView(Setting::Categorize),
                    Some("Layer snapshot") => self.step = Step::SettingView(Setting::LayerSnapshot),
                    Some("live Section") => self.step = Step::SettingView(Setting::LiveSection),
                    Some("Ucs") => self.step = Step::Ucs,
                    Some(_) => self.step = Step::SettingView(Setting::VisualStyle),
                    None => {
                        self.step = Step::Settings;
                        return Some(CmdResult::ReportError("Invalid option keyword.".into()));
                    }
                }
                CmdResult::NeedPoint
            }
            Step::SettingView(setting) => {
                if t == "?" {
                    self.step = Step::SettingViewList(setting);
                    return Some(CmdResult::NeedPoint);
                }
                match self.view(t).map(|view| view.name.clone()) {
                    Some(view) => {
                        self.step = Step::SettingValue(setting, view);
                        CmdResult::NeedPoint
                    }
                    None => {
                        self.step = Step::SettingView(setting);
                        CmdResult::ReportError("No matching view names found.".into())
                    }
                }
            }
            Step::SettingViewList(setting) => {
                self.step = Step::SettingView(setting);
                CmdResult::ReportMeasurement(self.list_views(t))
            }
            Step::SettingValue(setting, view) => self.on_setting_value(setting, view, t),
            Step::SettingValueList(setting, view) => {
                let listing = self.value_listing(setting, t);
                self.step = Step::SettingValue(setting, view);
                CmdResult::ReportMeasurement(listing)
            }
            Step::Ucs => match keyword(t, &["Yes", "No"]) {
                Some(answer) => CmdResult::Dispatch(format!("-VIEW >UCSVIEW {}", u8::from(answer == "Yes"))),
                None => {
                    self.step = Step::Ucs;
                    CmdResult::ReportError("Invalid option keyword.".into())
                }
            },
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match std::mem::replace(&mut self.step, Step::Main) {
            Step::WindowFirst(name) => {
                self.step = Step::WindowSecond(name, pt);
                CmdResult::NeedPoint
            }
            Step::WindowSecond(name, first) => CmdResult::Dispatch(format!(
                "-VIEW >WINDOW {}\u{1f}{},{}\u{1f}{},{}",
                name, first.x, first.y, pt.x, pt.y
            )),
            step => {
                self.step = step;
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.step, Step::Main) {
            Step::Main | Step::Delete | Step::Restore | Step::Save | Step::WindowName => {
                CmdResult::Dispatch("-VIEW >END".into())
            }
            Step::Orthographic => CmdResult::Dispatch("-VIEW >PRESET TOP".into()),
            Step::List => CmdResult::Measurement(self.list_views("*")),
            Step::WindowFirst(_) | Step::WindowSecond(..) => Self::end("Invalid window specification."),
            // Enter at the settings prompt goes back to the main prompt.
            Step::Settings => CmdResult::NeedPoint,
            Step::SettingView(_) | Step::Ucs => {
                self.step = Step::Settings;
                CmdResult::NeedPoint
            }
            Step::SettingViewList(setting) => {
                self.step = Step::SettingView(setting);
                CmdResult::ReportMeasurement(self.list_views("*"))
            }
            // Enter keeps the value shown as the default.
            Step::SettingValue(..) => {
                self.step = Step::Settings;
                CmdResult::NeedPoint
            }
            Step::SettingValueList(setting, view) => {
                let listing = self.value_listing(setting, "*");
                self.step = Step::SettingValue(setting, view);
                CmdResult::ReportMeasurement(listing)
            }
        }
    }
}
