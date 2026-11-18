//! The windows the Text Editor tab opens on the text being edited: the
//! paragraph settings (tabs, indents, alignment, spacing, line spacing), the
//! column settings, the background mask and find and replace. Each holds a
//! draft; OK writes it to the editor in one step, Cancel drops it.

use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip};

/// A tab stop's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Left,
    Center,
    Right,
    Decimal,
}

/// How the line spacing of a paragraph is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMode {
    Exactly,
    AtLeast,
    Multiple,
}

/// The paragraph alignments the window offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justified,
    Distributed,
}

#[derive(Debug, Clone)]
pub struct ParagraphDraft {
    pub tab_kind: TabKind,
    pub tab_position: String,
    pub tabs: Vec<(TabKind, f64)>,
    pub selected_tab: Option<usize>,
    /// The decimal separator decimal tabs line up on.
    pub decimal: char,
    pub first_line: String,
    pub hanging: String,
    pub right: String,
    pub align_on: bool,
    pub align: Align,
    pub spacing_on: bool,
    pub before: String,
    pub after: String,
    pub line_on: bool,
    pub line_mode: LineMode,
    pub line_at: String,
}

/// The kind of columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    DynamicAuto,
    DynamicManual,
    Static,
    None,
}

#[derive(Debug, Clone)]
pub struct ColumnsDraft {
    pub kind: ColumnKind,
    pub number: String,
    pub height: String,
    pub width: String,
    pub gutter: String,
}

impl ColumnsDraft {
    /// The total width: the columns and the gutters between them.
    pub fn total(&self) -> String {
        let n = self.number.trim().parse::<f64>().unwrap_or(1.0).max(1.0);
        let width = self.width.trim().parse::<f64>().unwrap_or(0.0);
        let gutter = self.gutter.trim().parse::<f64>().unwrap_or(0.0);
        crate::entities::common::format_length(n * width + (n - 1.0) * gutter)
    }
}

#[derive(Debug, Clone)]
pub struct MaskDraft {
    pub on: bool,
    pub factor: String,
    pub background: bool,
    pub color: document::types::Color,
}

#[derive(Debug, Clone, Default)]
pub struct FindDraft {
    pub find: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_words: bool,
    pub wildcards: bool,
    pub diacritics: bool,
    pub half_full: bool,
    /// The message box after a search: "Finished searching." or the
    /// replacement count. The buttons wait for its OK.
    pub notice: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MTextDialog {
    Paragraph(ParagraphDraft),
    Columns(ColumnsDraft),
    Mask(MaskDraft),
    Find(FindDraft),
    Charset(super::mtext_tool_dialogs::CharsetDraft),
    StyleChange(super::mtext_tool_dialogs::StyleChangeDraft),
    Dictionaries(super::mtext_tool_dialogs::DictionariesDraft),
    SpellSettings(super::mtext_tool_dialogs::SpellSettingsDraft),
    Highlight(super::mtext_tool_dialogs::HighlightDraft),
    Stack(super::mtext_tool_dialogs::StackDraft),
    AutoStack(super::mtext_tool_dialogs::AutoStackDraft),
    ListsToggle(super::mtext_tool_dialogs::ListsToggleDraft),
}

#[derive(Debug, Clone)]
pub enum MTextDialogMsg {
    TabKind(TabKind),
    TabPosition(String),
    TabAdd,
    TabModify,
    TabRemove,
    TabSelect(usize),
    Decimal(char),
    FirstLine(String),
    Hanging(String),
    Right(String),
    AlignOn(bool),
    Align(Align),
    SpacingOn(bool),
    Before(String),
    After(String),
    LineOn(bool),
    LineMode(LineMode),
    LineAt(String),
    ColumnKind(ColumnKind),
    ColumnNumber(String),
    ColumnHeight(String),
    ColumnWidth(String),
    ColumnGutter(String),
    MaskOn(bool),
    MaskFactor(String),
    MaskBackground(bool),
    MaskColor(document::types::Color),
    MaskMoreColor,
    Find(String),
    Replace(String),
    MatchCase(bool),
    WholeWords(bool),
    Wildcards(bool),
    Diacritics(bool),
    HalfFull(bool),
    /// OK on the search message.
    DismissNotice,
    FindNext,
    ReplaceOne,
    ReplaceAll,
    Charset(u8),
    StyleAlways(bool),
    StyleYes,
    StyleNo,
    ListsAlways(bool),
    ListsContinue,
    DictLanguage(String),
    DictSource(Option<usize>),
    DictWord(String),
    DictAdd,
    DictDelete,
    DictSelect(usize),
    DictImport,
    DictImported(Vec<String>),
    DictManage(bool),
    DictManageSelect(usize),
    DictNewFile,
    DictAddFile,
    DictFilePicked(String, bool),
    DictRemoveFile,
    DictHelp,
    SpellOption(super::spell_dialog::SpellSetting, bool),
    HighlightR(String),
    HighlightG(String),
    HighlightB(String),
    HighlightTheme,
    StackUpper(String),
    StackLower(String),
    StackStyleSet(super::mtext_tool_dialogs::StackStyle),
    StackPositionSet(super::mtext_tool_dialogs::StackPosition),
    StackSize(String),
    /// Defaults: save the window's style, position and size (true) or
    /// restore the saved ones (false).
    StackDefaults(bool),
    AutoStackOpen,
    AutoEnabled(bool),
    AutoDiagonal(bool),
    AutoTrim(bool),
    /// Back from AutoStack to the Stack Properties window it came from.
    AutoBack,
    Ok,
}

fn msg(message: MTextDialogMsg) -> Message {
    Message::MTextDialog(message)
}

fn input<'a>(value: &str, on: Option<fn(String) -> MTextDialogMsg>) -> Element<'a, Message> {
    let mut field = text_input("", value)
        .size(11)
        .padding([4, 8])
        .width(Length::Fixed(96.0))
        .style(field_style);
    if let Some(on) = on {
        field = field.on_input(move |v| msg(on(v)));
    }
    field.into()
}

fn labelled<'a>(label: String, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(label).size(11).width(Length::Fixed(110.0)),
        control.into()
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Joined buttons, one choice out of a few.
fn segmented<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, String)>,
    current: T,
    enabled: bool,
    on: fn(T) -> MTextDialogMsg,
) -> Element<'a, Message> {
    let mut line = row![].spacing(2);
    for (value, label) in options {
        let on_press = enabled.then(|| msg(on(value)));
        line = line.push(
            button(text(label).size(11))
                .on_press_maybe(on_press)
                .style(button_style(value == current))
                .padding([3, 9]),
        );
    }
    line.into()
}

fn footer<'a>(ok: bool) -> Element<'a, Message> {
    row![
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
        dialog_button_styled_opt(
            crate::t!("OK"),
            ok.then(|| msg(MTextDialogMsg::Ok)),
            button::primary
        ),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn t(key: &str) -> String {
    crate::t!(key).into_owned()
}

/// The paragraph window.
pub fn view_paragraph<'a>(
    d: &'a ParagraphDraft,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use MTextDialogMsg as M;
    let kinds = vec![
        (TabKind::Left, t("Left")),
        (TabKind::Center, t("Center")),
        (TabKind::Right, t("Right")),
        (TabKind::Decimal, t("Decimal")),
    ];
    let mut tab_list = column![].spacing(2);
    for (n, (kind, position)) in d.tabs.iter().enumerate() {
        let name = kinds
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        tab_list = tab_list.push(
            button(
                row![
                    text(crate::entities::common::format_length(*position)).size(11),
                    Space::new().width(Fill),
                    text(name).size(11).style(muted_style),
                ]
                .align_y(iced::Center),
            )
            .on_press(msg(M::TabSelect(n)))
            .style(button_style(d.selected_tab == Some(n)))
            .padding([3, 8])
            .width(Fill),
        );
    }
    if d.tabs.is_empty() {
        tab_list = tab_list.push(text(t("No tab stops")).size(11).style(muted_style));
    }
    let picked = d.selected_tab.is_some();
    let separators = vec![
        ('.', t("'.' period")),
        (',', t("',' comma")),
        (' ', t("' ' space")),
    ];
    let tabs = card(
        t("Tab stops"),
        column![
            segmented(kinds, d.tab_kind, true, M::TabKind),
            row![
                input(&d.tab_position, Some(M::TabPosition)),
                button(text(t("Add")).size(11))
                    .on_press(msg(M::TabAdd))
                    .style(button_style(false))
                    .padding([4, 10]),
                button(text(t("Modify")).size(11))
                    .on_press_maybe(picked.then(|| msg(M::TabModify)))
                    .style(button_style(false))
                    .padding([4, 10]),
                button(text(t("Remove")).size(11))
                    .on_press_maybe(picked.then(|| msg(M::TabRemove)))
                    .style(button_style(false))
                    .padding([4, 10]),
            ]
            .spacing(6)
            .align_y(iced::Center),
            container(tab_list).height(Length::Fixed(72.0)).width(Fill),
            labelled(
                t("Decimal style"),
                segmented(separators, d.decimal, d.tab_kind == TabKind::Decimal, M::Decimal)
            ),
        ]
        .spacing(8),
    );
    let indents = card(
        t("Indents"),
        column![
            labelled(t("First line"), input(&d.first_line, Some(M::FirstLine))),
            labelled(t("Hanging"), input(&d.hanging, Some(M::Hanging))),
            labelled(t("Right"), input(&d.right, Some(M::Right))),
        ]
        .spacing(6),
    );
    let aligns = vec![
        (Align::Left, t("Left")),
        (Align::Center, t("Center")),
        (Align::Right, t("Right")),
        (Align::Justified, t("Justified")),
        (Align::Distributed, t("Distributed")),
    ];
    let alignment = card(
        t("Paragraph alignment"),
        column![
            chip(t("Set the alignment"), d.align_on, msg(M::AlignOn(!d.align_on))),
            segmented(aligns, d.align, d.align_on, M::Align),
        ]
        .spacing(6),
    );
    let spacing_input = |value: &str, on: fn(String) -> MTextDialogMsg, active: bool| {
        input(value, active.then_some(on))
    };
    let spacing = card(
        t("Paragraph spacing"),
        column![
            chip(t("Set the spacing"), d.spacing_on, msg(M::SpacingOn(!d.spacing_on))),
            labelled(t("Before"), spacing_input(&d.before, M::Before, d.spacing_on)),
            labelled(t("After"), spacing_input(&d.after, M::After, d.spacing_on)),
        ]
        .spacing(6),
    );
    let modes = vec![
        (LineMode::Exactly, t("Exactly")),
        (LineMode::AtLeast, t("At least")),
        (LineMode::Multiple, t("Multiple")),
    ];
    let line = card(
        t("Paragraph line spacing"),
        column![
            chip(t("Set the line spacing"), d.line_on, msg(M::LineOn(!d.line_on))),
            segmented(modes, d.line_mode, d.line_on, M::LineMode),
            labelled(t("At"), spacing_input(&d.line_at, M::LineAt, d.line_on)),
        ]
        .spacing(6),
    );
    column![
        row![tabs, indents].spacing(10),
        alignment,
        row![spacing, line].spacing(10),
        footer(true),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

/// The column settings window.
pub fn view_columns<'a>(
    d: &'a ColumnsDraft,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use MTextDialogMsg as M;
    let kinds = vec![
        (ColumnKind::DynamicAuto, t("Dynamic, auto height")),
        (ColumnKind::DynamicManual, t("Dynamic, manual height")),
        (ColumnKind::Static, t("Static")),
        (ColumnKind::None, t("No columns")),
    ];
    let columns = d.kind != ColumnKind::None;
    let number = d.kind == ColumnKind::Static;
    let height = matches!(d.kind, ColumnKind::DynamicManual | ColumnKind::Static);
    let mut kind_list = column![].spacing(2);
    for (kind, label) in kinds {
        kind_list = kind_list.push(
            button(text(label).size(11))
                .on_press(msg(M::ColumnKind(kind)))
                .style(button_style(kind == d.kind))
                .padding([4, 10])
                .width(Fill),
        );
    }
    let kind = card(t("Column type"), kind_list);
    let size = card(
        t("Size"),
        column![
            labelled(t("Columns"), input(&d.number, number.then_some(M::ColumnNumber as fn(String) -> MTextDialogMsg))),
            labelled(t("Height"), input(&d.height, height.then_some(M::ColumnHeight as fn(String) -> MTextDialogMsg))),
            labelled(t("Column width"), input(&d.width, columns.then_some(M::ColumnWidth as fn(String) -> MTextDialogMsg))),
            labelled(t("Gutter"), input(&d.gutter, columns.then_some(M::ColumnGutter as fn(String) -> MTextDialogMsg))),
            labelled(
                t("Total width"),
                text(if columns { d.total() } else { String::new() })
                    .size(11)
                    .style(muted_style)
            ),
        ]
        .spacing(6),
    );
    column![row![kind, size].spacing(10), footer(true)]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

/// The background mask window.
pub fn view_mask<'a>(
    d: &'a MaskDraft,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use MTextDialogMsg as M;
    let colors: Vec<(document::types::Color, String)> = [
        (1u8, "Red"),
        (2, "Yellow"),
        (3, "Green"),
        (4, "Cyan"),
        (5, "Blue"),
        (6, "Magenta"),
        (7, "White"),
    ]
    .into_iter()
    .map(|(index, name)| (document::types::Color::Index(index), t(name)))
    .collect();
    let names: Vec<String> = colors.iter().map(|(_, name)| name.clone()).collect();
    let current = colors
        .iter()
        .find(|(color, _)| *color == d.color)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| crate::ui::properties::acad_color_display(d.color).1.to_string());
    let fill_on = d.on && !d.background;
    let color_list = pick_list(Some(current), names, |name| name.clone())
        .on_select(move |name| {
            let color = colors
                .iter()
                .find(|(_, n)| *n == name)
                .map(|(c, _)| *c)
                .unwrap_or(document::types::Color::Index(1));
            msg(M::MaskColor(color))
        })
        .text_size(11)
        .padding([3, 6])
        .width(Length::Fixed(140.0));
    let color: Element<'a, Message> = if fill_on {
        color_list.into()
    } else {
        text(crate::ui::properties::acad_color_display(d.color).1.to_string())
            .size(11)
            .style(muted_style)
            .into()
    };
    let mask = card(
        t("Background mask"),
        column![
            chip(t("Use background mask"), d.on, msg(M::MaskOn(!d.on))),
            labelled(
                t("Border offset factor"),
                input(&d.factor, d.on.then_some(M::MaskFactor as fn(String) -> MTextDialogMsg))
            ),
        ]
        .spacing(6),
    );
    let fill = card(
        t("Fill color"),
        column![
            chip(
                t("Use drawing background color"),
                d.background,
                if d.on {
                    msg(M::MaskBackground(!d.background))
                } else {
                    Message::Noop
                }
            ),
            row![
                color,
                button(text(t("Select Color...")).size(11))
                    .on_press_maybe(fill_on.then(|| msg(M::MaskMoreColor)))
                    .style(button_style(false))
                    .padding([4, 10]),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(6),
    );
    // A fixed width: the window is measured from it.
    let _ = sizing;
    column![mask, fill, footer(true)]
        .spacing(10)
        .padding([10, 12])
        .width(Length::Fixed(430.0))
        .into()
}

/// The find and replace window.
pub fn view_find<'a>(
    d: &'a FindDraft,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use MTextDialogMsg as M;
    let field = |value: &str, on: fn(String) -> MTextDialogMsg| {
        text_input("", value)
            .size(11)
            .padding([4, 8])
            .width(Fill)
            .style(field_style)
            .on_input(move |v| msg(on(v)))
    };
    let search = !d.find.is_empty() && d.notice.is_none();
    let action = |label: String, message: MTextDialogMsg| {
        button(text(label).size(11))
            .on_press_maybe(search.then(|| msg(message)))
            .style(button_style(false))
            .padding([4, 10])
    };
    let texts = card(
        t("Text"),
        column![
            labelled(t("Find what"), field(&d.find, M::Find)),
            labelled(t("Replace with"), field(&d.replace, M::Replace)),
        ]
        .spacing(6),
    );
    let options = card(
        t("Options"),
        column![
            chip(t("Match case"), d.match_case, msg(M::MatchCase(!d.match_case))),
            chip(t("Find whole words only"), d.whole_words, msg(M::WholeWords(!d.whole_words))),
            chip(t("Use wildcards"), d.wildcards, msg(M::Wildcards(!d.wildcards))),
            chip(t("Match diacritics"), d.diacritics, msg(M::Diacritics(!d.diacritics))),
            chip(
                t("Match half/full width forms (East Asian languages)"),
                d.half_full,
                msg(M::HalfFull(!d.half_full))
            ),
        ]
        .spacing(4),
    );
    let notice: Element<'a, Message> = match &d.notice {
        Some(notice) => container(
            row![
                crate::ui::icons::semantic(crate::ui::icons::INFO, 14.0),
                text(notice.clone()).size(11),
                Space::new().width(Fill),
                button(text(crate::t!("OK")).size(11))
                    .on_press(msg(M::DismissNotice))
                    .style(button_style(true))
                    .padding([4, 14]),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([6, 10])
        .width(Fill)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(
                theme.palette().primary.base.color.scale_alpha(0.14),
            )),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into(),
        // The message's room is kept: the window does not grow for it.
        None => Space::new().height(Length::Fixed(50.0)).into(),
    };
    column![
        texts,
        options,
        notice,
        row![
            Space::new().width(Fill),
            action(t("Find Next"), M::FindNext),
            action(t("Replace"), M::ReplaceOne),
            action(t("Replace All"), M::ReplaceAll),
            dialog_button_styled_opt(crate::t!("Close"), Some(Message::CloseModal), button::secondary),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
