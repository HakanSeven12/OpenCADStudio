//! The main window's title bar, drawn by the app when it has no system frame
//! (Windows and Linux, unless "Use the system title bar" is on): the logo and
//! the Quick Access Toolbar, the drawing's name, the search box and the
//! window buttons in one row over the ribbon. Its empty space moves the
//! window, and a double click maximizes or restores it.

use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::window::Direction;
use iced::{mouse, Background, Element, Fill, Length, Padding, Theme};

use crate::app::Message;
use crate::ui::kit::{tooltip, TipPosition};
use crate::ui::style::{chrome, ribbon_metrics as metrics, text as ui_text};

/// What the title bar and the window's resize edges ask for.
#[derive(Debug, Clone, Copy)]
pub enum TitleBarMsg {
    /// The empty space was pressed: moving the pointer now moves the window.
    Press,
    /// The pointer moved over the empty space.
    Move,
    /// The press ended without moving the window.
    Release,
    /// Double click on the empty space, or the maximize button.
    ToggleMaximize,
    Minimize,
    Close,
    /// An edge of a window without a frame was pressed: resize that way.
    Resize(Direction),
    /// The window is now maximized (or not).
    Maximized(bool),
}

/// Whether the session runs a tiling window manager, which draws no title
/// bars and moves windows itself: the app then draws none either.
pub fn tiling_desktop() -> bool {
    static TILING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TILING.get_or_init(|| {
        cfg!(target_os = "linux") && {
            let names: Vec<String> = [
                "XDG_CURRENT_DESKTOP",
                "XDG_SESSION_DESKTOP",
                "DESKTOP_SESSION",
            ]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .collect();
            is_tiling(&names.join(":"))
        }
    })
}

/// Whether desktop `names` (`:`/`;` separated, as in `XDG_CURRENT_DESKTOP`)
/// name a tiling window manager.
fn is_tiling(names: &str) -> bool {
    const TILING: &[&str] = &[
        "i3",
        "sway",
        "hyprland",
        "bspwm",
        "awesome",
        "dwm",
        "xmonad",
        "qtile",
        "river",
        "niri",
        "herbstluftwm",
        "spectrwm",
        "leftwm",
    ];
    names
        .to_ascii_lowercase()
        .split([':', ';'])
        .any(|name| TILING.contains(&name.trim()))
}

/// Stroked icons on the 24-unit grid of [`crate::ui::icons::line_icon`].
const MINIMIZE: &str = "M6 12h12";
const MAXIMIZE: &str = "M6 6h12v12H6z";
const RESTORE: &str = "M8 9h9v9H8z M10 6h8v8";
const CLOSE: &str = "M6.5 6.5l11 11M17.5 6.5l-11 11";

/// The title bar: `start` (logo and toolbar), the drawing `name` with a dot
/// while it has unsaved changes, `search`, and the window buttons.
/// `pressed`: its empty space is held down (see [`grab`]).
pub fn view<'a>(
    start: Element<'a, Message>,
    name: &str,
    dirty: bool,
    search: Element<'a, Message>,
    maximized: bool,
    pressed: bool,
) -> Element<'a, Message> {
    let mut title = row![].spacing(6).align_y(iced::Center);
    if dirty {
        title = title.push(
            text("●")
                .size(ui_text::UI_SMALL)
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::strong(theme)),
                }),
        );
    }
    title = title
        .push(
            text(name.to_owned())
                .size(ui_text::UI_LARGE)
                .wrapping(text::Wrapping::None)
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::strong(theme)),
                }),
        )
        .push(
            text("— Open CAD Studio")
                .size(ui_text::UI_LARGE)
                .wrapping(text::Wrapping::None)
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::muted(theme)),
                }),
        );

    let grab = grab(
        container(title).center_x(Fill).center_y(Fill).clip(true),
        pressed,
    );

    let bar = row![
        container(start).padding(Padding {
            left: metrics::TITLE_BAR_PAD,
            ..Padding::ZERO
        }),
        container(grab).width(Fill).height(Fill),
        container(search).padding([0.0, metrics::TITLE_BAR_PAD]),
        captions(maximized),
    ]
    .align_y(iced::Center)
    .height(Length::Fixed(metrics::TITLE_BAR_H));

    container(bar)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::bar(theme))),
            ..Default::default()
        })
        .into()
}

/// The empty space, title included, that moves the window. The move starts
/// once the pointer moves while `pressed`, so a double click stays one; only
/// then does it listen to moves, so hovering costs nothing.
fn grab<'a>(content: impl Into<Element<'a, Message>>, pressed: bool) -> Element<'a, Message> {
    let mut grab = mouse_area(content)
        .on_press(Message::TitleBar(TitleBarMsg::Press))
        .on_release(Message::TitleBar(TitleBarMsg::Release))
        .on_double_click(Message::TitleBar(TitleBarMsg::ToggleMaximize));
    if pressed {
        grab = grab
            .on_move(|_| Message::TitleBar(TitleBarMsg::Move))
            // A release outside the bar never reaches it: leaving ends the press.
            .on_exit(Message::TitleBar(TitleBarMsg::Release));
    }
    grab.into()
}

/// The minimize, maximize (or restore) and close buttons.
fn captions<'a>(maximized: bool) -> Element<'a, Message> {
    row![
        caption(
            MINIMIZE,
            crate::t!("Minimize").into_owned(),
            TitleBarMsg::Minimize,
            false
        ),
        caption(
            if maximized { RESTORE } else { MAXIMIZE },
            if maximized {
                crate::t!("Restore").into_owned()
            } else {
                crate::t!("Maximize").into_owned()
            },
            TitleBarMsg::ToggleMaximize,
            false,
        ),
        caption(
            CLOSE,
            crate::t!("Close").into_owned(),
            TitleBarMsg::Close,
            true
        ),
    ]
    .into()
}

/// Only the parts of the title bar that move the window and the window
/// buttons, without a background: laid over a dialog's backdrop, which
/// covers the title bar, so the window can still be moved, minimized,
/// maximized and closed while the dialog is open.
pub fn controls<'a>(maximized: bool, pressed: bool) -> Element<'a, Message> {
    let bar = row![
        grab(Space::new().width(Fill).height(Fill), pressed),
        captions(maximized)
    ]
    .height(Length::Fixed(metrics::TITLE_BAR_H));
    column![bar, Space::new().height(Fill)].into()
}

/// A window button: `close` turns the danger colour while hovered.
fn caption<'a>(
    icon: &'static str,
    tip: String,
    msg: TitleBarMsg,
    close: bool,
) -> Element<'a, Message> {
    let press =
        button(container(crate::ui::icons::line_icon(icon, metrics::CAPTION_ICON)).center(Fill))
            .on_press(Message::TitleBar(msg))
            .width(Length::Fixed(metrics::CAPTION_W))
            .height(Length::Fixed(metrics::TITLE_BAR_H))
            .padding(0)
            .style(move |theme: &Theme, status| {
                let lit = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: lit.then(|| {
                        Background::Color(if close {
                            chrome::danger(theme)
                        } else {
                            chrome::hover(theme)
                        })
                    }),
                    text_color: if lit && close {
                        chrome::strong(theme)
                    } else {
                        chrome::text(theme)
                    },
                    ..Default::default()
                }
            });
    tooltip(press, tip, TipPosition::Bottom)
}

/// `content` inside invisible edges and corners that resize the window,
/// for a window without a system frame (which has no edges of its own).
/// Without `enabled` (a maximized window) the edges are left out, but the
/// widget tree keeps its shape, so no widget below loses its state.
pub fn with_resize_edges<'a>(content: Element<'a, Message>, enabled: bool) -> Element<'a, Message> {
    if !enabled {
        return stack![content, Space::new().width(0).height(0)].into();
    }
    let e = metrics::RESIZE_EDGE;
    let edge = |w: Length, h: Length, dir: Direction, cursor: mouse::Interaction| {
        mouse_area(Space::new().width(w).height(h))
            .on_press(Message::TitleBar(TitleBarMsg::Resize(dir)))
            .interaction(cursor)
    };
    let fixed = Length::Fixed(e);
    let ns = mouse::Interaction::ResizingVertically;
    let ew = mouse::Interaction::ResizingHorizontally;
    let nwse = mouse::Interaction::ResizingDiagonallyDown;
    let nesw = mouse::Interaction::ResizingDiagonallyUp;
    let top = row![
        edge(fixed, fixed, Direction::NorthWest, nwse),
        edge(Fill, fixed, Direction::North, ns),
        edge(fixed, fixed, Direction::NorthEast, nesw),
    ];
    let middle = row![
        edge(fixed, Fill, Direction::West, ew),
        Space::new().width(Fill).height(Fill),
        edge(fixed, Fill, Direction::East, ew),
    ]
    .height(Fill);
    let bottom = row![
        edge(fixed, fixed, Direction::SouthWest, nesw),
        edge(Fill, fixed, Direction::South, ns),
        edge(fixed, fixed, Direction::SouthEast, nwse),
    ];
    stack![content, column![top, middle, bottom]].into()
}

#[cfg(test)]
mod tests {
    use super::is_tiling;

    #[test]
    fn tiling_window_managers_are_recognised() {
        assert!(is_tiling("sway"));
        assert!(is_tiling("Hyprland"));
        assert!(is_tiling("i3:ubuntu"));
        assert!(!is_tiling("GNOME"));
        assert!(!is_tiling("KDE"));
        assert!(!is_tiling("ubuntu:GNOME"));
        assert!(!is_tiling(""));
    }
}
