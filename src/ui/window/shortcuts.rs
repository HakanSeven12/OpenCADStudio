//! Editable keyboard shortcut table opened by CUI / SHORTCUTS.

use crate::app::Message;
use crate::t;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Element, Length, Theme};

#[derive(Clone, Copy, Debug)]
pub enum ShortcutField {
    Key,
    Command,
}

const GUTTER: f32 = 16.0;

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

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

pub fn view_window<'a>(
    rows: &'a [(String, String)],
    capture_row: Option<usize>,
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
    let gutter = iced::Padding {
        top: 0.0,
        right: GUTTER,
        bottom: 0.0,
        left: 0.0,
    };

    let head = container(
        row![
            container(text(t!("Key")).size(11).style(muted_style))
                .width(Length::Fixed(180.0)),
            container(text(t!("Command")).size(11).style(muted_style)).width(sizing.width),
            Space::new().width(Length::Fixed(30.0)),
        ]
        .spacing(8),
    )
    .padding(gutter);

    let mut list = column![].spacing(3);
    for (idx, (key, command)) in rows.iter().enumerate() {
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
                .width(Length::Fixed(180.0))
                .style(button::primary)
        } else if duplicate {
            // The whole field turns red, not just the text.
            button(text(key_label).size(13))
                .on_press(Message::ShortcutCaptureStart(idx))
                .padding([3, 6])
                .width(Length::Fixed(180.0))
                .style(button::danger)
        } else {
            button(text(key_label).size(13).style(muted_style))
                .on_press(Message::ShortcutCaptureStart(idx))
                .padding([3, 6])
                .width(Length::Fixed(180.0))
                .style(button::secondary)
        };
        let unknown_command =
            !command.is_empty() && unknown_commands.contains(command.trim());
        let command_box = text_input(t!("command").as_ref(), command)
            .on_input(move |value| Message::ShortcutEditorInput {
                idx,
                field: ShortcutField::Command,
                value,
            })
            .size(13)
            .padding([3, 6])
            .width(sizing.width);
        // An unrunnable command turns the whole field red, mirroring the
        // duplicate key cell.
        let command_box = if unknown_command {
            command_box.style(danger_input_style)
        } else {
            command_box
        };
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
            row![button(crate::ui::icons::themed_danger_text(
                crate::ui::icons::TRASH,
                12.0,
            ))
            .on_press(Message::ShortcutEditorRemove(idx))
            .padding([2, 6])
            .style(button::danger)]
            .align_y(iced::Center)
        };
        list = list.push(
            row![key_box, command_box, remove]
                .spacing(8)
                .align_y(iced::Center),
        );
    }

    // While a draft row is pending, the add button doubles as the visible
    // cancel affordance (Esc works too).
    let add = if pending_add {
        button(text(t!("Cancel add (Esc)")).size(12))
            .on_press(Message::ShortcutCaptureCancel)
            .padding([4, 10])
            .style(button::danger)
    } else {
        button(text(format!("+ {}", t!("Add"))).size(12))
            .on_press(Message::ShortcutEditorAdd)
            .padding([4, 10])
            .style(button::secondary)
    };
    // The reset confirmation replaces the shortcut count so the warning
    // gets the whole left side of the bar.
    let stats = if reset_confirm {
        row![Space::new().width(Length::Fixed(0.0))]
    } else {
        row![text(format!("{}: {}", t!("Number of shortcuts"), rows.len()))
            .size(12)
            .style(muted_style)]
    };
    // Reset asks for confirmation in place: the add/reset buttons are
    // replaced by the question with Yes / No.
    let (reset_area, add_area) = if reset_confirm {
        (
            row![
                text(t!(
                    "Are you sure you want to reset? You will lose all your current shortcuts!"
                ))
                .size(12)
                .style(danger_text_style),
                button(text(t!("Yes, reset")).size(12))
                    .on_press(Message::ShortcutEditorResetConfirm)
                    .padding([4, 10])
                    .style(button::danger),
                button(text(t!("No")).size(12))
                    .on_press(Message::ShortcutEditorResetDeny)
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
                .on_press(Message::ShortcutEditorResetAsk)
                .padding([4, 10])
                .style(button::secondary)]
            .spacing(8)
            .align_y(iced::Center),
            row![add],
        )
    };
    let apply = button(text(t!("Apply")).size(12))
        .on_press(Message::ShortcutEditorApply)
        .padding([4, 16])
        .style(button::primary);
    let apply_exit = button(text(t!("Apply && Exit")).size(12))
        .on_press(Message::ShortcutEditorApplyExit)
        .padding([4, 16])
        .style(button::primary);
    // The reset confirmation takes over the whole action bar.
    let apply_area = if reset_confirm {
        row![Space::new().width(Length::Fixed(0.0))]
    } else {
        row![apply, apply_exit].spacing(8).align_y(iced::Center)
    };

    // Persistent duplicate warning: shown until every conflicting key is
    // resolved (re-captured, changed, or a row removed).
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
    .on_press(Message::ShortcutEditorCloseKeep)
    .interaction(iced::mouse::Interaction::Idle);
    let panel = container(
        column![
            text(t!("Unsaved changes will be discarded.")).size(14),
            Space::new().height(10),
            row![
                button(text(t!("Discard && close")).size(12))
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
