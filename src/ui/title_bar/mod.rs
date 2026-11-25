//! The main window's title bar, drawn by the app over the ribbon: the logo
//! and the Quick Access Toolbar, the drawing's name, the search box and the
//! window buttons in one row. Its empty space moves the window, and a double
//! click maximizes or restores it.
//!
//! Windows and Linux open the window without a system frame and the app
//! draws the window buttons in the desktop's style and order (see
//! [`platform`]); macOS keeps its traffic lights over the bar. A tiling
//! window manager keeps the system's way.

pub mod geometry;
mod platform;
#[cfg(target_os = "windows")]
pub mod windows;

use platform::button_metrics;
pub use platform::{
    double_click, layout, tiling_desktop, Button, ButtonLayout, ButtonStyle, DoubleClick,
};

use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::window::Direction;
use iced::{mouse, Background, Element, Fill, Length, Padding, Theme};

use crate::app::Message;
use crate::ui::kit::{tooltip, TipPosition};
use crate::ui::style::{chrome, ribbon_metrics as metrics, text as ui_text};

/// Whether the app draws the main window's title bar: on Windows, Linux and
/// macOS, unless a tiling window manager arranges the windows.
pub fn drawn() -> bool {
    cfg!(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos"
    )) && !tiling_desktop()
}

/// The bar's height: on macOS that of a window's title, so the traffic
/// lights sit in its middle; on Windows that of the system's title bar, so
/// the window buttons match the system's.
pub fn bar_height() -> f32 {
    if cfg!(target_os = "macos") {
        metrics::TITLE_BAR_H_MAC
    } else if cfg!(target_os = "windows") {
        metrics::TITLE_BAR_H_WINDOWS
    } else {
        metrics::TITLE_BAR_H
    }
}

/// Whether the app draws resize edges of its own: without a system frame,
/// unless the system finds the edges itself (Windows, see [`windows`]).
pub fn app_draws_edges() -> bool {
    #[cfg(target_os = "windows")]
    let system_edges = windows::installed();
    #[cfg(not(target_os = "windows"))]
    let system_edges = false;
    frameless() && !system_edges
}

/// Whether the main window opens without a system frame: where the app draws
/// the title bar, except on macOS, which keeps the frame for its traffic
/// lights and its resizing.
pub fn frameless() -> bool {
    drawn() && !cfg!(target_os = "macos")
}

/// What the title bar and the window's resize edges ask for.
#[derive(Debug, Clone, Copy)]
pub enum TitleBarMsg {
    /// The empty space was pressed: moving the pointer now moves the window.
    Press,
    /// The pointer moved over the empty space.
    Move,
    /// The press ended without moving the window.
    Release,
    /// The maximize button.
    ToggleMaximize,
    /// Double click on the empty space: as the desktop does it.
    DoubleClick,
    Minimize,
    Close,
    /// An edge of a window without a frame was pressed: resize that way.
    Resize(Direction),
    /// The window is now maximized (or not).
    Maximized(bool),
}

/// The title bar: `start` (logo and toolbar), the drawing `name` with a dot
/// while it has unsaved changes, `search`, and the window buttons.
/// `pressed`: its empty space is held down (see [`grab`]).
/// `room`: the widths around the name, which keep it in place.
pub fn view<'a>(
    start: Element<'a, Message>,
    name: &str,
    dirty: bool,
    search: Element<'a, Message>,
    room: Room,
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
                .ellipsis(text::Ellipsis::End)
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::strong(theme)),
                }),
        )
        .push(
            // Short of room, the product's name gives way first.
            text("— Open CAD Studio")
                .size(ui_text::UI_LARGE)
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::End)
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::muted(theme)),
                }),
        );

    // The name sits in the middle of the window, as far from both ends as
    // the wider of them reaches at rest. A collapsed search box that opens
    // for a moment then draws over the name instead of moving it.
    let buttons = layout();
    let ends = room.ends();
    // The clip holds the name between the ends: a container clips to its own
    // bounds, so it sits inside the padding, not around it.
    let name_layer =
        container(container(title).center_x(Fill).center_y(Fill).clip(true)).padding(Padding {
            left: ends,
            right: ends,
            ..Padding::ZERO
        });
    let grab = grab(Space::new().width(Fill).height(Fill), pressed);

    let bar = row![
        captions(buttons, &buttons.left, maximized, Side::Left),
        container(start).padding(Padding {
            left: leading_inset(),
            ..Padding::ZERO
        }),
        container(grab).width(Fill).height(Fill),
        container(search).padding([0.0, metrics::TITLE_BAR_PAD]),
        captions(buttons, &buttons.right, maximized, Side::Right),
    ]
    .align_y(iced::Center)
    .height(Length::Fixed(bar_height()));

    container(stack![name_layer, bar].height(Length::Fixed(bar_height())))
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
        .on_double_click(Message::TitleBar(TitleBarMsg::DoubleClick));
    if pressed {
        grab = grab
            .on_move(|_| Message::TitleBar(TitleBarMsg::Move))
            // A release outside the bar never reaches it: leaving ends the press.
            .on_exit(Message::TitleBar(TitleBarMsg::Release));
    }
    grab.into()
}

/// The Windows and Adwaita icons, on the 24-unit grid of
/// [`crate::ui::icons::line_icon`].
const MINIMIZE: &str = "M5 12h14";
const MAXIMIZE: &str = "M5 5h14v14H5z";
const RESTORE: &str = "M5 8h11v11H5z M8 5h11v11h-3";
const CLOSE: &str = "M5 5l14 14M19 5L5 19";
/// GNOME's and KDE's, a little smaller in their round buttons.
const ROUND_MINIMIZE: &str = "M6 12h12";
const ROUND_MAXIMIZE: &str = "M6 6h12v12H6z";
const ROUND_RESTORE: &str = "M8 9h9v9H8z M10 6h8v8";
const ROUND_CLOSE: &str = "M6.5 6.5l11 11M17.5 6.5l-11 11";

/// The widths that decide where the drawing's name sits.
#[derive(Debug, Clone, Copy, Default)]
pub struct Room {
    /// The logo and the Quick Access Toolbar, as last laid out.
    pub start: f32,
    /// The search box at rest: the box, or its magnifier while collapsed,
    /// not the box a collapsed search opens for a moment.
    pub search: f32,
}

impl Room {
    /// How far the name keeps from each end of the bar.
    fn ends(self) -> f32 {
        ends(self.start, self.search)
    }
}

/// How far the name keeps from each end of the bar, with a `start` (logo
/// and toolbar or menus) and a `search` box this wide: the wider of the two
/// ends with their window buttons, so the name stays in the window's middle.
fn ends(start: f32, search: f32) -> f32 {
    let buttons = layout();
    let group = |n| platform::group_width(buttons.style, n);
    let left = group(buttons.left.len()) + leading_inset() + start;
    let right = search + 2.0 * metrics::TITLE_BAR_PAD + group(buttons.right.len());
    left.max(right) + metrics::TITLE_BAR_PAD
}

/// The width left to the drawing's name in a window `window_w` wide, with a
/// `start` and a `search` box this wide: the search box and the menus give
/// way when it runs short.
pub fn name_room(window_w: f32, start: f32, search: f32) -> f32 {
    window_w - 2.0 * ends(start, search)
}

/// Which end of the bar a group of window buttons sits at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// Room before the logo: the traffic lights on macOS, else a small margin.
fn leading_inset() -> f32 {
    if layout().style == ButtonStyle::System {
        metrics::TRAFFIC_LIGHTS_W
    } else {
        metrics::TITLE_BAR_PAD
    }
}

/// The width the window buttons and the inset before the logo take, for
/// the room left to the search box.
pub fn buttons_width() -> f32 {
    platform::buttons_width_of(layout())
}

/// A group of window buttons, `side` telling at which end its margin goes.
fn captions<'a>(
    layout: &ButtonLayout,
    buttons: &[Button],
    maximized: bool,
    side: Side,
) -> Element<'a, Message> {
    if buttons.is_empty() {
        return Space::new().width(0).height(0).into();
    }
    let (_, gap, margin) = button_metrics(layout.style);
    let group = buttons.iter().fold(
        row![].spacing(gap).align_y(iced::Center),
        |group, &which| group.push(caption(layout.style, which, maximized)),
    );
    let padding = match side {
        Side::Left => Padding {
            left: margin,
            ..Padding::ZERO
        },
        Side::Right => Padding {
            right: margin,
            ..Padding::ZERO
        },
    };
    container(group).padding(padding).into()
}

/// Stroked icons on the 24-unit grid of [`crate::ui::icons::line_icon`].
fn icon(style: ButtonStyle, button: Button, maximized: bool) -> &'static str {
    match (style, button, maximized) {
        // Breeze's chevrons and diamond.
        (ButtonStyle::Breeze, Button::Minimize, _) => "M7 10l5 5 5-5",
        (ButtonStyle::Breeze, Button::Maximize, false) => "M7 14l5-5 5 5",
        (ButtonStyle::Breeze, Button::Maximize, true) => "M12 6.5l5.5 5.5-5.5 5.5-5.5-5.5z",
        (ButtonStyle::Breeze, Button::Close, _) => ROUND_CLOSE,
        (ButtonStyle::Adwaita, Button::Minimize, _) => ROUND_MINIMIZE,
        (ButtonStyle::Adwaita, Button::Maximize, false) => ROUND_MAXIMIZE,
        (ButtonStyle::Adwaita, Button::Maximize, true) => ROUND_RESTORE,
        (ButtonStyle::Adwaita, Button::Close, _) => ROUND_CLOSE,
        (_, Button::Minimize, _) => MINIMIZE,
        (_, Button::Maximize, false) => MAXIMIZE,
        (_, Button::Maximize, true) => RESTORE,
        (_, Button::Close, _) => CLOSE,
    }
}

/// Whether the system reports the pointer over the maximize button (Windows
/// sends it there as a non-client button, see [`windows`]).
fn system_hover(button: Button) -> bool {
    #[cfg(target_os = "windows")]
    if button == Button::Maximize {
        return windows::maximize_hovered();
    }
    let _ = button;
    false
}

/// One window button in the desktop's `style`.
fn caption<'a>(style: ButtonStyle, which: Button, maximized: bool) -> Element<'a, Message> {
    let (msg, tip) = match which {
        Button::Minimize => (TitleBarMsg::Minimize, crate::t!("Minimize")),
        Button::Maximize if maximized => (TitleBarMsg::ToggleMaximize, crate::t!("Restore Down")),
        Button::Maximize => (TitleBarMsg::ToggleMaximize, crate::t!("Maximize")),
        Button::Close => (TitleBarMsg::Close, crate::t!("Close")),
    };
    let close = which == Button::Close;
    let (w, h, icon_size, radius) = match style {
        ButtonStyle::Windows => (metrics::CAPTION_W, bar_height(), metrics::CAPTION_ICON, 0.0),
        ButtonStyle::Adwaita => (
            metrics::ADWAITA_BUTTON,
            metrics::ADWAITA_BUTTON,
            metrics::ADWAITA_ICON,
            metrics::ADWAITA_BUTTON / 2.0,
        ),
        ButtonStyle::Breeze | ButtonStyle::System => (
            metrics::BREEZE_BUTTON,
            metrics::BREEZE_BUTTON,
            metrics::BREEZE_ICON,
            metrics::BREEZE_BUTTON / 2.0,
        ),
    };
    let press = button(
        container(crate::ui::icons::line_icon(
            icon(style, which, maximized),
            icon_size,
        ))
        .center(Fill),
    )
    .on_press(Message::TitleBar(msg))
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .padding(0)
    .style(move |theme: &Theme, status| {
        let lit = matches!(status, button::Status::Hovered | button::Status::Pressed)
            || system_hover(which);
        // Windows and Breeze light the close button red; Adwaita keeps its
        // round tint, a step darker under the pointer.
        let red = lit && close && style != ButtonStyle::Adwaita;
        let background = match style {
            ButtonStyle::Adwaita if lit => Some(chrome::border(theme)),
            ButtonStyle::Adwaita => Some(chrome::hover(theme)),
            _ if red => Some(chrome::danger(theme)),
            _ if lit => Some(chrome::hover(theme)),
            _ => None,
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: if red {
                chrome::strong(theme)
            } else {
                chrome::text(theme)
            },
            border: iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });
    tooltip(press, tip.into_owned(), TipPosition::Bottom)
}

/// Only the parts of the title bar that move the window and the window
/// buttons, without a background: laid over a dialog's backdrop, which
/// covers the title bar, so the window can still be moved, minimized,
/// maximized and closed while the dialog is open.
pub fn controls<'a>(maximized: bool, pressed: bool) -> Element<'a, Message> {
    let buttons = layout();
    let bar = row![
        captions(buttons, &buttons.left, maximized, Side::Left),
        grab(Space::new().width(Fill).height(Fill), pressed),
        captions(buttons, &buttons.right, maximized, Side::Right),
    ]
    .align_y(iced::Center)
    .height(Length::Fixed(bar_height()));
    column![bar, Space::new().height(Fill)].into()
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
