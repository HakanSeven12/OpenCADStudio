//! Check Spelling window (SPELL): where to look, the word not in the
//! dictionary with the sentence around it, suggestions and what to do with
//! the word, the main dictionary, and the checking options with the words
//! added to the user's dictionary.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card, chip, well_style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpellScope {
    #[default]
    Drawing,
    Space,
    Selection,
}

#[derive(Debug, Clone)]
pub enum SpellMsg {
    Scope(SpellScope),
    SelectObjects,
    Start,
    Replacement(String),
    Suggestion(String),
    Change,
    ChangeAll,
    Ignore,
    IgnoreAll,
    AddWord,
    Undo,
    Language(String),
    ToggleSettings,
    Setting(SpellSetting, bool),
    RemoveWord(String),
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellSetting {
    Dimensions,
    Attributes,
    Xrefs,
    IgnoreCapitalized,
    IgnoreMixed,
    IgnoreUpper,
    IgnoreNumbers,
    IgnorePunctuation,
}

/// What is checked and which words pass without a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SpellOptions {
    pub dimensions: bool,
    pub attributes: bool,
    pub xrefs: bool,
    pub ignore_capitalized: bool,
    pub ignore_mixed: bool,
    pub ignore_upper: bool,
    pub ignore_numbers: bool,
    pub ignore_punctuation: bool,
}

impl Default for SpellOptions {
    fn default() -> Self {
        Self {
            dimensions: true,
            attributes: true,
            xrefs: false,
            ignore_capitalized: false,
            ignore_mixed: false,
            ignore_upper: false,
            ignore_numbers: false,
            ignore_punctuation: false,
        }
    }
}

impl SpellOptions {
    pub fn get(&self, s: SpellSetting) -> bool {
        match s {
            SpellSetting::Dimensions => self.dimensions,
            SpellSetting::Attributes => self.attributes,
            SpellSetting::Xrefs => self.xrefs,
            SpellSetting::IgnoreCapitalized => self.ignore_capitalized,
            SpellSetting::IgnoreMixed => self.ignore_mixed,
            SpellSetting::IgnoreUpper => self.ignore_upper,
            SpellSetting::IgnoreNumbers => self.ignore_numbers,
            SpellSetting::IgnorePunctuation => self.ignore_punctuation,
        }
    }

    pub fn set(&mut self, s: SpellSetting, on: bool) {
        match s {
            SpellSetting::Dimensions => self.dimensions = on,
            SpellSetting::Attributes => self.attributes = on,
            SpellSetting::Xrefs => self.xrefs = on,
            SpellSetting::IgnoreCapitalized => self.ignore_capitalized = on,
            SpellSetting::IgnoreMixed => self.ignore_mixed = on,
            SpellSetting::IgnoreUpper => self.ignore_upper = on,
            SpellSetting::IgnoreNumbers => self.ignore_numbers = on,
            SpellSetting::IgnorePunctuation => self.ignore_punctuation = on,
        }
    }
}

/// The word being shown.
#[derive(Debug, Clone)]
pub struct SpellFind {
    pub handle: codec::Handle,
    pub word: String,
    /// Byte range of the word in `text`.
    pub start: usize,
    pub end: usize,
    /// The object's text, as shown.
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct SpellState {
    pub scope: SpellScope,
    pub selection: Vec<codec::Handle>,
    /// BCP-47 tag of the main dictionary, and the ones offered (tag, name).
    pub language: String,
    pub languages: Vec<(String, String)>,
    pub options: SpellOptions,
    pub show_settings: bool,
    /// The objects being checked, and where the check is: the object and the
    /// byte offset in its text.
    pub targets: Vec<codec::Handle>,
    pub cursor: (usize, usize),
    pub running: bool,
    pub current: Option<SpellFind>,
    pub suggestions: Vec<String>,
    pub replacement: String,
    /// Ignore All, for this check.
    pub ignored: Vec<String>,
    /// The user's dictionary.
    pub custom: Vec<String>,
    /// Changes the Undo button takes back: the object as it was and where
    /// the check was.
    pub undo: Vec<(Vec<(codec::Handle, codec::EntityType)>, (usize, usize))>,
    pub status: String,
}

fn msg(m: SpellMsg) -> Message {
    Message::Spell(m)
}

/// Where to look, as joined buttons.
fn scope_switch<'a>(scope: SpellScope, enabled: bool) -> Element<'a, Message> {
    let part = |label: String, value: SpellScope| {
        let b = button(text(label).size(11).width(Fill).align_x(iced::Center))
            .style(button_style(scope == value))
            .padding([5, 6])
            .width(Fill);
        if enabled {
            b.on_press(msg(SpellMsg::Scope(value)))
        } else {
            b
        }
    };
    container(
        row![
            part(crate::tr!("spell", "scope-drawing"), SpellScope::Drawing),
            part(crate::tr!("spell", "scope-space"), SpellScope::Space),
            part(crate::tr!("spell", "scope-selection"), SpellScope::Selection),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style)
    .into()
}

fn action<'a>(label: String, on: Option<SpellMsg>) -> Element<'a, Message> {
    let b = button(text(label).size(11).width(Fill).align_x(iced::Center))
        .style(button_style(false))
        .padding([5, 8])
        .width(Length::Fixed(150.0));
    match on {
        Some(m) => b.on_press(msg(m)).into(),
        None => b.into(),
    }
}

/// The word in its sentence, the word itself picked out.
fn word_card<'a>(state: &'a SpellState) -> Element<'a, Message> {
    let body: Element<'a, Message> = match &state.current {
        Some(found) => {
            let before = &found.text[..found.start];
            let after = &found.text[found.end..];
            let clip = |s: &str, tail: bool| {
                let chars: Vec<char> = s.chars().collect();
                if chars.len() <= 40 {
                    s.to_string()
                } else if tail {
                    format!("…{}", chars[chars.len() - 40..].iter().collect::<String>())
                } else {
                    format!("{}…", chars[..40].iter().collect::<String>())
                }
            };
            column![
                text(found.word.clone()).size(18).style(accent_text),
                row![
                    text(clip(before, true)).size(11).style(muted_style),
                    text(found.word.clone()).size(11),
                    text(clip(after, false)).size(11).style(muted_style),
                ]
                .spacing(0)
                .wrap(),
            ]
            .spacing(4)
            .into()
        }
        None => text(if state.status.is_empty() {
            crate::tr!("spell", "idle")
        } else {
            state.status.clone()
        })
        .size(12)
        .style(muted_style)
        .into(),
    };
    card(
        crate::tr!("spell", "not-in-dictionary"),
        container(body).padding([4, 6]).width(Fill),
    )
}

fn suggestions_card<'a>(state: &'a SpellState) -> Element<'a, Message> {
    let found = state.current.is_some();
    let input = text_input("", &state.replacement)
        .size(12)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    let input = if found {
        input.on_input(|v| msg(SpellMsg::Replacement(v)))
    } else {
        input
    };
    let list = state.suggestions.iter().map(|s| {
        let on = *s == state.replacement;
        button(text(s.as_str()).size(11).width(Fill))
            .on_press(msg(SpellMsg::Suggestion(s.clone())))
            .style(button_style(on))
            .padding([3, 8])
            .width(Fill)
            .into()
    });
    let list = container(
        scrollable(column(list.collect::<Vec<_>>()).spacing(1).width(Fill))
            .height(Length::Fixed(112.0)),
    )
    .padding(3)
    .width(Fill)
    .style(well_style);
    let some = |m: SpellMsg| found.then_some(m);
    let actions = column![
        action(crate::tr!("spell", "change"), some(SpellMsg::Change)),
        action(crate::tr!("spell", "change-all"), some(SpellMsg::ChangeAll)),
        Space::new().height(6),
        action(crate::tr!("spell", "ignore"), some(SpellMsg::Ignore)),
        action(crate::tr!("spell", "ignore-all"), some(SpellMsg::IgnoreAll)),
        Space::new().height(6),
        action(crate::tr!("spell", "add-word"), some(SpellMsg::AddWord)),
    ]
    .spacing(4);
    card(
        crate::tr!("spell", "suggestions"),
        row![column![input, list].spacing(6).width(Fill), actions].spacing(10),
    )
}

fn settings_card<'a>(state: &'a SpellState) -> Element<'a, Message> {
    let o = state.options;
    let toggle = |label: String, s: SpellSetting| chip(label, o.get(s), msg(SpellMsg::Setting(s, !o.get(s))));
    let include = row![
        toggle(crate::tr!("spell", "opt-dimensions"), SpellSetting::Dimensions),
        toggle(crate::tr!("spell", "opt-attributes"), SpellSetting::Attributes),
        toggle(crate::tr!("spell", "opt-xrefs"), SpellSetting::Xrefs),
    ]
    .spacing(6)
    .wrap();
    let ignore = row![
        toggle(crate::tr!("spell", "opt-capitalized"), SpellSetting::IgnoreCapitalized),
        toggle(crate::tr!("spell", "opt-mixed"), SpellSetting::IgnoreMixed),
        toggle(crate::tr!("spell", "opt-upper"), SpellSetting::IgnoreUpper),
        toggle(crate::tr!("spell", "opt-numbers"), SpellSetting::IgnoreNumbers),
        toggle(crate::tr!("spell", "opt-punctuation"), SpellSetting::IgnorePunctuation),
    ]
    .spacing(6)
    .wrap();
    let words: Vec<Element<'a, Message>> = state
        .custom
        .iter()
        .map(|w| {
            button(row![text(w.as_str()).size(11), text("×").size(11)].spacing(6))
                .on_press(msg(SpellMsg::RemoveWord(w.clone())))
                .style(button_style(false))
                .padding([3, 8])
                .into()
        })
        .collect();
    let words: Element<'a, Message> = if words.is_empty() {
        text(crate::tr!("spell", "no-words")).size(11).style(muted_style).into()
    } else {
        row(words).spacing(4).wrap().into()
    };
    card(
        crate::tr!("spell", "settings"),
        column![
            text(crate::tr!("spell", "include")).size(11).style(muted_style),
            include,
            text(crate::tr!("spell", "ignore-words")).size(11).style(muted_style),
            ignore,
            text(crate::tr!("spell", "user-dictionary")).size(11).style(muted_style),
            words,
        ]
        .spacing(6),
    )
}

pub fn view<'a>(state: &'a SpellState, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let select = button(text(crate::tr!("spell", "select")).size(11))
    .on_press(msg(SpellMsg::SelectObjects))
    .style(button_style(false))
    .padding([4, 9]);
    let start = button(text(crate::tr!("spell", "start")).size(12))
        .style(button::primary)
        .padding([5, 14]);
    let start = if state.running {
        start
    } else {
        start.on_press(msg(SpellMsg::Start))
    };
    let mut where_row = row![scope_switch(state.scope, !state.running)].spacing(6).align_y(iced::Center);
    if state.scope == SpellScope::Selection {
        where_row = where_row.push(select);
    }
    let where_row = where_row.push(start);
    let selection_note: Element<'a, Message> = if state.scope == SpellScope::Selection {
        text(crate::tr!("spell", "selected-count", count = state.selection.len().to_string()))
            .size(11)
            .style(muted_style)
            .into()
    } else {
        Space::new().height(0).into()
    };
    let names: Vec<String> = state.languages.iter().map(|(_, name)| name.clone()).collect();
    let current = state
        .languages
        .iter()
        .find(|(tag, _)| *tag == state.language)
        .map(|(_, name)| name.clone());
    let languages = iced::widget::pick_list(current, names, |name: &String| name.clone())
        .on_select(|name| msg(SpellMsg::Language(name)))
        .text_size(11)
        .padding([4, 8]);
    let dictionary = row![
        text(crate::tr!("spell", "main-dictionary")).size(11).style(muted_style),
        languages,
        Space::new().width(Fill),
        chip(
            crate::tr!("spell", "settings"),
            state.show_settings,
            msg(SpellMsg::ToggleSettings)
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let mut body = column![
        where_row,
        selection_note,
        word_card(state),
        suggestions_card(state),
        dictionary
    ]
    .spacing(10);
    if state.show_settings {
        body = body.push(settings_card(state));
    }
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(SpellMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(
            crate::tr!("spell", "undo"),
            (!state.undo.is_empty()).then_some(msg(SpellMsg::Undo)),
            button::secondary,
        ),
        dialog_button_styled_opt(crate::t!("Close"), Some(Message::CloseModal), button::secondary),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![
        scrollable(body).height(Fill),
        footer
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

/// Window titlebar text.
pub fn title() -> String {
    crate::tr!("spell", "title")
}
