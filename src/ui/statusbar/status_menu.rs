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
        .map(|entry| Item::new(entry.content).close_on_click(entry.close_on_click))
        .collect();
    let menu = Menu::new(items)
        .width(Length::Fixed(width))
        .padding(0)
        .spacing(0)
        .offset(1.0)
        .close_on_background_click(true);

    MenuBar::new(vec![Item::with_menu(root, menu)])
        .safe_bounds_margin(0.0)
        .close_on_background_click_global(true)
        .draw_path(DrawPath::Backdrop)
        .style(|theme: &Theme, _| {
            let palette = theme.extended_palette();
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
