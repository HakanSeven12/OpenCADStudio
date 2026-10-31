//! The MTEXT toolbar's second row: character formatting, undo, paragraph
//! alignment and the menus that hold the rest of the editor's commands.
use super::*;
use crate::app::mtext_editor::{EditorMenu, FormatState, MTextEditorState, MTextFmt, ParaAlign};

macro_rules! label {
    ($id:literal) => {
        crate::tr!("mtext-toolbar", $id)
    };
}

/// `content` with `hint` shown below it on hover.
pub(super) fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    hint: String,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        content,
        container(text(hint).size(11))
            .padding([3, 6])
            .style(container::rounded_box),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(iced::widget::rule::vertical(1))
        .height(24)
        .padding([0, 4])
        .into()
}

fn button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.background.strong,
        _ => palette.background.weak,
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

pub(super) fn format_row(ed: &MTextEditorState) -> Element<'static, Message> {
    let icon = |bytes: &'static [u8], message: Message, hint: String| {
        tip(
            button(crate::ui::icons::themed(bytes, 18.0))
                .on_press(message)
                .padding(3)
                .style(button_style),
            hint,
        )
    };
    let glyph = |glyph: &'static str, message: Message, hint: String| {
        tip(
            button(text(glyph).size(13))
                .on_press(message)
                .padding([2, 6])
                .style(button_style),
            hint,
        )
    };
    // A format button shows whether the selection (or what is typed next)
    // has it: highlighted when on, a warning border when mixed, inert when
    // the text or selection cannot take it. The hint says which.
    let format = |bytes: &'static [u8], kind: MTextFmt, name: String| {
        let state = ed.format_state(kind);
        let status = match state {
            FormatState::Off => label!("format-off"),
            FormatState::On => label!("format-on"),
            FormatState::Mixed => label!("format-mixed"),
            FormatState::Unavailable => label!("format-unavailable"),
        };
        let control = button(crate::ui::icons::themed(bytes, 18.0))
            .on_press_maybe((state != FormatState::Unavailable).then_some(Message::MTextFmt(kind)))
            .padding(3)
            .style(move |theme: &Theme, status| {
                let mut style = if state == FormatState::On {
                    button::primary(theme, status)
                } else {
                    button_style(theme, status)
                };
                if state == FormatState::Mixed {
                    style.border.width = 2.0;
                    style.border.color = theme.palette().warning.base.color;
                }
                style
            });
        tip(control, format!("{name}: {status}"))
    };
    row![
        format(
            include_bytes!("../../../../assets/icons/mt_bold.svg"),
            MTextFmt::Bold,
            label!("bold")
        ),
        format(
            include_bytes!("../../../../assets/icons/mt_italic.svg"),
            MTextFmt::Italic,
            label!("italic")
        ),
        format(
            include_bytes!("../../../../assets/icons/mt_underline.svg"),
            MTextFmt::Underline,
            label!("underline")
        ),
        format(
            include_bytes!("../../../../assets/icons/mt_overline.svg"),
            MTextFmt::Overline,
            label!("overline")
        ),
        format(
            include_bytes!("../../../../assets/icons/mt_strike.svg"),
            MTextFmt::Strike,
            label!("strike")
        ),
        separator(),
        glyph("↶", Message::MTextUndo, label!("undo")),
        glyph("↷", Message::MTextRedo, label!("redo")),
        separator(),
        icon(
            include_bytes!("../../../../assets/icons/mt_align_left.svg"),
            Message::MTextAlign(ParaAlign::Left),
            label!("align-left")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_align_center.svg"),
            Message::MTextAlign(ParaAlign::Center),
            label!("align-center")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_align_right.svg"),
            Message::MTextAlign(ParaAlign::Right),
            label!("align-right")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_align_justify.svg"),
            Message::MTextAlign(ParaAlign::Justify),
            label!("align-justify")
        ),
        separator(),
        mtext_menu::toolbar_menu(ed, EditorMenu::More),
        mtext_menu::toolbar_menu(ed, EditorMenu::Paragraph),
        mtext_menu::toolbar_menu(ed, EditorMenu::Columns),
        mtext_menu::toolbar_menu(ed, EditorMenu::Justify),
        mtext_menu::toolbar_menu(ed, EditorMenu::Position),
        mtext_menu::toolbar_menu(ed, EditorMenu::Background),
        mtext_menu::toolbar_menu(ed, EditorMenu::Symbols),
        mtext_menu::toolbar_menu(ed, EditorMenu::Find),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .padding([2, 2])
    .into()
}

/// The bar under the text: what the text holds, the box width, and the
/// editor's actions. Cancel discards, as Escape does; Apply saves and keeps
/// editing; Close Text Editor saves and closes.
/// `width` is the editor's own: shrink while the window measures its natural
/// size, fill once laid out, so the bar never asks for the whole window.
pub(super) fn status_bar(ed: &MTextEditorState, width: Length) -> Element<'static, Message> {
    let counts = crate::tr!(
        "mtext-toolbar",
        "counts",
        characters = ed.visible_character_count(),
        paragraphs = ed.doc.paragraphs.len().max(1)
    );
    let box_width = tip(
        row![
            text(label!("box-width")).size(11),
            text_input("", &ed.rect_width_field())
                .on_input(Message::MTextRectWidthField)
                .width(90)
                .padding([3, 5])
                .size(11),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
        label!("tip-box"),
    );
    let action = |name: String, message: Message, primary: bool| {
        button(text(name).size(12))
            .on_press(message)
            .padding([5, 14])
            .style(if primary {
                button::primary
            } else {
                button::secondary
            })
    };
    container(
        row![
            text(counts).size(11),
            separator(),
            box_width,
            Space::new().width(width),
            action(t!("Cancel").into_owned(), Message::MTextCancel, false),
            action(t!("Apply").into_owned(), Message::MTextApply, false),
            action(t!("Close Text Editor").into_owned(), Message::MTextOk, true),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([5, 8])
    .width(width)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        ..Default::default()
    })
    .into()
}
