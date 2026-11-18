//! Editable keyboard shortcut table shared by the CUI dialog and key events.

use super::{Message, OpenCADStudio};
use iced::Task;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// True when the live `SPACE` binding resolves to something other than
/// `COMMANDSPACE`. The view's key-dispatch closure must stay a non-capturing fn
/// pointer, so instead of reading the binding map it reads this flag when a
/// text field reports a Captured space. When SPACE is default-bound to
/// COMMANDSPACE the flag is false and the text field keeps the space for
/// typing; when SPACE is rebound (e.g. to FINALIZE) the flag is true so the
/// Captured space still fires its shortcut. (#1710)
pub(crate) static SPACE_REBOUND_OFF_COMMANDSPACE: AtomicBool = AtomicBool::new(false);

/// Recompute the SPACE-dispatch flag from the current binding map. Call after
/// every change to `shortcut_bindings`. (#1710)
pub(super) fn sync_space_dispatch_flag(bindings: &FxHashMap<String, String>) {
    SPACE_REBOUND_OFF_COMMANDSPACE
        .store(space_rebound_off_commandspace(bindings), Ordering::Relaxed);
}

/// Whether `SPACE` is bound to something other than `COMMANDSPACE`. (#1710)
pub(crate) fn space_rebound_off_commandspace(bindings: &FxHashMap<String, String>) -> bool {
    matches!(bindings.get("SPACE"), Some(cmd) if cmd != "COMMANDSPACE")
}

#[cfg(target_os = "macos")]
const ACCEL: &str = "CMD";
#[cfg(not(target_os = "macos"))]
const ACCEL: &str = "CTRL";

/// Every active binding shipped with a fresh configuration. Values are command
/// names where possible; the small set of input actions is resolved below.
pub(super) fn default_bindings() -> BTreeMap<String, String> {
    [
        ("F1".to_string(), "HELP"),
        ("F2".to_string(), "COMMANDHISTORY"),
        ("F3".to_string(), "TOGGLEOSNAP"),
        ("F4".to_string(), "TOGGLE3DOSNAP"),
        ("F5".to_string(), "ISOPLANE"),
        ("F6".to_string(), "TOGGLEDUCS"),
        ("F7".to_string(), "GRID"),
        ("F8".to_string(), "ORTHO"),
        ("F9".to_string(), "SNAP"),
        ("F10".to_string(), "POLAR"),
        ("F11".to_string(), "OTRACK"),
        ("F12".to_string(), "DYNINPUT"),
        // F12 itself is Dynamic Input (as in the commercial solutions this
        // application mirrors), so the quad's master toggle rides the
        // nearest free seat beside it: Shift+F12.
        ("SHIFT+F12".to_string(), "QUADTOGGLE"),
        // The next and the previous open drawing.
        ("CTRL+TAB".to_string(), "NEXTDRAWING"),
        ("CTRL+SHIFT+TAB".to_string(), "PREVDRAWING"),
        (format!("{ACCEL}+0"), "CLEANSCREEN"),
        (format!("{ACCEL}+9"), "COMMANDLINETOGGLE"),
        (format!("{ACCEL}+1"), "PROPERTIES"),
        (format!("{ACCEL}+K"), "HYPERLINK"),
        (format!("{ACCEL}+N"), "NEW"),
        (format!("{ACCEL}+O"), "OPEN"),
        (format!("{ACCEL}+P"), "PLOT"),
        (format!("{ACCEL}+W"), "TOGGLESELECTIONCYCLING"),
        (format!("{ACCEL}+R"), "NEXTVIEWPORT"),
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
        // Group selection on and off (PICKSTYLE bit 1), as in AutoCAD.
        (format!("{ACCEL}+SHIFT+A"), "TOGGLEGROUPS"),
        ("ENTER".to_string(), "FINALIZE"),
        ("SPACE".to_string(), "COMMANDSPACE"),
        ("ESCAPE".to_string(), "CANCEL"),
        ("DELETE".to_string(), "DELETESELECTED"),
        ("BACKSPACE".to_string(), "BACKSPACE"),
        ("TAB".to_string(), "DYNTAB"),
        ("SHIFT+TAB".to_string(), "DYNTABPREV"),
        ("UP".to_string(), "HISTORYPREV"),
        ("DOWN".to_string(), "HISTORYNEXT"),
        ("LEFT".to_string(), "CARETLEFT"),
        ("RIGHT".to_string(), "CARETRIGHT"),
    ]
    .into_iter()
    .chain(
        PLATFORM_BINDINGS
            .iter()
            .map(|(key, action)| (key.to_string(), *action)),
    )
    .map(|(key, action)| (key, action.to_string()))
    .collect()
}

/// Bindings a platform expects of every application.
#[cfg(target_os = "macos")]
pub(super) const PLATFORM_BINDINGS: &[(&str, &str)] = &[("CMD+,", "OPTIONS")];
#[cfg(not(target_os = "macos"))]
pub(super) const PLATFORM_BINDINGS: &[(&str, &str)] = &[];

/// Split one leading CSV field off `text`: either a quoted field (with `""`
/// as an escaped quote) or a bare one running to the next comma. Returns the
/// field and the text after the separating comma (empty when none).
fn csv_field(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    if let Some(body) = text.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = body.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c != '"' {
                out.push(c);
            } else if matches!(chars.peek(), Some((_, '"'))) {
                out.push('"');
                chars.next();
            } else {
                let rest = body[i + 1..].trim_start();
                return match rest.strip_prefix(',') {
                    Some(rest) => Some((out, rest)),
                    None if rest.is_empty() => Some((out, rest)),
                    None => None,
                };
            }
        }
        None
    } else {
        Some(match text.split_once(',') {
            Some((field, rest)) => (field.trim().to_string(), rest),
            None => (text.trim().to_string(), ""),
        })
    }
}

/// Split one import line into its raw key and command text. Accepts the
/// tab-separated text and the comma-separated file written by the editor's
/// Export, and the "KEY COMMAND" line of CUIEXPORT.
fn split_import_line(line: &str) -> Option<(String, String)> {
    if line.starts_with('"') {
        let (key, rest) = csv_field(line)?;
        let (command, _) = csv_field(rest)?;
        return Some((key, command));
    }
    if let Some((key, command)) = line.split_once('\t') {
        return Some((key.trim().to_string(), command.trim().to_string()));
    }
    // First separator: whitespace (CUIEXPORT) or comma (csv). A comma that
    // directly follows `+` (e.g. `CTRL+,`) or starts the line (the bare `,`
    // key of a CUIEXPORT line) belongs to the key.
    let mut prev = None;
    for (i, c) in line.char_indices() {
        if c.is_whitespace() || (c == ',' && !matches!(prev, None | Some('+'))) {
            let (key, rest) = (&line[..i], &line[i..]);
            let rest = if c == ',' {
                csv_field(&rest[1..])?.0
            } else {
                rest.trim().to_string()
            };
            return Some((key.trim().to_string(), rest));
        }
        prev = Some(c);
    }
    None
}

/// Parse shortcut text: tab-separated or comma-separated rows as written by
/// the shortcut editor's Export, or "KEY COMMAND" lines as written by
/// CUIEXPORT. Blank lines, `#` comments and the Key/Command header are
/// ignored; lines without a usable key and command are skipped and counted.
/// Keys come back normalized and commands uppercased.
pub(super) fn parse_shortcut_import(text: &str) -> (Vec<(String, String)>, usize) {
    let mut rows = Vec::new();
    let mut skipped = 0usize;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match split_import_line(line) {
            Some((key, command)) => push_import_row(&mut rows, &mut skipped, &key, &command),
            None => skipped += 1,
        }
    }
    (rows, skipped)
}

/// The one rule every import source shares for a raw key/command pair: the
/// Key/Command header is dropped, a pair with a control character or without
/// a usable key and command is counted as skipped.
fn push_import_row(
    rows: &mut Vec<(String, String)>,
    skipped: &mut usize,
    key: &str,
    command: &str,
) {
    let is = |value: &str, word: &str, translated: &str| {
        value.trim().eq_ignore_ascii_case(word) || value.trim().eq_ignore_ascii_case(translated)
    };
    if is(key, "Key", &crate::t!("Key")) && is(command, "Command", &crate::t!("Command")) {
        return;
    }
    if key.chars().chain(command.chars()).any(char::is_control) {
        *skipped += 1;
        return;
    }
    let key = normalize_key(key);
    let command = command.trim().to_uppercase();
    if key.is_empty() || command.is_empty() {
        *skipped += 1;
    } else {
        rows.push((key, command));
    }
}

/// Decide what the bytes of a picked file are and read the shortcut rows from
/// them: a workbook (zip signature, whatever the file is called; first sheet,
/// first two columns) or UTF-8 text. Anything else, or text holding control
/// characters other than tab / CR / LF, is an error and yields no rows.
pub(super) fn read_shortcut_import(bytes: &[u8]) -> Result<(Vec<(String, String)>, usize), String> {
    let not_a_list = || crate::t!("Shortcuts: this file is not a shortcut list.").into_owned();
    if bytes.starts_with(b"PK\x03\x04") {
        let table = crate::app::annotation_data::workbook_first_sheet_rows(bytes)
            .map_err(|_| not_a_list())?;
        let mut rows = Vec::new();
        let mut skipped = 0usize;
        for row in table {
            let cell = |i: usize| row.get(i).map_or("", String::as_str);
            if cell(0).trim().is_empty() && cell(1).trim().is_empty() {
                continue;
            }
            push_import_row(&mut rows, &mut skipped, cell(0), cell(1));
        }
        return Ok((rows, skipped));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| not_a_list())?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Err(not_a_list());
    }
    Ok(parse_shortcut_import(text))
}

/// Merge imported rows into the editor rows: a key that is already present
/// gets the imported command, a new key is appended.
pub(super) fn merge_shortcut_rows(
    rows: &mut Vec<(String, String)>,
    imported: Vec<(String, String)>,
) {
    for (key, command) in imported {
        match rows.iter_mut().find(|row| normalize_key(&row.0) == key) {
            Some(row) => row.1 = command,
            None => rows.push((key, command)),
        }
    }
}

/// Canonical form used by the editor, command interface, config and key event
/// resolver. Modifier order is stable so equivalent user input shares one row.
pub(super) fn normalize_key(value: &str) -> String {
    let mut ctrl = false;
    let mut cmd = false;
    let mut alt = false;
    let mut shift = false;
    let mut key = String::new();

    for part in value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
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
    base.strip_prefix('F')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
        || matches!(base, "ESCAPE")
        || (key.starts_with(ACCEL)
            && !matches!(key.rsplit('+').next(), Some("A" | "C" | "V" | "X")))
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
    ) || key
        .strip_prefix('F')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
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
    "DYNTABPREV",
    "HISTORYPREV",
    "HISTORYNEXT",
    "CARETLEFT",
    "CARETRIGHT",
    "COMMANDHISTORY",
    "TOGGLEOSNAP",
    "TOGGLE3DOSNAP",
    "TOGGLEDUCS",
    "OTRACK",
    "DYNINPUT",
    "TOGGLESELECTIONCYCLING",
    "TOGGLEGROUPS",
    "NEXTVIEWPORT",
    "SELECTALL",
    "PASTECLIP",
    // The quad cursor menu's master toggle (docs/quad-menu.md §3.6).
    "QUADTOGGLE",
    "NEXTDRAWING",
    "PREVDRAWING",
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
        let mut rows: Vec<(String, String)> = default_bindings().into_iter().collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        self.shortcut_editor_rows = rows;
        self.shortcut_pending_add = false;
        self.shortcut_capture_row = None;
        self.shortcut_reset_confirm = false;
        self.apply_shortcut_editor_rows();
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

    /// Load imported shortcut text into the editor rows only; Apply persists.
    pub(super) fn import_shortcut_rows(&mut self, bytes: &[u8]) {
        let (imported, skipped) = match read_shortcut_import(bytes) {
            Ok(read) => read,
            Err(message) => {
                self.command_line.push_error(&message);
                return;
            }
        };
        let count = imported.len();
        merge_shortcut_rows(&mut self.shortcut_editor_rows, imported);
        self.shortcut_capture_row = None;
        self.command_line.push_output(
            crate::tf!(
                "Shortcuts: imported {count} shortcut(s), skipped {skipped} line(s). Apply to keep them."
            )
            .as_ref(),
        );
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
        self.shortcut_bindings = bindings;
        sync_space_dispatch_flag(&self.shortcut_bindings);
        self.persist_settings_if_changed();
    }

    /// Ctrl+R: make the next viewport of the layout current, from paper
    /// space the first one, keeping a running command going.
    fn next_viewport(&mut self) -> Task<Message> {
        let scene = &self.tabs[self.active_tab].scene;
        if scene.current_layout == "Model" {
            return Task::none();
        }
        let viewports: Vec<document::Handle> = scene
            .viewport_list()
            .into_iter()
            .map(|vp| vp.handle)
            .collect();
        let next = match scene
            .active_viewport
            .and_then(|h| viewports.iter().position(|v| *v == h))
        {
            Some(index) => viewports.get((index + 1) % viewports.len()),
            None => viewports.first(),
        };
        let Some(&next) = next else {
            return Task::none();
        };
        self.viewport_switch_keeps_command = true;
        Task::done(Message::EnterViewport(next))
    }

    pub(super) fn run_shortcut(&mut self, key: &str) -> Task<Message> {
        // Configurations saved before these keys were bound still switch
        // drawings with them, rather than falling back to plain Tab.
        let builtin = match key {
            "CTRL+TAB" => Some("NEXTDRAWING".to_string()),
            "CTRL+SHIFT+TAB" => Some("PREVDRAWING".to_string()),
            _ => None,
        };
        let builtin = builtin.filter(|_| !self.shortcut_bindings.contains_key(key));
        // Alt+letter opens that menu of the menu bar, unless it is bound.
        if let Some(letter) = key
            .strip_prefix("ALT+")
            .filter(|rest| rest.chars().count() == 1)
        {
            if !self.shortcut_bindings.contains_key(key) {
                if let Some(task) = letter
                    .chars()
                    .next()
                    .and_then(|ch| self.open_menu_bar_menu(ch))
                {
                    return task;
                }
            }
        }
        let action = builtin
            .as_ref()
            .or_else(|| self.shortcut_bindings.get(key))
            .or_else(|| {
                let base = key.rsplit('+').next()?;
                is_named_key(base)
                    .then(|| self.shortcut_bindings.get(base))
                    .flatten()
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
            "DYNTABPREV" => Message::DynTabPrev,
            "HISTORYPREV" => Message::CommandHistoryPrev,
            "HISTORYNEXT" => Message::CommandHistoryNext,
            "CARETLEFT" => Message::MTextCaretMove(-1),
            "CARETRIGHT" => Message::MTextCaretMove(1),
            "COMMANDHISTORY" => Message::CommandHistoryToggle,
            "TOGGLEOSNAP" => Message::ToggleSnapEnabled,
            "TOGGLE3DOSNAP" => Message::ToggleSnap3dEnabled,
            "TOGGLEDUCS" => Message::ToggleDynamicUcs,
            "OTRACK" => Message::ToggleOTrack,
            "DYNINPUT" => Message::ToggleDynInput,
            "TOGGLESELECTIONCYCLING" => Message::ToggleSelectionCycling,
            "TOGGLEGROUPS" => {
                let header = &mut self.tabs[self.active_tab].scene.document.header;
                header.pick_style ^= 1;
                let on = header.pick_style & 1 != 0;
                self.ribbon.group_selection = on;
                let state = if on { "on" } else { "off" };
                self.command_line.push_info(&format!("<Groups {state}>"));
                return Task::none();
            }
            // Quad cursor menu master toggle (Shift+F12, Options window,
            // status-bar field): the pure on/off flip, quad panel updates
            // included, lives in the quad router (docs/quad-menu.md §3.6).
            "QUADTOGGLE" => Message::Quad(crate::ui::popup::quad_menu::QuadMsg::Toggle),
            "NEXTVIEWPORT" => return self.next_viewport(),
            // Ctrl+Tab / Ctrl+Shift+Tab: the next / previous open drawing.
            "NEXTDRAWING" | "PREVDRAWING" => {
                let count = self.tabs.len();
                if count < 2 {
                    return Task::none();
                }
                let step = if action == "NEXTDRAWING" {
                    1
                } else {
                    count - 1
                };
                Message::TabSwitch((self.active_tab + step) % count)
            }
            "SELECTALL" => Message::SelectAllShortcut,
            "PASTECLIP" => Message::PasteShortcut,

            // The multiline text editor keeps its own history.
            "UNDO" if self.mtext_editor.is_some() => Message::MTextUndo,
            "REDO" if self.mtext_editor.is_some() => Message::MTextRedo,
            // In the text editor Ctrl+F inserts a field instead of finding.
            "FIND" if self.mtext_editor.is_some() => Message::MTextInsertField,
            // Ctrl+R opens the text editor's Find and Replace.
            "NEXTVIEWPORT" if self.mtext_editor.is_some() => Message::Command("_MTED FIND".into()),
            // Ctrl+X cuts the editor's selected text, not drawing objects.
            "CUTCLIP" if self.mtext_editor.is_some() => Message::Command("_MTED CUT".into()),

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

#[cfg(test)]
mod suggestion_tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::window::shortcuts::{suggestions, ShortcutField};

    /// Rows of the test table: more than the window shows at once.
    const ROWS: usize = 40;

    fn key(idx: usize) -> String {
        format!("ALT+F{}", idx + 1)
    }

    /// The editor open on a full table, `typed` typed into the Command box
    /// of row `idx`.
    fn editor_with_typed(idx: usize, typed: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.update(Message::ShortcutsPanelOpen);
        app.shortcut_editor_rows = (0..ROWS).map(|row| (key(row), "CIRCLE".to_string())).collect();
        if !typed.is_empty() {
            let _ = app.update(Message::ShortcutEditorInput {
                idx,
                field: ShortcutField::Command,
                value: typed.to_string(),
            });
        }
        app
    }

    fn offered(app: &OpenCADStudio, typed: &str) -> Vec<String> {
        suggestions(
            typed,
            &app.command_line.dynamic_commands,
            &app.command_aliases,
        )
    }

    /// Where `label` shows in the drawn window, if it shows.
    fn shown(app: &OpenCADStudio, label: &str) -> Option<iced::Rectangle> {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1280.0, 900.0),
            app.view_main(),
        );
        ui.find(label.to_string())
            .ok()
            .and_then(|found| found.visible_bounds())
    }

    #[test]
    fn the_list_is_the_command_lines_and_empty_for_a_finished_name() {
        let app = OpenCADStudio::new_for_test();
        let lin = offered(&app, "lin");
        assert_eq!(lin.first().map(String::as_str), Some("LINE"), "{lin:?}");
        assert!(lin.iter().all(|name| name.contains("LIN")), "{lin:?}");
        assert!(offered(&app, "").is_empty());
        assert!(offered(&app, "no such command").is_empty());
        // A name nothing else contains has nothing left to offer.
        let only = crate::ui::command_line::ranked_matches(
            "LINETYPE",
            &app.command_line.dynamic_commands,
            &app.command_aliases,
        );
        if only == ["LINETYPE"] {
            assert!(offered(&app, "linetype").is_empty());
        }
    }

    /// Typing in a Command box shows the list in the drawn window, every row
    /// of it whole, under the box, and no row of the table moves for it.
    #[test]
    fn typing_shows_the_list_and_moves_no_row() {
        let plain = editor_with_typed(0, "");
        let below_before = shown(&plain, &key(1)).expect("the second row shows");
        let apply_before = shown(&plain, "Apply").expect("Apply shows");

        let typed = editor_with_typed(0, "lin");
        let wanted = offered(&typed, "lin");
        assert!(wanted.len() > 1, "{wanted:?}");
        let box_row = shown(&typed, &key(0)).expect("the typed row shows");
        for name in &wanted {
            let row = shown(&typed, name).unwrap_or_else(|| panic!("{name} does not show"));
            assert!(row.height > 10.0, "{name} is cut off: {row:?}");
            assert!(
                row.y >= box_row.y + box_row.height - 1.0,
                "{name} {row:?} is not under {box_row:?}"
            );
        }
        assert_eq!(shown(&typed, &key(1)), Some(below_before), "the next row moved");
        assert_eq!(shown(&typed, "Apply"), Some(apply_before), "Apply moved");
    }

    /// The lowest row the window shows has no room under it: its list still
    /// shows whole, above the action bar.
    #[test]
    fn the_lowest_row_shows_its_whole_list() {
        let plain = editor_with_typed(0, "");
        let apply = shown(&plain, "Apply").expect("Apply shows");
        let lowest = (0..ROWS)
            .filter(|&row| shown(&plain, &key(row)).is_some_and(|at| at.height > 10.0))
            .last()
            .expect("a row shows");
        assert!(lowest > 5 && lowest < ROWS - 1, "the table scrolls: {lowest}");

        let typed = editor_with_typed(lowest, "lin");
        for name in &offered(&typed, "lin") {
            let row = shown(&typed, name).unwrap_or_else(|| panic!("{name} does not show"));
            assert!(
                row.height > 10.0 && row.y + row.height <= apply.y,
                "{name} {row:?} is cut off or over the action bar {apply:?}"
            );
        }
    }

    /// A click on a row of the list puts that command in the box and closes
    /// the list.
    #[test]
    fn a_click_on_a_row_fills_the_command() {
        let mut app = editor_with_typed(0, "lin");
        let last = offered(&app, "lin").last().expect("a suggestion").clone();
        let messages: Vec<_> = {
            let mut ui = iced_test::Simulator::with_size(
                iced::Settings::default(),
                iced::Size::new(1280.0, 900.0),
                app.view_main(),
            );
            ui.click(last.clone()).expect("the last row can be clicked");
            ui.into_messages().collect()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.shortcut_editor_rows[0].1, last);
        assert_eq!(app.shortcut_editor_rows[1].1, "CIRCLE", "only that row");
        assert_eq!(app.shortcut_suggest, None);
        assert_eq!(shown(&app, "LINE"), None, "the list is closed");
    }

    /// The list belongs to the text it was opened for: it is gone when the
    /// row moved, was removed, or the list was dismissed.
    #[test]
    fn the_list_closes_when_its_row_changes() {
        let mut app = editor_with_typed(1, "lin");
        assert!(shown(&app, "LINE").is_some());
        let _ = app.update(Message::ShortcutEditorRemove(0));
        assert_eq!(app.shortcut_suggest, None);

        let mut app = editor_with_typed(1, "lin");
        // Rows replaced behind the editor's back (an import, a reset).
        app.shortcut_editor_rows
            .insert(0, ("F4".to_string(), "ARC".to_string()));
        assert_eq!(shown(&app, "LINE"), None, "a moved row shows no list");

        let mut app = editor_with_typed(0, "lin");
        let _ = app.update(Message::ShortcutEditorSuggestionDismiss);
        assert_eq!(shown(&app, "LINE"), None);
        assert_eq!(app.shortcut_editor_rows[0].1, "LIN", "the typed text stays");
    }

    /// Enter in the box leaves the typed text alone: no row of the list is
    /// marked until an arrow key marks one. (The arrow and Esc keys are the
    /// shared list's; pressing them here would set its process-wide capture
    /// flags under the tests that read them.)
    #[test]
    fn enter_alone_keeps_the_typed_text() {
        let mut app = editor_with_typed(0, "lin");
        assert!(offered(&app, "lin").len() > 1);
        let messages: Vec<_> = {
            let mut ui = iced_test::Simulator::with_size(
                iced::Settings::default(),
                iced::Size::new(1280.0, 900.0),
                app.view_main(),
            );
            let _ = ui.tap_key(iced::keyboard::key::Named::Enter);
            ui.into_messages().collect()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.shortcut_editor_rows[0].1, "LIN");
    }

    /// The box being typed in keeps its focus when its list opens and
    /// closes: the box stays at the same place in the widget tree.
    #[test]
    fn the_command_box_keeps_focus_when_its_list_opens_and_closes() {
        use iced::advanced::widget::{tree, Tree};
        type InputState = iced::widget::text_input::State<
            <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
        >;
        fn inputs<'t>(tree: &'t mut Tree, found: &mut Vec<&'t mut InputState>) {
            if tree.tag == tree::Tag::of::<InputState>() {
                found.push(tree.state.downcast_mut::<InputState>());
                return;
            }
            for child in &mut tree.children {
                inputs(child, found);
            }
        }
        /// The last text box of the window: the last row's Command box.
        fn last_input(tree: &mut Tree) -> &mut InputState {
            let mut found = Vec::new();
            inputs(tree, &mut found);
            found.pop().expect("a command box")
        }

        let rows = vec![
            ("F2".to_string(), "CIRCLE".to_string()),
            ("F3".to_string(), "LIN".to_string()),
        ];
        let none = rustc_hash::FxHashSet::default();
        let list = vec!["LINE".to_string(), "LINETYPE".to_string()];
        let view = |open: bool| {
            crate::ui::window::shortcuts::view_window(
                &rows,
                "",
                None,
                open.then_some(1),
                if open { &list } else { &[] },
                false,
                false,
                &none,
                &[],
                &none,
                false,
                crate::ui::modal::ModalSizing::FILL,
            )
        };
        let mut window = view(false);
        let mut tree = Tree::new(window.as_widget());
        tree.diff(window.as_widget_mut());
        last_input(&mut tree).focus();
        drop(window);
        for open in [true, false, true] {
            let mut window = view(open);
            tree.diff(window.as_widget_mut());
            assert!(
                last_input(&mut tree).is_focused(),
                "focus lost with the list open={open}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;

    fn export_text(rows: &[(&str, &str)], ext: &str) -> String {
        let table: Vec<Vec<String>> =
            std::iter::once(vec!["Key".to_string(), "Command".to_string()])
                .chain(rows.iter().map(|(k, c)| vec![k.to_string(), c.to_string()]))
                .collect();
        let path = std::env::temp_dir().join(format!(
            "ocs-shortcut-import-{}-{ext}.{ext}",
            std::process::id()
        ));
        crate::app::annotation_data::write_extraction_file(&path, &table).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        text
    }

    fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(k, c)| (k.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn import_round_trips_the_editor_csv_and_txt_export() {
        let rows = [
            ("CTRL+1", "PROPERTIES"),
            ("F2", "TEXT"),
            ("CTRL+,", "OPTIONS"),
            ("F3", "LINE 0,0"),
            ("F4", "SAY \"HI\""),
        ];
        for ext in ["csv", "txt"] {
            let (parsed, skipped) = parse_shortcut_import(&export_text(&rows, ext));
            assert_eq!(skipped, 0, "{ext}");
            assert_eq!(parsed, pairs(&rows), "{ext}");
        }
    }

    fn export_bytes(rows: &[(&str, &str)], ext: &str) -> Vec<u8> {
        let table: Vec<Vec<String>> =
            std::iter::once(vec!["Key".to_string(), "Command".to_string()])
                .chain(rows.iter().map(|(k, c)| vec![k.to_string(), c.to_string()]))
                .collect();
        // Tests run side by side: each call writes a file of its own.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ocs-shortcut-import-bytes-{}-{call}.{ext}",
            std::process::id()
        ));
        crate::app::annotation_data::write_extraction_file(&path, &table).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        bytes
    }

    const ROUND_TRIP_ROWS: [(&str, &str); 4] = [
        ("CTRL+1", "PROPERTIES"),
        ("CTRL+,", "OPTIONS"),
        ("F3", "LINE 0,0"),
        ("F4", "ZOOM EXTENTS"),
    ];

    #[test]
    fn import_reads_the_xlsx_export_by_content() {
        let bytes = export_bytes(&ROUND_TRIP_ROWS, "xlsx");
        assert!(bytes.starts_with(b"PK\x03\x04"));
        let (rows, skipped) = read_shortcut_import(&bytes).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(rows, pairs(&ROUND_TRIP_ROWS));
    }

    #[test]
    fn import_reads_csv_and_txt_export_bytes() {
        for ext in ["csv", "txt"] {
            let (rows, skipped) = read_shortcut_import(&export_bytes(&ROUND_TRIP_ROWS, ext))
                .unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert_eq!(skipped, 0, "{ext}");
            assert_eq!(rows, pairs(&ROUND_TRIP_ROWS), "{ext}");
        }
    }

    #[test]
    fn import_never_turns_binary_into_rows() {
        let garbage: Vec<u8> = (0..=255u8).cycle().take(2048).collect();
        assert!(read_shortcut_import(&garbage).is_err());
        // A truncated workbook is no list either.
        let xlsx = export_bytes(&ROUND_TRIP_ROWS, "xlsx");
        assert!(read_shortcut_import(&xlsx[..40]).is_err());
        // Text with a control character anywhere is refused whole.
        assert!(read_shortcut_import(b"F1 HELP\nF2 \x01TEXT\n").is_err());
        let mut app = OpenCADStudio::new_for_test();
        app.shortcut_editor_rows = pairs(&[("F1", "HELP")]);
        app.import_shortcut_rows(&garbage);
        assert_eq!(app.shortcut_editor_rows, pairs(&[("F1", "HELP")]));
    }

    #[test]
    fn import_accepts_a_utf8_bom_and_skips_a_workbook_row_with_a_control_character() {
        let (rows, _) = read_shortcut_import(b"\xEF\xBB\xBFF1 HELP\n").unwrap();
        assert_eq!(rows, pairs(&[("F1", "HELP")]));
        let table: Vec<Vec<String>> = [["Key", "Command"], ["F1", "HELP"], ["F2", "TE\u{1}XT"]]
            .iter()
            .map(|r| r.iter().map(|c| c.to_string()).collect())
            .collect();
        let bytes = crate::app::annotation_data::xlsx_bytes(&table).unwrap();
        let (rows, skipped) = read_shortcut_import(&bytes).unwrap();
        assert_eq!(rows, pairs(&[("F1", "HELP")]));
        assert_eq!(skipped, 1);
    }

    #[test]
    fn import_reads_the_cuiexport_key_command_form() {
        let text =
            "# comment\nCTRL+1 PROPERTIES\n\nF2   text\nCTRL+, OPTIONS\nF3 LINE 0,0\n, ZOOM E\n";
        let (parsed, skipped) = parse_shortcut_import(text);
        assert_eq!(skipped, 0);
        assert_eq!(
            parsed,
            pairs(&[
                ("CTRL+1", "PROPERTIES"),
                ("F2", "TEXT"),
                ("CTRL+,", "OPTIONS"),
                ("F3", "LINE 0,0"),
                (",", "ZOOM E")
            ])
        );
    }

    #[test]
    fn import_skips_and_counts_bad_lines() {
        let text = "F1 HELP\nJUSTAKEY\n\"unterminated,CMD\nF2,\n,CMD\nF3\tLINE\n";
        let (parsed, skipped) = parse_shortcut_import(text);
        assert_eq!(parsed, pairs(&[("F1", "HELP"), ("F3", "LINE")]));
        assert_eq!(skipped, 4);
    }

    #[test]
    fn import_of_an_empty_file_is_empty() {
        assert_eq!(parse_shortcut_import(""), (vec![], 0));
        assert_eq!(
            parse_shortcut_import("\n  \n# only a comment\n"),
            (vec![], 0)
        );
    }

    #[test]
    fn import_merge_replaces_an_existing_key_and_appends_a_new_one() {
        let mut rows = pairs(&[("F1", "HELP"), ("CTRL+1", "PROPERTIES")]);
        let (imported, _) = parse_shortcut_import("ctrl+1 LINE\nF9 CIRCLE\n");
        merge_shortcut_rows(&mut rows, imported);
        assert_eq!(
            rows,
            pairs(&[("F1", "HELP"), ("CTRL+1", "LINE"), ("F9", "CIRCLE")])
        );
    }

    #[test]
    fn import_goes_to_the_editor_rows_not_the_saved_bindings() {
        let mut app = OpenCADStudio::new_for_test();
        app.shortcut_editor_rows = pairs(&[("F1", "HELP")]);
        let before = app.shortcut_bindings.clone();
        app.import_shortcut_rows(b"F1 LINE\nF9 CIRCLE\nbad\n");
        assert_eq!(
            app.shortcut_editor_rows,
            pairs(&[("F1", "LINE"), ("F9", "CIRCLE")])
        );
        assert_eq!(app.shortcut_bindings, before);
    }

    /// Bare F12 toggles dynamic input and SHIFT+F12 toggles the quad, also
    /// through the config-upgrade path: a settings file saved before the
    /// quad carries no SHIFT+F12 row, and the bare-base-key fallback of
    /// `run_shortcut` must not shadow it into F12 (DYNINPUT) — `apply_config`
    /// backfills the row (docs/quad-menu.md §3.6).
    #[test]
    fn f12_is_dyn_input_and_shift_f12_is_quad_toggle() {
        let mut app = OpenCADStudio::new_for_test();

        // Stale table: exact shape of a config saved before the quad existed.
        let mut cfg = crate::app::config::AppConfig::default();
        cfg.shortcuts.bindings = [(normalize_key("F12"), "DYNINPUT".to_string())]
            .into_iter()
            .collect();
        cfg.shortcuts.known_defaults.clear();
        app.apply_config(cfg);

        let dyn_before = app.dyn_input;
        let _ = app.run_shortcut("F12");
        assert_eq!(app.dyn_input, !dyn_before, "bare F12 toggles dynamic input");
        assert!(!app.quad, "bare F12 must not touch the quad");

        let _ = app.run_shortcut("SHIFT+F12");
        assert!(app.quad, "SHIFT+F12 toggles the quad via QUADTOGGLE");
    }

    #[test]
    fn ctrl_tab_switches_drawings_also_without_a_binding() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"new"}"#);
        let count = app.tabs.len();
        let start = app.active_tab;
        let _ = app.update(Message::ShortcutPressed("CTRL+TAB".into()));
        assert_eq!(app.active_tab, (start + 1) % count);
        let _ = app.update(Message::ShortcutPressed("CTRL+SHIFT+TAB".into()));
        assert_eq!(app.active_tab, start);
        // A configuration saved before the binding existed.
        app.shortcut_bindings.remove("CTRL+TAB");
        let _ = app.update(Message::ShortcutPressed("CTRL+TAB".into()));
        assert_eq!(app.active_tab, (start + 1) % count);
    }

    /// #2805: Ctrl+Shift+A turns group selection on and off.
    #[test]
    fn ctrl_shift_a_toggles_group_selection() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let style = |app: &OpenCADStudio| app.tabs[i].scene.document.header.pick_style;
        let before = style(&app);
        // Also from a configuration saved before the binding existed.
        let mut cfg = crate::app::config::AppConfig::default();
        cfg.shortcuts.bindings.remove(&format!("{ACCEL}+SHIFT+A"));
        cfg.shortcuts.known_defaults.clear();
        app.apply_config(cfg);
        let key = format!("{ACCEL}+SHIFT+A");
        let _ = app.update(Message::ShortcutPressed(key.clone()));
        assert_eq!(style(&app), before ^ 1);
        let _ = app.update(Message::ShortcutPressed(key));
        assert_eq!(style(&app), before);
        // The Groups panel's Group Selection button, and its light.
        let _ = app.dispatch_command("TOGGLEGROUPS");
        assert_eq!(style(&app), before ^ 1);
        assert_eq!(app.ribbon.group_selection, before & 1 == 0);
    }
}
