//! Layer Translator — map this drawing's layers onto a set loaded from another
//! drawing, then translate.
//!
//! Two lists side by side: what the drawing has, and what the loaded standard
//! offers. Selecting one of each and mapping them builds up the list of moves,
//! which is only applied when Translate is pressed — so a mapping can be
//! reconsidered, and a translation is one undo step rather than many.

use iced::widget::{button, checkbox, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::draw::layers::laytrans::{Mapping, TargetLayer};
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};

/// The dialog's working state: what was loaded, what is selected, and the
/// mappings built so far. None of it touches the drawing until Translate.
#[derive(Default)]
pub struct State {
    /// Where the target set came from, shown so it is clear what is being
    /// translated to.
    pub source_file: String,
    pub targets: Vec<TargetLayer>,
    pub mappings: Vec<Mapping>,
    pub selected_from: Option<String>,
    pub selected_to: Option<String>,
    pub force_bylayer: bool,
    pub write_log: bool,
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

fn list_row<'a>(label: String, selected: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(12).width(Fill))
        .on_press(message)
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let mut style = button::text(theme, status);
            if selected {
                style.background = Some(Background::Color(palette.primary.weak.color));
                style.text_color = palette.primary.weak.text;
            }
            style.border = Border {
                radius: 3.0.into(),
                ..Default::default()
            };
            style
        })
        .width(Fill)
        .padding([4, 8])
        .into()
}

fn pane<'a>(
    title: String,
    rows: Vec<Element<'a, Message>>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let body: Element<'_, Message> = if rows.is_empty() {
        container(text(t!("(none)")).size(11).style(muted))
            .padding(8)
            .into()
    } else {
        scrollable(column(rows).spacing(1))
            .height(sizing.height)
            .into()
    };
    container(column![text(title).size(11).style(muted), body].spacing(4))
        .padding(6)
        .width(sizing.width)
        .height(sizing.height)
        .style(|theme: &Theme| container::Style {
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    state: &'a State,
    sources: Vec<String>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // A layer already spoken for should not be offered again; mapping it twice
    // could only mean the second one wins, which is not a choice worth making
    // silently.
    let mapped: Vec<&str> = state.mappings.iter().map(|m| m.from.as_str()).collect();
    let from_rows: Vec<Element<'_, Message>> = sources
        .iter()
        .filter(|name| !mapped.contains(&name.as_str()))
        .map(|name| {
            list_row(
                name.clone(),
                state.selected_from.as_deref() == Some(name.as_str()),
                Message::LayerTranslatorSelectFrom(name.clone()),
            )
        })
        .collect();
    let to_rows: Vec<Element<'_, Message>> = state
        .targets
        .iter()
        .map(|target| {
            list_row(
                target.name.clone(),
                state.selected_to.as_deref() == Some(target.name.as_str()),
                Message::LayerTranslatorSelectTo(target.name.clone()),
            )
        })
        .collect();
    let mapping_rows: Vec<Element<'_, Message>> = state
        .mappings
        .iter()
        .map(|mapping| {
            row![
                text(format!("{}  →  {}", mapping.from, mapping.to))
                    .size(12)
                    .width(Fill),
                button(text(t!("Remove")).size(11))
                    .on_press(Message::LayerTranslatorUnmap(mapping.from.clone()))
                    .style(button::text)
                    .padding([2, 6]),
            ]
            .align_y(iced::Center)
            .padding([2, 6])
            .into()
        })
        .collect();

    let loaded: Element<'_, Message> = if state.source_file.is_empty() {
        text(t!("No standard loaded.")).size(11).style(muted).into()
    } else {
        text(state.source_file.clone()).size(11).style(muted).into()
    };

    let can_map = state.selected_from.is_some() && state.selected_to.is_some();
    let controls = row![
        button(text(t!("Load…")).size(12))
            .on_press(Message::LayerTranslatorLoad)
            .padding([4, 10]),
        button(text(t!("Map")).size(12))
            .on_press_maybe(can_map.then_some(Message::LayerTranslatorMap))
            .padding([4, 10]),
        button(text(t!("Map same")).size(12))
            .on_press_maybe(
                (!state.targets.is_empty()).then_some(Message::LayerTranslatorMapSame)
            )
            .padding([4, 10]),
        Space::new().width(Fill),
        loaded,
    ]
    .spacing(6)
    .align_y(iced::Center);

    let options = row![
        checkbox(state.force_bylayer)
            .on_toggle(Message::LayerTranslatorForceByLayer)
            .size(14),
        text(t!("Force objects to ByLayer")).size(11),
        Space::new().width(Length::Fixed(10.0)),
        checkbox(state.write_log)
            .on_toggle(Message::LayerTranslatorWriteLog)
            .size(14),
        text(t!("Write translation log")).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center);

    let actions = row![
        button(text(t!("Save mappings…")).size(12))
            .on_press_maybe(
                (!state.mappings.is_empty()).then_some(Message::LayerTranslatorSaveMappings)
            )
            .padding([4, 10]),
        button(text(t!("Load mappings…")).size(12))
            .on_press(Message::LayerTranslatorLoadMappings)
            .padding([4, 10]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button_styled_opt(
            t!("Translate"),
            (!state.mappings.is_empty()).then_some(Message::LayerTranslatorTranslate),
            button::primary,
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);

    let lists = row![
        pane(t!("Translate from").into_owned(), from_rows, sizing),
        pane(t!("Translate to").into_owned(), to_rows, sizing),
    ]
    .spacing(8)
    .height(sizing.height);

    column![
        controls,
        lists,
        pane(
            t!("%{n} mapping(s)", n = state.mappings.len()).into_owned(),
            mapping_rows,
            sizing,
        ),
        options,
        actions,
    ]
    .spacing(8)
    .padding(10)
    .width(sizing.width)
    .into()
}
