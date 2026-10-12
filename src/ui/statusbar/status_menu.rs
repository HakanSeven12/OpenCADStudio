//! Shared `iced_aw::MenuBar` plumbing for status-bar menus.

use iced::{Background, Border, Color, Element, Length, Shadow, Theme};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};

use crate::app::Message;

/// One row in a status-bar menu.
pub struct Entry<'a> {
    content: Element<'a, Message>,
    close_on_click: bool,
}

impl<'a> Entry<'a> {
    /// Keep the menu open after this row is clicked.
    pub fn stay(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            content: content.into(),
            close_on_click: false,
        }
    }

    /// Close the menu after this row is clicked.
    pub fn close(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            content: content.into(),
            close_on_click: true,
        }
    }
}

/// Attach a menu to `root`. Each status-bar menu uses one root so the existing
/// wrapping layout can still move pills independently between rows.
pub fn menu_bar<'a>(
    root: impl Into<Element<'a, Message>>,
    entries: Vec<Entry<'a>>,
    width: f32,
) -> Element<'a, Message> {
    let items = entries
        .into_iter()
        .map(|entry| {
            // Every row answers for the cursor over it, even one that does
            // nothing. A disabled button reports no interaction at all, and the
            // menu is an overlay: when the overlay reports none, the cursor is
            // decided by what lies beneath, which over the drawing is the
            // canvas hiding it for the crosshair. Hovering a greyed-out option
            // therefore made the pointer vanish. `mouse_area` only fills in
            // where the content stays silent, so an enabled row keeps its own
            // pointer. (#684)
            let content =
                iced::widget::mouse_area(entry.content).interaction(iced::mouse::Interaction::Idle);
            Item::new(content).close_on_click(entry.close_on_click)
        })
        .collect();
    let menu = Menu::new(items)
        .width(Length::Fixed(width))
        .padding(0)
        .spacing(0)
        .offset(1.0)
        .close_on_background_click(true);

    MenuBar::new(vec![Item::with_menu(root, menu)])
        // The cursor may leave the menu by this much before it closes. At zero
        // the safe area was the menu's own rectangle, and these menus hang off
        // a pill barely wider than its icon: reaching for a row's text meant
        // moving up and across at once, and the diagonal left the rectangle
        // between the pill and the menu. One row of the bar the menu belongs to
        // is enough room for that diagonal, in every direction. (#682)
        .safe_bounds_margin(super::ROW_HEIGHT)
        .close_on_background_click_global(true)
        .draw_path(DrawPath::Backdrop)
        .style(|theme: &Theme, _| {
            let palette = theme.palette();
            iced_aw::style::menu_bar::Style {
                bar_background: Background::Color(Color::TRANSPARENT),
                bar_border: Border::default(),
                bar_shadow: Shadow::default(),
                menu_background: Background::Color(palette.background.weakest.color),
                menu_border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                menu_shadow: Shadow {
                    color: palette.background.strongest.color.scale_alpha(0.35),
                    offset: iced::Vector::new(0.0, -2.0),
                    blur_radius: 6.0,
                },
                path: Background::Color(palette.primary.weak.color),
                path_border: Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
            }
        })
        .into()
}
