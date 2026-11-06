//! The QuickCalc pallet's layout: toolbar, history, input with its preview
//! or error, and the collapsible sections.

use iced::widget::text::{Ellipsis, Span, Wrapping};
use iced::widget::{
    button, column, container, rich_text, row, scrollable, svg, text, text_input, tooltip, Space,
};
use iced::{Background, Border, Color, Element, Fill, Font, Length, Theme};

use super::{
    continues_ans, has_length_suffix, KeypadMode, Preview, QuickCalcMsg, QuickCalcPalette, Section,
    UnitCat, INPUT_ID,
};
use crate::app::calc::{AngleMode, Value};
use crate::app::Message;
use crate::modules::draw::inquiry::calc_pick::PickKind;
use crate::ui::dock::{Chrome, PanelId};

pub const ICON: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/quickcalc.svg");
pub(super) const POINT: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/point.svg");
const DISTANCE: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/distance.svg");
const ANGLE: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/angle.svg");
const COPY: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/copy.svg");
const SEND: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/send.svg");
pub(super) const CHEVRON_DOWN: &[u8] =
    include_bytes!("../../../../assets/icons/quickcalc/chevron_down.svg");
pub(super) const CHEVRON_RIGHT: &[u8] =
    include_bytes!("../../../../assets/icons/quickcalc/chevron_right.svg");
const CHEVRON_UP: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/chevron_up.svg");
const KEYPAD_STANDARD: &[u8] =
    include_bytes!("../../../../assets/icons/quickcalc/keypad_standard.svg");
const SWAP: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/swap.svg");
const ERROR: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/error.svg");

/// The heavier weight for keys, section headers and results. The default
/// font is the system's, which often has no Medium, and asking for one
/// would quietly come back Regular; Semibold is reliably there.
pub(super) const STRONG: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

pub(super) fn m(msg: QuickCalcMsg) -> Message {
    Message::QuickCalc(msg)
}

// ── Colours ─────────────────────────────────────────────────────────────

pub(super) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn luminance(c: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text for the accent fill: near-black (#06121A) or white, whichever reads
/// better, so "=" keeps at least 4.5:1 on any theme's accent.
fn on_accent(accent: Color) -> Color {
    let dark = Color::from_rgb8(0x06, 0x12, 0x1A);
    if contrast(dark, accent) >= contrast(Color::WHITE, accent) {
        dark
    } else {
        Color::WHITE
    }
}

/// The pallet's colours, all derived from the theme so Fusion Black, Fusion
/// White and every other theme get a matching pallet.
#[derive(Clone, Copy)]
pub(super) struct Tokens {
    pub(super) weak: Color,
    pub(super) strong: Color,
    pub(super) line: Color,
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) faint: Color,
    pub(super) accent: Color,
    pub(super) on_accent: Color,
    pub(super) danger: Color,
    /// Error text: the danger colour, lightened on dark chrome for contrast.
    pub(super) danger_text: Color,
    pub(super) field: Color,
    /// Keypad: function and operator keys, digit keys, their border.
    pub(super) key: Color,
    pub(super) key_hover: Color,
    pub(super) digit: Color,
    pub(super) digit_hover: Color,
    pub(super) key_line: Color,
    /// Flyout panels over the keys, and their shadow.
    pub(super) fly: Color,
    pub(super) shadow: Color,
}

/// The colour of a control that is switched off, for text and icons alike.
pub(super) fn disabled(t: &Tokens) -> Color {
    t.faint.scale_alpha(0.55)
}

pub(super) fn tokens(theme: &Theme) -> Tokens {
    let p = theme.palette();
    let dark = p.is_dark;
    let bg = p.background.base.color;
    let text = p.background.base.text;
    // Keys step away from the pallet's background: lighter on dark chrome
    // (#2A2A2A function, #383838 digit on Fusion Black), darker on light
    // chrome with white digits (#F3F3F3 / #FFFFFF on Fusion White).
    let step = |t: f32| {
        if dark {
            mix(bg, Color::WHITE, t)
        } else {
            mix(bg, Color::BLACK, t)
        }
    };
    Tokens {
        weak: p.background.weak.color,
        strong: p.background.strong.color,
        line: p.background.neutral.color,
        text,
        muted: mix(bg, text, 0.68),
        faint: mix(bg, text, 0.52),
        accent: p.primary.base.color,
        on_accent: on_accent(p.primary.base.color),
        danger: p.danger.base.color,
        danger_text: if dark {
            Color::from_rgb8(0xFF, 0x6B, 0x66)
        } else {
            p.danger.base.color
        },
        field: if dark {
            mix(bg, Color::BLACK, 0.3)
        } else {
            Color::WHITE
        },
        key: if dark { step(0.065) } else { step(0.028) },
        key_hover: if dark { step(0.095) } else { step(0.064) },
        digit: if dark { step(0.135) } else { Color::WHITE },
        digit_hover: if dark {
            step(0.17)
        } else {
            mix(Color::WHITE, Color::BLACK, 0.035)
        },
        key_line: if dark { step(0.04) } else { step(0.12) },
        fly: if dark { step(0.05) } else { Color::WHITE },
        shadow: Color::BLACK.scale_alpha(if dark { 0.55 } else { 0.16 }),
    }
}

pub(super) fn icon<'a>(
    bytes: &'static [u8],
    size: f32,
    color: fn(&Tokens) -> Color,
) -> Element<'a, Message> {
    crate::ui::icons::tinted(bytes, size, move |theme: &Theme| color(&tokens(theme)))
}

/// A tooltip as the ribbon shows them: after a short delay, on a panel.
pub(super) fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: String,
) -> Element<'a, Message> {
    tooltip(
        content,
        crate::ui::ribbon::tooltip_content(label),
        tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(400))
    .style(crate::ui::ribbon::tooltip_style)
    .into()
}

/// Full-strength text in the theme's text colour.
pub(super) fn strong<'a>(s: impl text::IntoFragment<'a>, size: f32) -> text::Text<'a, Theme> {
    text(s)
        .size(size)
        .font(STRONG)
        .style(|theme: &Theme| text::Style {
            color: Some(tokens(theme).text),
        })
}

pub(super) fn muted<'a>(s: impl text::IntoFragment<'a>, size: f32) -> text::Text<'a, Theme> {
    text(s).size(size).style(|theme: &Theme| text::Style {
        color: Some(tokens(theme).muted),
    })
}

// ── Buttons ─────────────────────────────────────────────────────────────

/// A 28 px toolbar button; `on` highlights the running pick tool.
fn tool<'a>(bytes: &'static [u8], msg: QuickCalcMsg, on: bool) -> Element<'a, Message> {
    button(
        container(icon(
            bytes,
            18.0,
            if on { |t| t.accent } else { |t| t.muted },
        ))
        .center(Fill),
    )
    .width(28)
    .height(28)
    .padding(0)
    .on_press(m(msg))
    .style(move |theme: &Theme, status| {
        let t = tokens(theme);
        let background = if on {
            Some(Background::Color(t.accent.scale_alpha(0.16)))
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(Background::Color(t.strong))
        } else {
            None
        };
        button::Style {
            background,
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            text_color: t.text,
            ..Default::default()
        }
    })
    .into()
}

pub(super) fn plain_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(Background::Color(t.strong)),
        border: Border {
            color: t.line,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: t.text,
        ..Default::default()
    }
}

/// [`plain_button_style`] without the border, for icon buttons.
pub(super) fn plain_button_style_borderless(
    theme: &Theme,
    status: button::Status,
) -> button::Style {
    let mut s = plain_button_style(theme, status);
    s.border.width = 0.0;
    s
}

/// A bordered text button (Cancel, Insert, New variable).
pub(super) fn text_button<'a>(
    content: impl Into<Element<'a, Message>>,
    msg: QuickCalcMsg,
) -> Element<'a, Message> {
    button(content)
        .padding([4, 10])
        .on_press(m(msg))
        .style(plain_button_style)
        .into()
}

/// One option of a segmented control; the options share the width.
pub(super) fn segment<'a>(label: String, on: bool, msg: QuickCalcMsg) -> Element<'a, Message> {
    button(container(text(label).size(10.5)).center(Fill))
        .width(Fill)
        .height(20)
        .padding([0, 8])
        .on_press(m(msg))
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: on.then_some(Background::Color(t.strong)),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                text_color: if on || matches!(status, button::Status::Hovered) {
                    t.text
                } else {
                    t.muted
                },
                ..Default::default()
            }
        })
        .into()
}

pub(super) fn segmented<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(row(items).spacing(1))
        .padding(2)
        .style(|theme: &Theme| container::Style {
            border: Border {
                color: tokens(theme).line,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

// ── Sections ────────────────────────────────────────────────────────────

/// A section's 28 px header, one button that toggles the section: the
/// title on the left, a chevron on the right that points down while the
/// section is closed and up while it is open.
fn section_header<'a>(section: Section, label: String, open: bool) -> Element<'a, Message> {
    let chevron = icon(if open { CHEVRON_UP } else { CHEVRON_DOWN }, 11.0, |t| {
        t.muted
    });
    let content = row![strong(label, 11.5).width(Fill), chevron]
        .spacing(7)
        .align_y(iced::Center);
    // A button puts its content at the top; centre it in the 28 px bar.
    let toggle = button(container(content).center_y(Fill))
        .width(Fill)
        .height(28)
        .padding(iced::Padding {
            left: 8.0,
            right: 14.0,
            ..iced::Padding::ZERO
        })
        .on_press(m(QuickCalcMsg::Toggle(section)))
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: Some(Background::Color(
                    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        t.strong
                    } else {
                        t.weak
                    },
                )),
                text_color: t.text,
                ..Default::default()
            }
        });
    column![
        container(Space::new().width(Fill).height(1)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(tokens(theme).line)),
            ..Default::default()
        }),
        toggle,
    ]
    .into()
}

// ── Toolbar controls ────────────────────────────────────────────────────

/// The Standard | Scientific switch: a calculator icon and an italic f(x).
fn keypad_mode_switch<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let option = |content: Element<'a, Message>, mode: KeypadMode| {
        let on = p.keypad_mode == mode;
        button(container(content).center(Fill))
            .width(26)
            .height(22)
            .padding(0)
            .on_press(m(QuickCalcMsg::SetKeypadMode(mode)))
            .style(move |theme: &Theme, status| {
                let t = tokens(theme);
                button::Style {
                    background: on.then_some(Background::Color(t.strong)),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    text_color: if on || matches!(status, button::Status::Hovered) {
                        t.text
                    } else {
                        t.muted
                    },
                    ..Default::default()
                }
            })
    };
    let on_std = p.keypad_mode == KeypadMode::Standard;
    // The icon follows the button's text colour: full when active or hovered.
    let calculator = svg(crate::ui::icons::themed_handle(KEYPAD_STANDARD))
        .width(16)
        .height(16)
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            svg::Style {
                color: Some(if on_std || matches!(status, svg::Status::Hovered) {
                    t.text
                } else {
                    t.muted
                }),
            }
        });
    let fx = text("f(x)").size(13).font(super::keypad_view::MATH);
    container(
        row![
            option(calculator.into(), KeypadMode::Standard),
            option(fx.into(), KeypadMode::Scientific),
        ]
        .spacing(1),
    )
    .padding(1)
    .style(|theme: &Theme| container::Style {
        border: Border {
            color: tokens(theme).line,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// DEG or RAD: shows the angle mode and switches to the other one.
fn angle_mode_button<'a>(mode: AngleMode) -> Element<'a, Message> {
    let short = match mode {
        AngleMode::Degrees => "DEG",
        AngleMode::Radians => "RAD",
    };
    button(container(strong(short, 10.5)).center(Fill))
        .width(38)
        .height(24)
        .padding([0, 6])
        .on_press(m(QuickCalcMsg::ToggleAngleMode))
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(t.strong)),
                border: Border {
                    color: t.line,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                text_color: t.text,
                ..Default::default()
            }
        })
        .into()
}

fn chip<'a>(label: &'static str, on: bool, msg: QuickCalcMsg) -> Element<'a, Message> {
    button(container(text(label).size(11.0)).center_y(Fill))
        .height(22)
        .padding([0, 8])
        .on_press(m(msg))
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: on.then_some(Background::Color(t.accent.scale_alpha(0.16))),
                border: Border {
                    color: if on { t.accent } else { t.line },
                    width: 1.0,
                    radius: 11.0.into(),
                },
                text_color: if on || matches!(status, button::Status::Hovered) {
                    t.text
                } else {
                    t.muted
                },
                ..Default::default()
            }
        })
        .into()
}

pub(super) fn field_style(
    theme: &Theme,
    status: text_input::Status,
    bad: bool,
) -> text_input::Style {
    let t = tokens(theme);
    let border = if bad {
        t.danger
    } else if matches!(status, text_input::Status::Focused { .. }) {
        t.accent
    } else {
        t.line
    };
    text_input::Style {
        background: Background::Color(t.field),
        border: Border {
            color: border,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: t.muted,
        placeholder: t.faint,
        value: t.text,
        selection: t.accent.scale_alpha(0.35),
    }
}

fn units<'a>(p: &'a QuickCalcPalette) -> Element<'a, Message> {
    let cat_label = |c: UnitCat| match c {
        UnitCat::Length => crate::t!("Length"),
        UnitCat::Area => crate::t!("Area"),
        UnitCat::Angle => crate::t!("Angle"),
    };
    let cats = segmented(
        UnitCat::ALL
            .iter()
            .map(|c| {
                segment(
                    cat_label(*c).into_owned(),
                    *c == p.unit_cat,
                    QuickCalcMsg::UnitCat(*c),
                )
            })
            .collect(),
    );
    let chips = |selected: &'static str, to: bool| {
        row(p.unit_cat.units().iter().map(|(name, _)| {
            let msg = if to {
                QuickCalcMsg::UnitTo(name)
            } else {
                QuickCalcMsg::UnitFrom(name)
            };
            chip(name, *name == selected, msg)
        }))
        .spacing(4)
    };
    let label = |s: &str| muted(crate::t!(s).into_owned(), 10.5).width(32);
    let result = p
        .converted()
        .map_or_else(|| "—".to_string(), |v| Value::Number(v).display());
    let value_row = row![
        text_input("", &p.unit_value)
            .on_input(|v| m(QuickCalcMsg::UnitValue(v)))
            .size(12)
            .padding([4, 6])
            .width(Length::FillPortion(1))
            .style(|theme, status| field_style(theme, status, false)),
        muted(p.unit_from, 11.0).width(32),
        tip(
            button(icon(SWAP, 14.0, |t| t.muted))
                .padding(4)
                .on_press(m(QuickCalcMsg::UnitSwap))
                .style(plain_button_style_borderless),
            crate::t!("Swap units").into_owned(),
        ),
        container(strong(result, 13.0)).align_right(Length::FillPortion(1)),
        muted(p.unit_to, 11.0).width(32),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let insert: Element<'a, Message> = if p.converted_literal().is_some() {
        text_button(text(crate::t!("Insert")).size(11), QuickCalcMsg::UnitInsert)
    } else {
        Space::new().into()
    };
    column![
        cats,
        row![label("From"), chips(p.unit_from, false)]
            .spacing(6)
            .align_y(iced::Center),
        row![label("To"), chips(p.unit_to, true)]
            .spacing(6)
            .align_y(iced::Center),
        value_row,
        container(insert).align_right(Fill),
    ]
    .spacing(8)
    .padding([8, 2])
    .into()
}

// ── The pallet ──────────────────────────────────────────────────────────

/// Everything the view needs from the app besides the pallet itself.
pub struct Inputs {
    /// The drawing's system variables, for `getvar`.
    pub sysvars: Option<crate::app::calc::SysVars>,
    /// Millimetres per drawing unit.
    pub base_unit: f64,
    /// The drawing unit's short name (`mm`, `m`, `in`, …).
    pub unit_label: &'static str,
    /// The step of the running QuickCalc pick, if one is running.
    pub pick_step: Option<u8>,
    /// The app's theme, for the colours inside the error echo.
    pub theme: Theme,
}

const HISTORY_ROW_H: f32 = 22.0;
/// The gap between the pallet's blocks, the same as the frame's padding
/// beside them.
const GAP: f32 = 6.0;
/// Space between an embedded scrollbar and the content it scrolls.
const SCROLLBAR_GAP: f32 = 4.0;

pub fn view<'a>(
    p: &'a QuickCalcPalette,
    inputs: Inputs,
    width: f32,
    chrome: Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::QuickCalc,
        crate::t!("QuickCalc").into_owned(),
        chrome,
    );
    let body = iced::widget::responsive(move |size| body(p, &inputs, size.height));
    crate::ui::dock::frame(title_bar, body, width)
}

fn body<'a>(p: &'a QuickCalcPalette, inputs: &Inputs, height: f32) -> Element<'a, Message> {
    let (ctx, var_errors) = p.context_and_errors(inputs.base_unit, inputs.sysvars.clone());
    let preview = p.preview(&ctx);
    let picking = inputs.pick_step.zip(p.pick);
    let active = |kind: PickKind| picking.is_some_and(|(_, k)| k == kind);

    // Toolbar.
    let divider =
        container(Space::new().width(1).height(16)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(tokens(theme).line)),
            ..Default::default()
        });
    let toolbar = row![
        tool(
            POINT,
            QuickCalcMsg::Pick(PickKind::Point),
            active(PickKind::Point)
        ),
        tool(
            DISTANCE,
            QuickCalcMsg::Pick(PickKind::Distance),
            active(PickKind::Distance)
        ),
        tool(
            ANGLE,
            QuickCalcMsg::Pick(PickKind::Angle),
            active(PickKind::Angle)
        ),
        container(divider).padding([0, 4]),
        tool(crate::ui::icons::TRASH, QuickCalcMsg::ClearHistory, false),
        tool(COPY, QuickCalcMsg::Copy, false),
        tool(SEND, QuickCalcMsg::PasteToCommandLine, false),
        Space::new().width(Fill),
        keypad_mode_switch(p),
        container(angle_mode_button(p.mode)).padding(iced::Padding {
            left: 4.0,
            ..iced::Padding::ZERO
        }),
    ]
    .spacing(2)
    .align_y(iced::Center);

    // History, newest at the bottom next to the input. Empty, it is just
    // empty space. The list around the rows is built once the layout knows
    // whether they overflow (see below).
    let history_rows: Vec<Element<'a, Message>> = {
        let rows = p.history.iter().enumerate().map(|(i, e)| {
            let expr = button(
                muted(e.expr.clone(), 11.5)
                    .wrapping(Wrapping::None)
                    .ellipsis(Ellipsis::End),
            )
            .width(Fill)
            .padding(0)
            .on_press(m(QuickCalcMsg::HistoryLoad(i)))
            .style(button::text);
            let result = button(text(e.value.display()).size(12.5).font(STRONG))
                .padding(0)
                .on_press(m(QuickCalcMsg::HistoryCopy(i)))
                .style(|theme: &Theme, status| {
                    let t = tokens(theme);
                    button::Style {
                        text_color: if matches!(status, button::Status::Hovered) {
                            t.accent
                        } else {
                            t.text
                        },
                        ..Default::default()
                    }
                });
            container(
                row![expr, tip(result, crate::t!("Copy result").into_owned()),]
                    .spacing(10)
                    .align_y(iced::Center),
            )
            .padding([3, 4])
            .into()
        });
        rows.collect()
    };

    // Input, with the pick banner above it and the preview or error below.
    let mut input_block = column![].spacing(6);
    if let Some((step, kind)) = picking {
        let (title, prompt) = match (kind, step) {
            (PickKind::Point, _) => (
                crate::t!("Get point"),
                crate::t!("Specify a point in the drawing"),
            ),
            (PickKind::Distance, 1) => (
                crate::t!("Distance"),
                crate::t!("Specify first point in the drawing"),
            ),
            (PickKind::Distance, _) => (
                crate::t!("Distance"),
                crate::t!("Specify second point in the drawing"),
            ),
            (PickKind::Angle, 1) => (
                crate::t!("Angle"),
                crate::t!("Specify first point in the drawing"),
            ),
            (PickKind::Angle, _) => (
                crate::t!("Angle"),
                crate::t!("Specify second point in the drawing"),
            ),
        };
        let mut heading = row![text(title.into_owned()).size(11.5)].spacing(6);
        if kind.steps() > 1 {
            heading = heading.push(muted(
                crate::tf!("step {} of {}", step, kind.steps()).into_owned(),
                11.5,
            ));
        }
        input_block = input_block.push(
            container(
                row![
                    icon(POINT, 16.0, |t| t.accent),
                    column![heading, text(prompt.into_owned()).size(11.5)]
                        .spacing(1)
                        .width(Fill),
                    text_button(text(crate::t!("Cancel")).size(11), QuickCalcMsg::CancelPick),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .padding([7, 8])
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(tokens(theme).accent.scale_alpha(0.16))),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
    }
    let show_error = matches!(preview, Preview::Err(_)) && picking.is_none() && p.error_shown();
    let bad = show_error;
    let placeholder = if p.history.is_empty() {
        crate::t!("Expression, e.g. 2*300 + 90")
    } else {
        crate::t!("Expression, or start with * to reuse ans")
    };
    input_block = input_block.push(
        text_input(&placeholder, &p.input)
            .id(iced::widget::Id::new(INPUT_ID))
            .on_input(|v| m(QuickCalcMsg::Input(v)))
            .on_submit(m(QuickCalcMsg::Submit))
            .size(13)
            .padding([6, 8])
            .style(move |theme, status| field_style(theme, status, bad)),
    );
    let trimmed = p.input.trim();
    let flash = p.flash.as_ref().map(|(message, _)| message.clone());
    match &preview {
        Preview::Err(e) if show_error => {
            let start = e.span.start.min(p.input.len());
            let end = e.span.end.clamp(start, p.input.len());
            let bad_text = match &p.input[start..end] {
                "" => " ".to_string(),
                s => s.to_string(),
            };
            let t = tokens(&inputs.theme);
            let echo = rich_text::<(), _, _, _>(vec![
                Span::new(p.input[..start].to_string()).color(t.muted),
                Span::new(bad_text)
                    .color(t.danger_text)
                    .background(t.danger.scale_alpha(0.18))
                    .underline(true),
                Span::new(p.input[end..].to_string()).color(t.muted),
            ])
            .size(12)
            .wrapping(Wrapping::None);
            input_block = input_block.push(
                column![
                    echo,
                    row![
                        icon(ERROR, 13.0, |t| t.danger_text),
                        text(e.message.clone())
                            .size(11.5)
                            .style(|theme: &Theme| text::Style {
                                color: Some(tokens(theme).danger_text),
                            }),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                ]
                .spacing(3),
            );
        }
        _ if picking.is_some() => {}
        Preview::Ok(value) => {
            let lead = if let Some((name, _)) = crate::app::calc::assigned_variable(trimmed) {
                crate::tf!("sets ${}", name).into_owned()
            } else if continues_ans(trimmed) {
                format!("ans {trimmed}")
            } else if has_length_suffix(trimmed) {
                crate::tf!("in {}", inputs.unit_label).into_owned()
            } else {
                String::new()
            };
            input_block = input_block.push(preview_line(
                lead,
                format!("= {}", value.display()),
                flash.clone(),
            ));
        }
        _ => {
            let lead = p
                .ans
                .map(|a| format!("ans = {}", a.display()))
                .unwrap_or_default();
            input_block = input_block.push(preview_line(lead, String::new(), flash.clone()));
        }
    }

    // Sections.
    let mut sections = column![];
    for section in Section::ALL {
        let open = p.is_open(section);
        let label = match section {
            Section::Units => crate::t!("Units"),
            Section::Variables => crate::t!("Variables"),
        };
        sections = sections.push(section_header(section, label.into_owned(), open));
        if open {
            sections = sections.push(match section {
                Section::Units => units(p),
                Section::Variables => super::variables_view::variables(
                    p,
                    &ctx,
                    &var_errors,
                    p.form
                        .as_ref()
                        .map(|_| p.check_form(inputs.base_unit, inputs.sysvars.clone())),
                ),
            });
        }
    }

    // One scroll for everything below the toolbar, as in the other pallets.
    // While it all fits, the history fills the room above the input with
    // its newest row at the bottom. Once it doesn't, the history takes its
    // natural height and the whole column scrolls, held at the bottom so
    // the input, the keypad and the sections stay in view.
    let input_h = 30.0
        + 6.0
        + if show_error { 38.0 } else { 20.0 }
        + if picking.is_some() { 52.0 } else { 0.0 };
    let below_toolbar = height - 28.0 - GAP;
    let history_h = history_rows.len() as f32 * HISTORY_ROW_H;
    let rest = input_h + super::keypad_view::height(p) + sections_height(p) + 3.0 * GAP;
    let overflows = history_h + rest > below_toolbar;
    let history = container(column(history_rows))
        .align_bottom(if overflows {
            Length::Shrink
        } else {
            Length::Fill
        })
        .width(Fill);
    let content = column![
        history,
        input_block,
        super::keypad_view::keypad(p),
        sections,
    ]
    .spacing(GAP);
    let content: Element<'a, Message> = if overflows {
        // The scrollbar takes its own room beside the content (`spacing`
        // embeds it) instead of covering the keys or the results.
        scrollable(content)
            .anchor_bottom()
            .spacing(SCROLLBAR_GAP)
            .height(Fill)
            .into()
    } else {
        content.height(Fill).into()
    };

    column![toolbar, content].spacing(GAP).height(Fill).into()
}

/// The height a newly floated pallet opens at: the toolbar, a few history
/// rows, the input, the Standard keypad and both section headers closed.
pub fn float_height() -> f32 {
    const FRAME_PADDING: f32 = 2.0 * 6.0;
    const HISTORY_ROWS: f32 = 3.0;
    let p = QuickCalcPalette::default();
    FRAME_PADDING
        + 28.0
        + HISTORY_ROWS * HISTORY_ROW_H
        + (30.0 + 6.0 + 20.0)
        + super::keypad_view::height(&p)
        + sections_height(&p)
        + 4.0 * GAP
}

/// The sections' height when laid out in full: every header plus the open
/// bodies, from the fixed row sizes they are built with.
fn sections_height(p: &QuickCalcPalette) -> f32 {
    const HEADER: f32 = 29.0;
    Section::ALL
        .iter()
        .map(|section| {
            HEADER
                + if p.is_open(*section) {
                    match section {
                        Section::Units => 24.0 + 2.0 * 22.0 + 28.0 + 26.0 + 4.0 * 8.0 + 16.0,
                        Section::Variables => super::variables_view::height(p),
                    }
                } else {
                    0.0
                }
        })
        .sum()
}

/// The line under the input: what the value means (`sets wall`, `in mm`) and
/// the value. A status message ("Copied 870") takes the lead's place while
/// it shows.
fn preview_line<'a>(lead: String, value: String, status: Option<String>) -> Element<'a, Message> {
    let flashing = status.is_some();
    row![
        text(status.unwrap_or(lead))
            .size(11.0)
            .wrapping(Wrapping::None)
            .ellipsis(Ellipsis::End)
            .width(Fill)
            .style(move |theme: &Theme| {
                let t = tokens(theme);
                text::Style {
                    color: Some(if flashing { t.accent } else { t.muted }),
                }
            }),
        strong(value, 16.0),
    ]
    .spacing(8)
    .height(Length::Fixed(20.0))
    .align_y(iced::Center)
    .into()
}
