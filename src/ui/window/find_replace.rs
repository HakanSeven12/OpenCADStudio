//! Find and Replace — searches the drawing's texts (FIND and the ribbon's
//! Find text box). Cards for the texts, where to look, the options and the
//! listed results; the end of a search, and the questions a replacement asks
//! about locked layers, blocks and external references, show as a card
//! above the footer and hold the other buttons until they are answered.

use crate::app::{FindReplaceState, Message};
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip, well_style};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

pub const FIND_INPUT_ID: &str = "find-replace-search";

/// Where to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindScope {
    #[default]
    Drawing,
    Space,
    Selection,
}

/// What a search compares and which texts it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_words: bool,
    pub wildcards: bool,
    pub xrefs: bool,
    pub blocks: bool,
    pub ignore_hidden: bool,
    pub diacritics: bool,
    pub half_full: bool,
    pub attributes: bool,
    pub dimensions: bool,
    pub texts: bool,
    pub tables: bool,
    pub link_texts: bool,
    pub links: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            whole_words: false,
            wildcards: true,
            xrefs: true,
            blocks: true,
            ignore_hidden: true,
            diacritics: true,
            half_full: false,
            attributes: true,
            dimensions: true,
            texts: true,
            tables: true,
            link_texts: true,
            links: true,
        }
    }
}

impl FindOptions {
    pub(crate) fn matching(&self) -> crate::app::text_match::MatchOptions {
        crate::app::text_match::MatchOptions {
            match_case: self.match_case,
            // Whole words cannot be combined with wildcards.
            whole_words: self.whole_words && !self.wildcards,
            wildcards: self.wildcards,
            diacritics: self.diacritics,
            half_full: self.half_full,
        }
    }
}

/// One switch of the options cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindOpt {
    MatchCase,
    WholeWords,
    Wildcards,
    Xrefs,
    Blocks,
    IgnoreHidden,
    Diacritics,
    HalfFull,
    Attributes,
    Dimensions,
    Texts,
    Tables,
    LinkTexts,
    Links,
}

impl FindOpt {
    pub const ALL: [FindOpt; 14] = [
        FindOpt::MatchCase,
        FindOpt::WholeWords,
        FindOpt::Wildcards,
        FindOpt::Xrefs,
        FindOpt::Blocks,
        FindOpt::IgnoreHidden,
        FindOpt::Diacritics,
        FindOpt::HalfFull,
        FindOpt::Attributes,
        FindOpt::Dimensions,
        FindOpt::Texts,
        FindOpt::Tables,
        FindOpt::LinkTexts,
        FindOpt::Links,
    ];

    pub fn slot(self, o: &mut FindOptions) -> &mut bool {
        match self {
            FindOpt::MatchCase => &mut o.match_case,
            FindOpt::WholeWords => &mut o.whole_words,
            FindOpt::Wildcards => &mut o.wildcards,
            FindOpt::Xrefs => &mut o.xrefs,
            FindOpt::Blocks => &mut o.blocks,
            FindOpt::IgnoreHidden => &mut o.ignore_hidden,
            FindOpt::Diacritics => &mut o.diacritics,
            FindOpt::HalfFull => &mut o.half_full,
            FindOpt::Attributes => &mut o.attributes,
            FindOpt::Dimensions => &mut o.dimensions,
            FindOpt::Texts => &mut o.texts,
            FindOpt::Tables => &mut o.tables,
            FindOpt::LinkTexts => &mut o.link_texts,
            FindOpt::Links => &mut o.links,
        }
    }

    /// The automation name.
    pub fn key(self) -> &'static str {
        match self {
            FindOpt::MatchCase => "match_case",
            FindOpt::WholeWords => "whole_words",
            FindOpt::Wildcards => "wildcards",
            FindOpt::Xrefs => "xrefs",
            FindOpt::Blocks => "blocks",
            FindOpt::IgnoreHidden => "ignore_hidden",
            FindOpt::Diacritics => "diacritics",
            FindOpt::HalfFull => "half_full",
            FindOpt::Attributes => "attributes",
            FindOpt::Dimensions => "dimensions",
            FindOpt::Texts => "texts",
            FindOpt::Tables => "tables",
            FindOpt::LinkTexts => "link_texts",
            FindOpt::Links => "links",
        }
    }

    fn label(self) -> String {
        match self {
            FindOpt::MatchCase => crate::tr!("find", "match-case"),
            FindOpt::WholeWords => crate::tr!("find", "whole-words"),
            FindOpt::Wildcards => crate::tr!("find", "wildcards"),
            FindOpt::Xrefs => crate::tr!("find", "xrefs"),
            FindOpt::Blocks => crate::tr!("find", "blocks"),
            FindOpt::IgnoreHidden => crate::tr!("find", "ignore-hidden"),
            FindOpt::Diacritics => crate::tr!("find", "diacritics"),
            FindOpt::HalfFull => crate::tr!("find", "half-full"),
            FindOpt::Attributes => crate::tr!("find", "attributes"),
            FindOpt::Dimensions => crate::tr!("find", "dimensions"),
            FindOpt::Texts => crate::tr!("find", "texts-type"),
            FindOpt::Tables => crate::tr!("find", "tables"),
            FindOpt::LinkTexts => crate::tr!("find", "link-texts"),
            FindOpt::Links => crate::tr!("find", "links"),
        }
    }
}

/// The kind of text a match is, as the result list names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindKind {
    Text,
    MText,
    Dimension,
    MLeader,
    Table,
    Attribute,
    Block,
    Xref,
    LinkText,
    Link,
}

impl FindKind {
    pub fn label(self) -> String {
        match self {
            FindKind::Text => crate::tr!("find", "kind-text"),
            FindKind::MText => crate::tr!("find", "kind-mtext"),
            FindKind::Dimension => crate::tr!("find", "kind-dimension"),
            FindKind::MLeader => crate::tr!("find", "kind-mleader"),
            FindKind::Table => crate::tr!("find", "kind-table"),
            FindKind::Attribute => crate::tr!("find", "kind-attribute"),
            FindKind::Block => crate::tr!("find", "kind-block"),
            FindKind::Xref => crate::tr!("find", "kind-xref"),
            FindKind::LinkText => crate::tr!("find", "kind-link-text"),
            FindKind::Link => crate::tr!("find", "kind-link"),
        }
    }
}

/// The card shown at the end of a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindNotice {
    Finished { found: usize, changed: usize },
    /// The listed objects are in more than one space.
    CannotSelect,
}

/// What a replacement asks before it goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPrompt {
    Locked,
    Block(String),
    Xref(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockChoice {
    Keep,
    Unlock,
    Skip,
}

/// A replacement in progress: the matches still to go, and the answers
/// given so far.
#[derive(Debug, Clone)]
pub(crate) struct ReplaceRun {
    pub matches: Vec<crate::app::FindResult>,
    pub next: usize,
    /// Replace one match (then show the next) rather than all.
    pub single: bool,
    pub prompt: Option<FindPrompt>,
    pub locked: Option<LockChoice>,
    pub all_blocks: bool,
    pub blocks: Vec<(String, bool)>,
    pub all_xrefs: bool,
    pub xrefs: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAnswer {
    KeepLocked,
    Unlock,
    SkipLocked,
    AllBlocks,
    ThisBlock,
    SkipBlock,
    ThisXref,
    AllXrefs,
    SkipXref,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum FindMsg {
    Search(String),
    Replacement(String),
    Scope(FindScope),
    SelectObjects,
    ListResults(bool),
    MoreOptions(bool),
    Option(FindOpt, bool),
    Find,
    Replace,
    ReplaceAll,
    Done,
    Help,
    /// A click on a list row; `Some((ctrl, shift))` stands for held keys
    /// (automation), `None` reads the keyboard.
    Row(usize, Option<(bool, bool)>),
    ZoomToResult,
    SelectHighlighted,
    SelectAll,
    Dismiss,
    Answer(FindAnswer),
}

fn msg(m: FindMsg) -> Message {
    Message::Find(m)
}

const LABEL_W: f32 = 96.0;

/// A text field with its history behind a drop-down.
fn history_field<'a>(
    label: String,
    value: &'a str,
    history: &'a [String],
    on: fn(String) -> FindMsg,
    id: Option<&'static str>,
    submit: Option<FindMsg>,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .size(12)
        .padding([5, 8])
        .width(Fill)
        .style(field_style)
        .on_input(move |v| msg(on(v)));
    if let Some(id) = id {
        input = input.id(iced::widget::Id::new(id));
    }
    if let Some(submit) = submit {
        input = input.on_submit(msg(submit));
    }
    let picker = crate::ui::dropdown_list::pick_list(None::<String>, history.to_vec(), |s: &String| {
        s.clone()
    })
    .on_select(move |v| msg(on(v)))
    .text_size(11)
    .padding([4, 6])
    .width(Length::Fixed(34.0));
    row![
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(LABEL_W)),
        input,
        picker,
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn scope_switch<'a>(scope: FindScope) -> Element<'a, Message> {
    let part = |label: String, value: FindScope| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(FindMsg::Scope(value)))
            .style(button_style(scope == value))
            .padding([5, 6])
            .width(Fill)
    };
    container(
        row![
            part(crate::tr!("find", "scope-drawing"), FindScope::Drawing),
            part(crate::tr!("find", "scope-space"), FindScope::Space),
            part(crate::tr!("find", "scope-selection"), FindScope::Selection),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style)
    .into()
}

fn option_chips<'a>(state: &'a FindReplaceState, opts: &[FindOpt]) -> Element<'a, Message> {
    let mut list = column![].spacing(4);
    for opt in opts {
        let mut options = state.options;
        let on = *opt.slot(&mut options);
        let enabled = !(*opt == FindOpt::WholeWords && state.options.wildcards);
        let mut content = row![].spacing(4).align_y(iced::Center);
        if on && enabled {
            content = content.push(crate::ui::icons::tinted(
                crate::ui::icons::CHECK,
                11.0,
                |theme: &Theme| theme.palette().primary.base.text,
            ));
        }
        let chip = button(content.push(text(opt.label()).size(11)))
            .style(button_style(on && enabled))
            .padding([4, 10]);
        list = list.push(if enabled {
            chip.on_press(msg(FindMsg::Option(*opt, !on)))
        } else {
            chip
        });
    }
    list.into()
}

fn results_card<'a>(state: &'a FindReplaceState, busy: bool) -> Element<'a, Message> {
    let header = row![
        text(crate::tr!("find", "col-location"))
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(90.0)),
        text(crate::tr!("find", "col-type"))
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(150.0)),
        text(crate::tr!("find", "col-text")).size(11).style(muted_style),
    ]
    .spacing(8);
    let mut rows = column![].spacing(1);
    for (index, result) in state.results.iter().enumerate() {
        let selected = state.highlighted.contains(&result.key);
        let line = row![
            text(result.location.clone())
                .size(11)
                .width(Length::Fixed(90.0)),
            text(result.kind.label()).size(11).width(Length::Fixed(150.0)),
            text(result.text.clone())
                .size(11)
                .wrapping(iced::advanced::text::Wrapping::None),
        ]
        .spacing(8);
        let b = button(line)
            .style(if selected { button::primary } else { button::text })
            .padding([3, 6])
            .width(Fill);
        rows = rows.push(if busy {
            b
        } else {
            b.on_press(msg(FindMsg::Row(index, None)))
        });
    }
    let has_row = !state.highlighted.is_empty() && !busy;
    let has_any = !state.results.is_empty() && !busy;
    let side = |label: String, on: bool, m: FindMsg| {
        let b = button(text(label).size(11).width(Fill).align_x(iced::Center))
            .style(button_style(false))
            .padding([4, 8])
            .width(Length::Fixed(150.0));
        if on {
            b.on_press(msg(m))
        } else {
            b
        }
    };
    card(
        crate::tr!("find", "results"),
        row![
            container(
                column![
                    container(header).padding([0, 6]),
                    scrollable(rows).height(Length::Fixed(180.0)),
                ]
                .spacing(4),
            )
            .padding(4)
            .width(Fill)
            .style(well_style),
            column![
                side(
                    crate::tr!("find", "zoom-result"),
                    has_row && state.highlighted.len() == 1,
                    FindMsg::ZoomToResult
                ),
                side(
                    crate::tr!("find", "select-highlighted"),
                    has_row,
                    FindMsg::SelectHighlighted
                ),
                side(crate::tr!("find", "select-all"), has_any, FindMsg::SelectAll),
            ]
            .spacing(6),
        ]
        .spacing(8),
    )
}

fn band_style(warning: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| container::Style {
        background: Some(Background::Color(if warning {
            theme.palette().warning.base.color.scale_alpha(0.18)
        } else {
            theme.palette().primary.base.color.scale_alpha(0.14)
        })),
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

static ICON_WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

/// The end of a search, or a question a replacement asks.
fn message_card<'a>(state: &'a FindReplaceState) -> Option<Element<'a, Message>> {
    let answer = |label: String, a: FindAnswer, primary: bool| -> Element<'a, Message> {
        dialog_button(label, msg(FindMsg::Answer(a)), primary).into()
    };
    if let Some(prompt) = state.run.as_ref().and_then(|run| run.prompt.clone()) {
        let (lines, buttons): (Vec<String>, Vec<Element<'a, Message>>) = match prompt {
            FindPrompt::Locked => (
                vec![
                    crate::tr!("find", "locked-found"),
                    crate::tr!("find", "locked-question"),
                ],
                vec![
                    answer(crate::tr!("find", "locked-keep"), FindAnswer::KeepLocked, true),
                    answer(crate::tr!("find", "locked-unlock"), FindAnswer::Unlock, false),
                    answer(crate::tr!("find", "locked-skip"), FindAnswer::SkipLocked, false),
                ],
            ),
            FindPrompt::Block(name) => (
                vec![
                    crate::i18n::plain(crate::tr!("find", "block-found", name = name)),
                    crate::tr!("find", "block-question"),
                ],
                vec![
                    answer(crate::tr!("find", "block-all"), FindAnswer::AllBlocks, true),
                    answer(crate::tr!("find", "block-this"), FindAnswer::ThisBlock, false),
                    answer(crate::tr!("find", "block-skip"), FindAnswer::SkipBlock, false),
                ],
            ),
            FindPrompt::Xref(path) => (
                vec![
                    crate::i18n::plain(crate::tr!("find", "xref-found", path = path)),
                    crate::tr!("find", "xref-question"),
                ],
                vec![
                    answer(crate::tr!("find", "xref-this"), FindAnswer::ThisXref, true),
                    answer(crate::tr!("find", "xref-all"), FindAnswer::AllXrefs, false),
                    answer(crate::tr!("find", "xref-skip"), FindAnswer::SkipXref, false),
                ],
            ),
        };
        let mut texts = column![].spacing(4);
        for line in lines {
            texts = texts.push(text(line).size(11));
        }
        let mut choices = row![Space::new().width(Fill)].spacing(6);
        for b in buttons {
            choices = choices.push(b);
        }
        choices = choices.push(dialog_button_styled_opt(
            crate::t!("Cancel"),
            Some(msg(FindMsg::Answer(FindAnswer::Cancel))),
            button::secondary,
        ));
        return Some(
            container(
                column![
                    row![crate::ui::icons::semantic(ICON_WARN, 16.0), texts]
                        .spacing(10)
                        .align_y(iced::Center),
                    choices.wrap(),
                ]
                .spacing(8),
            )
            .padding([8, 10])
            .width(Fill)
            .style(band_style(true))
            .into(),
        );
    }
    let notice = state.notice?;
    let (icon, lines): (&'static [u8], Vec<String>) = match notice {
        FindNotice::Finished { found, changed } => (
            crate::ui::icons::INFO,
            vec![
                crate::tr!("find", "finished"),
                if found == 0 {
                    crate::tr!("find", "found-none")
                } else if found == 1 {
                    crate::tr!("find", "found-one")
                } else {
                    crate::i18n::plain(crate::tr!("find", "found-many", count = found.to_string()))
                },
                if found == 0 {
                    String::new()
                } else if changed == 1 {
                    crate::tr!("find", "changed-one")
                } else {
                    crate::i18n::plain(crate::tr!("find", "changed-many", count = changed.to_string()))
                },
            ],
        ),
        FindNotice::CannotSelect => (
            ICON_WARN,
            vec![
                crate::tr!("find", "cannot-select"),
                crate::tr!("find", "cannot-select-why"),
            ],
        ),
    };
    let mut texts = column![].spacing(4);
    for (n, line) in lines.into_iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        texts = texts.push(if n == 0 {
            text(line).size(12)
        } else {
            text(line).size(11).style(muted_style)
        });
    }
    Some(
        container(
            row![
                crate::ui::icons::semantic(icon, 16.0),
                texts,
                Space::new().width(Fill),
                dialog_button(crate::t!("OK"), msg(FindMsg::Dismiss), true),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .padding([8, 10])
        .width(Fill)
        .style(band_style(notice == FindNotice::CannotSelect))
        .into(),
    )
}

/// Build the window.
pub(crate) fn view_window<'a>(
    state: &'a FindReplaceState,
    _sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let busy = state.notice.is_some() || state.run.as_ref().is_some_and(|r| r.prompt.is_some());
    let texts = card(
        crate::tr!("find", "texts"),
        column![
            history_field(
                crate::tr!("find", "find-what"),
                &state.search,
                &state.search_history,
                FindMsg::Search,
                Some(FIND_INPUT_ID),
                (!busy).then_some(FindMsg::Find),
            ),
            history_field(
                crate::tr!("find", "replace-with"),
                &state.replacement,
                &state.replace_history,
                FindMsg::Replacement,
                None,
                None,
            ),
        ]
        .spacing(6),
    );
    let mut where_col = column![row![
        scope_switch(state.scope),
        button(text(crate::tr!("find", "select-objects")).size(11))
            .on_press(msg(FindMsg::SelectObjects))
            .style(button_style(false))
            .padding([5, 10]),
    ]
    .spacing(6)
    .align_y(iced::Center)]
    .spacing(4);
    if state.scope == FindScope::Selection {
        where_col = where_col.push(
            text(crate::i18n::plain(crate::tr!(
                "find",
                "selected-count",
                count = state.selection.len().to_string()
            )))
            .size(11)
            .style(muted_style),
        );
    }
    let where_card = card(crate::tr!("find", "where"), where_col);
    let toggles = row![
        chip(
            crate::tr!("find", "list-results"),
            state.list_results,
            msg(FindMsg::ListResults(!state.list_results)),
        ),
        Space::new().width(Fill),
        chip(
            crate::tr!("find", "more-options"),
            state.more_options,
            msg(FindMsg::MoreOptions(!state.more_options)),
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let mut body = column![texts, where_card, toggles].spacing(10);
    if state.list_results {
        body = body.push(results_card(state, busy));
    }
    if state.more_options {
        body = body.push(
            row![
                card(
                    crate::tr!("find", "search-options"),
                    option_chips(state, &FindOpt::ALL[..8]),
                ),
                card(
                    crate::tr!("find", "text-types"),
                    option_chips(state, &FindOpt::ALL[8..]),
                ),
            ]
            .spacing(10),
        );
    }
    let has_search = !state.search.trim().is_empty() && !busy;
    let find_label = if state.current_match.is_some() && !state.list_results {
        crate::tr!("find", "find-next")
    } else {
        crate::tr!("find", "find")
    };
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(FindMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(
            crate::tr!("find", "replace"),
            (has_search && state.current_match.is_some()).then_some(msg(FindMsg::Replace)),
            button::secondary,
        ),
        dialog_button_styled_opt(
            crate::tr!("find", "replace-all"),
            has_search.then_some(msg(FindMsg::ReplaceAll)),
            button::secondary,
        ),
        dialog_button_styled_opt(
            find_label,
            has_search.then_some(msg(FindMsg::Find)),
            button::primary,
        ),
        dialog_button_styled_opt(
            crate::tr!("find", "done"),
            (!busy).then_some(msg(FindMsg::Done)),
            button::secondary,
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    // A steady window: the cards scroll while the footer stays put.
    let mut window = column![scrollable(body).height(Fill)].spacing(10);
    if let Some(message) = message_card(state) {
        window = window.push(message);
    }
    window
        .push(footer)
        .padding([10, 12])
        .width(Length::Fixed(660.0))
        .height(Length::Fixed(720.0))
        .into()
}
