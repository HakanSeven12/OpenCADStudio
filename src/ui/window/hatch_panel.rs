//! The Hatch panel: HATCH's settings in one place while the command runs —
//! fill, angle and scale, origin, the boundary method, islands, the options
//! and the properties the hatch takes — floating over the drawing so
//! boundaries can still be picked.

use crate::app::Message;
use crate::modules::draw::draw::hatch::{HatchFill, HatchSettings};
use crate::scene::model::hatch_model::GradientKind;
use crate::ui::style::form::{self, dialog_button, form_radio, LABEL_WIDTH};
use crate::ui::window::attman_dialog::{color_label, COLORS};
use document::entities::HatchStyleType;
use iced::widget::{checkbox, column, container, row, text, text_input};
use iced::{Background, Border, Element, Length, Theme};

#[derive(Debug, Clone)]
pub enum HatchPanelMsg {
    /// 0 solid, 1 gradient, 2 pattern, 3 user defined.
    Kind(u8),
    Pattern(String),
    /// A gradient's choice label (see `GradientKind::choice_label`).
    Gradient(String),
    Angle(String),
    Scale(String),
    Spacing(String),
    Double(bool),
    OriginX(String),
    OriginY(String),
    Gap(String),
    Method(u8),
    Islands(u8),
    Associative(bool),
    Separate(bool),
    KeepBoundaries(bool),
    /// A colour label, or "Use current".
    Color(String),
    /// A colour label, or "None".
    Background(String),
    GradientColor1(String),
    GradientColor2(String),
    /// A layer name, or "Use current".
    Layer(String),
    /// 0–90, or empty for the current transparency.
    Transparency(String),
    /// HPDRAWORDER, 0–4.
    DrawOrder(u8),
    Annotative(bool),
    Accept,
    Cancel,
}

/// Numbers as typed, shown until HATCH starts again: a half-typed value
/// ("1.") is kept while only valid ones reach the command.
#[derive(Debug, Clone, Default)]
pub struct Drafts {
    pub angle: Option<String>,
    pub scale: Option<String>,
    pub spacing: Option<String>,
    pub origin_x: Option<String>,
    pub origin_y: Option<String>,
    pub gap: Option<String>,
    pub transparency: Option<String>,
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

pub fn kind_code(fill: &HatchFill) -> u8 {
    match fill {
        HatchFill::Solid => 0,
        HatchFill::Gradient(..) => 1,
        HatchFill::Pattern(_) => 2,
        HatchFill::User { .. } => 3,
    }
}

/// "Use current" at the top of the colour and layer lists.
pub fn current_label() -> String {
    crate::tr!("hatch-panel", "use-current")
}

/// "None" at the top of the background list.
pub fn none_label() -> String {
    crate::tr!("hatch-panel", "none")
}

/// A labelled choice in a list, kept by its code.
#[derive(Debug, Clone, PartialEq)]
struct Choice(u8, String);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn choices(keys: &[&str]) -> Vec<Choice> {
    keys.iter()
        .enumerate()
        .map(|(code, key)| Choice(code as u8, crate::i18n::loader().get_attr("hatch-panel", key)))
        .collect()
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

fn list<'a>(
    label: String,
    options: Vec<String>,
    selected: String,
    on: fn(String) -> HatchPanelMsg,
) -> Element<'a, Message> {
    form::labeled_pick_list(
        label.into(),
        options,
        Some(selected),
        move |value| Message::HatchPanel(on(value)),
        Length::Fill,
    )
}

/// A swatch of the fill as it is set.
fn preview<'a>(settings: &HatchSettings) -> Element<'a, Message> {
    let swatch = match &settings.fill {
        HatchFill::Gradient(kind, invert) => crate::ui::window::gradient_editor::preview_image(
            crate::ui::window::gradient_editor::fill_preview(
                *kind,
                *invert,
                settings.gradient_colors[0].clone(),
                settings.gradient_colors[1].clone(),
                settings.angle_deg,
            ),
        ),
        fill => {
            let (_, pattern) = crate::modules::draw::draw::hatch::fill_pattern(
                fill,
                0.0,
                &settings.gradient_colors,
            );
            crate::ui::properties::hatch_pattern_preview(pattern, 32.0)
        }
    };
    container(swatch)
        .height(Length::Fixed(32.0))
        .width(Length::Fill)
        .into()
}

pub fn view<'a>(settings: HatchSettings, drafts: &Drafts, layers: Vec<String>) -> Element<'a, Message> {
    let tr = |key: &str| crate::i18n::loader().get_attr("hatch-panel", key);
    let kinds = choices(&["solid", "gradient", "pattern", "user-defined"]);
    let kind = form::labeled_pick_list(
        tr("type").into(),
        kinds.clone(),
        kinds.get(kind_code(&settings.fill) as usize).cloned(),
        |choice: Choice| Message::HatchPanel(HatchPanelMsg::Kind(choice.0)),
        Length::Fill,
    );
    let mut fill = column![kind].spacing(7);
    match &settings.fill {
        HatchFill::Pattern(name) => {
            let patterns: Vec<String> = crate::scene::model::hatch_patterns::catalog()
                .iter()
                .map(|entry| entry.name.clone())
                .collect();
            fill = fill.push(list(tr("pattern"), patterns, name.clone(), HatchPanelMsg::Pattern));
        }
        HatchFill::Gradient(gradient, invert) => {
            let names: Vec<String> = GradientKind::CHOICES
                .iter()
                .map(|(kind, inverted)| kind.choice_label(*inverted).to_string())
                .collect();
            fill = fill.push(list(
                tr("gradient"),
                names,
                gradient.choice_label(*invert).to_string(),
                HatchPanelMsg::Gradient,
            ));
        }
        _ => {}
    }
    fill = fill.push(preview(&settings));
    if matches!(settings.fill, HatchFill::Gradient(..)) {
        let stops: Vec<String> = COLORS[2..].iter().map(|c| c.to_string()).collect();
        fill = fill
            .push(list(
                tr("color-1"),
                stops.clone(),
                color_label(settings.gradient_colors[0].clone()),
                HatchPanelMsg::GradientColor1,
            ))
            .push(list(
                tr("color-2"),
                stops,
                color_label(settings.gradient_colors[1].clone()),
                HatchPanelMsg::GradientColor2,
            ));
    } else {
        let mut colors = vec![current_label()];
        colors.extend(COLORS.iter().map(|c| c.to_string()));
        let mut backgrounds = vec![none_label()];
        backgrounds.extend(COLORS.iter().map(|c| c.to_string()));
        fill = fill
            .push(list(
                tr("color"),
                colors,
                settings.color.clone().map_or_else(current_label, color_label),
                HatchPanelMsg::Color,
            ))
            .push(list(
                tr("background"),
                backgrounds,
                settings.background.clone().map_or_else(none_label, color_label),
                HatchPanelMsg::Background,
            ));
    }
    fill = fill.push(field(
        tr("angle"),
        drafts.angle.clone().unwrap_or_else(|| number(settings.angle_deg)),
        HatchPanelMsg::Angle,
    ));
    match settings.fill {
        HatchFill::Pattern(_) => {
            fill = fill.push(field(
                tr("scale"),
                drafts.scale.clone().unwrap_or_else(|| number(settings.scale)),
                HatchPanelMsg::Scale,
            ));
        }
        HatchFill::User { spacing, double } => {
            fill = fill
                .push(field(
                    tr("spacing"),
                    drafts.spacing.clone().unwrap_or_else(|| number(spacing)),
                    HatchPanelMsg::Spacing,
                ))
                .push(
                    checkbox(double)
                        .label(tr("double"))
                        .text_size(11)
                        .size(14)
                        .on_toggle(|on| Message::HatchPanel(HatchPanelMsg::Double(on))),
                );
        }
        _ => {}
    }
    if matches!(settings.fill, HatchFill::Pattern(_) | HatchFill::User { .. }) {
        fill = fill
            .push(field(
                tr("origin-x"),
                drafts.origin_x.clone().unwrap_or_else(|| number(settings.origin[0])),
                HatchPanelMsg::OriginX,
            ))
            .push(field(
                tr("origin-y"),
                drafts.origin_y.clone().unwrap_or_else(|| number(settings.origin[1])),
                HatchPanelMsg::OriginY,
            ));
    }
    let method = settings.method;
    let methods = column![
        form_radio(tr("pick-points"), 0u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
        form_radio(tr("select-objects"), 1u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
        form_radio(tr("draw-boundary"), 2u8, Some(method), |m| {
            Message::HatchPanel(HatchPanelMsg::Method(m))
        }),
    ]
    .spacing(4);
    let islands = island_code(settings.islands);
    let island_row = row![
        form_radio(tr("normal"), 0u8, Some(islands), |code| {
            Message::HatchPanel(HatchPanelMsg::Islands(code))
        }),
        form_radio(tr("outer"), 1u8, Some(islands), |code| {
            Message::HatchPanel(HatchPanelMsg::Islands(code))
        }),
        form_radio(tr("ignore"), 2u8, Some(islands), |code| {
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
        tr("gap-tolerance"),
        drafts.gap.clone().unwrap_or_else(|| number(settings.gap_tolerance)),
        HatchPanelMsg::Gap,
    );
    let orders = choices(&["order-none", "order-back", "order-front", "order-behind", "order-in-front"]);
    let draw_order = form::labeled_pick_list(
        tr("draw-order").into(),
        orders.clone(),
        orders.get(settings.draw_order.min(4) as usize).cloned(),
        |choice: Choice| Message::HatchPanel(HatchPanelMsg::DrawOrder(choice.0)),
        Length::Fill,
    );
    let mut layer_names = vec![current_label()];
    layer_names.extend(layers);
    let layer = list(
        tr("layer"),
        layer_names,
        settings.layer.clone().unwrap_or_else(current_label),
        HatchPanelMsg::Layer,
    );
    let transparency = field(
        tr("transparency"),
        drafts.transparency.clone().unwrap_or_else(|| {
            settings.transparency.map(|t| t.to_string()).unwrap_or_default()
        }),
        HatchPanelMsg::Transparency,
    );
    let buttons = row![
        iced::widget::Space::new().width(Length::Fill),
        dialog_button(crate::tr!("hatch-panel", "cancel"), Message::HatchPanel(HatchPanelMsg::Cancel), false),
        dialog_button(crate::tr!("hatch-panel", "ok"), Message::HatchPanel(HatchPanelMsg::Accept), true),
    ]
    .spacing(6);
    let body = column![
        text(crate::tr!("hatch-panel", "title")).size(13),
        fill,
        form::hdivider(Length::Fill),
        form::section_label(tr("boundaries").into()),
        methods,
        form::section_label(tr("islands").into()),
        island_row,
        form::hdivider(Length::Fill),
        check(tr("associative"), settings.associative, HatchPanelMsg::Associative),
        check(tr("separate"), settings.separate, HatchPanelMsg::Separate),
        check(tr("keep-boundaries"), settings.keep_boundaries, HatchPanelMsg::KeepBoundaries),
        check(tr("annotative"), settings.annotative, HatchPanelMsg::Annotative),
        gap,
        draw_order,
        layer,
        transparency,
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
