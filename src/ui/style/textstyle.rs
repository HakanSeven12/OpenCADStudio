//! Text Style Font Browser window — fills the entire OS window.

use crate::app::Message;
use crate::app::StyleKind;
use iced::widget::{
    button, canvas, checkbox, column, container, row, scrollable, text, text_input, Space,
};
use iced::{mouse, Background, Border, Element, Fill, Length, Point, Rectangle, Theme};

/// View-model for the Text Style editor window.
pub struct TextStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    /// Name of the current text style (marked with ✓ in the list).
    pub current: &'a str,
    pub font_buf: &'a str,
    pub width_buf: &'a str,
    pub oblique_buf: &'a str,
    pub height_buf: &'a str,
    pub bigfont_buf: &'a str,
    pub ttf_buf: &'a str,
    pub backward: bool,
    pub upside_down: bool,
    pub annotative: bool,
    /// Name of the style being renamed inline (double-clicked), if any.
    pub rename_active: Option<&'a str>,
    /// Edit buffer for the inline rename text input.
    pub rename_buf: &'a str,
}

const BUILTIN_FONTS: &[&str] = &[
    "Standard", "ISO", "Simplex", "RomanS", "RomanD", "RomanC", "RomanT", "ItalicC", "ItalicT",
    "ScriptS", "ScriptC", "GothGBT", "GothGRT", "GothITT", "GreekC", "Symbol",
    "ISO3098", "Unicode",
];

fn list_item(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
        let pair = match (active, st) {
            (true, _) => Some(palette.primary.strong),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(palette.background.strong)
            }
            _ => None,
        };
        button::Style {
        background: pair.map(|p| Background::Color(p.color)),
        text_color: pair.map(|p| p.text).unwrap_or(palette.background.base.text),
        ..Default::default()
        }
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
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

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

/// Live sample-text preview. Tessellates a fixed sample string through the same
/// stroke path the text entities use (`lff::tessellate_text_ex`), so the chosen
/// font, width factor, oblique angle and backward/upside-down flags all show up,
/// then scales the result to fit the preview box (Y flipped: glyph space is
/// Y-up, screen is Y-down). Height is not bound — it only scales uniformly and
/// the fit cancels it.
struct TextPreviewCanvas {
    font: String,
    /// Negative mirrors left-right (backward).
    width_factor: f32,
    /// Radians.
    oblique: f32,
    /// 0 or π (upside-down rotates 180° about the origin).
    rotation: f32,
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
        let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
            [0.0, 0.0],
            9.0,
            self.rotation,
            self.width_factor,
            self.oblique,
            &self.font,
            "AaBbCc 0123",
        );
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for s in &strokes {
            for &[x, y] in s {
                min[0] = min[0].min(x);
                min[1] = min[1].min(y);
                max[0] = max[0].max(x);
                max[1] = max[1].max(y);
            }
        }
        if min[0] > max[0] {
            return vec![frame.into_geometry()];
        }
        let pad = 10.0;
        let span = [(max[0] - min[0]).max(1e-3), (max[1] - min[1]).max(1e-3)];
        let scale = ((bounds.width - 2.0 * pad) / span[0])
            .min((bounds.height - 2.0 * pad) / span[1])
            .max(0.0);
        let mid = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
        let center = [bounds.width * 0.5, bounds.height * 0.5];
        let map = |x: f32, y: f32| {
            Point::new(
                center[0] + (x - mid[0]) * scale,
                center[1] - (y - mid[1]) * scale,
            )
        };
        let stroke = canvas::Stroke {
            width: 1.4,
            style: canvas::Style::Solid(theme.extended_palette().background.base.text),
            ..Default::default()
        };
        for s in &strokes {
            if s.len() < 2 {
                continue;
            }
            let path = canvas::Path::new(|p| {
                p.move_to(map(s[0][0], s[0][1]));
                for &[x, y] in &s[1..] {
                    p.line_to(map(x, y));
                }
            });
            frame.stroke(&path, stroke.clone());
        }
        vec![frame.into_geometry()]
    }
}

pub fn view_window<'a>(v: TextStyleView<'a>) -> Element<'a, Message> {
    let TextStyleView {
        styles,
        selected,
        current,
        font_buf,
        width_buf,
        oblique_buf,
        height_buf,
        bigfont_buf,
        ttf_buf,
        backward,
        upside_down,
        annotative,
        rename_active,
        rename_buf,
    } = v;
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
            text("Font File").size(10).style(muted_style),
            container(scrollable(column(font_items).spacing(1)).height(Fill))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
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

    // Labeled numeric/text field row → TextStyleEdit { field, value }.
    fn frow<'a>(
        label: &'a str,
        ph: &'a str,
        buf: &'a str,
        field: &'static str,
    ) -> Element<'a, Message> {
        row![
            text(label).size(11).style(muted_style).width(120),
            text_input(ph, buf)
                .on_input(move |v| Message::TextStyleEdit { field, value: v })
                .style(field_style)
                .size(11)
                .width(140),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    }

    // Live preview reflecting the in-progress edits. Effective font follows the
    // entity rule: TrueType name wins when set, else the stroke font file.
    let eff_font = if !ttf_buf.trim().is_empty() {
        ttf_buf
    } else {
        font_buf
    };
    let prev_base_wf = width_buf
        .trim()
        .parse::<f32>()
        .unwrap_or(1.0)
        .abs()
        .clamp(0.01, 100.0);
    let preview = canvas(TextPreviewCanvas {
        font: eff_font.to_string(),
        width_factor: if backward { -prev_base_wf } else { prev_base_wf },
        oblique: oblique_buf.trim().parse::<f32>().unwrap_or(0.0).to_radians(),
        rotation: if upside_down { std::f32::consts::PI } else { 0.0 },
    })
    .width(Fill)
    .height(Length::Fixed(56.0));

    // ── Right: Properties ─────────────────────────────────────────────────
    let props_panel = container(
        column![
            text("Properties").size(11).style(primary_style),
            frow("Big Font:", "big-font file…", bigfont_buf, "bigfont"),
            frow("TrueType Font:", "e.g. Arial", ttf_buf, "ttf"),
            frow("Fixed Height:", "0 = variable", height_buf, "height"),
            frow("Width Factor:", "1.0", width_buf, "width"),
            frow("Oblique (°):", "0.0", oblique_buf, "oblique"),
            row![
                checkbox(backward)
                    .label("Backward")
                    .on_toggle(|_| Message::TextStyleToggle("backward"))
                    .size(15)
                    .text_size(11),
                checkbox(upside_down)
                    .label("Upside down")
                    .on_toggle(|_| Message::TextStyleToggle("upside_down"))
                    .size(15)
                    .text_size(11),
            ]
            .spacing(16),
            checkbox(annotative)
                .label("Annotative")
                .on_toggle(|_| Message::TextStyleToggle("annotative"))
                .size(15)
                .text_size(11),
            Space::new().height(8),
            text("Preview").size(10).style(muted_style),
            container(preview)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.base.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 4.0.into()
                    },
                    ..Default::default()
                    }
                })
                .padding(8)
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

    // ── System TrueType fonts (read from the user's machine via fontdb) ──
    let ttf_items: Vec<Element<'_, Message>> = crate::scene::text::sysfont::families()
        .iter()
        .map(|fam| {
            let is_sel = ttf_buf.eq_ignore_ascii_case(fam);
            button(text(fam).size(10))
                .on_press(Message::TextStyleEdit {
                    field: "ttf",
                    value: fam.clone(),
                })
                .style(list_item(is_sel))
                .padding([3, 8])
                .width(Fill)
                .into()
        })
        .collect();

    let ttf_panel = container(
        column![
            text("TrueType (system)").size(10).style(muted_style),
            container(scrollable(column(ttf_items).spacing(1)).height(Fill))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
            text_input("TrueType font…", ttf_buf)
                .on_input(|v| Message::TextStyleEdit {
                    field: "ttf",
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

    let editor = row![font_panel, vsep(), ttf_panel, vsep(), props_panel].height(Fill);

    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        kind: StyleKind::Text,
        styles: &styles,
        selected,
        current: Some(current),
        rename_active,
        rename_buf,
        on_new: Message::TextStyleDialogNew,
        on_copy: Message::TextStyleDialogCopy,
        on_delete: Message::TextStyleDialogDelete,
        on_select: Message::TextStyleDialogSelect,
        on_set_current: Message::TextStyleDialogSetCurrent,
        on_apply: Message::TextStyleApply,
        editor: editor.into(),
    })
}
