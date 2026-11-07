//! The colour editor: RGB, HSB or CMYK sliders above a saturation/brightness
//! field with a hue strip, plus a hex box. Shared by the CAD Select Color
//! panel and the interface colour picker.

use super::math::{self, Hsb};
use super::widgets::ColorArea;
use super::ColorPickerMsg;
use crate::app::Message;
use crate::t;
use iced::widget::{
    button, column, container, row, slider, stack, text, text_input, vertical_slider, Space,
};
use iced::{Background, Border, Color, Element, Gradient, Length, Radians, Theme};

const ROW_HEIGHT: f32 = 26.0;
const TRACK: f32 = 18.0;
const STRIP_WIDTH: f32 = 22.0;
const ROW_GAP: f32 = 6.0;
/// The field's height in the interface picker, which has no fixed height.
const FIELD_HEIGHT: f32 = 96.0;
const MODE_WIDTH: f32 = 48.0;
const MODE_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Rgb,
    Hsb,
    Cmyk,
}

#[derive(Debug, Clone)]
pub enum EditorMsg {
    Mode(EditorMode),
    Hue(f32),
    /// Saturation in percent.
    Saturation(f32),
    /// Brightness in percent.
    Brightness(f32),
    /// Saturation and brightness from the field, both 0–1.
    Field(f32, f32),
    Red(u8),
    Green(u8),
    Blue(u8),
    /// One CMYK channel (0 cyan … 3 black), in percent.
    Cmyk(usize, f32),
    HexInput(String),
    HexSubmit,
}

pub struct ColorEditor {
    mode: EditorMode,
    /// Kept beside `rgb` so hue and saturation survive grays and black.
    hsb: Hsb,
    /// Kept too, so black keeps its inks and rounding doesn't move them.
    cmyk: [f32; 4],
    rgb: (u8, u8, u8),
    hex_draft: Option<String>,
}

impl ColorEditor {
    pub fn new(rgb: (u8, u8, u8)) -> Self {
        let mut editor = Self {
            mode: EditorMode::default(),
            hsb: Hsb {
                h: 0.0,
                s: 0.0,
                b: 1.0,
            },
            cmyk: [0.0; 4],
            rgb,
            hex_draft: None,
        };
        editor.set_rgb(rgb);
        editor
    }

    pub fn rgb(&self) -> (u8, u8, u8) {
        self.rgb
    }

    pub fn set_rgb(&mut self, (r, g, b): (u8, u8, u8)) {
        let next = math::rgb_to_hsb(r, g, b);
        // A gray has no hue and black no saturation; keep the previous ones
        // so the sliders and field don't jump.
        let gray = next.s == 0.0 || next.b == 0.0;
        self.hsb = Hsb {
            h: if gray { self.hsb.h } else { next.h },
            s: if next.b == 0.0 { self.hsb.s } else { next.s },
            b: next.b,
        };
        self.cmyk = math::rgb_to_cmyk(r, g, b);
        self.rgb = (r, g, b);
        self.hex_draft = None;
    }

    fn set_hsb(&mut self, hsb: Hsb) {
        self.hsb = Hsb {
            h: hsb.h.clamp(0.0, 360.0),
            s: hsb.s.clamp(0.0, 1.0),
            b: hsb.b.clamp(0.0, 1.0),
        };
        self.rgb = self.hsb.to_rgb();
        self.cmyk = math::rgb_to_cmyk(self.rgb.0, self.rgb.1, self.rgb.2);
        self.hex_draft = None;
    }

    fn set_cmyk(&mut self, channel: usize, value: f32) {
        let mut cmyk = self.cmyk;
        cmyk[channel.min(3)] = value.clamp(0.0, 100.0);
        self.set_rgb(math::cmyk_to_rgb(cmyk));
        self.cmyk = cmyk;
    }

    /// Apply a message; returns whether the colour changed.
    pub fn update(&mut self, msg: EditorMsg) -> bool {
        let before = self.rgb;
        let (r, g, b) = self.rgb;
        match msg {
            EditorMsg::Mode(mode) => self.mode = mode,
            EditorMsg::Hue(h) => self.set_hsb(Hsb { h, ..self.hsb }),
            EditorMsg::Saturation(s) => self.set_hsb(Hsb {
                s: s / 100.0,
                ..self.hsb
            }),
            EditorMsg::Brightness(v) => self.set_hsb(Hsb {
                b: v / 100.0,
                ..self.hsb
            }),
            EditorMsg::Field(s, b) => self.set_hsb(Hsb { s, b, ..self.hsb }),
            EditorMsg::Red(v) => self.set_rgb((v, g, b)),
            EditorMsg::Green(v) => self.set_rgb((r, v, b)),
            EditorMsg::Blue(v) => self.set_rgb((r, g, v)),
            EditorMsg::Cmyk(channel, v) => self.set_cmyk(channel, v),
            EditorMsg::HexInput(value) => {
                // Six digits are unambiguous, so apply them while typing; the
                // short #RGB form waits for Enter.
                let complete = value.trim().trim_start_matches('#').len() == 6;
                if let Some(rgb) = math::parse_hex(&value).filter(|_| complete) {
                    self.set_rgb(rgb);
                }
                self.hex_draft = Some(value);
            }
            EditorMsg::HexSubmit => {
                if let Some(rgb) = self.hex_draft.take().and_then(|d| math::parse_hex(&d)) {
                    self.set_rgb(rgb);
                }
            }
        }
        self.rgb != before
    }

    /// `mode_end` sits at the far right of the mode buttons (the pipette);
    /// `hex_end` at the far right of the hex row (the nearest index colour).
    /// With `fill` the field takes all the height the sliders leave, so the
    /// hex row stays at the bottom and CMYK's fourth slider only makes the
    /// field a little lower.
    pub fn view<'a>(
        &'a self,
        mode_end: Option<Element<'a, Message>>,
        hex_end: Option<Element<'a, Message>>,
        fill: bool,
    ) -> Element<'a, Message> {
        let height = if fill {
            Length::Fill
        } else {
            Length::Fixed(FIELD_HEIGHT)
        };
        column![
            self.mode_row(mode_end),
            self.slider_rows(),
            self.field(height),
            self.hex_row(hex_end),
        ]
        .spacing(12)
        .into()
    }

    /// RGB, HSB and CMYK, with `end` at the far right.
    fn mode_row<'a>(&self, end: Option<Element<'a, Message>>) -> Element<'a, Message> {
        let mode_button = |label: &'static str, mode: EditorMode| {
            let active = self.mode == mode;
            button(container(text(label).size(13)).center(Length::Fill))
                .on_press(msg(EditorMsg::Mode(mode)))
                .width(MODE_WIDTH)
                .height(MODE_HEIGHT)
                .padding(0)
                .style(move |theme: &Theme, status| {
                    super::picker::toggle_style(theme, active, status)
                })
        };
        let mut modes = row![
            mode_button("RGB", EditorMode::Rgb),
            mode_button("HSB", EditorMode::Hsb),
            mode_button("CMYK", EditorMode::Cmyk),
            Space::new().width(Length::Fill),
        ]
        .spacing(6);
        if let Some(end) = end {
            modes = modes.push(end);
        }
        modes.into()
    }

    /// Gradient sliders: red, green and blue; hue, saturation and
    /// brightness; or cyan, magenta, yellow and black.
    fn slider_rows<'a>(&self) -> Element<'a, Message> {
        let Hsb { h, s, b } = self.hsb;
        let (r, g, bl) = self.rgb;
        let rows = if self.mode == EditorMode::Cmyk {
            let ink = |i: usize, letter| {
                let at = |v| {
                    let mut cmyk = self.cmyk;
                    cmyk[i] = v;
                    rgb_color(math::cmyk_to_rgb(cmyk))
                };
                channel(
                    letter,
                    self.cmyk[i],
                    100.0,
                    Some('%'),
                    vec![(0.0, at(0.0)), (1.0, at(100.0))],
                    move |v| EditorMsg::Cmyk(i, v),
                )
            };
            column![ink(0, "C"), ink(1, "M"), ink(2, "Y"), ink(3, "K")]
        } else if self.mode == EditorMode::Rgb {
            let ends = |lo: (u8, u8, u8), hi: (u8, u8, u8)| {
                vec![(0.0, rgb_color(lo)), (1.0, rgb_color(hi))]
            };
            let byte = |f: fn(u8) -> EditorMsg| move |v: f32| f(v.round() as u8);
            column![
                channel(
                    "R",
                    f32::from(r),
                    255.0,
                    None,
                    ends((0, g, bl), (255, g, bl)),
                    byte(EditorMsg::Red)
                ),
                channel(
                    "G",
                    f32::from(g),
                    255.0,
                    None,
                    ends((r, 0, bl), (r, 255, bl)),
                    byte(EditorMsg::Green)
                ),
                channel(
                    "B",
                    f32::from(bl),
                    255.0,
                    None,
                    ends((r, g, 0), (r, g, 255)),
                    byte(EditorMsg::Blue)
                ),
            ]
        } else {
            column![
                channel("H", h, 360.0, Some('°'), hue_stops(), EditorMsg::Hue),
                channel(
                    "S",
                    s * 100.0,
                    100.0,
                    Some('%'),
                    vec![(0.0, hsb_color(h, 0.0, b)), (1.0, hsb_color(h, 1.0, b))],
                    EditorMsg::Saturation,
                ),
                channel(
                    "B",
                    b * 100.0,
                    100.0,
                    Some('%'),
                    vec![(0.0, Color::BLACK), (1.0, hsb_color(h, s, 1.0))],
                    EditorMsg::Brightness,
                ),
            ]
        };
        rows.spacing(ROW_GAP).into()
    }

    /// The saturation/brightness field with its hue strip.
    fn field<'a>(&self, height: Length) -> Element<'a, Message> {
        let Hsb { h, s, b } = self.hsb;
        let area = ColorArea::new(hsb_color(h, 1.0, 1.0), rgb_color(self.rgb), s, b, |s, b| {
            msg(EditorMsg::Field(s, b))
        })
        .height(height);
        // A vertical slider grows upwards; the strip runs red at the top.
        let strip = stack![
            container(Space::new())
                .width(STRIP_WIDTH)
                .height(height)
                .style(|_theme: &Theme| gradient_box(std::f32::consts::PI, &hue_stops())),
            vertical_slider(0.0..=360.0, 360.0 - h, |v| msg(EditorMsg::Hue(360.0 - v)))
                .width(STRIP_WIDTH)
                .height(height)
                .style(handle_style),
        ];
        row![area, strip].spacing(12).into()
    }

    /// The hex value with `end` (the nearest index colour) at the far right.
    fn hex_row<'a>(&self, end: Option<Element<'a, Message>>) -> Element<'a, Message> {
        let (r, g, b) = self.rgb;
        let hex = self
            .hex_draft
            .clone()
            .unwrap_or_else(|| math::hex_string(r, g, b));
        let mut hex_row = row![
            text(t!("Hex")).size(11).style(super::picker::muted_text),
            text_input("", &hex)
                .on_input(|value| msg(EditorMsg::HexInput(value)))
                .on_submit(msg(EditorMsg::HexSubmit))
                .size(12)
                .font(iced::Font::MONOSPACE)
                .padding([5, 6])
                .width(Length::Fixed(74.0)),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(iced::Center);
        if let Some(end) = end {
            hex_row = hex_row.push(end);
        }
        hex_row.into()
    }
}

fn msg(editor: EditorMsg) -> Message {
    Message::ColorPicker(ColorPickerMsg::Editor(editor))
}

/// One labelled channel: a gradient slider and its number box.
fn channel<'a>(
    letter: &'static str,
    value: f32,
    max: f32,
    unit: Option<char>,
    stops: Vec<(f32, Color)>,
    on_change: impl Fn(f32) -> EditorMsg + Copy + 'static,
) -> Element<'a, Message> {
    // iced's slider splits its own rail at the handle, which would squeeze a
    // gradient; draw the gradient underneath and keep the rail transparent.
    let track = container(
        container(Space::new())
            .width(Length::Fill)
            .height(TRACK)
            .style(move |_theme: &Theme| gradient_box(std::f32::consts::FRAC_PI_2, &stops)),
    )
    .height(ROW_HEIGHT)
    .center_y(ROW_HEIGHT);
    row![
        container(text(letter).size(12).font(iced::Font::MONOSPACE))
            .width(12)
            .align_x(iced::Center),
        stack![
            track,
            slider(0.0..=max, value, move |v| msg(on_change(v)))
                .height(ROW_HEIGHT)
                .shift_step(10.0)
                .style(handle_style),
        ],
        number_box(value, max, unit, on_change),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A compact spin box with the unit drawn inside, after the digits.
fn number_box(
    value: f32,
    max: f32,
    unit: Option<char>,
    on_change: impl Fn(f32) -> EditorMsg + Copy + 'static,
) -> Element<'static, Message> {
    let input =
        iced_aw::widget::NumberInput::new(&(value.round() as u16), 0..=max as u16, move |v| {
            msg(on_change(f32::from(v)))
        })
        .set_size(12.0)
        .font(iced::Font::MONOSPACE)
        .width(Length::Fixed(54.0))
        // Padding under 5 would make NumberInput swap its arrows for + / −;
        // the extra right padding keeps the digits clear of the arrows.
        .padding(iced::Padding {
            top: 5.0,
            right: 18.0,
            bottom: 5.0,
            left: 2.0,
        })
        .align_x(iced::alignment::Horizontal::Right)
        .style(
            |theme: &Theme, _status| iced_aw::style::number_input::Style {
                button_background: None,
                icon_color: crate::ui::style::readable::muted_text(theme),
            },
        );
    match unit {
        Some(code_point) => input
            .icon(text_input::Icon {
                font: iced::Font::MONOSPACE,
                code_point,
                size: Some(iced::Pixels(12.0)),
                spacing: 1.0,
                side: text_input::Side::Right,
            })
            .into(),
        None => input.into(),
    }
}

/// Transparent rail with a white bar handle, for both slider directions.
fn handle_style(_theme: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (Color::TRANSPARENT.into(), Color::TRANSPARENT.into()),
            width: TRACK,
            border: Border::default(),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 8,
                border_radius: 3.0.into(),
            },
            background: Color::WHITE.into(),
            border_width: 1.0,
            border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
        },
    }
}

/// A rounded box filled with a linear gradient. Angles run clockwise from
/// "bottom to top": a quarter turn is left to right, a half turn top to bottom.
fn gradient_box(angle: f32, stops: &[(f32, Color)]) -> container::Style {
    let linear = stops.iter().fold(
        iced::gradient::Linear::new(Radians(angle)),
        |linear, &(at, color)| linear.add_stop(at, color),
    );
    container::Style {
        background: Some(Background::Gradient(Gradient::Linear(linear))),
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

fn hue_stops() -> Vec<(f32, Color)> {
    (0..=6)
        .map(|step| {
            let at = step as f32 / 6.0;
            (at, hsb_color(at * 360.0, 1.0, 1.0))
        })
        .collect()
}

fn hsb_color(h: f32, s: f32, b: f32) -> Color {
    rgb_color(math::hsb_to_rgb(h, s, b))
}

pub(super) fn rgb_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_produce_colours() {
        let mut e = ColorEditor::new((255, 0, 0));
        assert!(e.update(EditorMsg::Hue(120.0)));
        assert_eq!(e.rgb(), (0, 255, 0));
        e.update(EditorMsg::Red(10));
        assert_eq!(e.rgb(), (10, 255, 0));
        assert!(!e.update(EditorMsg::Mode(EditorMode::Rgb)));
    }

    #[test]
    fn hue_survives_a_trip_through_gray() {
        let mut e = ColorEditor::new((0, 0, 255));
        e.update(EditorMsg::Saturation(0.0));
        e.update(EditorMsg::Saturation(100.0));
        assert_eq!(e.rgb(), (0, 0, 255));
    }

    #[test]
    fn hex_applies_six_digits_while_typing() {
        let mut e = ColorEditor::new((0, 0, 0));
        assert!(!e.update(EditorMsg::HexInput("#39BF".into())));
        assert!(e.update(EditorMsg::HexInput("#39BFFC".into())));
        assert_eq!(e.rgb(), (57, 191, 252));
    }

    #[test]
    fn short_hex_waits_for_enter() {
        let mut e = ColorEditor::new((0, 0, 0));
        assert!(!e.update(EditorMsg::HexInput("#3bf".into())));
        assert!(e.update(EditorMsg::HexSubmit));
        assert_eq!(e.rgb(), (0x33, 0xBB, 0xFF));
    }
}
