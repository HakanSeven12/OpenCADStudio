//! Annotation-scale manager — add / delete / edit the drawing's ACAD_SCALELIST.
//!
//! Shares the style managers' frame (toolbar + list + editor + palette) so it
//! looks and behaves like Text / Dim / MLeader style managers, but scales are
//! plain `Scale` objects rather than symbol-table styles, so it drives its own
//! messages instead of the StyleKind machinery.

use crate::app::Message;
use crate::ui::style::style_manager::{
    hdivider, muted_text_style, tb_button, vsep,
};
use iced::widget::{
    column, container, mouse_area, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Theme};

/// Inline-rename text-input id, so the rename-start handler can focus it.
pub fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("scale-rename-input")
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

/// `scales` is (name, "paper:drawing" ratio label). The editor buffers hold the
/// name / paper / drawing of the row being added or edited.
pub fn view_window<'a, 'b>(
    scales: &'b [(String, String)],
    selected: &'b str,
    current: &'b str,
    rename_active: Option<&'b str>,
    rename_buf: &'a str,
    paper_buf: &'a str,
    drawing_buf: &'a str,
) -> Element<'a, Message> {
    // ── Toolbar: New / Delete | Set Current / Apply ───────────────────────
    let toolbar = container(
        row![
            tb_button("New", Message::ScaleManagerNew, false),
            tb_button("Copy", Message::ScaleManagerCopy, false),
            tb_button("Delete", Message::ScaleManagerDelete, false),
            Space::new().width(Fill),
            tb_button("Set Current", Message::ScaleManagerSetCurrent, false),
            tb_button("Apply", Message::ScaleManagerApply, true),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weak.color
        )),
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 8]);

    // ── Left: scale list ──────────────────────────────────────────────────
    let rows: Vec<Element<'_, Message>> = scales
        .iter()
        .map(|(name, ratio)| {
            // The row being renamed shows an inline text field; a single click
            // selects, a double click starts renaming.
            if rename_active == Some(name.as_str()) {
                return text_input("", rename_buf)
                    .id(rename_input_id())
                    .on_input(Message::ScaleRenameEdit)
                    .on_submit(Message::ScaleRenameCommit)
                    .size(11)
                    .padding([4, 8])
                    .width(Fill)
                    .into();
            }
            let is_sel = name.as_str() == selected;
            let is_cur = name.eq_ignore_ascii_case(current);
            let check = crate::ui::icons::themed_check_cell(is_cur);
            let label = row![
                check,
                text(name.clone()).size(11).width(Fill),
                text(ratio.clone()).size(10).style(muted_text_style),
            ]
            .spacing(4)
            .align_y(iced::Center);
            let cell = container(label)
                .padding([4, 8])
                .width(Fill)
                .style(move |theme: &Theme| {
                    let pair = theme.extended_palette().primary.strong;
                    container::Style {
                    background: is_sel.then_some(Background::Color(pair.color)),
                    text_color: is_sel.then_some(pair.text),
                    ..Default::default()
                    }
                });
            mouse_area(cell)
                .on_press(Message::ScaleManagerSelect(name.clone()))
                .on_double_click(Message::ScaleRenameStart(name.clone()))
                .into()
        })
        .collect();

    let list_panel = container(
        column![
            text("Scales").size(10).style(muted_text_style),
            container(scrollable(column(rows).spacing(1)).height(Fill))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(190)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: editor (name + paper:drawing) ──────────────────────────────
    let field =
        |label: &'static str, ph: &'static str, value: &str, on: fn(String) -> Message| {
            row![
                text(label).size(11).style(muted_text_style).width(96),
                text_input(ph, value)
                    .on_input(on)
                    .style(field_style)
                    .size(12)
                    .padding([5, 8])
                    .width(Fill),
            ]
            .align_y(iced::Center)
            .spacing(6)
        };

    let editor = container(
        column![
            text("Scale").size(10).style(muted_text_style),
            field("Paper units", "1", paper_buf, Message::ScaleManagerPaperBuf),
            field("Drawing units", "50", drawing_buf, Message::ScaleManagerDrawingBuf),
            Space::new().height(6),
            text("Double-click a scale to rename it; edit its paper : drawing ratio here. New / Copy add a scale. Changes are kept only if you click Apply before closing.")
                .size(10)
                .style(muted_text_style),
        ]
        .spacing(8),
    )
    .width(Fill)
    .height(Fill)
    .padding(12);

    let body = row![list_panel, vsep(), editor].height(Fill);

    container(column![toolbar, hdivider(), body])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
