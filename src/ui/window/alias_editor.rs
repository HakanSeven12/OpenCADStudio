//! Command-alias editor — an in-canvas modal (Plan B) for adding, remapping and
//! removing command-line aliases (the `ocad.pgp` table). Opened by ALIASEDIT.
//! Rows are `(alias, command)`; edits are buffered in `alias_editor_rows` and
//! committed to the alias table when the dialog closes. Mirrors the editable-row
//! pattern of the attribute editor and plugin manager.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Element, Fill, Length, Theme};

/// Which column of an alias row a text edit targets.
#[derive(Clone, Copy, Debug)]
pub enum AliasField {
    Alias,
    Command,
}

/// Right-hand lane reserved for the scrollbar so it never overlaps the ✕ column.
const GUTTER: f32 = 16.0;

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

/// Build the alias editor content. `rows` is the live working buffer.
pub fn view_window(rows: &[(String, String)]) -> Element<'_, Message> {
    let title = text("Command Aliases").size(15);
    let hint = text(
        "Type an alias and the command it runs (e.g. L → LINE). \
         Apply to save to ocad.pgp; closing discards unapplied edits.",
    )
    .size(11)
    .style(muted_style);

    // Right gutter reserved so the scrollbar has its own lane and never sits on
    // top of the row delete (✕) buttons. Applied to both the header and the
    // scrollable rows so the columns stay aligned.
    let gutter = iced::Padding { top: 0.0, right: GUTTER, bottom: 0.0, left: 0.0 };

    let head = container(
        row![
            container(text("Alias").size(11).style(muted_style)).width(Length::Fixed(120.0)),
            container(text("Command").size(11).style(muted_style)).width(Fill),
            Space::new().width(Length::Fixed(30.0)),
        ]
        .spacing(8),
    )
    .padding(gutter);

    let mut list = column![].spacing(3);
    for (idx, (alias, cmd)) in rows.iter().enumerate() {
        let alias_box = text_input("alias", alias)
            .on_input(move |v| Message::AliasEditorInput { idx, field: AliasField::Alias, value: v })
            .size(13)
            .padding([3, 6])
            .width(Length::Fixed(120.0));
        let cmd_box = text_input("command", cmd)
            .on_input(move |v| Message::AliasEditorInput { idx, field: AliasField::Command, value: v })
            .size(13)
            .padding([3, 6])
            .width(Fill);
        let del = button(crate::ui::icons::themed_danger(crate::ui::icons::CLOSE, 12.0))
            .on_press(Message::AliasEditorRemove(idx))
            .padding([2, 6])
            .style(button::danger);
        list = list.push(
            row![alias_box, cmd_box, del]
                .spacing(8)
                .align_y(iced::Center),
        );
    }

    let add = button(text("+ Add alias").size(12))
        .on_press(Message::AliasEditorAdd)
        .padding([4, 10])
        .style(button::secondary);

    // Apply — primary action; commits the rows to ocad.pgp and stays open.
    let apply = button(text("Apply").size(12))
        .on_press(Message::AliasEditorApply)
        .padding([4, 16])
        .style(button::primary);

    container(
        column![
            title,
            hint,
            Space::new().height(6),
            head,
            scrollable(container(list).padding(gutter)).height(Fill),
            Space::new().height(6),
            row![add, Space::new().width(Fill), apply].align_y(iced::Center),
        ]
        .spacing(6)
        .width(Fill)
        .height(Fill),
    )
    .padding(12)
    .width(Fill)
    .height(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        ..Default::default()
    })
    .into()
}
