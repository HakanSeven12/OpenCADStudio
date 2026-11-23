//! Gradient flyout of the Graphic Attributes palette.

use crate::app::Message;
use crate::scene::model::hatch_model::GradientKind;
use crate::ui::kit;
use crate::ui::style::{chrome, kit_metrics as km, text as ui_text};
use crate::ui::window::graphic_attributes::GraphicAttributesMsg;
use document::types::Color as AcadColor;
use iced::widget::{
    button, checkbox, column, container, image, mouse_area, row, slider, text, text_input, Space,
};
use iced::{Background, Color, Element, Fill, Length, Theme};

// Layout of the editor, built on the UI kit (`kit`, `km`, `chrome`,
// `ui_text`). Every row has a fixed height, so the flyout's height is known
// up front: the palette's drop-down needs it to move the editor up rather
// than clip it when there is little room below the button.

/// A row of the editor: the kit's toolbar / field height.
const ROW_H: f32 = km::TOOL_H;
/// The large preview at the top.
const PREVIEW_H: f32 = 80.0;
/// A gradient type button: its icon above its name.
const TYPE_H: f32 = 48.0;
const TYPE_W: f32 = 48.0;
const TYPE_ICON: f32 = 25.0;
const INVERT_W: f32 = 42.0;
/// The line between the types and Invert.
const SEPARATOR_H: f32 = 36.0;
/// The name column in front of each row.
const LABEL_W: f32 = 43.0;
const ANGLE_W: f32 = 92.0;
/// Room between the rows.
const GAP: f32 = km::TOP_INSET;
/// Room inside the frame, inside its 1 px border.
const PAD: f32 = km::MENU_PAD_X;
/// Width of the editor flyout.
pub const WIDTH: f32 = 390.0;
/// Height of the editor flyout: preview, colour mode, colours, type, angle
/// and the Cancel / OK row, with the gaps between them and the padding.
pub const HEIGHT: f32 = 2.0 * PAD + PREVIEW_H + 4.0 * ROW_H + TYPE_H + 5.0 * GAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientColorMode {
    One,
    Two,
}

#[derive(Debug, Clone)]
pub struct GradientEditorState {
    pub handles: Vec<document::Handle>,
    pub color_mode: GradientColorMode,
    pub color_1: AcadColor,
    pub color_2: AcadColor,
    pub shade_tint: f32,
    pub kind: GradientKind,
    pub inverted: bool,
    pub angle: String,
    pub centered: bool,
    /// Large preview, rebuilt only when a setting it depends on changes.
    preview: std::cell::RefCell<Option<(PreviewKey, Option<iced::widget::image::Handle>)>>,
}

type PreviewKey = (
    GradientColorMode,
    AcadColor,
    AcadColor,
    u32,
    GradientKind,
    bool,
    String,
    bool,
);

#[derive(Debug, Clone)]
pub enum GradientMsg {
    Open,
    Cancel,
    Apply,
    ColorMode(GradientColorMode),
    /// Stop colour 1 or 2.
    Color(u8, AcadColor),
    ShadeTint(f32),
    Kind(GradientKind),
    InvertToggled,
    Angle(String),
    AngleReset,
    Centered(bool),
}

fn msg(message: GradientMsg) -> Message {
    Message::GraphicAttributes(GraphicAttributesMsg::Gradient(message))
}

impl GradientEditorState {
    pub fn from_hatch(handles: Vec<document::Handle>, hatch: &document::entities::Hatch) -> Self {
        let color = |index: usize| {
            hatch
                .gradient_color
                .colors
                .get(index)
                .map(|entry| entry.color)
                .unwrap_or(AcadColor::Index(7))
        };
        let (kind, inverted) = GradientKind::from_name(&hatch.gradient_color.name);
        Self {
            handles,
            color_mode: if hatch.gradient_color.is_single_color {
                GradientColorMode::One
            } else {
                GradientColorMode::Two
            },
            color_1: color(0),
            color_2: color(1),
            shade_tint: hatch.gradient_color.color_tint as f32,
            kind,
            inverted,
            angle: format!("{:.1}°", hatch.gradient_color.angle.to_degrees()),
            centered: hatch.gradient_color.shift.abs() < 1e-9,
            preview: Default::default(),
        }
    }

    pub fn update(&mut self, message: GradientMsg) {
        match message {
            GradientMsg::ColorMode(mode) => self.color_mode = mode,
            GradientMsg::Color(1, color) => self.color_1 = color,
            GradientMsg::Color(_, color) => self.color_2 = color,
            GradientMsg::ShadeTint(value) => self.shade_tint = value.clamp(0.0, 1.0),
            GradientMsg::Kind(kind) => {
                self.kind = kind;
                if kind == GradientKind::Linear {
                    self.inverted = false;
                }
            }
            GradientMsg::InvertToggled => {
                if self.kind != GradientKind::Linear {
                    self.inverted ^= true;
                }
            }
            GradientMsg::Angle(value) => self.angle = value,
            GradientMsg::AngleReset => self.angle = "0.0°".into(),
            GradientMsg::Centered(value) => self.centered = value,
            GradientMsg::Open | GradientMsg::Cancel | GradientMsg::Apply => {}
        }
    }

    pub fn angle_radians(&self) -> f64 {
        self.angle
            .trim()
            .trim_end_matches('°')
            .replace(',', ".")
            .parse::<f64>()
            .unwrap_or(0.0)
            .rem_euclid(360.0)
            .to_radians()
    }
}

fn rgb(color: &AcadColor) -> Color {
    let (r, g, b) = match color {
        AcadColor::Rgb { r, g, b } => (*r, *g, *b),
        AcadColor::Index(i) => {
            document::types::aci_table::aci_to_rgb(*i).unwrap_or((128, 128, 128))
        }
        _ => (255, 255, 255),
    };
    Color::from_rgb8(r, g, b)
}

fn rgb_array(color: &AcadColor) -> [f32; 4] {
    let c = rgb(color);
    [c.r, c.g, c.b, 1.0]
}

fn mix_rgba(a: Color, b: Color, t: f32) -> [u8; 4] {
    let channel = |a: f32, b: f32| ((a + (b - a) * t) * 255.0).round() as u8;
    [channel(a.r, b.r), channel(a.g, b.g), channel(a.b, b.b), 255]
}

fn preview_sized(
    state: &GradientEditorState,
    width: u32,
    height: u32,
) -> Option<iced::widget::image::Handle> {
    let c1 = rgb(&state.color_1);
    let c2 = if state.color_mode == GradientColorMode::One {
        let c = crate::scene::gradient_tint_color(rgb_array(&state.color_1), state.shade_tint);
        Color::from_rgb(c[0], c[1], c[2])
    } else {
        rgb(&state.color_2)
    };
    let angle = state.angle_radians();
    let frame = ocs_geom::geom2d::gradient_frame(
        &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        angle,
        if state.centered { 0.0 } else { 1.0 },
        ocs_geom::geom2d::Tolerance::default(),
    )?;
    let (sin, cos) = angle.sin_cos();
    let radial = state.kind.radial();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let py = y as f64 / (height - 1) as f64;
        for x in 0..width {
            let px = x as f64 / (width - 1) as f64;
            let mut t = if radial {
                ((px - frame.center[0]).hypot(py - frame.center[1]) / frame.radius).clamp(0.0, 1.0)
                    as f32
            } else {
                (((px * cos + py * sin) - frame.projection_min) / frame.projection_span)
                    .clamp(0.0, 1.0) as f32
            };
            if state.kind == GradientKind::Cylinder {
                t = 1.0 - (2.0 * t - 1.0).abs();
            } else if state.kind == GradientKind::Curved {
                t *= t;
            } else if state.kind == GradientKind::Hemispherical {
                t = t.sqrt();
            }
            if state.inverted {
                t = 1.0 - t;
            }
            pixels.extend_from_slice(&if radial {
                mix_rgba(c2, c1, t)
            } else {
                mix_rgba(c1, c2, t)
            });
        }
    }
    Some(iced::widget::image::Handle::from_rgba(
        width, height, pixels,
    ))
}

fn preview(state: &GradientEditorState) -> Element<'static, Message> {
    let key: PreviewKey = (
        state.color_mode,
        state.color_1,
        state.color_2,
        state.shade_tint.to_bits(),
        state.kind,
        state.inverted,
        state.angle.clone(),
        state.centered,
    );
    let mut cache = state.preview.borrow_mut();
    if cache.as_ref().is_none_or(|(cached, _)| *cached != key) {
        *cache = Some((key, preview_sized(state, 370, 80)));
    }
    preview_image(cache.as_ref().and_then(|(_, handle)| handle.clone()))
}

/// A preview of a two-colour gradient fill (the Hatch panel swatch).
pub(crate) fn fill_preview(
    kind: GradientKind,
    inverted: bool,
    color_1: AcadColor,
    color_2: AcadColor,
    angle_deg: f64,
) -> Option<iced::widget::image::Handle> {
    // ponytail: rebuilt every frame the window draws; cache it if it shows.
    let state = GradientEditorState {
        handles: Vec::new(),
        color_mode: GradientColorMode::Two,
        color_1,
        color_2,
        shade_tint: 1.0,
        kind,
        inverted,
        angle: format!("{angle_deg}"),
        centered: true,
        preview: Default::default(),
    };
    preview_sized(&state, 240, 32)
}

pub(crate) fn compact_preview(
    hatch: &document::entities::Hatch,
) -> Option<iced::widget::image::Handle> {
    preview_sized(&GradientEditorState::from_hatch(Vec::new(), hatch), 256, 26)
}

pub(crate) fn preview_image(
    handle: Option<iced::widget::image::Handle>,
) -> Element<'static, Message> {
    match handle {
        Some(handle) => image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(iced::ContentFit::Fill)
            .into(),
        None => Space::new().width(Fill).height(Fill).into(),
    }
}

fn icon(kind: GradientKind, inverted: bool) -> &'static [u8] {
    match (kind, inverted) {
        (GradientKind::Linear, _) => {
            include_bytes!("../../../assets/icons/gradient/gradient_linear.svg")
        }
        (GradientKind::Cylinder, false) => {
            include_bytes!("../../../assets/icons/gradient/gradient_cylindrical.svg")
        }
        (GradientKind::Cylinder, true) => {
            include_bytes!("../../../assets/icons/gradient/gradient_cylindrical_inverted.svg")
        }
        (GradientKind::Spherical, false) => {
            include_bytes!("../../../assets/icons/gradient/gradient_spherical.svg")
        }
        (GradientKind::Spherical, true) => {
            include_bytes!("../../../assets/icons/gradient/gradient_spherical_inverted.svg")
        }
        (GradientKind::Hemispherical, false) => {
            include_bytes!("../../../assets/icons/gradient/gradient_hemispherical.svg")
        }
        (GradientKind::Hemispherical, true) => {
            include_bytes!("../../../assets/icons/gradient/gradient_hemispherical_inverted.svg")
        }
        (GradientKind::Curved, false) => {
            include_bytes!("../../../assets/icons/gradient/gradient_curved.svg")
        }
        (GradientKind::Curved, true) => {
            include_bytes!("../../../assets/icons/gradient/gradient_curved_inverted.svg")
        }
    }
}

pub fn view(state: &GradientEditorState, dimmed: bool) -> Element<'_, Message> {
    let label = |caption: std::borrow::Cow<'static, str>| {
        text(caption)
            .size(ui_text::UI)
            .line_height(ui_text::line())
            .width(LABEL_W)
    };
    let segment = |caption, mode| {
        button(
            container(text(caption).size(ui_text::UI).line_height(ui_text::line()))
                .center_x(Fill)
                .center_y(Fill),
        )
        .on_press(msg(GradientMsg::ColorMode(mode)))
        .width(Fill)
        .height(ROW_H)
        .padding(0)
        .style(kit::text_button_style(state.color_mode == mode))
    };
    let color_field = |caption, index, color: AcadColor| {
        let (background, _) = crate::ui::properties::acad_color_display(color);
        let picker = button(
            row![
                crate::ui::color_select::swatch(background),
                text(crate::ui::color_select::color_display_name(color))
                    .size(ui_text::UI)
                    .line_height(ui_text::line()),
            ]
            .spacing(km::TOOL_GAP)
            .align_y(iced::Center),
        )
        .on_press(Message::OpenColorWindow(
            crate::app::ColorPickTarget::Gradient(index),
            color,
        ))
        .style(kit::box_button_style(false))
        .height(ROW_H)
        .padding([0.0, km::CELL_PAD_X])
        .width(Fill);
        row![label(caption), picker]
            .spacing(km::TOOL_GAP)
            .align_y(iced::Center)
            .width(Fill)
    };
    let kinds = [
        (GradientKind::Linear, crate::t!("Linear")),
        (GradientKind::Cylinder, crate::t!("Cyl.")),
        (GradientKind::Spherical, crate::t!("Spher.")),
        (GradientKind::Hemispherical, crate::t!("Hemi.")),
        (GradientKind::Curved, crate::t!("Curved")),
    ];
    let type_button = |content: Element<'static, Message>, message, active, width| {
        button(container(content).center(Fill))
            .on_press_maybe(message)
            .width(width)
            .height(TYPE_H)
            .padding(0)
            .style(kit::flat_button_style(active))
    };
    let caption = |glyph: Element<'static, Message>, caption: std::borrow::Cow<'static, str>| {
        column![
            glyph,
            text(caption)
                .size(ui_text::UI_SMALL)
                .line_height(ui_text::line())
        ]
        .align_x(iced::Center)
        .into()
    };
    let mut type_row = row![].spacing(km::TOOL_GAP).align_y(iced::Center);
    for (kind, name) in kinds {
        let glyph = crate::ui::icons::semantic(
            icon(kind, state.inverted && kind != GradientKind::Linear),
            TYPE_ICON,
        );
        type_row = type_row.push(kit::tooltip(
            type_button(
                caption(glyph, name),
                Some(msg(GradientMsg::Kind(kind))),
                state.kind == kind,
                TYPE_W,
            ),
            kind.choice_label(false),
            kit::TipPosition::Bottom,
        ));
    }
    let invert = type_button(
        caption(
            text("⇄").size(ui_text::UI_TITLE).into(),
            crate::t!("Invert"),
        ),
        (state.kind != GradientKind::Linear).then(|| msg(GradientMsg::InvertToggled)),
        state.inverted && state.kind != GradientKind::Linear,
        INVERT_W,
    );
    let separator =
        container(Space::new().width(km::line()).height(SEPARATOR_H)).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::edge(theme))),
                ..Default::default()
            }
        });
    type_row = type_row.push(separator).push(kit::tooltip(
        invert,
        crate::t!("Invert gradient"),
        kit::TipPosition::Bottom,
    ));
    let colors: Element<'_, Message> = if state.color_mode == GradientColorMode::Two {
        row![
            color_field(crate::t!("Color 1"), 1, state.color_1),
            color_field(crate::t!("Color 2"), 2, state.color_2)
        ]
        .spacing(km::TOOL_GAP)
        .height(ROW_H)
        .into()
    } else {
        row![
            color_field(crate::t!("Color 1"), 1, state.color_1),
            row![
                label(crate::t!("Tone")),
                slider(0..=100, (state.shade_tint * 100.0) as i32, |v| {
                    msg(GradientMsg::ShadeTint(v as f32 / 100.0))
                })
                .width(Fill)
            ]
            .spacing(km::TOOL_GAP)
            .align_y(iced::Center)
            .width(Fill)
        ]
        .spacing(km::TOOL_GAP)
        .height(ROW_H)
        .align_y(iced::Center)
        .into()
    };
    let angle = row![
        label(crate::t!("Angle")),
        text_input("0.0°", &state.angle)
            .on_input(|value| msg(GradientMsg::Angle(value)))
            .size(ui_text::UI)
            .line_height(ui_text::line())
            .padding(kit::BOX_PAD)
            .style(kit::combo_style)
            .width(ANGLE_W),
        kit::tool_button(
            crate::ui::icons::themed_undo(km::BAR_ICON, true),
            crate::t!("Reset angle"),
            Some(msg(GradientMsg::AngleReset)),
            false,
        ),
        Space::new().width(Fill),
        checkbox(state.centered)
            .label(crate::t!("Centered"))
            .on_toggle(|value| msg(GradientMsg::Centered(value)))
            .size(km::CHECK)
            .text_size(ui_text::UI)
    ]
    .spacing(km::TOOL_GAP)
    .height(ROW_H)
    .align_y(iced::Center);
    let types = row![label(crate::t!("Type")), type_row]
        .spacing(km::TOOL_GAP)
        .height(TYPE_H)
        .align_y(iced::Center);
    let actions = row![
        Space::new().width(Fill),
        kit::text_button(crate::t!("Cancel"), Some(msg(GradientMsg::Cancel)), false),
        kit::text_button(crate::t!("OK"), Some(msg(GradientMsg::Apply)), true),
    ]
    .spacing(km::TOOL_GAP)
    .height(ROW_H)
    .align_y(iced::Center);
    let content = column![
        container(preview(state)).width(Fill).height(PREVIEW_H),
        row![
            segment(crate::t!("One Color"), GradientColorMode::One),
            segment(crate::t!("Two Colors"), GradientColorMode::Two)
        ]
        .spacing(km::TOOL_GAP)
        .height(ROW_H),
        colors,
        types,
        angle,
        actions
    ]
    .spacing(GAP)
    .padding(PAD);
    // The kit menu frame, as the hatch pattern flyout beside it.
    let panel: Element<'_, Message> = container(content)
        .width(Length::Fixed(WIDTH))
        .height(Length::Fixed(HEIGHT))
        .style(kit::menu::frame_style)
        .into();
    let panel: Element<'_, Message> = if dimmed {
        let shield = container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::surface(theme).scale_alpha(0.55))),
                ..Default::default()
            });
        iced::widget::stack![panel, iced::widget::opaque(shield)]
            .width(Length::Fixed(WIDTH))
            .into()
    } else {
        panel
    };
    mouse_area(panel)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}
