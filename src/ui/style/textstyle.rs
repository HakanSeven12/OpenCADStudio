//! Text style manager.

use crate::app::{Message, StyleKind};
use crate::t;
use iced::widget::{button, canvas, checkbox, column, container, row, scrollable, text, text_input};
use iced::{mouse, Background, Border, Element, Length, Point, Rectangle, Theme};
use crate::ui::style::common::muted_style;
use std::borrow::Cow;

pub struct TextStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub current: &'a str,
    pub tab: u8,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub read_only: bool,
    pub in_use: bool,
    pub font_buf: &'a str,
    pub width_buf: &'a str,
    pub oblique_buf: &'a str,
    pub height_buf: &'a str,
    pub bigfont_buf: &'a str,
    pub ttf_buf: &'a str,
    pub backward: bool,
    pub upside_down: bool,
    pub vertical: bool,
    pub annotative: bool,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
}

const BUILTIN_FONTS: &[&str] = &[
    "Standard", "ISO", "Simplex", "RomanS", "RomanD", "RomanC", "RomanT", "ItalicC",
    "ItalicT", "ScriptS", "ScriptC", "GothGBT", "GothGRT", "GothITT", "GreekC",
    "Symbol", "ISO3098", "Unicode",
];

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                palette.primary.base.color
            } else {
                palette.background.neutral.color
            },
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn list_item(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let palette = theme.palette();
        let pair = match (active, status) {
            (true, _) => Some(palette.primary.strong),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(palette.background.strong)
            }
            _ => None,
        };
        button::Style {
            background: pair.map(|value| Background::Color(value.color)),
            text_color: pair
                .map(|value| value.text)
                .unwrap_or(palette.background.base.text),
            ..Default::default()
        }
    }
}

struct TextPreviewCanvas {
    font: String,
    big_font: String,
    width_factor: f32,
    oblique: f32,
    height: f32,
    upside_down: bool,
    vertical: bool,
    annotative: bool,
}

impl canvas::Program<Message> for TextPreviewCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let ink = theme.palette().background.base.text;
        let glyph_height = if self.height <= 0.0 {
            18.0
        } else {
            (self.height * 7.0).clamp(6.0, 52.0)
        };
        let rotation = if self.upside_down {
            std::f32::consts::PI
        } else {
            0.0
        };
        let strokes = if self.vertical {
            let mut result = Vec::new();
            for (index, character) in "Ab3".chars().enumerate() {
                let (mut glyph, _) = crate::scene::text::lff::tessellate_text_ex(
                    [0.0, -(index as f32) * glyph_height * 1.25],
                    glyph_height,
                    rotation,
                    self.width_factor,
                    self.oblique,
                    &self.font,
                    &character.to_string(),
                );
                result.append(&mut glyph);
            }
            result
        } else {
            crate::scene::text::lff::tessellate_text_ex(
                [0.0, 0.0],
                glyph_height,
                rotation,
                self.width_factor,
                self.oblique,
                &self.font,
                "AaBbCc 0123",
            )
            .0
        };
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for stroke in &strokes {
            for &[x, y] in stroke {
                min[0] = min[0].min(x);
                min[1] = min[1].min(y);
                max[0] = max[0].max(x);
                max[1] = max[1].max(y);
            }
        }
        if min[0] <= max[0] {
            let span = [(max[0] - min[0]).max(1.0), (max[1] - min[1]).max(1.0)];
            let available = [bounds.width - 24.0, bounds.height - 34.0];
            let scale = (available[0] / span[0])
                .min(available[1] / span[1])
                .min(1.0)
                .max(0.0);
            let middle = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
            let center = [bounds.width * 0.5, (bounds.height - 18.0) * 0.5];
            let map = |x: f32, y: f32| {
                Point::new(
                    center[0] + (x - middle[0]) * scale,
                    center[1] - (y - middle[1]) * scale,
                )
            };
            for stroke in &strokes {
                if stroke.len() < 2 {
                    continue;
                }
                let path = canvas::Path::new(|builder| {
                    builder.move_to(map(stroke[0][0], stroke[0][1]));
                    for &[x, y] in &stroke[1..] {
                        builder.line_to(map(x, y));
                    }
                });
                frame.stroke(
                    &path,
                    canvas::Stroke::default().with_color(ink).with_width(1.35),
                );
            }
        }
        let font_label = if self.big_font.trim().is_empty() {
            self.font.clone()
        } else {
            format!("{} + {}", self.font, self.big_font)
        };
        let mode = if self.annotative {
            t!("Paper height")
        } else {
            t!("Drawing height")
        };
        frame.fill_text(canvas::Text {
            content: format!("{}  ·  {} {:.3}", font_label, mode, self.height),
            position: Point::new(10.0, bounds.height - 9.0),
            color: ink.scale_alpha(0.65),
            size: iced::Pixels(10.0),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

fn input_row<'a>(
    label: Cow<'static, str>,
    placeholder: Cow<'static, str>,
    value: &'a str,
    field: &'static str,
    read_only: bool,
) -> Element<'a, Message> {
    let control: Element<'a, Message> = if read_only {
        crate::ui::read_only::field(value, 11.0, Length::Fixed(180.0))
    } else {
        text_input(placeholder.as_ref(), value)
            .on_input(move |value| Message::TextStyleEdit { field, value })
            .style(field_style)
            .size(11)
            .width(180)
            .into()
    };
    row![text(label).size(11).style(muted_style).width(150), control]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn toggle<'a>(
    label: Cow<'static, str>,
    value: bool,
    field: &'static str,
    read_only: bool,
) -> Element<'a, Message> {
    let checkbox = checkbox(value).label(label).size(14).text_size(11);
    if read_only {
        checkbox.into()
    } else {
        checkbox
            .on_toggle(move |_| Message::TextStyleToggle(field))
            .into()
    }
}

fn font_list<'a>(
    title: Cow<'static, str>,
    values: Vec<String>,
    selected: &'a str,
    system: bool,
    read_only: bool,
) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = values
        .into_iter()
        .map(|font| {
            let active = selected.eq_ignore_ascii_case(&font);
            let button = button(text(font.clone()).size(10))
                .style(list_item(active))
                .padding([3, 8])
                .width(Length::Fill);
            if read_only {
                button.into()
            } else if system {
                button
                    .on_press(Message::TextStyleEdit {
                        field: "ttf",
                        value: font,
                    })
                    .into()
            } else {
                button.on_press(Message::TextStyleFontPick(font)).into()
            }
        })
        .collect();
    container(column![text(title).size(10).style(muted_style), scrollable(column(items).spacing(1)).height(250)].spacing(5))
        .width(Length::FillPortion(1))
        .padding(4)
        .into()
}

pub fn view_window<'a>(
    v: TextStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let effective_font = if v.ttf_buf.trim().is_empty() {
        v.font_buf
    } else {
        v.ttf_buf
    };
    let width = v
        .width_buf
        .trim()
        .parse::<f32>()
        .unwrap_or(1.0)
        .abs()
        .clamp(0.01, 100.0);
    let preview = canvas(TextPreviewCanvas {
        font: effective_font.to_string(),
        big_font: v.bigfont_buf.to_string(),
        width_factor: if v.backward { -width } else { width },
        oblique: v.oblique_buf.trim().parse::<f32>().unwrap_or(0.0).to_radians(),
        height: v.height_buf.trim().parse::<f32>().unwrap_or(0.0).max(0.0),
        upside_down: v.upside_down,
        vertical: v.vertical,
        annotative: v.annotative,
    })
    .width(Length::Fill)
    .height(112);

    let content: Element<'a, Message> = match v.tab {
        0 => {
            let built_in = BUILTIN_FONTS.iter().map(|value| (*value).to_string()).collect();
            let system = crate::scene::text::sysfont::families().to_vec();
            column![
                row![
                    font_list(t!("Stroke fonts"), built_in, v.font_buf, false, v.read_only),
                    font_list(t!("System fonts"), system, v.ttf_buf, true, v.read_only),
                ]
                .spacing(10),
                input_row(t!("Font file"), t!("font file…"), v.font_buf, "font", v.read_only),
                input_row(t!("Big font"), t!("big-font file…"), v.bigfont_buf, "bigfont", v.read_only),
                input_row(t!("System font"), t!("font family…"), v.ttf_buf, "ttf", v.read_only),
            ]
            .spacing(8)
            .into()
        }
        _ => column![
            text(t!("Size")).size(11).style(primary_style),
            input_row(
                if v.annotative { t!("Paper text height") } else { t!("Fixed height") },
                t!("0 = variable"),
                v.height_buf,
                "height",
                v.read_only,
            ),
            text(t!("Effects")).size(11).style(primary_style),
            input_row(t!("Width factor"), "1.0".into(), v.width_buf, "width", v.read_only),
            input_row(t!("Oblique angle"), "0.0".into(), v.oblique_buf, "oblique", v.read_only),
            row![
                toggle(t!("Backward"), v.backward, "backward", v.read_only),
                toggle(t!("Upside down"), v.upside_down, "upside_down", v.read_only),
                toggle(
                    t!("Vertical"),
                    v.vertical,
                    "vertical",
                    v.read_only || !v.ttf_buf.trim().is_empty(),
                ),
            ]
            .spacing(18),
            toggle(t!("Annotative"), v.annotative, "annotative", v.read_only),
        ]
        .spacing(9)
        .into(),
    };
    let status = if v.read_only {
        t!("Referenced style · Read only")
    } else if v.selected.eq_ignore_ascii_case(v.current) {
        t!("Current style")
    } else if v.in_use {
        t!("Used in drawing")
    } else {
        t!("Available style")
    };
    let summary = if v.comparison_sections.is_empty() {
        t!("No differences").into_owned()
    } else {
        format!("{}: {}", t!("Different"), v.comparison_sections.join(", "))
    };
    let editor = crate::ui::style::style_manager::editor_shell(
        crate::ui::style::style_manager::EditorShell {
            sizing,
            selected: v.selected.to_string(),
            status,
            preview: preview.into(),
            comparison: Some(crate::ui::style::style_manager::EditorComparison {
                selected: v.compare_name,
                options: v.compare_opts,
                summary,
                on_select: Message::TextStyleDialogCompare,
            }),
            tabs: vec![
                crate::ui::style::style_manager::EditorTab {
                    label: t!("Fonts"),
                    active: v.tab == 0,
                    on_press: Message::TextStyleDialogTab(0),
                },
                crate::ui::style::style_manager::EditorTab {
                    label: t!("Size and Effects"),
                    active: v.tab == 1,
                    on_press: Message::TextStyleDialogTab(1),
                },
            ],
            content,
        },
    );
    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: StyleKind::Text,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: Message::TextStyleDialogNew,
        on_copy: Message::TextStyleDialogCopy,
        on_delete: Message::TextStyleDialogDelete,
        on_select: Message::TextStyleDialogSelect,
        on_set_current: Message::TextStyleDialogSetCurrent,
        on_apply: Message::TextStyleApply,
        read_only: v.read_only,
        editor,
    })
}
