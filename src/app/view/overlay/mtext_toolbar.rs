//! The MTEXT toolbar's second row: character formatting, undo, paragraph
//! alignment and the menus that hold the rest of the editor's commands.
use super::*;
use crate::app::mtext_editor::{EditorMenu, MTextEditorState, MTextFmt, ParaAlign};

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
    row![
        icon(
            include_bytes!("../../../../assets/icons/mt_bold.svg"),
            Message::MTextFmt(MTextFmt::Bold),
            label!("bold")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_italic.svg"),
            Message::MTextFmt(MTextFmt::Italic),
            label!("italic")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_underline.svg"),
            Message::MTextFmt(MTextFmt::Underline),
            label!("underline")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_overline.svg"),
            Message::MTextFmt(MTextFmt::Overline),
            label!("overline")
        ),
        icon(
            include_bytes!("../../../../assets/icons/mt_strike.svg"),
            Message::MTextFmt(MTextFmt::Strike),
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
        mtext_menu::toolbar_menu(ed, EditorMenu::Symbols),
        mtext_menu::toolbar_menu(ed, EditorMenu::Find),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .padding([2, 2])
    .into()
}
