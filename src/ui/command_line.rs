//! OpenCADStudio-style command line — bottom panel with input and history

use iced::time::Instant;

use crate::app::Message;
use crate::command::CmdOption;
use crate::t;
use iced::widget::{
    button, column, container, opaque, row, rule, scrollable, stack, text, text_editor,
    text_input, tooltip, Space,
};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};
use std::ops::Range;

pub const CMD_INPUT_ID: &str = "cmd_input";
pub const HISTORY_SCROLL_ID: &str = "command_history_scroll";

/// How long a history entry stays visible on the overlay before fading
/// out. Picking the full archive happens through the dropdown button.
const HISTORY_VISIBLE_SECS: f32 = 3.0;

/// Resizable full-history editor bounds. The live upper bound also follows the
/// window height so the command input and a useful drawing area remain visible.
pub const HISTORY_HEIGHT_MIN: f32 = 72.0;
pub const HISTORY_HEIGHT_DEFAULT: f32 = 180.0;
pub const HISTORY_HEIGHT_MAX: f32 = 560.0;

pub fn history_max_height(window_height: f32) -> f32 {
    if window_height.is_finite() {
        (window_height * 0.60).clamp(HISTORY_HEIGHT_MIN, HISTORY_HEIGHT_MAX)
    } else {
        HISTORY_HEIGHT_DEFAULT
    }
}

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
    /// Successfully dispatched commands used for ↑/↓ recall, newest last.
    /// Stored separately because recall also maintains its own cursor and draft.
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
    /// Persisted height of the full-history editor in logical pixels.
    pub history_height: f32,
    /// Index of the currently-highlighted autocomplete suggestion, or
    /// `None` before keyboard navigation begins. Reset when input changes.
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

/// Per-logical-line entry kinds for the selectable full-history editor.
/// Keeping this metadata beside the editor's plain text preserves drag
/// selection and copy while allowing the renderer to restore each entry's
/// original color.
#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryHighlightSettings {
    line_kinds: Vec<EntryKind>,
}

#[derive(Debug)]
struct HistoryHighlighter {
    settings: HistoryHighlightSettings,
    current_line: usize,
}

impl iced::advanced::text::Highlighter for HistoryHighlighter {
    type Settings = HistoryHighlightSettings;
    type Highlight = EntryKind;
    type Iterator<'a> = std::option::IntoIter<(Range<usize>, EntryKind)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            settings: settings.clone(),
            current_line: 0,
        }
    }

    fn update(&mut self, settings: &Self::Settings) {
        self.settings = settings.clone();
        self.current_line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let kind = self
            .settings
            .line_kinds
            .get(self.current_line)
            .cloned()
            .unwrap_or(EntryKind::Output);
        self.current_line += 1;
        Some((0..line.len(), kind)).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

impl CommandLine {
    pub fn new() -> Self {
        let mut cl = Self::default();
        cl.history_height = HISTORY_HEIGHT_DEFAULT;
        cl.push_info(&crate::tr!("command-line-ready"));
        cl.push_info(&crate::tr!("command-line-hint"));
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
        self.push_command(&self.input.clone());
        self.input.clear();
        Some(cmd)
    }

    /// Record a successfully dispatched command for both the right-click
    /// "Repeat" menu and ↑/↓ recall. Consecutive duplicates are skipped and
    /// both lists are capped. Called from the dispatch choke point so commands
    /// from every source are captured.
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
        if self.cmd_recall.last().map(String::as_str) != Some(cmd) {
            self.cmd_recall.push(cmd.to_string());
            if self.cmd_recall.len() > 50 {
                self.cmd_recall.remove(0);
            }
        }
        self.cancel_history_navigation();
    }

    pub fn history_navigation_active(&self) -> bool {
        self.recall_cursor.is_some()
    }

    pub fn cancel_history_navigation(&mut self) {
        self.recall_cursor = None;
        self.recall_draft.clear();
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
        self.push(EntryKind::Command, format!("{} {cmd}", t!("Command:")));
    }
    pub fn push_output(&mut self, msg: &str) {
        self.push(EntryKind::Output, msg.to_string());
    }
    pub fn push_error(&mut self, msg: &str) {
        self.push(EntryKind::Error, format!("*{}*  {msg}", t!("Invalid")));
    }
    /// Append an error unless it is already the latest history line. Repeated
    /// retry failures should refresh the concise message, not flood history
    /// with identical copies (#498).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn push_error_once(&mut self, msg: &str) {
        let text = format!("*{}*  {msg}", t!("Invalid"));
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

    fn history_highlight_settings(&self) -> HistoryHighlightSettings {
        let line_kinds = self
            .history
            .iter()
            .flat_map(|entry| {
                std::iter::repeat(entry.kind.clone()).take(entry.text.split('\n').count())
            })
            .collect();
        HistoryHighlightSettings { line_kinds }
    }

    /// Drop every history line. The step-prompt mirror is reset too so a
    /// later step still repins correctly.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.step_prompt = None;
    }

    /// Move the autocomplete highlight up one entry. Wraps to the last match.
    pub fn autocomplete_prev(&mut self) -> bool {
        let len = self.autocomplete_matches().len();
        if len == 0 {
            return false;
        }
        let current = self.autocomplete_cursor.unwrap_or(0).min(len - 1);
        self.autocomplete_cursor = Some(if current == 0 {
            len - 1
        } else {
            current - 1
        });
        true
    }

    /// Move the autocomplete highlight down one entry. Wraps to the first match.
    pub fn autocomplete_next(&mut self) -> bool {
        let len = self.autocomplete_matches().len();
        if len == 0 {
            return false;
        }
        let current = self.autocomplete_cursor.unwrap_or(0).min(len - 1);
        self.autocomplete_cursor = Some(if current + 1 < len {
            current + 1
        } else {
            0
        });
        true
    }

    /// The command name explicitly highlighted in the autocomplete popup.
    pub fn selected_suggestion(&self) -> Option<String> {
        let matches = self.autocomplete_matches();
        self.autocomplete_cursor
            .and_then(|index| matches.get(index).cloned())
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
        window_height: f32,
    ) -> Element<'a, Message> {
        // Only the most recent entries pushed within the last few
        // seconds show on the overlay. The dropdown button keeps the
        // full backlog reachable when the user actually wants it.
        let mut visible: Vec<&HistoryEntry> = self
            .history
            .iter()
            .filter(|e| e.pinned || e.created_at.elapsed().as_secs_f32() < HISTORY_VISIBLE_SECS)
            .collect();
        // Keep the active prompt/options immediately above the input. Commands
        // may emit informational lines while waiting for the next option; those
        // lines belong above the pinned interaction row, not below it.
        if let Some(index) = visible.iter().position(|entry| entry.pinned) {
            let pinned = visible.remove(index);
            visible.push(pinned);
        }
        let start = visible.len().saturating_sub(4);
        let has_recent_history = start < visible.len();
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
                            let palette = theme.palette();
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
            text(crate::tr!("command-line-label")).size(11).style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.palette().success.base.color),
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
            let palette = theme.palette();
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
        let literal_tip = container(text(crate::tr!("command-line-literal-spaces")).size(11))
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
        let input = input.size(11).padding(Padding {
            top: 4.0,
            right: 30.0,
            bottom: 4.0,
            left: 6.0,
        });
        // Autocomplete suggestions panel, shown above the input row
        // when the user has typed a prefix that matches at least one
        // command. Each row is a button — clicking it dispatches the
        // command directly.
        let autocomplete: Element<'_, Message> = if show_autocomplete {
            let matches = self.autocomplete_matches();
            if matches.is_empty() {
                container(column![]).height(0).into()
            } else {
                let cursor = self.autocomplete_cursor.unwrap_or(0);
                let mut col = column![].spacing(0).width(Length::Fill);
                for (idx, cmd) in matches.iter().enumerate() {
                    let is_selected = idx == cursor;
                    let row = button(text(cmd.clone()).size(11))
                        .on_press(Message::CommandSuggestionPick(cmd.clone()))
                        .width(Length::Fill)
                        .padding([2, 8])
                        .style(move |theme: &Theme, status| {
                            let palette = theme.palette();
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
            .style(button::text)
            .padding([4, 8]);
        let input_with_history = stack![
            input,
            container(dropdown_btn)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Center),
        ]
        .width(Length::Fill);
        let input_row = row![prompt, literal_btn, input_with_history]
            .spacing(4)
            .align_y(iced::Center);

        // Full backlog dropdown — appears ABOVE the input pill when open. The
        // whole log is rendered in ONE read-only `text_editor` (backed by
        // `history_content`) rather than per-line labels, so a single mouse
        // drag selects across lines and Ctrl+C copies the lot — issue #232.
        // Edits are dropped in the update handler, keeping it read-only. A
        // surrounding scrollable supplies the visible scrollbar while
        // `opaque` stops its mouse-wheel events reaching the drawing behind it.
        let dropdown: Element<'a, Message> = if self.history_open {
            let history_height = self
                .history_height
                .clamp(HISTORY_HEIGHT_MIN, history_max_height(window_height));
            let log = text_editor(history_content)
                .on_action(Message::CommandHistoryEdit)
                .size(11)
                .padding([2, 8])
                .height(Length::Shrink)
                .highlight_with::<HistoryHighlighter>(
                    self.history_highlight_settings(),
                    history_highlight_format,
                )
                .style(|theme: &Theme, _status| {
                    let palette = theme.palette();
                    text_editor::Style {
                        background: Background::Color(palette.background.base.color),
                        border: Border::default(),
                        placeholder: palette.background.base.text.scale_alpha(0.72),
                        value: palette.background.base.text,
                        selection: palette.primary.base.color.scale_alpha(0.5),
                    }
                });
            let log = scrollable(log)
                .id(iced::widget::Id::new(HISTORY_SCROLL_ID))
                .height(Length::Fixed(history_height))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(8)
                        .scroller_width(6),
                ))
                .anchor_bottom();
            // Header strip: a Copy-all and a Clear button pinned above the log.
            let copy_btn = button(
                row![
                    crate::ui::icons::themed_success(crate::ui::icons::COPY, 11.0),
                    text(t!("Copy")).size(11),
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
                    text(t!("Clear")).size(11),
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
                .width(Length::Fill)
                .padding([4, 8]);
            let resize = iced::widget::mouse_area(
                container(crate::ui::icons::themed_primary(
                    crate::ui::icons::RESIZE,
                    15.0,
                ))
                .padding([0, 2]),
            )
            .on_press(Message::CommandHistoryResizeGrab)
            .on_double_click(Message::CommandHistoryHeightReset)
            .interaction(iced::mouse::Interaction::ResizingVertically);
            let panel = stack![
                panel,
                container(resize)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Left)
                    .align_y(iced::alignment::Vertical::Top),
            ];
            opaque(panel).into()
        } else {
            container(column![]).height(0).into()
        };

        // The transient recent lines and the full archive are two views of the
        // same history. Showing both while the archive is open creates a fake
        // second history region and visually disconnects the input row (#555).
        let recent_history: Element<'a, Message> = if self.history_open || !has_recent_history {
            container(column![]).height(0).into()
        } else {
            container(history_rows)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        theme.palette().background.base.color,
                    )),
                    ..Default::default()
                })
                .width(Length::Fill)
                .padding([2, 0])
                .into()
        };
        let history_divider: Element<'a, Message> = if self.history_open {
            rule::horizontal(1).into()
        } else {
            container(column![]).height(0).into()
        };

        let history_open = self.history_open;
        container(
            column![
                autocomplete,
                dropdown,
                recent_history,
                history_divider,
                container(input_row)
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(Background::Color(
                                palette.background.weakest.color,
                            )),
                            ..Default::default()
                        }
                    })
                    .width(Length::Fill)
                    // Match the drawing tab bar / status bar height, and vertically
                    // centre the prompt/input/dropdown within it (issue #216).
                    .center_y(Length::Fixed(30.0)),
            ]
            .width(Length::Fill),
        )
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: if history_open { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .width(Length::Fill.max(720.0))
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
    let palette = theme.palette();
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
    let palette = theme.palette();
    match kind {
        EntryKind::Command => palette.background.base.text,
        EntryKind::Output => palette.background.base.text.scale_alpha(0.72),
        EntryKind::Error => palette.danger.base.color,
        EntryKind::Info => palette.primary.base.color,
    }
}

fn history_highlight_format(
    kind: &EntryKind,
    theme: &Theme,
) -> iced::advanced::text::highlighter::Format<iced::Font> {
    iced::advanced::text::highlighter::Format {
        color: Some(history_color(theme, kind)),
        font: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ranked_matches, CommandLine};
    use crate::t;
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
        line.push_error_once(t!("Unable to save: file is in use.").as_ref());
        line.push_error_once(t!("Unable to save: file is in use.").as_ref());
        assert_eq!(line.history.len(), initial_len + 1);
    }

    #[test]
    fn command_recall_walks_both_directions_and_restores_the_draft() {
        let mut line = CommandLine::new();
        for command in ["LINE", "LINE", "CIRCLE"] {
            line.record_recent(command);
        }
        line.input = "PARTIAL".to_string();

        line.history_prev();
        assert_eq!(line.input, "CIRCLE");
        line.history_prev();
        assert_eq!(line.input, "LINE");
        line.history_prev();
        assert_eq!(line.input, "LINE");
        line.history_next();
        assert_eq!(line.input, "CIRCLE");
        line.history_next();
        assert_eq!(line.input, "PARTIAL");
        assert_eq!(line.cmd_recall, vec!["LINE".to_string(), "CIRCLE".to_string()]);
    }

    #[test]
    fn recalled_command_can_be_edited_before_submit() {
        let mut line = CommandLine::new();
        line.record_recent("MOVE");
        line.history_prev();
        line.input.push_str(" 0,0 10,0");
        let submitted = line.submit().expect("edited command");
        assert_eq!(submitted, "MOVE 0,0 10,0");
        line.record_recent(&submitted);
        assert_eq!(
            line.cmd_recall.last().map(String::as_str),
            Some("MOVE 0,0 10,0")
        );
    }
}
