//! Editable keyboard shortcut table opened by CUI / SHORTCUTS.

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::editor_table::{danger_input_style, danger_text_style};
use crate::ui::style::form::{dialog_button, hdivider};
use crate::ui::style::{chrome, kit_metrics as km};
use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Background, Border, Element, Length, Theme};

#[derive(Clone, Copy, Debug)]
pub enum ShortcutField {
    Key,
    Command,
}

/// Narrowest suggestion list; a longer command name widens it.
const SUGGESTION_LIST_MIN_WIDTH: f32 = 220.0;

/// Width of the Key column; the header and every row share it.
const KEY_WIDTH: f32 = 180.0;

/// Width of the actions column (trash, or check and cancel on a draft row).
const ACTIONS_WIDTH: f32 = 60.0;

/// Natural height of the framed list. The dialog is measured at this height
/// and keeps it: filtering never moves the buttons under the list. Resizing
/// the window grows the list.
const LIST_HEIGHT: f32 = 340.0;

/// A muted line centred in an empty list.
fn empty_note<'a>(note: String) -> Element<'a, Message> {
    container(
        text(note)
            .size(12)
            .style(muted_style)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .padding(16)
    .center(Length::Fill)
    .into()
}

/// Widget id of row `idx`'s Command box, so picking a suggestion can put the
/// caret after the picked name instead of where the typed prefix ended.
pub fn command_input_id(idx: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("shortcut-command-{idx}"))
}

/// Commands to offer for the text typed in a Command box: the command
/// line's own autocomplete list (names that start with the text first, then
/// names that contain it; an alias offers the command it stands for). Empty
/// when the text already is the only command it could become.
pub fn suggestions(
    typed: &str,
    dynamic: &[String],
    aliases: &rustc_hash::FxHashMap<String, String>,
) -> Vec<String> {
    let found = crate::ui::command_line::ranked_matches(typed, dynamic, aliases);
    match found.as_slice() {
        [only] if only.eq_ignore_ascii_case(typed.trim()) => Vec::new(),
        _ => found,
    }
}

/// Indices of the rows a search query keeps visible. The match is a
/// case-insensitive substring of the key or the command; an empty query keeps
/// every row. The unfinished draft row (index 0 while `pending_add`) is always
/// kept so adding stays possible while a filter is typed. Only the display is
/// filtered: the rows themselves, and so what Apply saves, are untouched.
pub fn visible_rows(rows: &[(String, String)], query: &str, pending_add: bool) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(idx, (key, command))| {
            q.is_empty()
                || (pending_add && *idx == 0)
                || key.to_lowercase().contains(&q)
                || command.to_lowercase().contains(&q)
        })
        .map(|(idx, _)| idx)
        .collect()
}

/// Width of the dialog: wide enough for the footer (counter, Reset, Export,
/// Import, Apply, Apply & Exit) in one row.
pub const SHORTCUTS_WIDTH: f32 = 900.0;

pub fn view_window<'a>(
    rows: &'a [(String, String)],
    filter: &str,
    capture_row: Option<usize>,
    suggest_row: Option<usize>,
    suggestions: &[String],
    pending_add: bool,
    reset_confirm: bool,
    duplicate_keys: &rustc_hash::FxHashSet<String>,
    duplicate_conflicts: &[(String, String)],
    unknown_commands: &rustc_hash::FxHashSet<String>,
    close_confirm: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let title = text(t!("Keyboard Shortcuts")).size(15);
    let hint = text(t!(
        "Click + Add, press a key combination, type the command, then Apply. Esc cancels the pending row."
    ))
    .size(11)
    .style(muted_style);
    // The header lines up with the rows below: same side insets as the
    // scroll area, plus the room its scroll bar takes.
    let head = container(
        row![
            container(text(t!("Key")).size(12).style(muted_style)).width(Length::Fixed(KEY_WIDTH)),
            container(text(t!("Command")).size(12).style(muted_style)).width(Length::Fill),
            Space::new().width(Length::Fixed(ACTIONS_WIDTH)),
        ]
        .spacing(8),
    )
    .padding(iced::Padding {
        top: 6.0,
        right: km::GRID_INSET_R + km::SCROLLBAR_W,
        bottom: 6.0,
        left: km::GRID_INSET_L,
    });

    let shown = visible_rows(rows, filter, pending_add);
    let shown_empty = shown.is_empty();
    let mut list = column![].spacing(3);
    for idx in shown {
        let (key, command) = &rows[idx];
        // Key cell: click to arm capture, then press the combination. The
        // armed cell shows what is expected and highlights until a key
        // arrives; clicking it again cancels. A key that already exists in
        // another row is shown red. Manual key entry is available via
        // SHORTCUTS SET.
        let armed = capture_row == Some(idx);
        let duplicate = !key.is_empty() && duplicate_keys.contains(key.as_str());
        let key_label = if armed {
            t!("Press a key combination...").into_owned()
        } else if key.is_empty() {
            t!("Click, then press keys...").into_owned()
        } else {
            key.clone()
        };
        let key_box = if armed {
            button(text(key_label).size(13))
                .on_press(Message::ShortcutCaptureClear)
                .padding([3, 6])
                .width(Length::Fixed(KEY_WIDTH))
                .style(button::primary)
        } else if duplicate {
            // The whole field turns red, not just the text.
            button(text(key_label).size(13))
                .on_press(Message::ShortcutCaptureStart(idx))
                .padding([3, 6])
                .width(Length::Fixed(KEY_WIDTH))
                .style(button::danger)
        } else {
            button(text(key_label).size(13).style(muted_style))
                .on_press(Message::ShortcutCaptureStart(idx))
                .padding([3, 6])
                .width(Length::Fixed(KEY_WIDTH))
                .style(button::secondary)
        };
        let unknown_command = !command.is_empty() && unknown_commands.contains(command.trim());
        let command_box = text_input(t!("command").as_ref(), command)
            .id(command_input_id(idx))
            .on_input(move |value| Message::ShortcutEditorInput {
                idx,
                field: ShortcutField::Command,
                value,
            })
            .size(13)
            .padding([3, 6])
            .width(Length::Fill);
        // An unrunnable command turns the whole field red, mirroring the
        // duplicate key cell.
        let command_box = if unknown_command {
            command_box.style(danger_input_style)
        } else {
            command_box
        };
        // The suggestion list floats under the box, so it moves no row and
        // needs no room in the window. Every row carries the (closed) list:
        // the box keeps its place in the widget tree, and with it its focus
        // and caret, when its list opens.
        let offered: &[String] = if suggest_row == Some(idx) {
            suggestions
        } else {
            &[]
        };
        let options = offered
            .iter()
            .map(|name| {
                crate::ui::kit::dropdown_option(
                    name.clone(),
                    name.clone(),
                    Message::ShortcutEditorSuggestionPick {
                        idx,
                        command: name.clone(),
                    },
                    false,
                )
            })
            .collect();
        let command_box = crate::ui::kit::dropdown(
            command_box.into(),
            crate::ui::dropdown_list::DropdownList::new(options)
                .query(command.clone())
                // No row is marked until an arrow key marks one: Enter alone
                // must leave the typed text as it is.
                .select_first_by_default(false)
                .on_escape(Message::ShortcutEditorSuggestionDismiss),
            crate::ui::kit::dropdown_width(
                offered.iter().map(String::as_str),
                SUGGESTION_LIST_MIN_WIDTH,
            ),
            Message::ShortcutEditorSuggestionDismiss,
            !offered.is_empty(),
        );
        // Draft rows get a check (finish the addition, without applying) and
        // a cancel ✕; committed rows get the trash bin.
        let remove = if pending_add && idx == 0 {
            // Done is only clickable for a valid row: key and command filled,
            // key not already used, command actually runnable.
            let done_ok = !key.is_empty()
                && !command.is_empty()
                && !duplicate
                && !unknown_commands.contains(command.trim());
            row![
                button(crate::ui::icons::themed_success_text(
                    crate::ui::icons::CHECK,
                    12.0,
                ))
                .on_press_maybe(done_ok.then_some(Message::ShortcutEditorDraftAccept))
                .padding([2, 6])
                .style(button::success),
                button(crate::ui::icons::themed_danger_text(
                    crate::ui::icons::CLOSE,
                    12.0,
                ))
                .on_press(Message::ShortcutCaptureCancel)
                .padding([2, 6])
                .style(button::danger),
            ]
            .spacing(4)
            .align_y(iced::Center)
        } else {
            row![
                button(crate::ui::icons::themed_danger_text(
                    crate::ui::icons::TRASH,
                    12.0,
                ))
                .on_press(Message::ShortcutEditorRemove(idx))
                .padding([2, 6])
                .style(button::danger)
            ]
            .align_y(iced::Center)
        };
        list = list.push(
            row![
                key_box,
                command_box,
                container(remove).width(Length::Fixed(ACTIONS_WIDTH))
            ]
                .spacing(8)
                .align_y(iced::Center),
        );
    }

    // While a draft row is pending, the add button doubles as the visible
    // cancel affordance (Esc works too).
    let add = if pending_add {
        button(text(t!("Cancel add (Esc)")).size(12))
            .on_press(Message::ShortcutCaptureCancel)
            .padding([6, 18])
            .style(button::danger)
    } else {
        dialog_button(format!("+ {}", t!("Add")), Message::ShortcutEditorAdd, false)
    };
    let toolbar = row![
        crate::ui::kit::search_field(
            t!("Search…").as_ref(),
            filter,
            Message::ShortcutEditorFilter
        ),
        add,
    ]
    .spacing(8)
    .align_y(iced::Center);

    // The framed list keeps its height whatever the filter leaves in it, so
    // typing in the search box never moves the buttons below.
    let body: Element<'a, Message> = if shown_empty {
        empty_note(crate::tf!("No shortcut matches \"{}\"", filter).into_owned())
    } else {
        crate::ui::kit::scroll_area(list)
    };
    let fill = sizing.height == Length::Fill;
    let well = container(column![head, hdivider(Length::Fill), body])
        .width(Length::Fill)
        .height(if fill {
            Length::Fill
        } else {
            Length::Fixed(LIST_HEIGHT)
        })
        .padding(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            border: Border {
                color: chrome::border(theme),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    // Persistent duplicate warning: shown until every conflicting key is
    // resolved (re-captured, changed, or a row removed). It sits right under
    // the list.
    let mut conflict_banner = column![].spacing(2);
    for (key, command) in duplicate_conflicts {
        conflict_banner = conflict_banner.push(
            text(crate::tf!(
                "Shortcut already used for command: {} → {}",
                key,
                command
            ))
            .size(12)
            .style(danger_text_style),
        );
    }
    for command in unknown_commands {
        conflict_banner = conflict_banner.push(
            text(crate::tf!("Unknown command: {}", command))
                .size(12)
                .style(danger_text_style),
        );
    }

    // Reset asks for confirmation in place: the left group becomes the
    // question with Yes / No, and the Apply buttons step aside.
    let footer: Element<'a, Message> = if reset_confirm {
        row![
            text(t!(
                "Are you sure you want to reset? You will lose all your current shortcuts!"
            ))
            .size(12)
            .style(danger_text_style),
            button(text(t!("Yes, reset")).size(12).wrapping(iced::widget::text::Wrapping::None))
                .on_press(Message::ShortcutEditorResetConfirm)
                .padding([6, 18])
                .style(button::danger),
            dialog_button(t!("No"), Message::ShortcutEditorResetDeny, false),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    } else {
        row![
            text(format!("{}: {}", t!("Number of shortcuts"), rows.len()))
                .size(12)
                .wrapping(iced::widget::text::Wrapping::None)
                .style(muted_style),
            dialog_button(
                t!("Reset to default"),
                Message::ShortcutEditorResetAsk,
                false
            ),
            dialog_button(t!("Export..."), Message::KeyListExport(false), false),
            // Import is greyed out while a draft row is pending.
            dialog_button(t!("Import..."), Message::ShortcutEditorImport, false)
                .on_press_maybe((!pending_add).then_some(Message::ShortcutEditorImport)),
            Space::new().width(Length::Fill),
            dialog_button(t!("Apply"), Message::ShortcutEditorApply, false),
            dialog_button(t!("Apply & Exit"), Message::ShortcutEditorApplyExit, true),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    let content = container(
        column![
            title,
            hint,
            toolbar,
            well,
            conflict_banner,
            hdivider(Length::Fill),
            footer,
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(if fill { Length::Fill } else { Length::Shrink }),
    )
    .padding(16)
    .width(if sizing.width == Length::Fill {
        Length::Fill
    } else {
        Length::Fixed(SHORTCUTS_WIDTH)
    })
    .height(if fill { Length::Fill } else { Length::Shrink })
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    });

    // Unsaved-changes guard: closing with un-applied rows stacks a dimmed
    // shield and a confirmation panel on top of the editor.
    if !close_confirm {
        return content.into();
    }
    let shield = iced::widget::mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.palette().background.strongest.color.scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_press(Message::ShortcutEditorCloseKeep)
    .interaction(iced::mouse::Interaction::Idle);
    let panel = container(
        column![
            text(t!("Unsaved changes will be discarded.")).size(14),
            Space::new().height(10),
            row![
                button(text(t!("Discard & close")).size(12))
                    .on_press(Message::ShortcutEditorCloseDiscard)
                    .padding([4, 12])
                    .style(button::danger),
                button(text(t!("Keep editing")).size(12))
                    .on_press(Message::ShortcutEditorCloseKeep)
                    .padding([4, 12])
                    .style(button::secondary),
            ]
            .spacing(8),
        ]
        .spacing(4),
    )
    .padding(16)
    .width(Length::Fixed(320.0))
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: iced::Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    });
    let centered = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center);
    iced::widget::stack![content, iced::widget::opaque(shield), centered].into()
}

#[cfg(test)]
mod tests {
    use super::visible_rows;

    fn rows() -> Vec<(String, String)> {
        vec![
            ("CTRL+D".to_owned(), "DIST".to_owned()),
            ("CTRL+SHIFT+L".to_owned(), "LAYER".to_owned()),
            ("F3".to_owned(), "OSNAP".to_owned()),
        ]
    }

    #[test]
    fn empty_query_keeps_all_rows() {
        assert_eq!(visible_rows(&rows(), "", false), vec![0, 1, 2]);
        assert_eq!(visible_rows(&rows(), "  ", false), vec![0, 1, 2]);
    }

    #[test]
    fn query_matches_command_name_case_insensitive() {
        assert_eq!(visible_rows(&rows(), "layer", false), vec![1]);
        assert_eq!(visible_rows(&rows(), "OSN", false), vec![2]);
    }

    #[test]
    fn query_matches_key_text() {
        assert_eq!(visible_rows(&rows(), "ctrl", false), vec![0, 1]);
        assert_eq!(visible_rows(&rows(), "f3", false), vec![2]);
    }

    #[test]
    fn query_matching_nothing_shows_no_rows() {
        assert!(visible_rows(&rows(), "zzz", false).is_empty());
    }

    #[test]
    fn draft_row_stays_visible_and_indices_stay_original() {
        let mut r = rows();
        r.insert(0, (String::new(), String::new()));
        assert_eq!(visible_rows(&r, "osnap", true), vec![0, 3]);
        assert_eq!(visible_rows(&r, "osnap", false), vec![3]);
    }
}
