//! Command-alias editor — an in-canvas modal (Plan B) for adding, remapping and
//! removing command-line aliases (the `ocad.pgp` table). Opened by ALIASEDIT.
//! Rows are `(alias, command)`; edits are buffered in `alias_editor_rows` and
//! committed to the alias table on Apply. Mirrors the shortcut editor:
//! transient draft row with explicit accept, duplicate/unknown validation,
//! reset-to-defaults with confirmation, apply-and-exit, alias count, and an
//! unsaved-changes guard on close.

use crate::app::Message;
use crate::ui::style::common::muted_style;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Element, Length, Theme};
use crate::t;

/// Which column of an alias row a text edit targets.
#[derive(Clone, Copy, Debug)]
pub enum AliasField {
    Alias,
    Command,
}

/// Right-hand lane reserved for the scrollbar so it never overlaps the ✕ column.
const GUTTER: f32 = 16.0;

fn danger_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().danger.base.color),
    }
}

/// Text-input variant flagging an invalid value: danger border and value
/// color on the theme's danger weak background.
fn danger_input_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let danger = theme.palette().danger.base;
    iced::widget::text_input::Style {
        background: Background::Color(theme.palette().danger.weak.color),
        border: iced::border::rounded(4)
            .color(danger.color)
            .width(
                if matches!(
                    status,
                    iced::widget::text_input::Status::Focused { is_hovered: _ }
                ) {
                    1.5
                } else {
                    1.0
                },
            ),
        icon: danger.color,
        placeholder: danger.color.scale_alpha(0.7),
        value: danger.text,
        selection: danger.color,
    }
}

/// Build the alias editor content. `rows` is the live working buffer.
pub fn view_window<'a>(
    rows: &'a [(String, String)],
    pending_add: bool,
    reset_confirm: bool,
    duplicate_aliases: &rustc_hash::FxHashSet<String>,
    duplicate_conflicts: &[(String, String)],
    unknown_commands: &rustc_hash::FxHashSet<String>,
    close_confirm: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let title = text(t!("Command Aliases")).size(15);
    let hint = text(t!(
        "Click + Add alias, type the alias and command, accept the row, then Apply. Esc cancels the pending row."
    ))
    .size(11)
    .style(muted_style);

    // Right gutter reserved so the scrollbar has its own lane and never sits on
    // top of the row delete (✕) buttons. Applied to both the header and the
    // scrollable rows so the columns stay aligned.
    let gutter = iced::Padding { top: 0.0, right: GUTTER, bottom: 0.0, left: 0.0 };

    let head = container(
        row![
            container(text(t!("Alias")).size(11).style(muted_style)).width(Length::Fixed(120.0)),
            container(text(t!("Command")).size(11).style(muted_style)).width(sizing.width),
            Space::new().width(Length::Fixed(62.0)),
        ]
        .spacing(8),
    )
    .padding(gutter);

    let mut list = column![].spacing(3);
    for (idx, (alias, cmd)) in rows.iter().enumerate() {
        let alias_key = alias.trim().to_uppercase();
        let duplicate = !alias_key.is_empty() && duplicate_aliases.contains(alias_key.as_str());
        let alias_box = text_input(t!("alias").as_ref(), alias)
            .on_input(move |v| Message::AliasEditorInput { idx, field: AliasField::Alias, value: v })
            .size(13)
            .padding([3, 6])
            .width(Length::Fixed(120.0));
        let alias_box = if duplicate {
            alias_box.style(danger_input_style)
        } else {
            alias_box
        };
        let unknown_command =
            !cmd.trim().is_empty() && unknown_commands.contains(cmd.trim());
        let cmd_box = text_input(t!("command").as_ref(), cmd)
            .on_input(move |v| Message::AliasEditorInput { idx, field: AliasField::Command, value: v })
            .size(13)
            .padding([3, 6])
            .width(sizing.width);
        let cmd_box = if unknown_command {
            cmd_box.style(danger_input_style)
        } else {
            cmd_box
        };
        // Draft rows get a check (finish the addition, without applying) and
        // a cancel ✕; committed rows get the trash bin.
        let remove = if pending_add && idx == 0 {
            let done_ok = !alias.trim().is_empty()
                && !cmd.trim().is_empty()
                && !duplicate
                && !unknown_commands.contains(cmd.trim());
            row![
                button(crate::ui::icons::themed_success_text(
                    crate::ui::icons::CHECK,
                    12.0,
                ))
                .on_press_maybe(done_ok.then_some(Message::AliasEditorDraftAccept))
                .padding([2, 6])
                .style(button::success),
                button(crate::ui::icons::themed_danger_text(
                    crate::ui::icons::CLOSE,
                    12.0,
                ))
                .on_press(Message::AliasEditorDraftCancel)
                .padding([2, 6])
                .style(button::danger),
            ]
            .spacing(4)
            .align_y(iced::Center)
        } else {
            row![button(crate::ui::icons::themed_danger_text(
                crate::ui::icons::TRASH,
                12.0,
            ))
            .on_press(Message::AliasEditorRemove(idx))
            .padding([2, 6])
            .style(button::danger)]
            .align_y(iced::Center)
        };
        list = list.push(
            row![alias_box, cmd_box, remove]
                .spacing(8)
                .align_y(iced::Center),
        );
    }

    // While a draft row is pending, the add button doubles as the visible
    // cancel affordance (Esc works too).
    let add = if pending_add {
        button(text(t!("Cancel add (Esc)")).size(12))
            .on_press(Message::AliasEditorDraftCancel)
            .padding([4, 10])
            .style(button::danger)
    } else {
        button(text(t!("+ Add alias")).size(12))
            .on_press(Message::AliasEditorAdd)
            .padding([4, 10])
            .style(button::secondary)
    };
    // The reset confirmation replaces the alias count so the warning
    // gets the whole left side of the bar.
    let stats = if reset_confirm {
        row![Space::new().width(Length::Fixed(0.0))]
    } else {
        row![text(format!("{}: {}", t!("Number of aliases"), rows.len()))
            .size(12)
            .style(muted_style)]
    };
    // Reset asks for confirmation in place: the add/reset buttons are
    // replaced by the question with Yes / No.
    let (reset_area, add_area) = if reset_confirm {
        (
            row![
                text(t!(
                    "Are you sure you want to reset? You will lose all your current aliases!"
                ))
                .size(12)
                .style(danger_text_style),
                button(text(t!("Yes, reset")).size(12))
                    .on_press(Message::AliasEditorResetConfirm)
                    .padding([4, 10])
                    .style(button::danger),
                button(text(t!("No")).size(12))
                    .on_press(Message::AliasEditorResetDeny)
                    .padding([4, 10])
                    .style(button::secondary),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![Space::new().width(Length::Fixed(0.0))],
        )
    } else {
        (
            row![button(text(t!("Reset to default")).size(12))
                .on_press(Message::AliasEditorResetAsk)
                .padding([4, 10])
                .style(button::secondary)]
            .spacing(8)
            .align_y(iced::Center),
            row![add],
        )
    };
    // Apply — primary action; commits the rows to ocad.pgp and stays open.
    let apply = button(text(t!("Apply")).size(12))
        .on_press(Message::AliasEditorApply)
        .padding([4, 16])
        .style(button::primary);
    let apply_exit = button(text(t!("Apply && Exit")).size(12))
        .on_press(Message::AliasEditorApplyExit)
        .padding([4, 16])
        .style(button::primary);
    // The reset confirmation takes over the whole action bar.
    let apply_area = if reset_confirm {
        row![Space::new().width(Length::Fixed(0.0))]
    } else {
        row![apply, apply_exit].spacing(8).align_y(iced::Center)
    };

    // Persistent validation warnings: shown until every conflicting alias is
    // resolved (edited or a row removed) and every command is runnable.
    let mut conflict_banner = column![].spacing(2);
    for (alias, command) in duplicate_conflicts {
        conflict_banner = conflict_banner.push(
            text(crate::tf!(
                "Alias already used for command: {} → {}",
                alias,
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

    let content = container(
        column![
            title,
            hint,
            Space::new().height(6),
            head,
            scrollable(container(list).padding(gutter)).height(sizing.height),
            Space::new().height(6),
            conflict_banner,
            Space::new().height(4),
            row![stats, add_area, reset_area, Space::new().width(sizing.width), apply_area]
                .spacing(8)
                .align_y(iced::Center),
        ]
        .spacing(6)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
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
                    theme
                        .palette()
                        .background
                        .strongest
                        .color
                        .scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_press(Message::AliasEditorCloseKeep)
    .interaction(iced::mouse::Interaction::Idle);
    let panel = container(
        column![
            text(t!("Unsaved changes will be discarded.")).size(14),
            Space::new().height(10),
            row![
                button(text(t!("Discard && close")).size(12))
                    .on_press(Message::AliasEditorCloseDiscard)
                    .padding([4, 12])
                    .style(button::danger),
                button(text(t!("Keep editing")).size(12))
                    .on_press(Message::AliasEditorCloseKeep)
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
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
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
