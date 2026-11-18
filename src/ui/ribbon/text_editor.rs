//! The "Text Editor" tab's live parts: what the open multiline text editor
//! holds (style, height, font, colour, layer, the switches that are on), set
//! by the app after every update, and the lists and boxes the Style and
//! Formatting panels draw from it. The panels' plain buttons come from
//! `modules::text_editor`.

use std::cell::RefCell;

use document::types::Color as AcadColor;
use iced::widget::{column, container, row, text, text_input};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::font_picker::{FontChoice, FontPickerEvent};
use crate::ui::style::text as ui_text;

/// What the tab shows of the open editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextEditorRibbon {
    pub styles: Vec<String>,
    /// Each style's TrueType family (None: a stroke font) and whether it is
    /// annotative, for the style gallery's samples.
    pub style_faces: Vec<Option<String>>,
    pub style_annotative: Vec<bool>,
    pub style: String,
    pub height: String,
    /// The font the selection (or the text) is drawn with, as the font list
    /// names it.
    pub font: String,
    pub color: AcadColor,
    pub color_open: bool,
    /// The layer the text goes on.
    pub layer: String,
    pub layers: Vec<String>,
    pub oblique: String,
    pub tracking: String,
    pub width: String,
    /// The `_MTED` tools that show as on (Bold, Ruler, the alignment, …).
    pub lit: Vec<&'static str>,
    /// The `_MTED` tools that cannot be used now (Bold on a stroke font, …).
    pub off: Vec<&'static str>,
}

thread_local! {
    static STATE: RefCell<Option<TextEditorRibbon>> = const { RefCell::new(None) };
    /// The fonts the Font list offers, gathered when the editor opens.
    static FONTS: RefCell<Vec<FontChoice>> = const { RefCell::new(Vec::new()) };
}

/// Hand the tab the open editor's state (None once it closes). Returns
/// whether the editor just opened.
pub fn set(state: Option<TextEditorRibbon>) -> bool {
    STATE.with(|cell| {
        let mut cell = cell.borrow_mut();
        let opened = cell.is_none() && state.is_some();
        if *cell != state {
            *cell = state;
        }
        opened
    })
}

/// The fonts the Font list offers.
pub fn set_fonts(fonts: Vec<FontChoice>) {
    FONTS.with(|cell| *cell.borrow_mut() = fonts);
}

/// Whether the editor is open.
pub fn open() -> bool {
    STATE.with(|cell| cell.borrow().is_some())
}

/// Whether the tool `id` shows as on.
pub(super) fn lit(id: &str) -> bool {
    STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| state.lit.contains(&id))
    })
}

/// Whether the tool `id` cannot be used now.
pub(super) fn unavailable(id: &str) -> bool {
    STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| state.off.contains(&id))
    })
}

fn with<R>(f: impl FnOnce(&TextEditorRibbon) -> R) -> Option<R> {
    STATE.with(|cell| cell.borrow().as_ref().map(f))
}

const LIST_W: f32 = 150.0;
const BOX_W: f32 = 64.0;

fn caption<'a>(label: impl Into<String>) -> Element<'a, Message> {
    text(label.into())
        .size(ui_text::UI)
        .style(|theme: &Theme| text::Style {
            color: Some(crate::ui::style::chrome::muted(theme)),
        })
        .into()
}

/// A number box with its caption, a tooltip naming it.
fn number_box<'a>(
    label: String,
    value: &str,
    on_input: fn(String) -> Message,
    width: f32,
) -> Element<'a, Message> {
    let input = text_input("", value)
        .on_input(on_input)
        .size(ui_text::UI)
        .padding([3, 6])
        .width(width);
    row![caption(label.clone()), input]
        .spacing(4)
        .align_y(iced::Center)
        .into()
}

/// What the panel `key` shows after its own buttons: the style list and
/// height box on Style, the font, colour and layer lists on Formatting.
pub(super) fn trail<'a>(key: &str) -> Option<Element<'a, Message>> {
    match key {
        "text_editor/Style" => with(style_controls),
        "text_editor/Formatting" => with(format_lists),
        _ => None,
    }
}

/// The style gallery: a sample of each style in its own font, the style's
/// name under it (an annotative style marked), the current one lit.
fn style_gallery<'a>(state: &TextEditorRibbon) -> Element<'a, Message> {
    let mut items = iced::widget::Row::new().spacing(4);
    for (n, name) in state.styles.iter().enumerate() {
        let face = state.style_faces.get(n).cloned().flatten();
        let annotative = state.style_annotative.get(n).copied().unwrap_or(false);
        let mut sample = text("AaBb123").size(15).color(iced::Color::BLACK);
        if let Some(face) = face {
            sample = sample.font(iced::Font::with_family(iced::font::Family::name(&face)));
        }
        let swatch = container(sample)
            .center_x(Length::Fixed(84.0))
            .center_y(Length::Fixed(30.0))
            .style(|_| container::Style {
                background: Some(iced::Background::Color(iced::Color::WHITE)),
                border: iced::Border {
                    width: 1.0,
                    color: iced::Color::from_rgb8(160, 160, 160),
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
        let label = if annotative {
            format!("\u{25b2} {name}")
        } else {
            name.clone()
        };
        let current = *name == state.style;
        items = items.push(
            iced::widget::button(
                column![swatch, text(label).size(ui_text::UI_SMALL)]
                    .spacing(2)
                    .align_x(iced::Center),
            )
            .on_press(Message::MTextStyle(name.clone()))
            .style(move |theme: &Theme, status| {
                crate::ui::kit::dropdown_row_style(
                    theme,
                    if current {
                        iced::widget::button::Status::Hovered
                    } else {
                        status
                    },
                )
            })
            .padding(2),
        );
    }
    let gallery = iced::widget::scrollable(items)
        .direction(iced::widget::scrollable::Direction::Horizontal(
            iced::widget::scrollable::Scrollbar::new()
                .width(3)
                .scroller_width(3),
        ))
        .width(Length::Fixed(276.0));
    crate::ui::kit::tooltip(
        gallery,
        t!("Text Style").into_owned(),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

fn style_controls<'a>(state: &TextEditorRibbon) -> Element<'a, Message> {
    let list = style_gallery(state);
    let height = crate::ui::kit::tooltip(
        number_box(t!("Height").into_owned(), &state.height, Message::MTextHeight, BOX_W),
        t!("Text Height").into_owned(),
        iced::widget::tooltip::Position::Bottom,
    );
    container(row![list, height].spacing(6).align_y(iced::Center))
        .padding([2, 4])
        .height(Fill)
        .into()
}

fn format_lists<'a>(state: &TextEditorRibbon) -> Element<'a, Message> {
    let fonts: Vec<FontChoice> = FONTS.with(|cell| cell.borrow().clone());
    let mut names: Vec<String> = fonts.iter().map(|font| font.label().into_owned()).collect();
    if !state.font.is_empty() && !names.contains(&state.font) {
        names.insert(0, state.font.clone());
    }
    let font = crate::ui::dropdown_list::pick_list(
        (!state.font.is_empty()).then(|| state.font.clone()),
        names,
        |value| value.to_string(),
    )
    .on_select(move |name| {
        let choice = fonts
            .iter()
            .find(|font| font.label() == name.as_str())
            .cloned()
            .unwrap_or_else(|| FontChoice::of_name(&name));
        Message::MTextFontPicker(FontPickerEvent::Pick(choice))
    })
    .text_size(ui_text::UI)
    .width(LIST_W);
    let color = container(crate::ui::color_select::color_selector(
        state.color,
        state.color_open,
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: false,
            ..Default::default()
        },
        Message::MTextColorChanged,
        Message::MTextColorPickerToggle,
        Message::OpenColorWindow(crate::app::ColorPickTarget::MText, state.color),
    ))
    .width(LIST_W);
    let mut layers = vec![t!("Use Current").into_owned()];
    layers.extend(state.layers.iter().cloned());
    let use_current = layers[0].clone();
    let shown = if state.layer.is_empty() {
        use_current.clone()
    } else {
        state.layer.clone()
    };
    let layer = crate::ui::dropdown_list::pick_list(Some(shown), layers, |value| value.to_string())
        .on_select(move |name| {
            Message::RibbonTextLayer(if name == use_current {
                String::new()
            } else {
                name
            })
        })
        .text_size(ui_text::UI)
        .width(LIST_W);
    let tip = |control: Element<'a, Message>, name: &str| -> Element<'a, Message> {
        crate::ui::kit::tooltip(
            control,
            t!(name).into_owned(),
            iced::widget::tooltip::Position::Bottom,
        )
    };
    container(
        column![
            tip(font.into(), "Font"),
            tip(color.into(), "Color"),
            tip(layer.into(), "Layer"),
        ]
        .spacing(4),
    )
    .padding([2, 4])
    .height(Fill)
    .into()
}

/// The Formatting slide-out: oblique angle, tracking and width factor of the
/// selection (or of what is typed next).
pub(super) fn format_more<'a>() -> Option<Element<'a, Message>> {
    with(|state| {
        column![
            number_box(
                t!("Oblique Angle").into_owned(),
                &state.oblique,
                Message::MTextOblique,
                BOX_W
            ),
            number_box(
                t!("Tracking").into_owned(),
                &state.tracking,
                Message::MTextCharSpace,
                BOX_W
            ),
            number_box(
                t!("Width Factor").into_owned(),
                &state.width,
                Message::MTextWidth,
                BOX_W
            ),
        ]
        .spacing(4)
        .width(Length::Shrink)
        .into()
    })
}
