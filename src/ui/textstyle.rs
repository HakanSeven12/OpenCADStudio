//! Text Style Font Browser window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Fill, Theme};

const TB: Color = Color {
    r: 0.13,
    g: 0.13,
    b: 0.13,
    a: 1.0,
};
const BG: Color = Color {
    r: 0.15,
    g: 0.15,
    b: 0.15,
    a: 1.0,
};
const BORDER: Color = Color {
    r: 0.35,
    g: 0.35,
    b: 0.35,
    a: 1.0,
};
const TEXT: Color = Color {
    r: 0.88,
    g: 0.88,
    b: 0.88,
    a: 1.0,
};
const DIM: Color = Color {
    r: 0.55,
    g: 0.55,
    b: 0.55,
    a: 1.0,
};
const ACCENT: Color = Color {
    r: 0.25,
    g: 0.50,
    b: 0.85,
    a: 1.0,
};
const ACTIVE: Color = Color {
    r: 0.20,
    g: 0.40,
    b: 0.70,
    a: 1.0,
};
const FIELD: Color = Color {
    r: 0.10,
    g: 0.10,
    b: 0.10,
    a: 1.0,
};
const LIST: Color = Color {
    r: 0.12,
    g: 0.12,
    b: 0.12,
    a: 1.0,
};

const BUILTIN_FONTS: &[&str] = &[
    "Standard",
    "ISO",
    "Simplex",
    "RomanS",
    "RomanD",
    "RomanC",
    "RomanT",
    "ItalicC",
    "ItalicT",
    "ScriptS",
    "ScriptC",
    "GothGBT",
    "GothGRT",
    "GothITT",
    "Cursive",
    "GreekC",
    "Symbol",
    "ISO",
    "ISO3098",
    "Unicode",
];

fn btn_s(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_: &Theme, st| button::Style {
        background: Some(Background::Color(match (accent, st) {
            (true, button::Status::Hovered | button::Status::Pressed) => Color {
                r: 0.20,
                g: 0.42,
                b: 0.72,
                a: 1.0,
            },
            (false, button::Status::Hovered | button::Status::Pressed) => Color {
                r: 0.28,
                g: 0.28,
                b: 0.28,
                a: 1.0,
            },
            (true, _) => ACCENT,
            _ => Color {
                r: 0.22,
                g: 0.22,
                b: 0.22,
                a: 1.0,
            },
        })),
        text_color: TEXT,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn list_item(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_: &Theme, st| button::Style {
        background: Some(Background::Color(match (active, st) {
            (true, _) => ACTIVE,
            (false, button::Status::Hovered | button::Status::Pressed) => Color {
                r: 0.26,
                g: 0.26,
                b: 0.26,
                a: 1.0,
            },
            _ => Color::TRANSPARENT,
        })),
        text_color: TEXT,
        ..Default::default()
    }
}

fn field_style(_: &Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(FIELD),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: TEXT,
        placeholder: DIM,
        value: TEXT,
        selection: ACCENT,
    }
}

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BORDER)),
            ..Default::default()
        })
        .into()
}

fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BORDER)),
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    styles: Vec<String>,
    selected: &'a str,
    font_buf: &'a str,
    width_buf: &'a str,
    oblique_buf: &'a str,
) -> Element<'a, Message> {
    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            button(text("New").size(11))
                .on_press(Message::TextStyleDialogNew)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text("Delete").size(11))
                .on_press(Message::TextStyleDialogDelete)
                .style(btn_s(false))
                .padding([4, 10]),
            Space::new().width(Fill),
            button(text("Set Current").size(11))
                .on_press(Message::TextStyleDialogSetCurrent)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text("Apply").size(11))
                .on_press(Message::TextStyleApply)
                .style(btn_s(true))
                .padding([4, 14]),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(TB)),
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 8]);

    // ── Left: Style list ──────────────────────────────────────────────────
    let style_items: Vec<Element<'_, Message>> = styles
        .iter()
        .map(|name| {
            let is_sel = name.as_str() == selected;
            button(text(name.clone()).size(11))
                .on_press(Message::TextStyleDialogSelect(name.clone()))
                .style(list_item(is_sel))
                .padding([4, 8])
                .width(Fill)
                .into()
        })
        .collect();

    let style_panel = container(
        column![
            text("Styles").size(10).color(DIM),
            container(scrollable(column(style_items).spacing(1)).height(Fill))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(LIST)),
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(170)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Middle: Font browser ──────────────────────────────────────────────
    let font_items: Vec<Element<'_, Message>> = BUILTIN_FONTS
        .iter()
        .map(|&f| {
            let is_sel = font_buf == f;
            button(text(f).size(10))
                .on_press(Message::TextStyleFontPick(f.to_string()))
                .style(list_item(is_sel))
                .padding([3, 8])
                .width(Fill)
                .into()
        })
        .collect();

    let font_panel = container(
        column![
            text("Font File").size(10).color(DIM),
            container(scrollable(column(font_items).spacing(1)).height(Fill))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(LIST)),
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
            text_input("font file…", font_buf)
                .on_input(|v| Message::TextStyleEdit {
                    field: "font",
                    value: v
                })
                .style(field_style)
                .size(11)
                .width(Fill),
        ]
        .spacing(6)
        .height(Fill),
    )
    .width(190)
    .height(Fill)
    .padding([12, 8]);

    // ── Right: Properties ─────────────────────────────────────────────────
    let props_panel = container(
        column![
            text("Properties").size(11).color(ACCENT),
            row![
                text("Width Factor:").size(11).color(DIM).width(120),
                text_input("1.0", width_buf)
                    .on_input(|v| Message::TextStyleEdit {
                        field: "width",
                        value: v
                    })
                    .style(field_style)
                    .size(11)
                    .width(90),
            ]
            .spacing(6)
            .align_y(iced::Center),
            row![
                text("Oblique (°):").size(11).color(DIM).width(120),
                text_input("0.0", oblique_buf)
                    .on_input(|v| Message::TextStyleEdit {
                        field: "oblique",
                        value: v
                    })
                    .style(field_style)
                    .size(11)
                    .width(90),
            ]
            .spacing(6)
            .align_y(iced::Center),
            Space::new().height(8),
            text("Preview").size(10).color(DIM),
            container(text("AaBbCc 0123").size(20))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(FIELD)),
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: 4.0.into()
                    },
                    ..Default::default()
                })
                .padding(12)
                .width(Fill),
        ]
        .spacing(10)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 12.0,
        bottom: 12.0,
        left: 8.0,
    });

    let body = row![style_panel, vsep(), font_panel, vsep(), props_panel].height(Fill);

    container(column![toolbar, hdivider(), body].spacing(0))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BG)),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
