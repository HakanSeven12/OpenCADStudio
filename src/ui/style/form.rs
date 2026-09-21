//! Form building blocks shared by the in-canvas dialogs.
//!
//! The Plot dialog grew its own `label : control` rows, field and button
//! styles, and section dividers; every dialog that followed copied them. They
//! live here once so a new dialog reads like the existing ones and a theme
//! tweak lands everywhere.

use crate::app::Message;
use crate::ui::style::common::muted_style;
use iced::widget::{button, container, pick_list, row, text, text_input, Space};
use iced::{Background, Border, Element, Length, Theme};
use std::borrow::Cow;

/// Width of the label column in a `label : control` row.
pub const LABEL_WIDTH: f32 = 92.0;

/// Dialog button: accent (primary action) or neutral.
pub fn button_style(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let palette = theme.palette();
        let pair = match (accent, status) {
            (true, button::Status::Hovered | button::Status::Pressed) => palette.primary.strong,
            (false, button::Status::Hovered | button::Status::Pressed) => palette.background.strong,
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        button::Style {
            background: Some(Background::Color(pair.color)),
            text_color: pair.text,
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: iced::Shadow::default(),
            snap: false,
        }
    }
}

/// Text field: base background, neutral border that turns primary on focus.
pub fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

/// One-pixel horizontal rule.
pub fn hdivider<'a>(width: Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
        .into()
}

/// One-pixel vertical rule.
pub fn vseparator<'a>(height: Length) -> Element<'a, Message> {
    container(Space::new().width(1).height(height))
        .width(1)
        .height(height)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
        .into()
}

/// Alias kept for call sites that historically used `vsep`.
/// Same implementation as [`vseparator`].
pub fn vsep<'a>(height: Length) -> Element<'a, Message> {
    vseparator(height)
}

/// Muted section heading.
pub fn section_label<'a>(label: Cow<'static, str>) -> Element<'a, Message> {
    text(label).size(11).style(muted_style).into()
}

fn row_label<'a>(label: Cow<'static, str>) -> Element<'a, Message> {
    text(label)
        .size(11)
        .style(muted_style)
        .width(LABEL_WIDTH)
        .into()
}

/// A `label : dropdown` row.
pub fn labeled_pick_list<'a, T>(
    label: Cow<'static, str>,
    options: Vec<T>,
    selected: Option<T>,
    on_select: impl Fn(T) -> Message + 'a,
    width: Length,
) -> Element<'a, Message>
where
    T: ToString + Clone + PartialEq + 'a,
{
    let list = pick_list(selected, options, |value| value.to_string())
        .on_select(on_select)
        .text_size(12)
        .padding([3, 6])
        .width(width);
    row![row_label(label), list]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// A `label : dropdown` row that renders read-only when `enabled` is false.
pub fn labeled_pick_list_enabled<'a, T>(
    label: Cow<'static, str>,
    options: Vec<T>,
    selected: Option<T>,
    on_select: impl Fn(T) -> Message + 'a,
    width: Length,
    enabled: bool,
) -> Element<'a, Message>
where
    T: ToString + Clone + PartialEq + 'a,
{
    if enabled {
        return labeled_pick_list(label, options, selected, on_select, width);
    }
    row![
        row_label(label),
        crate::ui::read_only::field(
            selected
                .map(|choice| choice.to_string())
                .unwrap_or_default()
                .as_str(),
            12.0,
            width,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A `label : text field` row.
pub fn labeled_field<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    width: f32,
) -> Element<'a, Message> {
    row![
        row_label(label),
        text_input("", value)
            .on_input(on_input)
            .style(field_style)
            .size(12)
            .width(width),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A `label : text field` row that renders read-only when `enabled` is false.
pub fn labeled_field_enabled<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    width: f32,
    enabled: bool,
) -> Element<'a, Message> {
    if enabled {
        return labeled_field(label, value, on_input, width);
    }
    row![
        row_label(label),
        crate::ui::read_only::field(value, 12.0, Length::Fixed(width)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A `label : text field` pair with its own label width, for rows that put
/// several small values side by side.
pub fn labeled_field_compact<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    label_width: f32,
    field_width: f32,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(label_width),
        text_input("", value)
            .on_input(on_input)
            .style(field_style)
            .size(12)
            .padding([3, 6])
            .width(field_width),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}
