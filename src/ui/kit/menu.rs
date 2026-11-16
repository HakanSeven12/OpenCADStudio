//! The one menu of the palette area: the palette strip's right-click and +
//! menus, the drawing-tab bar's ☰ menu and a tab's hover list of layouts.
//! Same item height, check marks, separators and colours everywhere.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text as ui_text};
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{Background, Border, Element, Length, Theme};

/// The mark before an item's label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// No mark column content.
    None,
    /// A check mark when on (a toggle).
    Check(bool),
    /// A dot when on (one of a set of choices).
    Radio(bool),
}

/// One entry of a [`menu`].
pub enum Item {
    Action {
        mark: Mark,
        icon: Option<&'static [u8]>,
        label: String,
        /// Dirty dot after the label (an open drawing with unsaved changes).
        dirty: bool,
        /// `None` shows the item disabled.
        on_press: Option<Box<Message>>,
        /// A tooltip beside the item (why a disabled item is disabled).
        tip: Option<String>,
        /// A key that runs it too ("F2"), muted at the row's end.
        shortcut: Option<String>,
    },
    Separator,
    /// A muted caption over the items after it ("EXPAND BY").
    Heading(String),
    /// A row with a ▸ that opens `items` beside the menu: hovering or
    /// pressing it sends `on_open` (`None` shows it disabled); the owner
    /// keeps which one is `open`. See [`menu_with_submenus`].
    Submenu {
        label: String,
        icon: Option<&'static [u8]>,
        open: bool,
        on_open: Option<Box<Message>>,
        items: Vec<Item>,
    },
}

impl Item {
    /// A plain item.
    pub fn new(label: impl Into<String>, on_press: Message) -> Self {
        Item::Action {
            mark: Mark::None,
            icon: None,
            label: label.into(),
            dirty: false,
            on_press: Some(Box::new(on_press)),
            tip: None,
            shortcut: None,
        }
    }

    /// A row opening `items` beside the menu; hovering or pressing it sends
    /// `on_open`.
    pub fn submenu(label: impl Into<String>, on_open: Message, items: Vec<Item>) -> Self {
        Item::Submenu {
            label: label.into(),
            icon: None,
            open: false,
            on_open: Some(Box::new(on_open)),
            items,
        }
    }

    /// The submenu shown open (its items beside the menu).
    pub fn open(mut self, on: bool) -> Self {
        if let Item::Submenu { open, .. } = &mut self {
            *open = on;
        }
        self
    }

    /// The item with `key` muted at its end.
    pub fn shortcut(mut self, key: impl Into<String>) -> Self {
        if let Item::Action { shortcut, .. } = &mut self {
            *shortcut = Some(key.into());
        }
        self
    }

    /// A greyed item that does nothing, with `reason` as its tooltip.
    pub fn disabled(label: impl Into<String>, reason: impl Into<String>) -> Self {
        Item::Action {
            mark: Mark::None,
            icon: None,
            label: label.into(),
            dirty: false,
            on_press: None,
            tip: Some(reason.into()),
            shortcut: None,
        }
    }

    /// The item with `mark` before its label.
    pub fn mark(mut self, m: Mark) -> Self {
        if let Item::Action { mark, .. } = &mut self {
            *mark = m;
        }
        self
    }

    /// The item with an icon before its label.
    pub fn icon(mut self, art: &'static [u8]) -> Self {
        match &mut self {
            Item::Action { icon, .. } | Item::Submenu { icon, .. } => *icon = Some(art),
            _ => {}
        }
        self
    }

    /// The item greyed out and inert unless `on`.
    pub fn enabled(mut self, on: bool) -> Self {
        if !on {
            match &mut self {
                Item::Action { on_press, .. } => *on_press = None,
                Item::Submenu { on_open, open, .. } => {
                    *on_open = None;
                    *open = false;
                }
                _ => {}
            }
        }
        self
    }

    /// The item with `tip` beside it while the pointer rests on it.
    pub fn tip(mut self, text: impl Into<String>) -> Self {
        if let Item::Action { tip, .. } = &mut self {
            *tip = Some(text.into());
        }
        self
    }

    /// The item with a dirty dot after its label.
    pub fn dirty(mut self, on: bool) -> Self {
        if let Item::Action { dirty, .. } = &mut self {
            *dirty = on;
        }
        self
    }
}

/// A small filled dot, `size` across, in `color`.
pub fn dot<'a>(size: f32, color: fn(&Theme) -> iced::Color) -> Element<'a, Message> {
    container(Space::new().width(size).height(size))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(color(theme))),
            border: Border {
                radius: (size / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn mark_cell<'a>(mark: Mark) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match mark {
        Mark::Check(true) => {
            crate::ui::icons::tinted(crate::ui::icons::CHECK, MARK_ICON, chrome::text)
        }
        Mark::Radio(true) => dot(RADIO_DOT, chrome::text),
        _ => Space::new().into(),
    };
    container(inner)
        .center_x(Length::Fixed(km::MENU_CHECK_W))
        .center_y(Length::Fixed(km::MENU_ITEM_H))
        .into()
}

/// Size of a check mark.
const MARK_ICON: f32 = 12.0;
/// Size of a radio dot.
const RADIO_DOT: f32 = 6.0;
/// Size of an item's icon.
const ITEM_ICON: f32 = 16.0;

/// One item as a row: mark, optional icon, label, optional dirty dot.
/// For a menu built by hand (one with rows of its own between the items);
/// [`menu`] builds the usual one.
pub fn entry<'a>(it: Item) -> Element<'a, Message> {
    entry_with(it, None)
}

/// The row of an item, pressable or not: lit while hovered (when it does
/// something), muted when not. Pressed through a `mouse_area`, not a
/// button: an `iced_aw::ContextMenu` rebuilds its overlay on every view
/// pass, and a button there loses its press before the release lands.
/// `on_enter` is sent when the pointer comes onto it (a menu with submenus
/// closes the open one so).
fn row_area<'a>(
    content: iced::widget::Row<'a, Message>,
    on_press: Option<Message>,
    on_enter: Option<Message>,
    lit: bool,
    tip: Option<String>,
) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let style = move |hovered: bool| {
        move |theme: &Theme| container::Style {
            background: (lit || (hovered && enabled))
                .then(|| Background::Color(chrome::hover(theme))),
            // A lit row takes the strong text: the plain text colour is
            // too faint on the hover surface in some themes (WCAG AA).
            text_color: Some(if enabled && (lit || hovered) {
                chrome::strong(theme)
            } else if enabled {
                chrome::text(theme)
            } else {
                chrome::muted(theme)
            }),
            ..Default::default()
        }
    };
    let body = |hovered: bool| {
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(km::MENU_ITEM_H))
            .style(style(hovered))
    };
    let face = container(
        content
            .height(Length::Fixed(km::MENU_ITEM_H))
            .padding(iced::Padding::ZERO.right(km::MENU_PAD_X)),
    )
    .width(Length::Fill)
    .style(style(false));
    let mut area = mouse_area(iced::widget::stack![
        iced::widget::hover(body(false), body(true)),
        face
    ]);
    if let Some(m) = on_press {
        area = area
            .on_press(m)
            .interaction(iced::mouse::Interaction::Pointer);
    }
    if let Some(m) = on_enter {
        area = area.on_enter(m);
    }
    match tip {
        Some(tip) => crate::ui::kit::tooltip(area, tip, crate::ui::kit::TipPosition::Right),
        None => area.into(),
    }
}

/// [`entry`], with `on_enter` sent when the pointer comes onto a row.
fn entry_with<'a>(it: Item, on_enter: Option<Message>) -> Element<'a, Message> {
    match it {
        Item::Separator => container(
            container(Space::new().width(Length::Fill).height(km::line())).style(
                |theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::border(theme))),
                    ..Default::default()
                },
            ),
        )
        .center_y(Length::Fixed(km::MENU_SEPARATOR_H))
        .width(Length::Fill)
        .into(),
        Item::Heading(caption) => container(
            text(caption)
                .size(ui_text::UI_SMALL)
                .line_height(ui_text::line())
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(chrome::muted(theme)),
                }),
        )
        .padding(iced::Padding::ZERO.left(km::MENU_CHECK_W))
        .center_y(Length::Fixed(km::MENU_ITEM_H))
        .width(Length::Fill)
        .into(),
        Item::Action {
            mark,
            icon,
            label,
            dirty,
            on_press,
            tip,
            shortcut,
        } => {
            let mut content = row![mark_cell(mark)].align_y(iced::Center);
            if let Some(art) = icon {
                content = content.push(
                    container(crate::ui::icons::tinted(art, ITEM_ICON, chrome::text))
                        .padding(iced::Padding::ZERO.right(km::TOOL_GAP)),
                );
            }
            content = content.push(
                text(label)
                    .size(ui_text::UI)
                    .line_height(ui_text::line())
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            );
            if dirty {
                content = content.push(dot(km::DIRTY_DOT, chrome::text));
            }
            if let Some(key) = shortcut {
                content = content.push(
                    container(
                        text(key)
                            .size(ui_text::UI)
                            .line_height(ui_text::line())
                            .wrapping(iced::widget::text::Wrapping::None)
                            .style(|theme: &Theme| iced::widget::text::Style {
                                color: Some(chrome::muted(theme)),
                            }),
                    )
                    .padding(iced::Padding::ZERO.left(km::MENU_PAD_X)),
                );
            }
            row_area(content, on_press.map(|m| *m), on_enter, false, tip)
        }
        Item::Submenu {
            label,
            icon,
            open,
            on_open,
            ..
        } => {
            let mut content = row![mark_cell(Mark::None)].align_y(iced::Center);
            if let Some(art) = icon {
                content = content.push(
                    container(crate::ui::icons::tinted(art, ITEM_ICON, chrome::text))
                        .padding(iced::Padding::ZERO.right(km::TOOL_GAP)),
                );
            }
            content = content
                .push(
                    text(label)
                        .size(ui_text::UI)
                        .line_height(ui_text::line())
                        .wrapping(iced::widget::text::Wrapping::None)
                        .width(Length::Fill),
                )
                .push(crate::ui::icons::themed_arrow_right(km::CARET));
            let open_msg = on_open.map(|m| *m);
            row_area(content, open_msg.clone(), open_msg, open, None)
        }
    }
}

/// The menu: its items in a raised frame as wide as its longest item
/// (see [`width`]).
/// The padding inside a menu frame: `pad_y` above and below, and the
/// border's width all round, so a lit row stops inside the border and the
/// border stays on top of it.
pub fn inset(pad_y: f32) -> iced::Padding {
    let line = km::line();
    iced::Padding {
        top: pad_y + line,
        bottom: pad_y + line,
        left: line,
        right: line,
    }
}

pub fn view<'a>(items: Vec<Item>) -> Element<'a, Message> {
    view_at_least(items, 0.0)
}

/// [`view`], at least `min_width` wide.
pub fn view_at_least<'a>(items: Vec<Item>, min_width: f32) -> Element<'a, Message> {
    let width = width(&items).max(min_width);
    container(column(items.into_iter().map(entry)).width(Length::Fill))
        .width(Length::Fixed(width))
        .padding(inset(km::MENU_PAD_Y))
        .style(frame_style)
        .into()
}

/// A menu whose submenu rows open their items beside it: the open one's
/// items hang from its row, and the menu keeps the width of its widest
/// submenu either way (near the window edge the overlay is moved in to fit,
/// and a width that changed with the submenu moved it out from under the
/// pointer, #2258). Coming onto any other row sends `close`.
pub fn with_submenus<'a>(items: Vec<Item>, close: Message) -> Element<'a, Message> {
    let main_w = width(&items);
    let flyout_w = items
        .iter()
        .filter_map(|it| match it {
            Item::Submenu { items, .. } => Some(width(items)),
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    // Where the open submenu's row starts, and its items.
    let mut top = km::MENU_PAD_Y + km::LINE;
    let mut flyout: Option<(f32, Vec<Item>)> = None;
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for it in items {
        let h = item_height(&it);
        match it {
            Item::Submenu {
                label,
                icon,
                open,
                on_open,
                items,
            } => {
                if open && on_open.is_some() {
                    flyout = Some((top, items));
                }
                rows.push(entry_with(
                    Item::Submenu {
                        label,
                        icon,
                        open,
                        on_open,
                        items: Vec::new(),
                    },
                    None,
                ));
            }
            it @ Item::Action { .. } => rows.push(entry_with(it, Some(close.clone()))),
            it => rows.push(entry_with(it, None)),
        }
        top += h;
    }
    let main = container(column(rows).width(Length::Fill))
        .width(Length::Fixed(main_w))
        .padding(inset(km::MENU_PAD_Y))
        .style(frame_style);
    let side: Element<'a, Message> = match flyout {
        // The submenu's first item level with its row.
        Some((at, items)) => column![
            Space::new().height(Length::Fixed(at - km::MENU_PAD_Y - km::LINE)),
            view(items)
        ]
        .into(),
        None => Space::new().width(Length::Fixed(flyout_w)).into(),
    };
    row![main, side]
        .align_y(iced::alignment::Vertical::Top)
        .into()
}

/// Height of one item's row.
fn item_height(it: &Item) -> f32 {
    match it {
        Item::Separator => km::MENU_SEPARATOR_H,
        _ => km::MENU_ITEM_H,
    }
}

/// Width of a menu holding `items`: its longest item — the mark column,
/// the icon, the label as the UI font measures it, a dirty dot — plus the
/// right padding and the frame's border.
pub fn width(items: &[Item]) -> f32 {
    let widest = items
        .iter()
        .filter_map(|it| match it {
            Item::Separator => None,
            Item::Heading(caption) => Some(
                km::MENU_CHECK_W
                    + crate::ui::style::text_metrics::width(caption, ui_text::UI_SMALL, false)
                        .ceil(),
            ),
            Item::Action {
                icon,
                label,
                dirty,
                shortcut,
                ..
            } => {
                let icon = if icon.is_some() {
                    ITEM_ICON + km::TOOL_GAP
                } else {
                    0.0
                };
                let dot = if *dirty {
                    km::TAB_INNER_GAP + km::DIRTY_DOT
                } else {
                    0.0
                };
                let key = shortcut.as_deref().map_or(0.0, |key| {
                    km::MENU_PAD_X
                        + crate::ui::style::text_metrics::width(key, ui_text::UI, false).ceil()
                });
                let text = crate::ui::style::text_metrics::width(label, ui_text::UI, false).ceil();
                Some(km::MENU_CHECK_W + icon + text + dot + key)
            }
            Item::Submenu { icon, label, .. } => {
                let icon = if icon.is_some() {
                    ITEM_ICON + km::TOOL_GAP
                } else {
                    0.0
                };
                let text = crate::ui::style::text_metrics::width(label, ui_text::UI, false).ceil();
                Some(km::MENU_CHECK_W + icon + text + km::TOOL_GAP + km::CARET)
            }
        })
        .fold(0.0_f32, f32::max);
    (widest + km::MENU_PAD_X + 2.0 * km::LINE).ceil()
}

/// Height of a menu holding `items`, for keeping it on screen.
pub fn height(items: &[Item]) -> f32 {
    2.0 * km::MENU_PAD_Y + 2.0 * km::LINE + items.iter().map(item_height).sum::<f32>()
}

/// The menu frame: the surface with a 1 px edge, no rounding.
pub fn frame_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome::surface(theme))),
        text_color: Some(chrome::text(theme)),
        border: Border {
            color: chrome::edge(theme),
            width: km::line(),
            radius: 0.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::BLACK.scale_alpha(MENU_SHADOW_ALPHA),
            offset: iced::Vector::new(0.0, MENU_SHADOW_Y),
            blur_radius: MENU_SHADOW_BLUR,
        },
        ..Default::default()
    }
}

/// The menu's drop shadow.
const MENU_SHADOW_ALPHA: f32 = 0.35;
const MENU_SHADOW_Y: f32 = 2.0;
const MENU_SHADOW_BLUR: f32 = 8.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_counts_items_and_separators() {
        let items = vec![
            Item::new("A", Message::Noop),
            Item::Separator,
            Item::new("B", Message::Noop).mark(Mark::Check(true)),
        ];
        assert_eq!(
            height(&items),
            2.0 * km::MENU_PAD_Y + 2.0 * km::LINE + 2.0 * km::MENU_ITEM_H + km::MENU_SEPARATOR_H
        );
    }

    #[test]
    fn a_menu_is_as_wide_as_its_longest_item() {
        let short = vec![Item::new("Start", Message::Noop)];
        let long = vec![
            Item::new("Start", Message::Noop),
            Item::Separator,
            Item::new("Tabs above the palettes", Message::Noop),
        ];
        let label =
            crate::ui::style::text_metrics::width("Tabs above the palettes", ui_text::UI, false)
                .ceil();
        assert!(width(&long) > width(&short));
        assert_eq!(
            width(&long),
            (km::MENU_CHECK_W + label + km::MENU_PAD_X + 2.0 * km::LINE).ceil()
        );
        // An icon and a dirty dot add their room.
        let more = vec![Item::new("Tabs above the palettes", Message::Noop)
            .icon(crate::ui::icons::PLUS)
            .dirty(true)];
        assert!(width(&more) > width(&long));
    }

    #[test]
    fn a_shortcut_and_a_submenu_caret_add_their_room() {
        let plain = vec![Item::new("Rename", Message::Noop)];
        let keyed = vec![Item::new("Rename", Message::Noop).shortcut("F2")];
        assert!(width(&keyed) > width(&plain));
        let sub = vec![Item::submenu(
            "Rename",
            Message::Noop,
            vec![Item::new("A", Message::Noop)],
        )];
        assert!(width(&sub) > width(&plain));
        assert_eq!(height(&sub), height(&plain));
    }

    #[test]
    fn a_disabled_submenu_is_closed_and_inert() {
        let item = Item::submenu("Change", Message::Noop, Vec::new())
            .open(true)
            .enabled(false);
        assert!(matches!(
            item,
            Item::Submenu {
                open: false,
                on_open: None,
                ..
            }
        ));
    }
}
