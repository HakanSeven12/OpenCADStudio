//! The Hatch panel: HATCH's settings in one place while the command runs —
//! pattern, angle and scale, the boundary method, islands and the options —
//! floating over the drawing so boundaries can still be picked.

use crate::app::Message;
use crate::modules::draw::draw::hatch::HatchSettings;
use crate::ui::style::form::{self, dialog_button, form_radio, LABEL_WIDTH};
use document::entities::HatchStyleType;
use iced::widget::{checkbox, column, container, row, text, text_input};
use iced::{Background, Border, Element, Length, Theme};

#[derive(Debug, Clone)]
pub enum HatchPanelMsg {
    Pattern(String),
    Angle(String),
    Scale(String),
    Gap(String),
    Method(u8),
    Islands(u8),
    Associative(bool),
    Separate(bool),
    KeepBoundaries(bool),
    Accept,
    Cancel,
}

/// Numbers as typed, shown until HATCH starts again: a half-typed value
/// ("1.") is kept while only valid ones reach the command.
#[derive(Debug, Clone, Default)]
pub struct Drafts {
    pub angle: Option<String>,
    pub scale: Option<String>,
    pub gap: Option<String>,
}

pub const WIDTH: f32 = 290.0;

pub fn island_code(style: HatchStyleType) -> u8 {
    match style {
        HatchStyleType::Normal => 0,
        HatchStyleType::Outer => 1,
        HatchStyleType::Ignore => 2,
    }
}

pub fn island_style(code: u8) -> HatchStyleType {
    match code {
        1 => HatchStyleType::Outer,
        2 => HatchStyleType::Ignore,
        _ => HatchStyleType::Normal,
    }
}

fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn field<'a>(
    label: String,
    value: String,
    on_input: fn(String) -> HatchPanelMsg,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(11)
            .style(crate::ui::style::common::muted_style)
            .width(LABEL_WIDTH),
        text_input("", &value)
            .on_input(move |typed| Message::HatchPanel(on_input(typed)))
            .style(form::field_style)
            .size(12)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(settings: HatchSettings, drafts: &Drafts) -> Element<'a, Message> {
    let patterns: Vec<String> = crate::scene::model::hatch_patterns::catalog()
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let pattern = form::labeled_pick_list(
        crate::tr!("hatch-panel", "pattern").into(),
        patterns,
        Some(settings.pattern.clone()),
        |name| Message::HatchPanel(HatchPanelMsg::Pattern(name)),
        Length::Fill,
    );
    let angle = field(
        crate::tr!("hatch-panel", "angle"),
        drafts.angle.clone().unwrap_or_else(|| number(settings.angle_deg)),
        HatchPanelMsg::Angle,
    );
    let scale = field(
        crate::tr!("hatch-panel", "scale"),
        drafts.scale.clone().unwrap_or_else(|| number(settings.scale)),
        HatchPanelMsg::Scale,
    );
    let method = settings.method;
    let methods = column![
        form_radio(crate::tr!("hatch-panel", "pick-points"), 0u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
        form_radio(crate::tr!("hatch-panel", "select-objects"), 1u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
        form_radio(crate::tr!("hatch-panel", "draw-boundary"), 2u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
    ]
    .spacing(4);
    let islands = island_code(settings.islands);
    let island_row = row![
        form_radio(crate::tr!("hatch-panel", "normal"), 0u8, Some(islands), |code| {
            Message::HatchPanel(HatchPanelMsg::Islands(code))
        }),
        form_radio(crate::tr!("hatch-panel", "outer"), 1u8, Some(islands), |code| {
            Message::HatchPanel(HatchPanelMsg::Islands(code))
        }),
        form_radio(crate::tr!("hatch-panel", "ignore"), 2u8, Some(islands), |code| {
            Message::HatchPanel(HatchPanelMsg::Islands(code))
        }),
    ]
    .spacing(10);
    let check = |label: String, on: bool, msg: fn(bool) -> HatchPanelMsg| {
        checkbox(on)
            .label(label)
            .text_size(11)
            .size(14)
            .on_toggle(move |value| Message::HatchPanel(msg(value)))
    };
    let gap = field(
        crate::tr!("hatch-panel", "gap-tolerance"),
        drafts.gap.clone().unwrap_or_else(|| number(settings.gap_tolerance)),
        HatchPanelMsg::Gap,
    );
    let buttons = row![
        iced::widget::Space::new().width(Length::Fill),
        dialog_button(crate::tr!("hatch-panel", "cancel"), Message::HatchPanel(HatchPanelMsg::Cancel), false),
        dialog_button(crate::tr!("hatch-panel", "ok"), Message::HatchPanel(HatchPanelMsg::Accept), true),
    ]
    .spacing(6);
    let body = column![
        text(crate::tr!("hatch-panel", "title")).size(13),
        pattern,
        angle,
        scale,
        form::hdivider(Length::Fill),
        form::section_label(crate::tr!("hatch-panel", "boundaries").into()),
        methods,
        form::section_label(crate::tr!("hatch-panel", "islands").into()),
        island_row,
        form::hdivider(Length::Fill),
        check(crate::tr!("hatch-panel", "associative"), settings.associative, HatchPanelMsg::Associative),
        check(crate::tr!("hatch-panel", "separate"), settings.separate, HatchPanelMsg::Separate),
        check(crate::tr!("hatch-panel", "keep-boundaries"), settings.keep_boundaries, HatchPanelMsg::KeepBoundaries),
        gap,
        buttons,
    ]
    .spacing(7)
    .padding(10)
    .width(Length::Fixed(WIDTH));
    container(body)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 6.0.into(),
            },
            text_color: Some(theme.palette().background.base.text),
            ..Default::default()
        })
        .into()
}
