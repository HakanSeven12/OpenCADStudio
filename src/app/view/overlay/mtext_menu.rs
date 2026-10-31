//! MTEXT editor menus: one popup anchored under each second-row trigger, a
//! headed panel of rows. Whether a menu is open is editor state, so typing in
//! one of its fields or a redraw never closes it. An action row closes the
//! menu and then acts; a field or a toggle leaves it open.
use super::*;
use crate::app::mtext_editor::{
    EditorMenu, FractionPart, JustifyChoice, ListKind, MTextEditorState, MTextFmt, ParaNumber,
    Script,
};
use codec::entities::mtext_format::StackingType;
use iced::widget::column;
pub(super) mod popup;

macro_rules! label {
    ($id:literal) => {
        crate::tr!("mtext-toolbar", $id)
    };
}

const ROW_H: f32 = 24.0;
const HEADER_H: f32 = 25.0;
const SEP_H: f32 = 9.0;
const PANEL_PAD: f32 = 4.0;

/// A press outside the open popup closes it; the editor routes the rest.
fn close() -> Message {
    Message::MTextMenu(None)
}

/// Readable but clearly secondary: the panel text at reduced opacity.
fn dim(theme: &Theme) -> Color {
    theme.palette().background.base.text.scale_alpha(0.45)
}

fn item_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(palette.primary.weak.color.into()),
            palette.primary.weak.text,
        ),
        button::Status::Disabled => (None, dim(theme)),
        button::Status::Active => (None, palette.background.base.text),
    };
    button::Style {
        background,
        text_color,
        border: Border::default().rounded(3),
        ..Default::default()
    }
}

/// A menu row: check/glyph column, then the label.
fn row_button(mark: &str, name: String, on_press: Message) -> Element<'static, Message> {
    button(
        row![
            text(mark.to_string()).size(12).width(22),
            text(name)
                .size(11)
                .width(Fill)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .height(Fill),
    )
    .on_press(on_press)
    .padding([0, 6])
    .width(Fill)
    .height(ROW_H)
    .style(item_style)
    .into()
}

/// An action: the menu closes, then `message` runs.
fn item(mark: &str, name: String, message: Message) -> Element<'static, Message> {
    row_button(mark, name, Message::MTextMenuPick(Box::new(message)))
}

fn check(on: bool) -> &'static str {
    if on {
        "✓"
    } else {
        ""
    }
}

/// A labelled number field; the menu stays open while it is typed in.
fn field(
    name: String,
    value: &str,
    on: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    row![
        text(name).size(11).width(Fill),
        text_input("0", value)
            .on_input(on)
            .size(12)
            .padding(2)
            .width(64),
    ]
    .spacing(6)
    .padding([0, 6])
    .align_y(iced::Alignment::Center)
    .height(ROW_H)
    .into()
}

/// The rows of one panel under a header.
struct Panel {
    title: String,
    width: f32,
    rows: Vec<Element<'static, Message>>,
}

impl Panel {
    fn new(title: String, width: f32) -> Self {
        Self {
            title,
            width,
            rows: Vec::new(),
        }
    }

    fn push(mut self, row: Element<'static, Message>) -> Self {
        self.rows.push(row);
        self
    }

    fn separator(mut self) -> Self {
        self.rows.push(
            container(iced::widget::rule::horizontal(1))
                .height(SEP_H)
                .center_y(SEP_H)
                .into(),
        );
        self
    }

    fn build(self) -> Element<'static, Message> {
        let header = container(text(self.title).size(12))
            .height(HEADER_H)
            .padding([0, 6])
            .center_y(HEADER_H);
        container(column![header, column(self.rows).spacing(0).width(Fill)])
            .width(self.width)
            .padding(PANEL_PAD)
            .style(crate::ui::color_select::popup_panel_style)
            .into()
    }
}

fn trigger_style(open: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = theme.palette();
        let pair = if open {
            palette.primary.weak
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            palette.background.strong
        } else {
            palette.background.weak
        };
        button::Style {
            background: Some(pair.color.into()),
            text_color: pair.text,
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    }
}

fn title(menu: EditorMenu) -> String {
    match menu {
        EditorMenu::More => label!("more"),
        EditorMenu::Paragraph => label!("paragraph"),
        EditorMenu::Columns => label!("columns"),
        EditorMenu::Justify => label!("justification"),
        EditorMenu::Symbols => label!("symbols"),
        EditorMenu::Find => label!("find"),
        EditorMenu::Fraction => label!("panel-fraction"),
    }
}

/// A second-row menu: its trigger, and its panel while it is open.
pub(super) fn toolbar_menu(ed: &MTextEditorState, menu: EditorMenu) -> Element<'static, Message> {
    // The Fraction panel opens from More formatting, under its trigger.
    let shown = ed.menu.map(|m| {
        if m == EditorMenu::Fraction {
            EditorMenu::More
        } else {
            m
        }
    });
    let open = shown == Some(menu);
    let base = button(
        row![text(title(menu)).size(11), text("▾").size(11)]
            .spacing(5)
            .align_y(iced::Alignment::Center),
    )
    .on_press(Message::MTextMenu(Some(menu)))
    .padding([4, 8])
    .style(trigger_style(open));
    let panel = open.then(|| match ed.menu.unwrap_or(menu) {
        EditorMenu::Fraction => fraction_panel(ed),
        EditorMenu::More => more_panel(ed),
        EditorMenu::Paragraph => paragraph_panel(ed),
        EditorMenu::Columns => columns_panel(ed),
        EditorMenu::Justify => justify_panel(ed),
        EditorMenu::Symbols => symbols_panel(),
        EditorMenu::Find => find_panel(ed),
    });
    popup::anchored(base, panel, close())
}

/// Character formatting the first row has no room for.
fn more_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    Panel::new(title(EditorMenu::More), 240.0)
        .push(item(
            if ed.script_active(Script::Super) {
                "✓"
            } else {
                "x²"
            },
            label!("superscript"),
            Message::MTextScript(true),
        ))
        .push(item(
            if ed.script_active(Script::Sub) {
                "✓"
            } else {
                "x₂"
            },
            label!("subscript"),
            Message::MTextScript(false),
        ))
        .push(item("½", label!("stack"), Message::MTextStack))
        .push(row_button(
            "",
            label!("fraction"),
            Message::MTextFractionOpen,
        ))
        .separator()
        .push(field(
            label!("oblique"),
            ed.span_field("oblique", &ed.oblique),
            Message::MTextOblique,
        ))
        .push(field(
            label!("tracking"),
            ed.span_field("tracking", &ed.char_space),
            Message::MTextCharSpace,
        ))
        .separator()
        .push(item(
            "AA",
            label!("uppercase"),
            Message::MTextFmt(MTextFmt::Uppercase),
        ))
        .push(item(
            "aa",
            label!("lowercase"),
            Message::MTextFmt(MTextFmt::Lowercase),
        ))
        .separator()
        .push(item("", label!("clear"), Message::MTextClearFormatting))
        .build()
}

fn paragraph_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    let list = ed.list_kind();
    let exact = ed.line_spacing_style == codec::entities::LineSpacingStyle::Exactly;
    let factor = ed
        .line_spacing_draft
        .clone()
        .unwrap_or_else(|| format!("{}", ed.line_spacing));
    let mut panel = Panel::new(title(EditorMenu::Paragraph), 260.0)
        .push(item(
            if list == Some(ListKind::Bullet) {
                "✓"
            } else {
                "•"
            },
            label!("bullets"),
            Message::MTextList(Some(ListKind::Bullet)),
        ))
        .push(item(
            if list == Some(ListKind::Number) {
                "✓"
            } else {
                "1."
            },
            label!("numbering"),
            Message::MTextList(Some(ListKind::Number)),
        ))
        .push(item(
            check(list.is_none()),
            label!("no-list"),
            Message::MTextList(None),
        ))
        .push(item("⇥", label!("tab"), Message::MTextTab))
        .separator();
    for (factor, name) in [(1.0, "1"), (1.5, "1.5"), (2.0, "2")] {
        panel = panel.push(item(
            check((ed.line_spacing - factor).abs() < 1e-3),
            format!("{} {name}", label!("line-spacing")),
            Message::MTextLineSpacing(factor),
        ));
    }
    let panel = panel
        .push(field(
            label!("spacing-factor"),
            &factor,
            Message::MTextLineSpacingInput,
        ))
        .push(row_button(
            check(!exact),
            label!("at-least"),
            Message::MTextLineSpacingMode(false),
        ))
        .push(row_button(
            check(exact),
            label!("exactly"),
            Message::MTextLineSpacingMode(true),
        ));
    let number = |name: String, value: &str, kind: ParaNumber| {
        field(name, value, move |v| Message::MTextParagraphNumber(kind, v))
    };
    panel
        .separator()
        .push(number(
            label!("first-indent"),
            &ed.paragraph_first_indent,
            ParaNumber::FirstIndent,
        ))
        .push(number(
            label!("left-indent"),
            &ed.paragraph_left_indent,
            ParaNumber::LeftIndent,
        ))
        .push(number(
            label!("right-indent"),
            &ed.paragraph_right_indent,
            ParaNumber::RightIndent,
        ))
        .push(number(
            label!("space-before"),
            &ed.paragraph_space_before,
            ParaNumber::SpaceBefore,
        ))
        .push(number(
            label!("space-after"),
            &ed.paragraph_space_after,
            ParaNumber::SpaceAfter,
        ))
        .build()
}

fn columns_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    let current = match (ed.column_type, ed.column_auto_height) {
        (1, _) => "Static",
        (2, true) => "Dynamic auto",
        (2, false) => "Dynamic manual",
        _ => "No columns",
    };
    let mut panel = Panel::new(title(EditorMenu::Columns), 250.0);
    for (mode, name) in [
        ("No columns", label!("no-columns")),
        ("Static", label!("static")),
        ("Dynamic auto", label!("dynamic-auto")),
        ("Dynamic manual", label!("dynamic-manual")),
    ] {
        panel = panel.push(item(
            check(mode == current),
            name,
            Message::MTextColumnMode(mode.into()),
        ));
    }
    panel
        .separator()
        .push(field(
            label!("count"),
            &ed.column_count,
            Message::MTextColumnCount,
        ))
        .push(field(
            label!("height"),
            &ed.rect_height,
            Message::MTextColumnHeight,
        ))
        .push(field(
            label!("width"),
            &ed.column_width,
            Message::MTextColumnWidth,
        ))
        .push(field(
            label!("gutter"),
            &ed.column_gutter,
            Message::MTextColumnGutter,
        ))
        .push(row_button(
            check(ed.column_flow_reversed),
            label!("reverse"),
            Message::MTextColumnFlowReversed(!ed.column_flow_reversed),
        ))
        .build()
}

/// The attachment point the text is placed and grows from.
fn justify_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    JustifyChoice::ALL
        .into_iter()
        .fold(
            Panel::new(title(EditorMenu::Justify), 190.0),
            |panel, choice| {
                panel.push(item(
                    check(choice.0 == ed.attachment),
                    t!(choice.to_string()).into_owned(),
                    Message::MTextJustify(choice.0),
                ))
            },
        )
        .build()
}

pub(super) fn symbols() -> Vec<(&'static str, String)> {
    vec![
        ("⌀", label!("diameter")),
        ("°", label!("degree")),
        ("±", label!("plus-minus")),
        ("²", label!("squared")),
        ("³", label!("cubed")),
        ("×", label!("multiply")),
        ("÷", label!("divide")),
        ("≤", label!("less-equal")),
        ("≥", label!("greater-equal")),
        ("≈", label!("approximately")),
    ]
}

fn symbols_panel() -> Element<'static, Message> {
    symbols()
        .into_iter()
        .fold(
            Panel::new(title(EditorMenu::Symbols), 220.0),
            |panel, (symbol, name)| {
                panel.push(item(symbol, name, Message::MTextInsert(symbol.into())))
            },
        )
        .build()
}

fn find_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    let input =
        |name: String, value: &str, on: fn(String) -> Message| -> Element<'static, Message> {
            row![
                text(name.clone()).size(11).width(90),
                text_input(&name, value)
                    .on_input(on)
                    .size(12)
                    .padding(2)
                    .width(Fill),
            ]
            .spacing(6)
            .padding([0, 6])
            .align_y(iced::Alignment::Center)
            .height(ROW_H + 4.0)
            .into()
        };
    let action = |name: String, message: Message| {
        button(text(name).size(11))
            .on_press(message)
            .padding([3, 8])
            .style(button::secondary)
    };
    Panel::new(title(EditorMenu::Find), 320.0)
        .push(input(
            label!("find-what"),
            &ed.find_text,
            Message::MTextFindText,
        ))
        .push(input(
            label!("replace-with"),
            &ed.replace_text,
            Message::MTextReplaceText,
        ))
        .separator()
        .push(
            row![
                Space::new().width(Fill),
                action(label!("find-next"), Message::MTextFindNext),
                action(label!("replace"), Message::MTextReplaceNext),
                action(label!("replace-all"), Message::MTextReplaceAll),
            ]
            .spacing(6)
            .padding([0, 6])
            .align_y(iced::Alignment::Center)
            .height(ROW_H + 6.0)
            .into(),
        )
        .build()
}

/// Numerator, denominator and stacking form; Insert writes one `\S` stack
/// over the selection (or at the caret), or replaces the selected one.
fn fraction_panel(ed: &MTextEditorState) -> Element<'static, Message> {
    let draft = ed.fraction.clone().unwrap_or_else(|| ed.fraction_draft());
    let part = |name: String, part: FractionPart, value: String| -> Element<'static, Message> {
        row![
            text(name).size(11).width(Fill),
            text_input("", &value)
                .on_input(move |v| Message::MTextFractionPart(part, v))
                .on_submit(Message::MTextFractionInsert)
                .size(12)
                .padding(2)
                .width(96),
        ]
        .spacing(6)
        .padding([0, 6])
        .align_y(iced::Alignment::Center)
        .height(ROW_H)
        .into()
    };
    let kind = |kind: StackingType, name: String| {
        row_button(
            check(draft.kind == kind),
            name,
            Message::MTextFractionKind(kind),
        )
    };
    let buttons = row![
        Space::new().width(Fill),
        button(text(label!("cancel")).size(11))
            .on_press(Message::MTextFractionOpen)
            .padding([3, 10])
            .style(button::secondary),
        button(text(label!("insert")).size(11))
            .on_press(Message::MTextFractionInsert)
            .padding([3, 10])
            .style(button::primary),
    ]
    .spacing(6)
    .padding([0, 6])
    .align_y(iced::Alignment::Center)
    .height(ROW_H + 6.0);
    Panel::new(title(EditorMenu::Fraction), 240.0)
        .push(part(
            label!("numerator"),
            FractionPart::Numerator,
            draft.numerator.clone(),
        ))
        .push(part(
            label!("denominator"),
            FractionPart::Denominator,
            draft.denominator.clone(),
        ))
        .separator()
        .push(kind(StackingType::Horizontal, label!("stack-horizontal")))
        .push(kind(StackingType::Diagonal, label!("stack-diagonal")))
        .push(kind(StackingType::Limit, label!("stack-tolerance")))
        .separator()
        .push(buttons.into())
        .build()
}
