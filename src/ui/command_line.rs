//! OpenCADStudio-style command line — bottom panel with input and history

use iced::time::Instant;

use crate::app::Message;
use crate::command::CmdOption;
use iced::widget::{
    button, column, container, opaque, row, text, text_editor, text_input, tooltip, Space,
};
use iced::{Background, Border, Color, Element, Length, Theme};

pub const CMD_INPUT_ID: &str = "cmd_input";

/// How long a history entry stays visible on the overlay before fading
/// out. Picking the full archive happens through the dropdown button.
const HISTORY_VISIBLE_SECS: f32 = 3.0;

/// How many autocomplete matches the suggestion popup shows at once.
const AUTOCOMPLETE_LIMIT: usize = 8;

fn cmd_input_id() -> iced::widget::Id {
    iced::widget::Id::new(CMD_INPUT_ID)
}

/// Drop a prompt's "[A / B / …]" option listing — used for the pinned line
/// when the same options render as clickable buttons beside it. Collapses the
/// leftover double spaces and the orphaned " :" the removal leaves behind.
fn strip_option_listing(s: &str) -> String {
    let Some(a) = s.find('[') else {
        return s.to_string();
    };
    let Some(off) = s[a..].find(']') else {
        return s.to_string();
    };
    let mut out = format!("{}{}", &s[..a], &s[a + off + 1..]);
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.replace(" :", ":").trim().to_string()
}

const MAX_HISTORY: usize = 64;

#[derive(Clone, Default)]
pub struct CommandLine {
    pub input: String,
    /// Persistent literal-space mode (the `>` toggle button): while on, every
    /// line behaves as if it started with `>` — Space stays in the input
    /// instead of submitting. Saved in the user config.
    pub literal_spaces: bool,
    pub history: Vec<HistoryEntry>,
    /// Commands the user has typed (for ↑/↓ recall). Holds the raw typed
    /// strings so the line can be re-edited; distinct from `recent_commands`.
    pub cmd_recall: Vec<String>,
    /// Commands actually dispatched, from any source (command line, ribbon,
    /// context menu, shortcuts), newest last. Drives the right-click "Repeat"
    /// menu so it reflects every command source, not only typed ones.
    pub recent_commands: Vec<String>,
    /// Current position in `cmd_recall` while navigating (None = not navigating).
    recall_cursor: Option<usize>,
    /// Saved draft input before the user started navigating history.
    recall_draft: String,
    /// When `true`, the dropdown showing the full backlog is open.
    pub history_open: bool,
    /// Index of the currently-highlighted autocomplete suggestion, or
    /// `None` when the user hasn't yet started navigating with the
    /// arrow keys. Reset on every keystroke.
    pub autocomplete_cursor: Option<usize>,
    /// Command names contributed by loaded plugins, refreshed whenever the
    /// enabled-plugin set changes. Merged into autocomplete alongside the
    /// compile-time command registry, so runtime plugin commands are typeable
    /// and discoverable — not only reachable via ribbon buttons (#272).
    pub dynamic_commands: Vec<String>,
    /// Command aliases (uppercase `alias → command`), a copy of the app's alias
    /// table kept here for autocomplete. Aliases are hidden from suggestions
    /// (a terse `CC` never appears) while their target command (`COPYCLIP`)
    /// still does; and when the whole input is an alias, its target is surfaced
    /// as the top suggestion so the highlight matches what Enter runs. (#288)
    pub command_aliases: rustc_hash::FxHashMap<String, String>,
    /// The active command step's prompt, mirrored here so a step change
    /// can be detected and the pinned (non-fading) history line updated.
    step_prompt: Option<String>,
    /// The active command step's clickable options, mirrored here so they
    /// render as buttons above the input row. Empty when no command is
    /// active or the current step offers no options. (#304)
    step_options: Vec<CmdOption>,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub kind: EntryKind,
    pub text: String,
    /// When this entry was pushed. Used by the overlay to fade entries
    /// out after `HISTORY_VISIBLE_SECS`. The dropdown popup ignores it
    /// and always shows the whole list.
    pub created_at: Instant,
    /// The active command step's prompt is pinned so it does not fade
    /// while the user is still working on that step. When the step
    /// completes the pin is cleared and the normal cooldown resumes.
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Command,
    Output,
    Error,
    Info,
}

impl CommandLine {
    pub fn new() -> Self {
        let mut cl = Self::default();
        cl.push_info("Open CAD Studio ready.");
        cl.push_info("Type a command or use the ribbon. Open OBJ: INSERT tab.");
        cl
    }

    pub fn submit(&mut self) -> Option<String> {
        let raw = self.input.trim().to_string();
        if raw.is_empty() {
            return None;
        }
        // Uppercase only the command verb (the first token); keep arguments
        // verbatim so case-sensitive values survive — file paths on
        // case-sensitive filesystems, identifiers, plugin command arguments.
        // Dispatch matches verbs in uppercase and each sub-command handler
        // re-uppercases its own keywords, so only free-form args are affected.
        let cmd = match raw.split_once(char::is_whitespace) {
            Some((verb, rest)) => format!("{} {}", verb.to_uppercase(), rest),
            None => raw.to_uppercase(),
        };
        // Record in recall list (avoid duplicates at the top).
        if self.cmd_recall.last().map(|s| s.as_str()) != Some(raw.as_str()) {
            self.cmd_recall.push(raw);
            if self.cmd_recall.len() > 50 {
                self.cmd_recall.remove(0);
            }
        }
        self.recall_cursor = None;
        self.recall_draft.clear();
        self.push_command(&self.input.clone());
        self.input.clear();
        Some(cmd)
    }

    /// Record a dispatched command for the right-click "Repeat" menu, skipping
    /// a consecutive duplicate and capping the list. Called from the dispatch
    /// choke point so commands from every source are captured.
    pub fn record_recent(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if self.recent_commands.last().map(|s| s.as_str()) != Some(cmd) {
            self.recent_commands.push(cmd.to_string());
            if self.recent_commands.len() > 50 {
                self.recent_commands.remove(0);
            }
        }
    }

    /// Navigate to the previous command in recall history (↑).
    pub fn history_prev(&mut self) {
        if self.cmd_recall.is_empty() {
            return;
        }
        let cursor = match self.recall_cursor {
            None => {
                self.recall_draft = self.input.clone();
                self.cmd_recall.len() - 1
            }
            Some(c) if c > 0 => c - 1,
            Some(c) => c,
        };
        self.recall_cursor = Some(cursor);
        self.input = self.cmd_recall[cursor].clone();
    }

    /// Navigate to the next command in recall history (↓).
    pub fn history_next(&mut self) {
        match self.recall_cursor {
            None => {}
            Some(c) if c + 1 < self.cmd_recall.len() => {
                let next = c + 1;
                self.recall_cursor = Some(next);
                self.input = self.cmd_recall[next].clone();
            }
            Some(_) => {
                self.recall_cursor = None;
                self.input = self.recall_draft.clone();
            }
        }
    }

    pub fn push_command(&mut self, cmd: &str) {
        self.push(EntryKind::Command, format!("Command: {cmd}"));
    }
    pub fn push_output(&mut self, msg: &str) {
        self.push(EntryKind::Output, msg.to_string());
    }
    pub fn push_error(&mut self, msg: &str) {
        self.push(EntryKind::Error, format!("*Invalid*  {msg}"));
    }
    /// Append an error unless it is already the latest history line. Repeated
    /// retry failures should refresh the concise message, not flood history
    /// with identical copies (#498).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn push_error_once(&mut self, msg: &str) {
        let text = format!("*Invalid*  {msg}");
        if let Some(last) = self
            .history
            .last_mut()
            .filter(|entry| entry.kind == EntryKind::Error && entry.text == text)
        {
            last.created_at = Instant::now();
            return;
        }
        self.push(EntryKind::Error, text);
    }
    pub fn push_info(&mut self, msg: &str) {
        self.push(EntryKind::Info, msg.to_string());
    }
    fn push(&mut self, kind: EntryKind, text: String) {
        self.history.push(HistoryEntry {
            kind,
            text,
            created_at: Instant::now(),
            pinned: false,
        });
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Mirror the active command step's prompt. While the step is current
    /// its history line is pinned so it does not fade; when the step
    /// changes (or the command ends, `prompt == None`) the previous line's
    /// cooldown restarts so it fades normally from now. The dispatch /
    /// step-transition code already pushes the prompt as an `Info` line, so
    /// the matching tail entry is pinned in place rather than duplicated.
    pub fn set_step_prompt(&mut self, prompt: Option<String>) {
        if prompt == self.step_prompt {
            return;
        }
        // Release the previous pin and let its cooldown start now.
        for e in self.history.iter_mut().filter(|e| e.pinned) {
            e.pinned = false;
            e.created_at = Instant::now();
        }
        if let Some(p) = &prompt {
            // Reuse the prompt line dispatch/step-transition just pushed;
            // otherwise add it.
            if self.history.last().map(|e| &e.text) != Some(p) {
                self.push(EntryKind::Info, p.clone());
            }
            if let Some(last) = self.history.last_mut() {
                last.pinned = true;
            }
        }
        self.step_prompt = prompt;
    }

    /// Mirror the active command step's clickable options so `view` can render
    /// them as buttons. Called each frame alongside [`Self::set_step_prompt`].
    pub fn set_step_options(&mut self, opts: Vec<CmdOption>) {
        self.step_options = opts;
    }

    /// `true` while at least one history entry is still within the
    /// visible window — the host app uses this to drive a low-frequency
    /// tick subscription so the overlay re-renders and fades the entry
    /// once it expires.
    pub fn has_visible_history(&self) -> bool {
        self.history
            .iter()
            .any(|e| e.pinned || e.created_at.elapsed().as_secs_f32() < HISTORY_VISIBLE_SECS)
    }

    pub fn toggle_history(&mut self) {
        self.history_open = !self.history_open;
    }

    pub fn close_history(&mut self) {
        self.history_open = false;
    }

    /// The whole history flattened to plain text, one entry per line, for the
    /// clipboard-copy button (issue #232). Entries carry no per-line prefix
    /// beyond what `push_*` already baked into `text` (e.g. "Command: "), so
    /// the pasted block reads the same as the on-screen log.
    pub fn history_plain_text(&self) -> String {
        self.history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Drop every history line. The step-prompt mirror is reset too so a
    /// later step still repins correctly.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.step_prompt = None;
    }

    /// Move the autocomplete highlight up one entry. Wraps to the last
    /// match. Returns `true` when there was a list to navigate.
    pub fn autocomplete_prev(&mut self) -> bool {
        let len = self.autocomplete_matches().len();
        if len == 0 {
            return false;
        }
        // No explicit cursor means the top match (index 0) is highlighted,
        // so ↑ from there wraps to the last entry.
        let cur = self.autocomplete_cursor.unwrap_or(0);
        let next = if cur == 0 { len - 1 } else { cur - 1 };
        self.autocomplete_cursor = Some(next);
        true
    }

    /// Move the autocomplete highlight down one entry. Wraps to the
    /// first match. Returns `true` when there was a list to navigate.
    pub fn autocomplete_next(&mut self) -> bool {
        let len = self.autocomplete_matches().len();
        if len == 0 {
            return false;
        }
        // No explicit cursor means the top match (index 0) is highlighted,
        // so ↓ from there advances to the next entry (wrapping at the end).
        let cur = self.autocomplete_cursor.unwrap_or(0);
        let next = if cur + 1 < len { cur + 1 } else { 0 };
        self.autocomplete_cursor = Some(next);
        true
    }

    /// The command name the user has currently highlighted in the
    /// autocomplete popup, if any.
    pub fn selected_suggestion(&self) -> Option<String> {
        let matches = self.autocomplete_matches();
        self.autocomplete_cursor
            .and_then(|i| matches.get(i).cloned())
    }

    /// Autocomplete suggestions for the current input — see
    /// [`ranked_matches`]. Includes loaded plugins' commands (#272).
    pub fn autocomplete_matches(&self) -> Vec<String> {
        ranked_matches(
            self.input.trim(),
            &self.dynamic_commands,
            &self.command_aliases,
        )
    }

    pub fn view<'a>(
        &'a self,
        show_autocomplete: bool,
        dyn_capturing: bool,
        history_content: &'a text_editor::Content,
    ) -> Element<'a, Message> {
        // Only the most recent entries pushed within the last few
        // seconds show on the overlay. The dropdown button keeps the
        // full backlog reachable when the user actually wants it.
        let visible: Vec<&HistoryEntry> = self
            .history
            .iter()
            .filter(|e| e.pinned || e.created_at.elapsed().as_secs_f32() < HISTORY_VISIBLE_SECS)
            .collect();
        let start = visible.len().saturating_sub(4);
        let history_rows = visible[start..]
            .iter()
            .fold(column![].spacing(0), |col, entry| {
                let kind = entry.kind.clone();
                let entry_text = |value: String| {
                    text(value).size(11).style({
                        let kind = kind.clone();
                        move |theme: &Theme| iced::widget::text::Style {
                            color: Some(history_color(theme, &kind)),
                        }
                    })
                };
                // The current step's prompt is the single pinned line. When the
                // step offers options, render them as clickable buttons inline
                // beside the prompt text — so options need never be typed and
                // don't clutter the prompt string itself. (#304) The buttons
                // already name every choice, so the prompt's own "[A / B / …]"
                // listing is dropped here (the history log keeps the full text).
                if entry.pinned && !self.step_options.is_empty() {
                    let shown = strip_option_listing(&entry.text);
                    let mut r = row![entry_text(shown)]
                        .spacing(6)
                        .align_y(iced::Center);
                    for opt in &self.step_options {
                        let btn = button(text(opt.label.to_uppercase()).size(11))
                        .on_press(Message::CommandOptionPick(opt.keyword.clone()))
                        .padding([1, 6])
                        .style(|theme: &Theme, status| {
                            let palette = theme.extended_palette();
                            let pair = if matches!(
                                status,
                                button::Status::Hovered | button::Status::Pressed
                            ) {
                                palette.primary.weak
                            } else {
                                palette.background.weakest
                            };
                            button::Style {
                                background: Some(Background::Color(pair.color)),
                                text_color: pair.text,
                                border: Border {
                                    color: palette.background.neutral.color,
                                    width: 1.0,
                                    radius: 3.0.into(),
                                },
                                ..Default::default()
                            }
                        });
                        r = r.push(btn);
                    }
                    col.push(container(r).padding([1, 8]))
                } else {
                    col.push(container(entry_text(entry.text.clone())).padding([1, 8]))
                }
            });
        let prompt = container(
            text("Command:").size(11).style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().success.base.color),
            }),
        )
        .padding([5, 8]);
        // Literal-space toggle: while active, every line behaves as if it
        // started with `>` — Space stays in the line instead of submitting, so
        // arguments with spaces (text strings, paths, `UCS Z 90` as one line)
        // can be typed. Persists until toggled off; a hand-typed leading `>`
        // lights the button too (same mode, one line only).
        let literal_active = self.literal_spaces || self.input.starts_with('>');
        let literal_btn = button(text(">").size(11))
        .on_press(Message::CommandLiteralToggle)
        .padding([2, 6])
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            let pair = if literal_active {
                palette.primary.weak
            } else if matches!(
                status,
                button::Status::Hovered | button::Status::Pressed
            ) {
                palette.background.weak
            } else {
                palette.background.weakest
            };
            button::Style {
                background: Some(Background::Color(pair.color)),
                text_color: pair.text,
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });
        let literal_tip = container(
            text("Literal spaces: Space stays in the line instead of running the command (same as typing a leading '>'). Stays on until toggled off.")
                .size(11),
        )
        .padding([3, 6])
        .style(container::bordered_box);
        let literal_btn = tooltip(literal_btn, literal_tip, tooltip::Position::Top).gap(4);
        // While dynamic input is capturing keystrokes, the command-line
        // text field is left without an `on_input` handler so it can't
        // grab focus or swallow numeric keys — those flow through the
        // global key subscription into the dynamic-input fields instead.
        let mut input = text_input("", &self.input).id(cmd_input_id());
        if !dyn_capturing {
            input = input
                .on_input(Message::CommandInput)
                .on_submit(Message::CommandSubmit);
        }
        let input = input
            .size(11)
            .padding([4, 6]);
        // Autocomplete suggestions panel, shown above the input row
        // when the user has typed a prefix that matches at least one
        // command. Each row is a button — clicking it dispatches the
        // command directly.
        let autocomplete: Element<'_, Message> = if show_autocomplete {
            let matches = self.autocomplete_matches();
            if matches.is_empty() {
                container(column![]).height(0).into()
            } else {
                // Before any arrow-key navigation, the top match is
                // highlighted so it's visible that Enter runs it — the
                // standard DWG command-line behavior.
                let cursor = self.autocomplete_cursor.unwrap_or(0);
                let mut col = column![].spacing(0).width(Length::Fill);
                for (idx, cmd) in matches.iter().enumerate() {
                    let is_selected = cursor == idx;
                    let row = button(text(cmd.clone()).size(11))
                        .on_press(Message::CommandSuggestionPick(cmd.clone()))
                        .width(Length::Fill)
                        .padding([2, 8])
                        .style(move |theme: &Theme, status| {
                            let palette = theme.extended_palette();
                            let pair = if is_selected {
                                palette.primary.weak
                            } else if matches!(
                                status,
                                button::Status::Hovered | button::Status::Pressed
                            ) {
                                palette.background.weak
                            } else {
                                palette.background.base
                            };
                            button::Style {
                                background: Some(Background::Color(pair.color)),
                                text_color: pair.text,
                                border: Border::default(),
                                ..Default::default()
                            }
                        });
                    col = col.push(row);
                }
                container(col)
                    .style(container::bordered_box)
                    .width(Length::Fill)
                    .into()
            }
        } else {
            container(column![]).height(0).into()
        };

        // Dropdown trigger next to the input. Clicking it pops up the
        // full backlog (everything pushed since the app started) so the
        // user can recover anything that has already faded off the
        // overlay.
        let dropdown_icon = if self.history_open {
            crate::ui::icons::themed_arrow_down(11.0)
        } else {
            crate::ui::icons::themed_arrow_right(11.0)
        };
        let dropdown_btn = button(dropdown_icon)
            .on_press(Message::CommandHistoryToggle)
        .style(button::subtle)
        .padding([2, 6]);
        let input_row = row![prompt, literal_btn, input, dropdown_btn]
            .spacing(4)
            .align_y(iced::Center);

        // Full backlog dropdown — appears ABOVE the input pill when open. The
        // whole log is rendered in ONE read-only `text_editor` (backed by
        // `history_content`) rather than per-line labels, so a single mouse
        // drag selects across lines and Ctrl+C copies the lot — issue #232.
        // Edits are dropped in the update handler, keeping it read-only. The
        // editor scrolls internally past `max_height`; `opaque` stops its
        // mouse-wheel events bubbling to the viewport shader behind it (else
        // scrolling the history zoomed the drawing).
        let dropdown: Element<'a, Message> = if self.history_open {
            let log = text_editor(history_content)
                .on_action(Message::CommandHistoryEdit)
                .size(11)
                .padding([2, 8])
                .max_height(180.0)
                .style(|theme: &Theme, _status| {
                    let palette = theme.extended_palette();
                    text_editor::Style {
                        background: Background::Color(palette.background.base.color),
                        border: Border::default(),
                        placeholder: palette.background.base.text.scale_alpha(0.72),
                        value: palette.background.base.text,
                        selection: palette.primary.base.color.scale_alpha(0.5),
                    }
                });
            // Header strip: a Copy-all and a Clear button pinned above the log.
            let copy_btn = button(
                row![
                    crate::ui::icons::themed_success(crate::ui::icons::COPY, 11.0),
                    text("Copy").size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::CommandHistoryCopy)
            .style(header_btn_style)
            .padding([2, 6]);
            let clear_btn = button(
                row![
                    crate::ui::icons::themed_warning(crate::ui::icons::TRASH, 11.0),
                    text("Clear").size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::CommandHistoryClear)
            .style(header_btn_style)
            .padding([2, 6]);
            let header = container(
                row![Space::new().width(Length::Fill), copy_btn, clear_btn]
                    .spacing(6)
                    .align_y(iced::Center),
            )
            .width(Length::Fill)
            .padding([2, 6]);
            let panel = container(column![header, log])
                .style(container::bordered_box)
                .width(Length::Fill)
                .padding([4, 0]);
            opaque(panel).into()
        } else {
            container(column![]).height(0).into()
        };

        container(column![
            autocomplete,
            dropdown,
            container(history_rows)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        theme.extended_palette().background.base.color,
                    )),
                    ..Default::default()
                })
                .width(Length::Fill)
                .padding([2, 0]),
            container(input_row)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weakest.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
                })
                .width(Length::Fill)
                // Match the drawing tab bar / status bar height, and vertically
                // centre the prompt/input/dropdown within it (issue #216).
                .center_y(Length::Fixed(30.0)),
        ])
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.base.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
            }
        })
        .width(Length::Fixed(720.0))
        .into()
    }
}

/// Command names matching `needle`, ranked for autocomplete: case-insensitive
/// substring match (typing `LEADER` surfaces `LEADER`, `MLEADER`, `QLEADER`),
/// prefix matches first, then alphabetical, capped at [`AUTOCOMPLETE_LIMIT`].
/// Shared by the suggestion popup and the Enter-key closest-match fallback so
/// both agree on the top suggestion.
///
/// Names come from `crate::command::all_registered_command_names()` — the
/// compile-time `inventory` registry — merged with `dynamic`, the command names
/// contributed by loaded plugins (runtime, so they can't be `&'static`; see
/// #272). Returns owned strings to carry both sources.
///
/// `aliases` is the `alias → command` table. Aliases themselves are dropped from
/// the results, so a terse `CC` is typeable and dispatches (via the alias table)
/// but never clutters the suggestions — only its target `COPYCLIP` is offered.
/// And when the whole input is an alias, its target command is forced to the top
/// so the highlighted suggestion matches what pressing Enter actually runs
/// (typing `AA` highlights `AREA`, `L` highlights `LINE`). (#288)
pub fn ranked_matches(
    needle: &str,
    dynamic: &[String],
    aliases: &rustc_hash::FxHashMap<String, String>,
) -> Vec<String> {
    let needle = needle.trim().to_uppercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<String> = crate::command::all_registered_command_names()
        .into_iter()
        .map(|cmd| cmd.to_string())
        // Plugin names are uppercased to match the built-ins and the needle,
        // so ranking and display stay consistent across both sources.
        .chain(dynamic.iter().map(|cmd| cmd.to_uppercase()))
        // Hide aliases (keys of the table); their target command still shows.
        .filter(|cmd| cmd.contains(&needle) && !aliases.contains_key(cmd))
        .collect();
    matches.sort();
    matches.dedup();
    // Prefix matches rank above mid-string ones, then alphabetical so the
    // order is stable as the user keeps typing.
    matches.sort_by(|a, b| {
        (!a.starts_with(&needle))
            .cmp(&!b.starts_with(&needle))
            .then_with(|| a.cmp(b))
    });
    // If the entire input is an alias, its target is what Enter runs, so make it
    // the first (highlighted) suggestion — even when the target has no substring
    // overlap with the alias (`AA` → `AREA`) and so wouldn't otherwise appear.
    if let Some(target) = aliases.get(&needle) {
        matches.retain(|m| m != target);
        matches.insert(0, target.clone());
    }
    matches.truncate(AUTOCOMPLETE_LIMIT);
    matches
}

/// Flat button style for the history dropdown's Copy / Clear strip: a subtle
/// filled pill that brightens on hover.
fn header_btn_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        palette.background.weak
    } else {
        palette.background.weakest
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn history_color(theme: &Theme, kind: &EntryKind) -> Color {
    let palette = theme.extended_palette();
    match kind {
        EntryKind::Command => palette.background.base.text,
        EntryKind::Output => palette.background.base.text.scale_alpha(0.72),
        EntryKind::Error => palette.danger.base.color,
        EntryKind::Info => palette.primary.base.color,
    }
}

#[cfg(test)]
mod tests {
    use super::{ranked_matches, CommandLine};
    use rustc_hash::FxHashMap;

    fn aliases(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(a, c)| (a.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn dynamic_plugin_commands_surface_in_autocomplete() {
        let none = FxHashMap::default();
        // No built-in command contains the plugin prefix, so an empty pool
        // yields nothing — reproducing the #272 bug's blind autocomplete.
        assert!(ranked_matches("LS_", &[], &none).is_empty());
        // A loaded plugin's commands become typeable, case-insensitively, and
        // the prefix substring surfaces every one of them.
        let pool = vec!["LS_LABEL".to_string(), "ls_autolabel".to_string()];
        let m = ranked_matches("LS_", &pool, &none);
        assert!(m.contains(&"LS_LABEL".to_string()), "got {m:?}");
        assert!(m.contains(&"LS_AUTOLABEL".to_string()), "got {m:?}");
    }

    #[test]
    fn builtin_commands_still_match_with_an_empty_pool() {
        let m = ranked_matches("LINE", &[], &FxHashMap::default());
        assert!(m.iter().any(|c| c == "LINE"), "got {m:?}");
    }

    #[test]
    fn aliases_are_hidden_but_their_command_survives() {
        // The alias `L` is dropped from suggestions while `LINE` (its target)
        // still appears. (#288)
        let a = aliases(&[("L", "LINE")]);
        let m = ranked_matches("L", &[], &a);
        assert!(!m.iter().any(|c| c == "L"), "alias L should be hidden: {m:?}");
        assert!(m.iter().any(|c| c == "LINE"), "LINE should remain: {m:?}");
    }

    #[test]
    fn exact_alias_forces_its_target_to_the_top() {
        // Typing a full alias highlights the command Enter will run, even when
        // the target shares no substring with the alias (`AA` → `AREA`). (#288)
        let a = aliases(&[("AA", "AREA"), ("L", "LINE")]);
        let m = ranked_matches("AA", &[], &a);
        assert_eq!(m.first().map(String::as_str), Some("AREA"), "got {m:?}");
        let m = ranked_matches("L", &[], &a);
        assert_eq!(m.first().map(String::as_str), Some("LINE"), "got {m:?}");
    }

    #[test]
    fn issue_498_repeated_save_error_is_not_duplicated() {
        let mut line = CommandLine::new();
        let initial_len = line.history.len();
        line.push_error_once("Unable to save: file is in use.");
        line.push_error_once("Unable to save: file is in use.");
        assert_eq!(line.history.len(), initial_len + 1);
    }
}
