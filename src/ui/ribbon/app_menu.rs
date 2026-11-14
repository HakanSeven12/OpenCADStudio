//! The logo button at the top left of the window and the application menu
//! it opens: the file commands, Options and the recent drawings.

use std::path::PathBuf;

use iced::widget::{button, container};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use super::{minimize::MINIMIZE_MENU_ID, Ribbon};
use crate::app::Message;
use crate::ui::icons;
use crate::ui::style::chrome;
use crate::ui::wrap_bar::PosReport;

/// The application menu's dropdown id.
pub const APP_MENU_ID: &str = "APP_MENU";
/// The large logo, with its padding, beside the menu row and the tab row.
const LOGO: f32 = 38.0;
pub(super) const LOGO_PAD: f32 = 6.0;
/// The compact logo at the start of the tab row.
const COMPACT_W: f32 = 38.0;
const COMPACT_H: f32 = 18.0;
const MENU_W: f32 = 300.0;
/// How many recent drawings the menu lists.
const RECENT_LIMIT: usize = 9;

/// Whether the dropdown `id` belongs to the window chrome and so works
/// without a drawing, on the Start tab.
pub fn works_without_drawing(id: &str) -> bool {
    id == APP_MENU_ID || id == MINIMIZE_MENU_ID
}

impl Ribbon {
    /// Close the application menu; whether it was open.
    pub fn close_app_menu(&mut self) -> bool {
        let open = self.open_dropdown.as_deref() == Some(APP_MENU_ID);
        if open {
            self.close_dropdown();
        }
        open
    }

    /// The logo button that opens the application menu: `big` beside the
    /// menu row and the tab row, else compact (its letters only) at the
    /// start of the tab row.
    pub fn logo_button(&self, big: bool) -> Element<'_, Message> {
        let open = self.open_dropdown.as_deref() == Some(APP_MENU_ID);
        let (logo, padding, radius) = if big {
            (icons::logo(false, LOGO, LOGO), Padding::new(LOGO_PAD), 0.0)
        } else {
            (
                icons::logo(true, COMPACT_W - 8.0, COMPACT_H - 6.0),
                Padding::from([3.0, 4.0]),
                3.0,
            )
        };
        let logo = button(logo)
            .on_press(Message::ToggleRibbonDropdown(APP_MENU_ID.to_string()))
            .padding(padding)
            .style(move |theme: &Theme, status| {
                // A pointer on it outlines it and lifts its fill a little.
                let lit =
                    open || matches!(status, button::Status::Hovered | button::Status::Pressed);
                let fill = match (big, lit) {
                    (true, false) => None,
                    (true, true) => Some(chrome::title_bar(theme)),
                    (false, false) => Some(chrome::page(theme)),
                    (false, true) => Some(chrome::bar(theme)),
                };
                button::Style {
                    background: fill.map(Background::Color),
                    border: Border {
                        color: if lit {
                            chrome::border(theme)
                        } else {
                            Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: radius.into(),
                    },
                    ..Default::default()
                }
            });
        PosReport::new(APP_MENU_ID, logo).into()
    }

    /// The application menu under the logo: the file commands and Options,
    /// then the recent drawings.
    pub(super) fn app_menu(&self, recent: &[PathBuf], win: (f32, f32)) -> Element<'_, Message> {
        use crate::ui::palette_menu::{self, Item};
        let mut items: Vec<Item> = commands()
            .into_iter()
            .map(|(icon, label, shortcut, command)| {
                let item = Item::new(label, Message::Command(command.to_string())).icon(icon);
                if shortcut.is_empty() {
                    item
                } else {
                    item.shortcut(shortcut)
                }
            })
            .collect();
        if !recent.is_empty() {
            items.push(Item::Separator);
            items.push(Item::Heading(
                crate::tr!("ribbon-menu", "recent-drawings").to_uppercase(),
            ));
            items.extend(recent.iter().take(RECENT_LIMIT).map(|path| {
                let name = path
                    .file_name()
                    .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
                    .into_owned();
                Item::new(name, Message::OpenRecent(path.clone()))
                    .icon(icons::DOC)
                    .tip(path.display().to_string())
            }));
        }
        // The palette menu, at least as wide as it always was.
        let width = palette_menu::width(&items).max(MENU_W);
        let panel = container(palette_menu::menu(items)).width(Length::Fixed(width));
        self.place_dropdown(APP_MENU_ID, panel.into(), width, win.0)
    }
}

/// The application menu's commands: icon, label, shortcut and command.
pub(super) fn commands() -> [(&'static [u8], String, &'static str, &'static str); 7] {
    [
        (
            icons::DOC_NEW,
            crate::t!("New").into_owned(),
            "Ctrl+N",
            "QNEW",
        ),
        (
            icons::FOLDER_OPEN,
            crate::t!("Open...").into_owned(),
            "Ctrl+O",
            "OPEN",
        ),
        (
            icons::SAVE,
            crate::t!("Save").into_owned(),
            "Ctrl+S",
            "QSAVE",
        ),
        (
            icons::FILE_EXPORT,
            crate::t!("Save as...").into_owned(),
            "",
            "SAVEAS",
        ),
        (
            icons::DOC,
            crate::t!("Export PDF").into_owned(),
            "",
            "EXPORTPDF",
        ),
        (
            icons::PRINT,
            crate::tr!("ribbon-menu", "plot"),
            "Ctrl+P",
            "PLOT",
        ),
        (
            icons::GEAR,
            crate::t!("Options").into_owned(),
            "",
            "OPTIONS",
        ),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_application_menu_command_is_known() {
        let known = crate::command::all_registered_command_names();
        for command in [
            "QNEW",
            "OPEN",
            "QSAVE",
            "SAVEAS",
            "EXPORTPDF",
            "PLOT",
            "OPTIONS",
        ] {
            assert!(known.contains(&command), "{command} is not registered");
        }
    }
}
