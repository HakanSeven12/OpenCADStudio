//! More windows the Text Editor tab opens on the text being edited: the
//! character set, the text style change question, the dictionaries and the
//! spelling settings, the selection highlight colour, and the stack and
//! AutoStack properties. Drafts as in `mtext_dialogs`; OK writes them.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::mtext_dialogs::MTextDialogMsg as M;
use crate::ui::window::pdf_dialogs::{card, chip};
use crate::ui::window::spell_dialog::{SpellOptions, SpellSetting};

pub use crate::app::mtext_prefs::{StackPosition, StackStyle};

/// The character sets the editor offers, with their Windows charset codes.
pub const CHARSETS: [(u8, &str); 14] = [
    (238, "Central Europe"),
    (204, "Cyrillic"),
    (177, "Hebrew"),
    (178, "Arabic"),
    (186, "Baltic"),
    (161, "Greek"),
    (162, "Turkish"),
    (163, "Vietnamese"),
    (128, "Japanese"),
    (129, "Korean"),
    (134, "CHINESE_GB2312"),
    (136, "CHINESE_BIG5"),
    (0, "Western"),
    (222, "Thai"),
];

#[derive(Debug, Clone)]
pub struct CharsetDraft {
    pub charset: u8,
}

/// The question asked when a style is picked with nothing selected.
#[derive(Debug, Clone)]
pub struct StyleChangeDraft {
    pub style: String,
    pub always: bool,
}

/// The question asked before Allow Bullets and Lists is turned on or off.
#[derive(Debug, Clone, Default)]
pub struct ListsToggleDraft {
    pub always: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DictionariesDraft {
    /// (tag, name) of each main dictionary offered, and the current tag.
    pub languages: Vec<(String, String)>,
    pub language: String,
    /// The custom dictionary files, and the one in use (None: the profile's
    /// own word list).
    pub files: Vec<String>,
    pub current: Option<usize>,
    /// The words of the custom dictionary in use.
    pub words: Vec<String>,
    pub word: String,
    pub selected: Option<usize>,
    /// The custom dictionary list is open for New / Add / Remove.
    pub manage: bool,
    pub manage_selected: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SpellSettingsDraft {
    pub options: SpellOptions,
}

#[derive(Debug, Clone)]
pub struct HighlightDraft {
    pub r: String,
    pub g: String,
    pub b: String,
}

impl HighlightDraft {
    pub fn rgb(&self) -> Option<[u8; 3]> {
        Some([
            self.r.trim().parse().ok()?,
            self.g.trim().parse().ok()?,
            self.b.trim().parse().ok()?,
        ])
    }
}

#[derive(Debug, Clone)]
pub struct StackDraft {
    pub upper: String,
    pub lower: String,
    pub style: StackStyle,
    pub position: StackPosition,
    pub size: String,
    /// The visible range of the stack being changed.
    pub range: (usize, usize),
}

impl StackDraft {
    /// The text size as a percentage, 1–100.
    pub fn percent(&self) -> Option<u8> {
        self.size
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| (1.0..=100.0).contains(v))
            .map(|v| v.round() as u8)
    }
}

#[derive(Debug, Clone)]
pub struct AutoStackDraft {
    pub enabled: bool,
    pub diagonal: bool,
    pub trim: bool,
    /// The Stack Properties window this one was opened from.
    pub back: Option<StackDraft>,
}

fn msg(message: M) -> Message {
    Message::MTextDialog(message)
}

fn t(key: &str) -> String {
    crate::t!(key).into_owned()
}

fn footer<'a>(ok: Message, cancel: Message) -> Element<'a, Message> {
    row![
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(cancel), button::secondary),
        dialog_button_styled_opt(crate::t!("OK"), Some(ok), button::primary),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn small_button<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(11))
        .on_press_maybe(message)
        .style(button_style(false))
        .padding([4, 10])
        .into()
}

fn number_field<'a>(label: String, value: &str, on: fn(String) -> M) -> Element<'a, Message> {
    row![
        text(label).size(11).width(Length::Fixed(90.0)),
        text_input("", value)
            .size(11)
            .padding([4, 8])
            .width(Length::Fixed(110.0))
            .style(field_style)
            .on_input(move |v| msg(on(v)))
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// One choice out of a list, as rows that light up.
fn choice_list<'a, T: Copy + PartialEq + 'a>(
    items: Vec<(T, String)>,
    current: T,
    on: fn(T) -> M,
    height: f32,
) -> Element<'a, Message> {
    let mut list = column![].spacing(1);
    for (value, label) in items {
        list = list.push(
            button(text(label).size(11))
                .on_press(msg(on(value)))
                .style(button_style(value == current))
                .width(Fill)
                .padding([3, 8]),
        );
    }
    container(scrollable(list).height(Length::Fixed(height)))
        .padding(2)
        .width(Fill)
        .into()
}

/// The character set window: the code page the selected text's TrueType
/// font is written with.
pub fn view_charset<'a>(d: &'a CharsetDraft) -> Element<'a, Message> {
    let items = CHARSETS
        .iter()
        .map(|(code, name)| (*code, t(name)))
        .collect();
    column![
        card(
            t("Character Set"),
            column![
                text(t("The code page the selected TrueType text is written with."))
                    .size(11)
                    .style(muted_style),
                choice_list(items, d.charset, M::Charset, 280.0),
            ]
            .spacing(6),
        ),
        footer(msg(M::Ok), Message::CloseModal),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(330.0))
    .into()
}

/// The question asked before a style replaces the formatting the style
/// itself sets (font, height, width, oblique) in the whole text.
pub fn view_style_change<'a>(d: &'a StyleChangeDraft) -> Element<'a, Message> {
    column![
        card(
            t("Text Style Change"),
            column![
                text(t(
                    "This change will affect all text objects using this style, not just the selection. Do you want to change the text style?"
                ))
                .size(12),
                chip(
                    t("Always change the text style"),
                    d.always,
                    msg(M::StyleAlways(!d.always))
                ),
            ]
            .spacing(8),
        ),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(crate::t!("No"), Some(msg(M::StyleNo)), button::secondary),
            dialog_button_styled_opt(crate::t!("Yes"), Some(msg(M::StyleYes)), button::primary),
        ]
        .spacing(6),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(460.0))
    .into()
}

/// Allow Bullets and Lists acts on the whole text: asked about first
/// unless "Always turn Bullets and Lists on or off".
pub fn view_lists_toggle<'a>(d: &'a ListsToggleDraft) -> Element<'a, Message> {
    column![
        card(
            t("Bullets and Lists"),
            column![
                text(t(
                    "Turning Bullets and Lists on or off changes the whole text object, not only the selection. Continue?"
                ))
                .size(12),
                chip(
                    t("Always turn Bullets and Lists on or off"),
                    d.always,
                    msg(M::ListsAlways(!d.always))
                ),
            ]
            .spacing(8),
        ),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
            dialog_button_styled_opt(t("Continue"), Some(msg(M::ListsContinue)), button::primary),
        ]
        .spacing(6),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(460.0))
    .into()
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// The dictionaries window: the main dictionary, and the words of the
/// custom dictionary in use, with the list of custom dictionaries behind
/// Manage.
pub fn view_dictionaries<'a>(d: &'a DictionariesDraft) -> Element<'a, Message> {
    let languages: Vec<String> = d.languages.iter().map(|(_, name)| name.clone()).collect();
    let current_language = d
        .languages
        .iter()
        .find(|(tag, _)| *tag == d.language)
        .map(|(_, name)| name.clone());
    let tags = d.languages.clone();
    let main = card(
        t("Main dictionary"),
        crate::ui::dropdown_list::pick_list(current_language, languages, |n| n.clone())
            .on_select(move |name| {
                let tag = tags
                    .iter()
                    .find(|(_, n)| *n == name)
                    .map(|(tag, _)| tag.clone())
                    .unwrap_or_default();
                msg(M::DictLanguage(tag))
            })
            .text_size(11)
            .width(Fill),
    );
    let mut sources: Vec<(Option<usize>, String)> =
        vec![(None, t("Profile word list"))];
    sources.extend(d.files.iter().enumerate().map(|(n, path)| (Some(n), file_name(path))));
    let mut source_row = row![].spacing(4);
    for (index, name) in sources {
        source_row = source_row.push(
            button(text(name).size(11))
                .on_press(msg(M::DictSource(index)))
                .style(button_style(index == d.current))
                .padding([3, 8]),
        );
    }
    let mut words = column![].spacing(1);
    for (n, word) in d.words.iter().enumerate() {
        words = words.push(
            button(text(word.clone()).size(11))
                .on_press(msg(M::DictSelect(n)))
                .style(button_style(d.selected == Some(n)))
                .width(Fill)
                .padding([2, 8]),
        );
    }
    let can_add = !d.word.trim().is_empty() && !d.word.contains(char::is_whitespace);
    let custom = card(
        t("Custom dictionary"),
        column![
            scrollable(source_row).direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new().width(3).scroller_width(3)
            )),
            row![
                text_input(&t("Add word here"), &d.word)
                    .size(11)
                    .padding([4, 8])
                    .style(field_style)
                    .on_input(|v| msg(M::DictWord(v)))
                    .on_submit_maybe(can_add.then(|| msg(M::DictAdd))),
                small_button(t("Add"), can_add.then(|| msg(M::DictAdd))),
                small_button(t("Delete"), d.selected.map(|_| msg(M::DictDelete))),
            ]
            .spacing(6)
            .align_y(iced::Center),
            container(scrollable(words).height(Length::Fixed(if d.manage { 90.0 } else { 150.0 })))
                .padding(2)
                .width(Fill),
            row![
                small_button(t("Import..."), Some(msg(M::DictImport))),
                Space::new().width(Fill),
                small_button(
                    t("Manage custom dictionaries..."),
                    Some(msg(M::DictManage(!d.manage)))
                ),
            ]
            .spacing(6),
        ]
        .spacing(6),
    );
    let mut body = column![main, custom].spacing(10);
    if d.manage {
        let mut files = column![].spacing(1);
        for (n, path) in d.files.iter().enumerate() {
            files = files.push(
                button(text(file_name(path)).size(11))
                    .on_press(msg(M::DictManageSelect(n)))
                    .style(button_style(d.manage_selected == Some(n)))
                    .width(Fill)
                    .padding([2, 8]),
            );
        }
        let path = d
            .manage_selected
            .and_then(|n| d.files.get(n))
            .cloned()
            .unwrap_or_default();
        body = body.push(card(
            t("Custom dictionaries"),
            column![
                container(scrollable(files).height(Length::Fixed(90.0)))
                    .padding(2)
                    .width(Fill),
                text(path).size(10).style(muted_style),
                row![
                    small_button(crate::tr!("text-editor-menu", "new-dictionary"), Some(msg(M::DictNewFile))),
                    small_button(t("Add..."), Some(msg(M::DictAddFile))),
                    small_button(
                        t("Remove"),
                        d.manage_selected.map(|_| msg(M::DictRemoveFile))
                    ),
                ]
                .spacing(6),
            ]
            .spacing(6),
        ));
    }
    column![
        body,
        row![
            small_button(t("Help"), Some(msg(M::DictHelp))),
            Space::new().width(Fill),
            dialog_button_styled_opt(crate::t!("Close"), Some(Message::CloseModal), button::primary),
        ]
        .spacing(6),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(440.0))
    .into()
}

/// The spelling settings of the text editor: what is checked is the text
/// being edited, so only the word options apply.
pub fn view_spell_settings<'a>(d: &'a SpellSettingsDraft) -> Element<'a, Message> {
    let o = &d.options;
    let option = |label: &str, on: bool, setting: SpellSetting| {
        chip(t(label), on, msg(M::SpellOption(setting, !on)))
    };
    column![
        card(
            t("Options"),
            column![
                option("Ignore capitalized words", o.ignore_capitalized, SpellSetting::IgnoreCapitalized),
                option("Ignore words with mixed cases", o.ignore_mixed, SpellSetting::IgnoreMixed),
                option("Ignore words in uppercase", o.ignore_upper, SpellSetting::IgnoreUpper),
                option("Ignore words with numbers", o.ignore_numbers, SpellSetting::IgnoreNumbers),
                option(
                    "Ignore words containing punctuation",
                    o.ignore_punctuation,
                    SpellSetting::IgnorePunctuation
                ),
            ]
            .spacing(4),
        ),
        text(t("Dimension text, block attributes and external references are checked by SPELL."))
            .size(10)
            .style(muted_style),
        footer(msg(M::Ok), Message::CloseModal),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(400.0))
    .into()
}

/// The colour the selection is filled with while editing.
pub fn view_highlight<'a>(d: &'a HighlightDraft) -> Element<'a, Message> {
    let swatch = d.rgb().map_or(iced::Color::TRANSPARENT, |[r, g, b]| {
        iced::Color::from_rgb8(r, g, b)
    });
    column![
        card(
            t("Text Highlight Color"),
            column![
                row![
                    column![
                        number_field(t("Red"), &d.r, M::HighlightR),
                        number_field(t("Green"), &d.g, M::HighlightG),
                        number_field(t("Blue"), &d.b, M::HighlightB),
                    ]
                    .spacing(6),
                    container(Space::new())
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .style(move |_| container::Style {
                            background: Some(iced::Background::Color(swatch)),
                            border: iced::Border {
                                width: 1.0,
                                color: iced::Color::from_rgb8(128, 128, 128),
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }),
                ]
                .spacing(12)
                .align_y(iced::Center),
                small_button(t("Theme color"), Some(msg(M::HighlightTheme))),
            ]
            .spacing(8),
        ),
        footer(msg(M::Ok), Message::CloseModal),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(360.0))
    .into()
}

/// One choice out of a few, as joined buttons.
fn segmented<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, String)>,
    current: T,
    on: fn(T) -> M,
) -> Element<'a, Message> {
    let mut line = row![].spacing(2);
    for (value, label) in options {
        line = line.push(
            button(text(label).size(11))
                .on_press(msg(on(value)))
                .style(button_style(value == current))
                .padding([3, 9]),
        );
    }
    line.into()
}

/// The stack properties window: the two halves, the style, where the stack
/// sits on its line and its text size; Defaults and AutoStack beside them.
pub fn view_stack<'a>(d: &'a StackDraft) -> Element<'a, Message> {
    let styles = vec![
        (StackStyle::Horizontal, t("Fraction (Horizontal)")),
        (StackStyle::Diagonal, t("Fraction (Diagonal)")),
        (StackStyle::Tolerance, t("Tolerance")),
        (StackStyle::Decimal, t("Decimal")),
    ];
    let positions = vec![
        (StackPosition::Top, t("Top")),
        (StackPosition::Center, t("Center")),
        (StackPosition::Bottom, t("Bottom")),
    ];
    let sizes = row![
        text_input("", &d.size)
            .size(11)
            .padding([4, 8])
            .width(Length::Fixed(70.0))
            .style(field_style)
            .on_input(|v| msg(M::StackSize(v))),
        row(["100%", "90%", "80%", "75%", "70%", "66%", "50%"].into_iter().map(|s| {
            button(text(s).size(10))
                .on_press(msg(M::StackSize(s.to_string())))
                .style(button_style(d.size.trim() == s))
                .padding([2, 5])
                .into()
        }))
        .spacing(2),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![
        card(
            t("Text"),
            column![
                number_field(t("Upper text"), &d.upper, M::StackUpper),
                number_field(t("Lower text"), &d.lower, M::StackLower),
            ]
            .spacing(6),
        ),
        card(
            t("Appearance"),
            column![
                segmented(styles, d.style, M::StackStyleSet),
                segmented(positions, d.position, M::StackPositionSet),
                sizes,
            ]
            .spacing(8),
        ),
        row![
            small_button(t("Restore defaults"), Some(msg(M::StackDefaults(false)))),
            small_button(t("Save as defaults"), Some(msg(M::StackDefaults(true)))),
            small_button(t("AutoStack..."), Some(msg(M::AutoStackOpen))),
        ]
        .spacing(6),
        footer(
            if d.percent().is_some() {
                msg(M::Ok)
            } else {
                Message::Noop
            },
            Message::CloseModal
        ),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(520.0))
    .into()
}

/// AutoStack: whether and how numbers typed around `/`, `#` and `^` stack.
pub fn view_autostack<'a>(d: &'a AutoStackDraft) -> Element<'a, Message> {
    let fraction = vec![(false, t("Horizontal fraction")), (true, t("Diagonal fraction"))];
    column![
        card(
            t("AutoStack"),
            column![
                text(t("Stack fractions automatically as you type")).size(12),
                chip(t("Enable AutoStacking"), d.enabled, msg(M::AutoEnabled(!d.enabled))),
                row![
                    text(t("x/y becomes")).size(11).width(Length::Fixed(100.0)),
                    segmented(fraction, d.diagonal, M::AutoDiagonal),
                ]
                .spacing(8)
                .align_y(iced::Center),
                text(t("x#y becomes a diagonal fraction, x^y a tolerance stack."))
                    .size(10)
                    .style(muted_style),
                chip(
                    t("Remove leading blank: 1 x/y becomes 1x/y"),
                    d.trim,
                    msg(M::AutoTrim(!d.trim))
                ),
            ]
            .spacing(8),
        ),
        footer(
            msg(M::Ok),
            if d.back.is_some() {
                msg(M::AutoBack)
            } else {
                Message::CloseModal
            }
        ),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(Length::Fixed(440.0))
    .into()
}
