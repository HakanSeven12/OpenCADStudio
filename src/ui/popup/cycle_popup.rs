//! Selection-cycling list box — shown at the cursor when a click lands on
//! two or more overlapping objects. Each row shows the object's drawn color
//! swatch, its type, and (right-aligned) its layer; clicking a row adds that
//! object to the current selection. Clicking outside dismisses it.

use iced::alignment::Horizontal;
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, space, text};
use iced::{border::Radius, color, Element, Fill, Length, Theme};

use crate::app::Message;

/// Row content for one overlapping candidate.
pub struct CycleCandidate {
    pub handle: acadrust::Handle,
    pub type_name: String,
    pub layer: String,
    /// The color the object is drawn with (layer-inherited when ByLayer).
    pub color: [f32; 4],
}

/// Full-canvas overlay: the list box anchored at `anchor` (canvas
/// coordinates) plus a transparent click-catcher that cancels.
pub fn cycle_popup_overlay(
    anchor: iced::Point,
    items: Vec<CycleCandidate>,
) -> Element<'static, Message> {
    // The type column and the layer column get one shared width each (the
    // widest entry of their kind, clamped) so the separators line up.
    let type_w = clamp_col_width(items.iter().map(|c| c.type_name.chars().count()), 11.0, 40.0, 96.0);
    let layer_w =
        clamp_col_width(items.iter().map(|c| c.layer.chars().count()), 10.0, 28.0, 84.0);

    let rows: Vec<Element<'static, Message>> = items
        .into_iter()
        .map(|c| item_row(c.handle, c.type_name, c.layer, c.color, type_w, layer_w))
        .collect();

    let panel = container(column(rows))
        .style(container::bordered_box)
        .width(Length::Fixed(type_w + layer_w + 42.0));

    let positioned = iced::widget::pin(opaque(panel))
        .position(iced::Point::new(anchor.x.max(0.0), anchor.y.max(0.0)));

    mouse_area(positioned).on_press(Message::CycleCancel).into()
}

/// Column width for the widest label, using a per-glyph estimate with a
/// minimum so short labels don't collapse and a maximum so long ones clip.
fn clamp_col_width(labels: impl Iterator<Item = usize>, glyph_w: f32, min: f32, max: f32) -> f32 {
    let widest = labels.max().unwrap_or(0) as f32 * glyph_w;
    widest.clamp(min, max)
}

fn item_row(
    handle: acadrust::Handle,
    type_name: String,
    layer: String,
    color: [f32; 4],
    type_w: f32,
    layer_w: f32,
) -> Element<'static, Message> {
    let swatch = container(
        space::Space::new()
            .width(Length::Fixed(9.0))
            .height(Length::Fixed(9.0)),
    )
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            color[0], color[1], color[2], color[3],
        ))),
        border: iced::Border {
            color: color!(0x6b6b6b),
            width: 1.0,
            radius: Radius::from(2.0),
        },
        ..Default::default()
    });

    let content = row![
        swatch,
        container(text(type_name).size(11))
            .width(Length::Fixed(type_w))
            .align_y(iced::alignment::Vertical::Center),
        container(rule::vertical(1)).height(Length::Fixed(12.0)),
        container(
            text(layer)
                .size(10)
                .align_x(Horizontal::Right)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.72)),
                })
        )
        .width(Length::Fixed(layer_w))
        .align_y(iced::alignment::Vertical::Center),
    ]
    .spacing(6)
    .align_y(iced::Center);

    let btn = button(content)
        .on_press(Message::CycleSelect(handle))
        .style(button::subtle)
        .width(Fill)
        .padding([3, 8]);
    // Highlight the underlying object while the cursor is over this row.
    mouse_area(btn)
        .on_enter(Message::CycleHover(Some(handle)))
        .on_exit(Message::CycleHoverExit(handle))
        .into()
}
