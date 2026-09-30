//! Editable keyboard shortcut table shared by the CUI dialog and key events.

use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use iced::Task;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(target_os = "macos")]
const ACCEL: &str = "CMD";
#[cfg(not(target_os = "macos"))]
const ACCEL: &str = "CTRL";

/// A built-in shortcut layout. Custom edits remain available through the
/// shortcut table; selecting a preset loads that layout as the working table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutPreset {
    #[default]
    OpenCadStudio,
    ArchicadWindows,
    ArchicadMacos,
    Custom,
}

impl ShortcutPreset {
    pub(crate) const PRESETS: [Self; 3] = [
        Self::OpenCadStudio,
        Self::ArchicadWindows,
        Self::ArchicadMacos,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::OpenCadStudio => "Open CAD Studio",
            Self::ArchicadWindows => "ArchiCAD (Windows)",
            Self::ArchicadMacos => "ArchiCAD (macOS)",
            Self::Custom => "Custom",
        }
    }

    pub(crate) fn bindings(self) -> BTreeMap<String, String> {
        match self {
            Self::OpenCadStudio | Self::Custom => default_bindings(),
            Self::ArchicadWindows => archicad_preset_bindings("CTRL", "ALT"),
            Self::ArchicadMacos => archicad_preset_bindings("CMD", "ALT"),
        }
    }
}

impl std::fmt::Display for ShortcutPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// ArchiCAD's published shortcuts mapped to commands that Open CAD Studio
/// supports. Duplicate combinations in the reference PDF (for example the
/// apostrophe shortcuts) are assigned once to the view commands.
fn archicad_bindings(accel: &str, alt: &str) -> BTreeMap<String, String> {
    let entries = [
        (format!("{accel}+N"), "NEW"),
        (format!("{accel}+{alt}+N"), "NEW"),
        (format!("{accel}+O"), "OPEN"),
        (format!("{accel}+L"), "LAYERS"),
        (format!("{accel}+W"), "CLOSE"),
        (format!("{accel}+S"), "SAVE"),
        (format!("{accel}+SHIFT+S"), "SAVEAS"),
        (format!("{accel}+P"), "PLOT"),
        (format!("{accel}+SHIFT+P"), "PAGESETUP"),
        (format!("{accel}+Q"), "QUIT"),
        (format!("{accel}+Z"), "UNDO"),
        (format!("{accel}+SHIFT+Z"), "REDO"),
        (format!("{accel}+A"), "SELECTALL"),
        (format!("{accel}+SHIFT+A"), "QSELECT"),
        (format!("{accel}+C"), "COPYCLIP"),
        (format!("{accel}+D"), "MOVE"),
        (format!("{accel}+E"), "ROTATE"),
        (format!("{accel}+SHIFT+E"), "ROTATECOPY"),
        (format!("{accel}+H"), "STRETCH"),
        (format!("{accel}+M"), "MIRROR"),
        (format!("{accel}+SHIFT+M"), "MIRRORCOPY"),
        (format!("{accel}+K"), "SCALE"),
        (format!("{accel}+-"), "ADJUST"),
        (format!("{accel}+0"), "TRIM"),
        (format!("{accel}+T"), "DSETTINGS"),
        (format!("{accel}+SHIFT+D"), "COPY"),
        (format!("{accel}+SHIFT+G"), "UNGROUP"),
        (format!("{alt}+SHIFT+G"), "GROUP"),
        (format!("{accel}+="), "EXPLODE"),
        (format!("{accel}+/"), "ZOOM IN"),
        (format!("{accel}+SHIFT+/"), "ZOOM OUT"),
        (format!("{accel}+,"), "PAN"),
        (format!("{accel}+'"), "ZOOM EXTENTS"),
        (format!("{accel}+SHIFT+'"), "ZOOM OBJECT"),
        (format!("{accel}+\\"), "CLEANSCREEN"),
        (format!("{accel}+SHIFT+\\"), "ZOOM ALL"),
        (format!("{accel}+["), "ZOOM PREVIOUS"),
        (format!("{accel}+2"), "PLAN"),
        (format!("{accel}+3"), "3DORBIT"),
        ("SHIFT+F8".to_string(), "DSETTINGS"),
    ];
    entries
        .into_iter()
        .map(|(key, command)| (key, command.to_string()))
        .collect()
}

/// Start from the OpenCAD Studio bindings and override only the combinations
/// that the ArchiCAD profile defines. This keeps useful defaults such as F8
/// (orthogonal mode) and the other OpenCAD Studio function-key shortcuts.
fn archicad_preset_bindings(accel: &str, alt: &str) -> BTreeMap<String, String> {
    let mut bindings = default_bindings();
    bindings.extend(archicad_bindings(accel, alt));
    bindings
}

/// Recognize the ArchiCAD-only shortcut maps written by earlier builds, so
/// upgrading does not turn an existing ArchiCAD profile into Custom or leave
/// its OpenCAD Studio defaults (such as F8) missing.
pub(crate) fn is_legacy_archicad_bindings(
    preset: ShortcutPreset,
    bindings: &FxHashMap<String, String>,
) -> bool {
    let (accel, alt) = match preset {
        ShortcutPreset::ArchicadWindows => ("CTRL", "ALT"),
        ShortcutPreset::ArchicadMacos => ("CMD", "ALT"),
        _ => return false,
    };
    let all = archicad_bindings(accel, alt);
    let optional = [
        format!("{accel}+L"),
        format!("{accel}+SHIFT+E"),
        format!("{accel}+SHIFT+M"),
    ];
    (0..(1 << optional.len())).any(|mask| {
        let mut legacy = all.clone();
        for (bit, key) in optional.iter().enumerate() {
            if mask & (1 << bit) == 0 {
                legacy.remove(key);
            }
        }
        legacy.into_iter().collect::<FxHashMap<_, _>>() == *bindings
    })
}

/// Every active binding shipped with a fresh configuration. Values are command
/// names where possible; the small set of input actions is resolved below.
pub(super) fn default_bindings() -> BTreeMap<String, String> {
    [
        ("F1".to_string(), "HELP"),
        ("F2".to_string(), "COMMANDHISTORY"),
        ("F3".to_string(), "TOGGLEOSNAP"),
        ("F4".to_string(), "TOGGLE3DOSNAP"),
        ("F5".to_string(), "ISOPLANE"),
        ("F7".to_string(), "GRID"),
        ("F8".to_string(), "ORTHO"),
        ("F9".to_string(), "SNAP"),
        ("F10".to_string(), "POLAR"),
        ("F11".to_string(), "OTRACK"),
        ("F12".to_string(), "DYNINPUT"),
        (format!("{ACCEL}+0"), "CLEANSCREEN"),
        (format!("{ACCEL}+1"), "PROPERTIES"),
        (format!("{ACCEL}+N"), "NEW"),
        (format!("{ACCEL}+O"), "OPEN"),
        (format!("{ACCEL}+L"), "LAYERS"),
        (format!("{ACCEL}+P"), "PLOT"),
        (format!("{ACCEL}+Q"), "QUIT"),
        (format!("{ACCEL}+S"), "SAVE"),
        (format!("{ACCEL}+SHIFT+S"), "SAVEAS"),
        (format!("{ACCEL}+Z"), "UNDO"),
        (format!("{ACCEL}+SHIFT+Z"), "REDO"),
        (format!("{ACCEL}+Y"), "REDO"),
        (format!("{ACCEL}+F"), "FIND"),
        (format!("{ACCEL}+H"), "FIND"),
        (format!("{ACCEL}+A"), "SELECTALL"),
        (format!("{ACCEL}+C"), "COPYCLIP"),
        (format!("{ACCEL}+SHIFT+C"), "COPYBASE"),
        (format!("{ACCEL}+X"), "CUTCLIP"),
        (format!("{ACCEL}+V"), "PASTECLIP"),
        (format!("{ACCEL}+SHIFT+V"), "PASTEBLOCK"),
        ("ENTER".to_string(), "FINALIZE"),
        ("SPACE".to_string(), "COMMANDSPACE"),
        ("ESCAPE".to_string(), "CANCEL"),
        ("DELETE".to_string(), "DELETESELECTED"),
        ("BACKSPACE".to_string(), "BACKSPACE"),
        ("TAB".to_string(), "DYNTAB"),
        ("UP".to_string(), "HISTORYPREV"),
        ("DOWN".to_string(), "HISTORYNEXT"),
        ("LEFT".to_string(), "CARETLEFT"),
        ("RIGHT".to_string(), "CARETRIGHT"),
    ]
    .into_iter()
    .map(|(key, action)| (key, action.to_string()))
    .collect()
}

/// Canonical form used by the editor, command interface, config and key event
/// resolver. Modifier order is stable so equivalent user input shares one row.
pub(super) fn normalize_key(value: &str) -> String {
    let mut ctrl = false;
    let mut cmd = false;
    let mut alt = false;
    let mut shift = false;
    let mut key = String::new();

    for part in value.split('+').map(str::trim).filter(|part| !part.is_empty()) {
        match part.to_uppercase().as_str() {
            "CTRL" | "CONTROL" => ctrl = true,
            "CMD" | "COMMAND" | "META" | "SUPER" => cmd = true,
            "ALT" | "OPTION" => alt = true,
            "SHIFT" => shift = true,
            "ESC" => key = "ESCAPE".to_string(),
            "RETURN" => key = "ENTER".to_string(),
            "ARROWUP" => key = "UP".to_string(),
            "ARROWDOWN" => key = "DOWN".to_string(),
            "ARROWLEFT" => key = "LEFT".to_string(),
            "ARROWRIGHT" => key = "RIGHT".to_string(),
            other => key = other.to_string(),
        }
    }

    // Iced reports the produced character for shifted punctuation (e.g. `?`
    // for Shift+/). Store the base key plus Shift so preset bindings use the
    // same spelling as the ArchiCAD shortcut reference.
    let key = match key.as_str() {
        "?" => {
            shift = true;
            "/"
        }
        "+" => {
            shift = true;
            "="
        }
        "\"" => {
            shift = true;
            "'"
        }
        "|" => {
            shift = true;
            "\\"
        }
        "{" => {
            shift = true;
            "["
        }
        "}" => {
            shift = true;
            "]"
        }
        ":" => {
            shift = true;
            ";"
        }
        "<" => {
            shift = true;
            ","
        }
        ">" => {
            shift = true;
            "."
        }
        "_" => {
            shift = true;
            "-"
        }
        other => other,
    }
    .to_string();

    let mut parts = Vec::with_capacity(5);
    if ctrl {
        parts.push("CTRL");
    }
    if cmd {
        parts.push("CMD");
    }
    if alt {
        parts.push("ALT");
    }
    if shift {
        parts.push("SHIFT");
    }
    if !key.is_empty() {
        parts.push(&key);
    }
    parts.join("+")
}

/// Keys that used to bypass focused widgets. Clipboard and selection commands
/// intentionally stay out so text fields retain their native shortcuts.
pub(super) fn is_global_key(key: &str) -> bool {
    let base = key.rsplit('+').next().unwrap_or(key);
    base.strip_prefix('F').is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
    })
        || matches!(base, "ESCAPE")
        || ((key.starts_with(ACCEL) || key.starts_with("ALT+"))
            && !matches!(
                key.rsplit('+').next(),
                Some("A" | "C" | "V" | "X")
            ))
}

fn is_named_key(key: &str) -> bool {
    matches!(
        key,
        "ENTER"
            | "SPACE"
            | "ESCAPE"
            | "DELETE"
            | "BACKSPACE"
            | "TAB"
            | "UP"
            | "DOWN"
            | "LEFT"
            | "RIGHT"
            | "HOME"
            | "END"
            | "PAGEUP"
            | "PAGEDOWN"
            | "INSERT"
    ) || key.strip_prefix('F').is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
    })
}

/// Input actions `run_shortcut` resolves directly instead of routing through
/// the command dispatcher — valid shortcut commands that never appear in the
/// command registry. Used to validate the shortcut editor's command column.
pub(super) const INPUT_ACTIONS: &[&str] = &[
    "SPACEMOUSEFIT",
    "SPACEMOUSETOP",
    "FINALIZE",
    "COMMANDSPACE",
    "CANCEL",
    "DELETESELECTED",
    "BACKSPACE",
    "DYNTAB",
    "HISTORYPREV",
    "HISTORYNEXT",
    "CARETLEFT",
    "CARETRIGHT",
    "COMMANDHISTORY",
    "TOGGLEOSNAP",
    "TOGGLE3DOSNAP",
    "OTRACK",
    "DYNINPUT",
    "SELECTALL",
    "PASTECLIP",
    "ROTATECOPY",
    "MIRRORCOPY",
];

impl OpenCADStudio {
    /// Commit the working rows to the live bindings, discarding an
    /// unfinished draft and reporting duplicates. Shared by Apply and
    /// Apply-and-Exit.
    pub(super) fn finish_shortcut_editor(&mut self) {
        // An add must end in a complete shortcut or a cancellation;
        // applying with a half-filled draft discards the draft.
        if self.shortcut_pending_add {
            self.shortcut_editor_rows.remove(0);
            self.shortcut_pending_add = false;
            self.command_line
                .push_error(crate::tf!("Incomplete shortcut row discarded.").as_ref());
        }
        self.shortcut_capture_row = None;
        // Rows normalizing to the same key silently overwrite each other in
        // the binding map — surface the conflict instead.
        let mut seen = rustc_hash::FxHashSet::default();
        let mut duplicates = Vec::new();
        for (key, _) in &self.shortcut_editor_rows {
            let key = normalize_key(key);
            if key.is_empty() {
                continue;
            }
            if !seen.insert(key.clone()) {
                duplicates.push(key);
            }
        }
        if !duplicates.is_empty() {
            self.command_line.push_error(
                crate::tf!(
                    "Duplicate shortcut(s) ignored on Apply: {}",
                    duplicates.join(", ")
                )
                .as_ref(),
            );
        }
        self.apply_shortcut_editor_rows();
        self.command_line.push_info(
            crate::tf!("{} shortcut(s) applied.", self.shortcut_bindings.len()).as_ref(),
        );
    }

    /// Reset the working rows and live bindings to the shipped defaults.
    pub(super) fn reset_shortcuts_to_defaults(&mut self) {
        let preset = match self.shortcut_preset {
            ShortcutPreset::Custom => ShortcutPreset::OpenCadStudio,
            preset => preset,
        };
        self.select_shortcut_preset(preset);
    }

    /// True when the working rows differ from the live bindings — the editor
    /// has un-applied changes a close would discard.
    pub(super) fn shortcut_editor_dirty(&self) -> bool {
        let rows: FxHashMap<String, String> = self
            .shortcut_editor_rows
            .iter()
            .filter_map(|(key, action)| {
                let key = normalize_key(key);
                let action = action.trim().to_uppercase();
                (!key.is_empty() && !action.is_empty()).then_some((key, action))
            })
            .collect();
        rows != self.shortcut_bindings
    }

    /// A pending add finishes successfully once its draft row (row 0) has
    /// both a key and a command; it then becomes a regular row of the table.
    /// Called after each edit while a draft is pending.
    pub(super) fn finish_pending_add(&mut self) {
        if !self.shortcut_pending_add {
            return;
        }
        if let Some((key, command)) = self.shortcut_editor_rows.first() {
            if !key.trim().is_empty() && !command.trim().is_empty() {
                self.shortcut_pending_add = false;
                self.shortcut_capture_row = None;
            }
        }
    }

    pub(super) fn apply_shortcut_editor_rows(&mut self) {
        let bindings: FxHashMap<String, String> = self
            .shortcut_editor_rows
            .iter()
            .filter_map(|(key, action)| {
                let key = normalize_key(key);
                let action = action.trim().to_uppercase();
                (!key.is_empty() && !action.is_empty()).then_some((key, action))
            })
            .collect();
        let selected_bindings: FxHashMap<String, String> = self
            .shortcut_preset
            .bindings()
            .into_iter()
            .collect();
        if bindings != selected_bindings {
            self.shortcut_preset = ShortcutPreset::Custom;
        }
        self.shortcut_bindings = bindings;
        self.persist_settings_if_changed();
    }

    pub(super) fn select_shortcut_preset(&mut self, preset: ShortcutPreset) {
        self.shortcut_preset = preset;
        let mut rows: Vec<(String, String)> = preset.bindings().into_iter().collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        self.shortcut_editor_rows = rows;
        self.shortcut_capture_row = None;
        self.shortcut_pending_add = false;
        self.shortcut_reset_confirm = false;
        self.shortcut_close_confirm = false;
        self.shortcut_bindings = preset.bindings().into_iter().collect();
        self.persist_settings_if_changed();
    }

    pub(super) fn run_shortcut(&mut self, key: &str) -> Task<Message> {
        let key = normalize_key(key);
        let action = self.shortcut_bindings.get(&key).or_else(|| {
            let base = key.rsplit('+').next()?;
            is_named_key(base).then(|| self.shortcut_bindings.get(base)).flatten()
        });
        let Some(action) = action.cloned() else {
            return Task::none();
        };
        self.run_action(&action)
    }

    /// Shared by keyboard bindings and exported device actions. In particular,
    /// Undo retains its command-local behavior during PLINE and SPLINE.
    pub(super) fn run_action(&mut self, action: &str) -> Task<Message> {
        let message = match action {
            "SPACEMOUSEFIT" => {
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all();
                self.arm_hover_after_navigation(i);
                return Task::none();
            }
            "SPACEMOUSETOP" => Message::ViewCubeHome,
            "SPACEMOUSE" => Message::SpaceMousePreferences,
            "SPACEMOUSEPAUSE" => Message::SpaceMousePause,
            "SPACEMOUSEPAN" => {
                Message::SpaceMouseMode(crate::input::spacemouse::NavigationMode::PanOnly)
            }
            "SPACEMOUSEPANZOOM" => {
                Message::SpaceMouseMode(crate::input::spacemouse::NavigationMode::PanZoom)
            }
            "SPACEMOUSEAUTO" => {
                Message::SpaceMouseMode(crate::input::spacemouse::NavigationMode::Auto)
            }
            "SPACEMOUSE3D" => {
                Message::SpaceMouseMode(crate::input::spacemouse::NavigationMode::Full3D)
            }
            "FINALIZE" => Message::CommandFinalize,
            "COMMANDSPACE" => Message::CommandSpace,
            "CANCEL" => Message::CommandEscape,
            "DELETESELECTED" => Message::DeleteSelected,
            "BACKSPACE" => Message::CommandBackspace,
            "DYNTAB" => Message::DynTabNext,
            "HISTORYPREV" => Message::CommandHistoryPrev,
            "HISTORYNEXT" => Message::CommandHistoryNext,
            "CARETLEFT" => Message::MTextCaretMove(-1),
            "CARETRIGHT" => Message::MTextCaretMove(1),
            "COMMANDHISTORY" => Message::CommandHistoryToggle,
            "TOGGLEOSNAP" => Message::ToggleSnapEnabled,
            "TOGGLE3DOSNAP" => Message::ToggleSnap3dEnabled,
            "OTRACK" => Message::ToggleOTrack,
            "DYNINPUT" => Message::ToggleDynInput,
            "SELECTALL" => Message::SelectAllShortcut,
            "PASTECLIP" => Message::PasteShortcut,
            "ROTATECOPY" | "MIRRORCOPY" => {
                let i = self.active_tab;
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect();
                if handles.is_empty() {
                    self.command_line.push_error("Select objects before using this shortcut.");
                    return Task::none();
                }
                if action == "ROTATECOPY" {
                    use crate::modules::draw::modify::rotate::RotateCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let mut cmd = RotateCommand::new_copy(handles, wires);
                    cmd.set_working_plane(self.tabs[i].ucs_xform().working_plane());
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::mirror::MirrorCommand;
                    let (wires, text_ghosts) = self.tabs[i].scene.mirror_preview_parts(&handles);
                    let mirror_text = self.tabs[i].scene.document.header.mirror_text;
                    let mut cmd = MirrorCommand::new_copy(handles, wires, text_ghosts, mirror_text);
                    cmd.set_working_plane(self.tabs[i].ucs_xform().working_plane());
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
                let options = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|cmd| cmd.options())
                    .unwrap_or_default();
                self.command_line.set_step_options(options);
                self.sync_dyn_fields();
                self.refresh_active_cmd_preview(i);
                return Task::none();
            }

            "UNDO" => {
                let i = self.active_tab;

                // Multi-step drawing commands may consume Ctrl+Z themselves.
                // For example PLINE/SPLINE should remove only the last entered
                // point instead of cancelling the whole in-progress command.
                let command_result = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|cmd| cmd.on_undo_step());

                if let Some(result) = command_result {
                    return self.apply_cmd_result(result);
                }

                Message::Undo
            }

            "REDO" => Message::Redo,

            other => Message::Command(other.to_string()),
            };

            self.update(message)
    }
}
