//! The ribbon's right-click menu: which tabs and panels show, a reset to
//! the default layout, panel titles, the menu bar and closing the ribbon.
//! It opens where the tab row or the tool area is right-clicked; its two
//! lists open beside it on hover.

use std::time::Duration;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, stack, text, tooltip, Space,
};
use iced::{
    Background, Border, Color, Element, Event, Fill, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme, Vector,
};

use super::layout::QatItem;
use super::widgets::{make_tip, popup_panel_style, tip_style};
use super::{extras, Ribbon};
use crate::app::Message;
use crate::ui::icons;
use crate::ui::popup::context_menu::{MENU_ROW_H, MENU_SEP_H};
use crate::ui::style::{chrome, text_metrics};

/// Size of the check mark at the end of a checked row, as in the viewport
/// menu.
const CHECK_SIZE: f32 = 10.0;
const CARET_SIZE: f32 = 9.0;
/// Size of a row's label.
const LABEL_SIZE: f32 = 12.0;
/// Least gap between a label and the check or caret at the row's end.
const MARK_GAP: f32 = 16.0;

/// The lists the menu opens beside itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submenu {
    Tabs,
    Panels,
}

/// What was right-clicked, which decides what the menu offers.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuTarget {
    /// The ribbon's tool area, or anything else of it.
    Ribbon,
    /// The tab row or the Quick Access Toolbar's empty space.
    TabRow,
    /// A ribbon tool that runs a command (or opens a dropdown): it can go
    /// on the Quick Access Toolbar.
    Tool(QatItem),
    /// The Quick Access item at this index.
    Qat(usize),
}

#[derive(Clone, Debug)]
pub enum RibbonMenuMsg {
    /// Open the menu for what was right-clicked at this window point.
    Open(Point, MenuTarget),
    Close,
    /// The pointer is over the row that opens this list, or over a row that
    /// opens none.
    Hover(Option<Submenu>),
    /// Show or hide the tab of this module.
    ToggleTab(&'static str),
    /// Show or hide the panel with this key.
    TogglePanel(String),
    Reset,
    TogglePanelTitles,
    /// Show or hide the menu bar (handled by the app).
    ShowMenuBar(bool),
    /// Show the ribbon (`true`) or close it (handled by the app).
    ShowRibbon(bool),
    /// Add a tool to the Quick Access Toolbar, or take it off.
    QatToggle(QatItem),
    /// Take the Quick Access item at this index off.
    QatRemove(usize),
    /// Add a separator after the Quick Access item at this index, or take
    /// away the one there.
    QatSeparatorAfter(usize),
    /// Show the Quick Access Toolbar below the ribbon (`true`) or above.
    QatBelow(bool),
}

#[derive(Clone, Debug)]
pub(super) struct MenuState {
    at: Point,
    open: Option<Submenu>,
    target: MenuTarget,
}

/// One row of the menu or of a list.
struct Row {
    label: String,
    checked: bool,
    enabled: bool,
    /// Shown while the pointer rests on a disabled row.
    reason: Option<String>,
    action: RibbonMenuMsg,
}

impl Ribbon {
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Close the menu; whether it was open.
    pub fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    /// Apply a menu message the ribbon owns. Returns whether the layout
    /// changed and should be saved.
    pub fn on_menu(&mut self, msg: RibbonMenuMsg) -> bool {
        match msg {
            RibbonMenuMsg::Open(at, target) => {
                self.close_dropdown();
                self.menu = Some(MenuState {
                    at,
                    open: None,
                    target,
                });
                false
            }
            RibbonMenuMsg::QatToggle(item) => {
                self.menu = None;
                self.layout.qat_toggle(item);
                true
            }
            RibbonMenuMsg::QatRemove(index) => {
                self.menu = None;
                if let Some(item) = self.layout.qat.get(index).cloned() {
                    if item == QatItem::Separator {
                        self.layout.qat.remove(index);
                    } else {
                        self.layout.qat_remove(&item);
                    }
                }
                true
            }
            RibbonMenuMsg::QatSeparatorAfter(index) => {
                self.menu = None;
                let next = index + 1;
                if self.layout.qat.get(next) == Some(&QatItem::Separator) {
                    self.layout.qat.remove(next);
                } else if index < self.layout.qat.len() {
                    self.layout.qat.insert(next, QatItem::Separator);
                }
                // What a reload would drop (one at an end, two in a row)
                // goes now, so the toolbar shows what is saved.
                self.layout.tidy_qat_separators();
                true
            }
            RibbonMenuMsg::QatBelow(below) => {
                self.menu = None;
                self.layout.qat_below = below;
                true
            }
            RibbonMenuMsg::Close => {
                self.menu = None;
                false
            }
            // Picked from this menu or the Quick Access customize menu.
            RibbonMenuMsg::ShowMenuBar(_) | RibbonMenuMsg::ShowRibbon(_) => {
                self.menu = None;
                self.close_dropdown();
                false
            }
            RibbonMenuMsg::Hover(open) => {
                if let Some(menu) = &mut self.menu {
                    menu.open = open;
                }
                false
            }
            RibbonMenuMsg::ToggleTab(id) => {
                self.menu = None;
                if !self.layout.hidden_tabs.remove(id) {
                    self.layout.hidden_tabs.insert(id.to_string());
                }
                self.show_a_visible_tab();
                true
            }
            RibbonMenuMsg::TogglePanel(key) => {
                self.menu = None;
                // A floating panel unticked docks and hides.
                self.dock_floating(&key);
                if !self.layout.hidden_panels.remove(&key) {
                    self.layout.hidden_panels.insert(key);
                }
                true
            }
            RibbonMenuMsg::Reset => {
                self.menu = None;
                self.layout.reset();
                true
            }
            RibbonMenuMsg::TogglePanelTitles => {
                self.menu = None;
                self.layout.show_panel_titles ^= true;
                true
            }
        }
    }

    /// The tabs the user can show or hide: every tab but the contextual ones,
    /// each with whether it shows.
    fn tab_rows(&self) -> Vec<Row> {
        let tabs: Vec<_> = self
            .modules
            .iter()
            .filter(|module| !matches!(module.id(), "layout" | "block_editor"))
            .map(|module| (module, !self.layout.hidden_tabs.contains(module.id())))
            .collect();
        let shown = tabs.iter().filter(|(_, shown)| *shown).count();
        tabs.into_iter()
            .map(|(module, checked)| Row {
                label: crate::i18n::ribbon_module_title(module.id(), module.title()),
                checked,
                // The last tab shown stays.
                enabled: !(checked && shown == 1),
                reason: None,
                action: RibbonMenuMsg::ToggleTab(module.id()),
            })
            .collect()
    }

    /// The panels of the tab on show, in the user's order, each with whether
    /// it shows.
    fn panel_rows(&self) -> Vec<Row> {
        let Some(module) = self.modules.get(self.active) else {
            return Vec::new();
        };
        let id = module.id();
        let panels = self.ordered_groups(self.active);
        let shown = panels
            .iter()
            .filter(|group| !self.layout.panel_hidden(id, group.title))
            .count();
        panels
            .into_iter()
            .map(|group| {
                let checked = !self.layout.panel_hidden(id, group.title);
                let floating = self.layout.floating_panel(id, group.title).is_some();
                Row {
                    label: if floating {
                        format!(
                            "{}{}",
                            crate::t!(group.title),
                            crate::tr!("ribbon-menu", "floating-suffix")
                        )
                    } else {
                        crate::t!(group.title).into_owned()
                    },
                    checked,
                    // The last panel shown stays.
                    enabled: !(checked && shown == 1),
                    reason: None,
                    action: RibbonMenuMsg::TogglePanel(extras::panel_key(id, group.title)),
                }
            })
            .collect()
    }

    /// The rows of the open menu `menu`, as it shows for what was
    /// right-clicked; `None` is a separator.
    fn menu_rows(
        &self,
        menu: &MenuState,
        show_menu_bar: bool,
        ribbon_closed: bool,
    ) -> Vec<Option<(Row, Option<Submenu>)>> {
        let is_default = self.is_default_layout();
        let plain = |label: String, action: RibbonMenuMsg| {
            Some((
                Row {
                    label,
                    checked: false,
                    enabled: true,
                    reason: None,
                    action,
                },
                None,
            ))
        };
        let menu_bar_row = plain(
            if show_menu_bar {
                crate::tr!("ribbon-menu", "hide-menu-bar")
            } else {
                crate::tr!("ribbon-menu", "show-menu-bar")
            },
            RibbonMenuMsg::ShowMenuBar(!show_menu_bar),
        );
        let below = self.layout.qat_below;
        let place_row = |label: String| {
            Some((
                Row {
                    label,
                    checked: false,
                    enabled: !ribbon_closed,
                    reason: ribbon_closed
                        .then(|| crate::tr!("ribbon-menu", "qat-place-unavailable")),
                    action: RibbonMenuMsg::QatBelow(!below),
                },
                None,
            ))
        };
        // The rows; `None` is a separator.
        let ribbon_rows: Vec<Option<(Row, Option<Submenu>)>> = vec![
            Some((
                Row {
                    label: crate::tr!("ribbon-menu", "show-tabs"),
                    checked: false,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::Hover(Some(Submenu::Tabs)),
                },
                Some(Submenu::Tabs),
            )),
            Some((
                Row {
                    label: crate::tr!("ribbon-menu", "show-panels"),
                    checked: false,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::Hover(Some(Submenu::Panels)),
                },
                Some(Submenu::Panels),
            )),
            Some((
                Row {
                    label: crate::tr!("ribbon-menu", "reset"),
                    checked: false,
                    enabled: !is_default,
                    reason: is_default.then(|| crate::tr!("ribbon-menu", "already-default")),
                    action: RibbonMenuMsg::Reset,
                },
                None,
            )),
            None,
            Some((
                Row {
                    label: crate::tr!("ribbon-menu", "show-panel-titles"),
                    checked: self.layout.show_panel_titles,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::TogglePanelTitles,
                },
                None,
            )),
            None,
            Some((
                Row {
                    label: if show_menu_bar {
                        crate::tr!("ribbon-menu", "hide-menu-bar")
                    } else {
                        crate::tr!("ribbon-menu", "show-menu-bar")
                    },
                    checked: false,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::ShowMenuBar(!show_menu_bar),
                },
                None,
            )),
            Some((
                Row {
                    label: if ribbon_closed {
                        crate::tr!("ribbon-menu", "show-ribbon")
                    } else {
                        crate::tr!("ribbon-menu", "close-ribbon")
                    },
                    checked: false,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::ShowRibbon(ribbon_closed),
                },
                None,
            )),
        ];

        let rows: Vec<Option<(Row, Option<Submenu>)>> = match &menu.target {
            MenuTarget::Ribbon => ribbon_rows,
            MenuTarget::TabRow => {
                let mut rows = ribbon_rows;
                rows.push(None);
                rows.push(place_row(if below {
                    crate::tr!("ribbon-menu", "qat-show-above")
                } else {
                    crate::tr!("ribbon-menu", "qat-show-below")
                }));
                rows
            }
            MenuTarget::Tool(item) => {
                let label = if self.layout.qat_has(item) {
                    crate::tr!("ribbon-menu", "remove-from-qat")
                } else {
                    crate::tr!("ribbon-menu", "add-to-qat")
                };
                let mut rows = vec![plain(label, RibbonMenuMsg::QatToggle(item.clone())), None];
                rows.extend(ribbon_rows);
                rows
            }
            MenuTarget::Qat(index) => {
                let qat = &self.layout.qat;
                let separator_after = qat.get(index + 1) == Some(&QatItem::Separator);
                // A separator goes only between two items: not after the
                // last one, nor after a separator.
                let separable = qat
                    .get(*index)
                    .is_some_and(|item| *item != QatItem::Separator)
                    && index + 1 < qat.len();
                let mut rows = vec![plain(
                    crate::tr!("ribbon-menu", "remove-from-qat"),
                    RibbonMenuMsg::QatRemove(*index),
                )];
                if separator_after {
                    rows.push(plain(
                        crate::tr!("ribbon-menu", "remove-separator"),
                        RibbonMenuMsg::QatSeparatorAfter(*index),
                    ));
                } else if separable {
                    rows.push(plain(
                        crate::tr!("ribbon-menu", "add-separator"),
                        RibbonMenuMsg::QatSeparatorAfter(*index),
                    ));
                }
                rows.extend([
                    None,
                    place_row(if below {
                        crate::tr!("ribbon-menu", "show-above")
                    } else {
                        crate::tr!("ribbon-menu", "show-below")
                    }),
                    menu_bar_row,
                ]);
                rows
            }
        };
        rows
    }

    /// The menu and its open list over the whole window, with a backdrop
    /// that closes it on any click outside.
    pub fn menu_overlay(
        &self,
        win: (f32, f32),
        show_menu_bar: bool,
        ribbon_closed: bool,
    ) -> Option<Element<'_, Message>> {
        let menu = self.menu.as_ref()?;
        let rows = self.menu_rows(menu, show_menu_bar, ribbon_closed);

        let height = list_height(rows.iter().map(Option::is_some));
        let menu_w = fit_width(rows.iter().flatten().map(|(row, _)| row.label.as_str()));
        let (left, top) = place(menu.at, (menu_w, height), win);

        // Where the open list's row sits, for placing the list beside it.
        let mut open_row_y = None;
        let mut y = frame();
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        for entry in rows {
            let Some((row, opens)) = entry else {
                items.push(separator());
                y += MENU_SEP_H;
                continue;
            };
            if opens.is_some() && opens == menu.open {
                open_row_y = Some(y);
            }
            items.push(match opens {
                Some(list) => submenu_row(row.label, menu.open == Some(list), list),
                None => mouse_area(item_row(row))
                    .on_enter(Message::RibbonMenu(RibbonMenuMsg::Hover(None)))
                    .into(),
            });
            y += MENU_ROW_H;
        }
        // Opaque, so the ribbon under the menu shows no hover or tooltip.
        let mut layers: Vec<Element<'_, Message>> = vec![
            opaque(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(Message::RibbonMenu(RibbonMenuMsg::Close))
                    .on_right_press(Message::RibbonMenu(RibbonMenuMsg::Close)),
            ),
            placed(panel_of_width(items, menu_w), left, top),
        ];

        if let (Some(list), Some(row_y)) = (menu.open, open_row_y) {
            let rows = match list {
                Submenu::Tabs => self.tab_rows(),
                Submenu::Panels => self.panel_rows(),
            };
            let width = fit_width(rows.iter().map(|row| row.label.as_str()));
            let height = list_height(rows.iter().map(|_| true));
            // Beside the menu on the right, or on the left when the right
            // has no room; its first row level with the row that opened it.
            // A list overlaps the menu by its border, so the two share one
            // line, and its first row is level with the row that opened it.
            let right = left + menu_w - frame();
            let x = if right + width <= win.0 {
                right
            } else {
                (left - width + frame()).max(0.0)
            };
            let y = (top + row_y - frame()).min((win.1 - height).max(0.0));
            let items = rows.into_iter().map(item_row).collect();
            layers.push(placed(panel_of_width(items, width), x, y));
        }
        Some(stack(layers).width(Fill).height(Fill).into())
    }
}

/// Top-left corner of a `size` panel opened at `at`, kept inside the window.
fn place(at: Point, size: (f32, f32), win: (f32, f32)) -> (f32, f32) {
    (
        at.x.min(win.0 - size.0).max(0.0),
        at.y.min(win.1 - size.1).max(0.0),
    )
}

/// Height of a menu panel whose rows are items (`true`) or separators.
fn list_height(rows: impl Iterator<Item = bool>) -> f32 {
    2.0 * frame()
        + rows
            .map(|item| if item { MENU_ROW_H } else { MENU_SEP_H })
            .sum::<f32>()
}

/// A menu as wide as its longest label, with room for the check or caret
/// at the end of a row and the border.
pub(super) fn fit_width<'a>(labels: impl Iterator<Item = &'a str>) -> f32 {
    let longest = labels
        .map(|label| text_metrics::width(label, LABEL_SIZE, false))
        .fold(0.0, f32::max);
    longest + ROW_PADDING.left + ROW_PADDING.right + MARK_GAP + CHECK_SIZE.max(CARET_SIZE) + 2.0
}

fn placed<'a>(panel: Element<'a, Message>, left: f32, top: f32) -> Element<'a, Message> {
    container(panel)
        .padding(Padding {
            top,
            left,
            ..Padding::ZERO
        })
        .width(Fill)
        .height(Fill)
        .into()
}

/// The width of a menu's border: one physical pixel. The rows sit just
/// inside it, with no room above the first or below the last.
fn frame() -> f32 {
    icons::hairline()
}

/// The menu's frame: a ribbon popup whose rows sit just inside its
/// one-pixel border, as the menu bar's do.
pub(super) fn panel_of_width(items: Vec<Element<'_, Message>>, width: f32) -> Element<'_, Message> {
    container(column(items).width(Fill))
        .style(popup_panel_style)
        .padding(frame())
        .width(Length::Fixed(width))
        .into()
}

/// A line one physical pixel high across the menu, inset at the sides: a
/// logical pixel would draw one or two pixels high at a fractional scale.
fn separator<'a>() -> Element<'a, Message> {
    let line =
        container(Space::new().width(Fill).height(icons::hairline())).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::border(theme))),
                ..Default::default()
            }
        });
    container(line)
        .padding(Padding {
            top: (MENU_SEP_H - icons::hairline()) / 2.0,
            bottom: (MENU_SEP_H - icons::hairline()) / 2.0,
            left: 6.0,
            right: 6.0,
        })
        .width(Fill)
        .height(MENU_SEP_H)
        .into()
}

const ROW_PADDING: Padding = Padding {
    top: 3.0,
    right: 12.0,
    bottom: 3.0,
    left: 12.0,
};

fn item_row<'a>(row: Row) -> Element<'a, Message> {
    let item = check_row(
        row.label,
        row.checked,
        row.enabled.then_some(Message::RibbonMenu(row.action)),
    );
    match row.reason {
        Some(reason) => tooltip(item, make_tip(reason), tooltip::Position::Right)
            .delay(Duration::from_millis(400))
            .style(tip_style)
            .into(),
        None => item.into(),
    }
}

/// A menu row: its label, and a check at the right when `checked`. Without
/// `on_press` it is disabled.
pub(super) fn check_row<'a>(
    label: String,
    checked: bool,
    on_press: Option<Message>,
) -> button::Button<'a, Message> {
    let enabled = on_press.is_some();
    let mut content = row![text(label).size(LABEL_SIZE)].align_y(iced::Center);
    if checked {
        content = content.push(Space::new().width(Fill)).push(if enabled {
            icons::themed(icons::CHECK, CHECK_SIZE)
        } else {
            icons::themed_disabled(icons::CHECK, CHECK_SIZE)
        });
    }
    button(content)
        .padding(ROW_PADDING)
        .width(Fill)
        .height(Length::Fixed(MENU_ROW_H))
        .style(row_style)
        .on_press_maybe(on_press)
}

/// A row that opens `list` beside the menu when hovered or clicked.
fn submenu_row<'a>(label: String, open: bool, list: Submenu) -> Element<'a, Message> {
    let content = row![
        text(label).size(LABEL_SIZE),
        Space::new().width(Fill),
        icons::themed_arrow_right(CARET_SIZE),
    ]
    .align_y(iced::Center);
    let item = button(content)
        .padding(ROW_PADDING)
        .width(Fill)
        .height(Length::Fixed(MENU_ROW_H))
        .on_press(Message::RibbonMenu(RibbonMenuMsg::Hover(Some(list))))
        .style(move |theme: &Theme, status| {
            // The row of the open list stays lit while the pointer is in it.
            row_style(
                theme,
                if open {
                    button::Status::Hovered
                } else {
                    status
                },
            )
        });
    mouse_area(item)
        .on_enter(Message::RibbonMenu(RibbonMenuMsg::Hover(Some(list))))
        .into()
}

/// Row colours of the viewport menu: hover tints the background, disabled
/// rows fade their text.
fn row_style(theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Background::Color(chrome::hover(theme))),
            chrome::strong(theme),
        ),
        button::Status::Disabled => (None, chrome::muted(theme)),
        button::Status::Active => (None, chrome::text(theme)),
    };
    button::Style {
        background,
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// Opens the ribbon menu where `child` is right-clicked, for `target`. It
/// reports the press only, so moving the pointer over the ribbon sends
/// nothing. The innermost one under the pointer answers.
pub(super) struct RightPress<'a> {
    child: Element<'a, Message>,
    target: MenuTarget,
}

impl<'a> RightPress<'a> {
    pub(super) fn new(child: impl Into<Element<'a, Message>>) -> Self {
        Self::for_target(MenuTarget::Ribbon, child)
    }

    pub(super) fn for_target(target: MenuTarget, child: impl Into<Element<'a, Message>>) -> Self {
        Self {
            child: child.into(),
            target,
        }
    }
}

/// The Quick Access item of a ribbon tool: the command it runs.
pub(super) fn tool_qat(tool: &crate::modules::ToolDef) -> Option<QatItem> {
    match &tool.event {
        crate::modules::ModuleEvent::Command(cmd) => Some(QatItem::Command(cmd.clone())),
        _ => None,
    }
}

/// The Quick Access item of a ribbon item that is one button: its tool's
/// command, or a dropdown, which the toolbar shows as a split item.
pub(super) fn item_qat(item: &crate::modules::RibbonItem) -> Option<QatItem> {
    use crate::modules::RibbonItem;
    match item {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => tool_qat(t),
        RibbonItem::Dropdown { id, .. }
        | RibbonItem::LabeledDropdown { id, .. }
        | RibbonItem::LargeDropdown { id, .. } => Some(QatItem::Command(id.to_string())),
        _ => None,
    }
}

/// `child`, a ribbon tool that runs `item`: right-clicked, the menu offers
/// to add it to the Quick Access Toolbar. Without an item it is left as is.
pub(super) fn tool_target<'a>(
    item: Option<QatItem>,
    child: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    match item {
        Some(item) => RightPress::for_target(MenuTarget::Tool(item), child).into(),
        None => child.into(),
    }
}

impl<'a> Widget<Message, Theme, Renderer> for RightPress<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
            if let Some(at) = cursor.position_over(layout.bounds()) {
                if !shell.is_event_captured() {
                    shell.publish(Message::RibbonMenu(RibbonMenuMsg::Open(
                        at,
                        self.target.clone(),
                    )));
                    shell.capture_event();
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<RightPress<'a>> for Element<'a, Message> {
    fn from(w: RightPress<'a>) -> Self {
        Element::new(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(ribbon: &Ribbon, id: &str) -> usize {
        ribbon
            .modules
            .iter()
            .position(|module| module.id() == id)
            .unwrap()
    }

    #[test]
    fn toggling_a_panel_hides_it_and_reset_restores_the_default() {
        let mut ribbon = Ribbon::default();
        let draw = index(&ribbon, "draw");
        ribbon.select(draw);
        assert!(ribbon.on_menu(RibbonMenuMsg::TogglePanel("draw/Modify".into())));
        assert!(!ribbon.groups_for(draw).any(|group| group.title == "Modify"));
        // Hidden panels stay in the list, unchecked.
        let modify = ribbon
            .panel_rows()
            .into_iter()
            .find(|row| row.label == "Modify")
            .unwrap();
        assert!(!modify.checked);
        assert!(!ribbon.is_default_layout());

        assert!(ribbon.on_menu(RibbonMenuMsg::Reset));
        assert!(ribbon.groups_for(draw).any(|group| group.title == "Modify"));
        assert!(ribbon.is_default_layout());
    }

    #[test]
    fn hiding_the_tab_on_show_moves_to_another_and_the_last_tab_stays() {
        let mut ribbon = Ribbon::default();
        let draw = index(&ribbon, "draw");
        ribbon.select(draw);
        ribbon.on_menu(RibbonMenuMsg::ToggleTab("draw"));
        assert_ne!(ribbon.active, draw);
        assert!(ribbon.tab_hidden(draw));

        while let Some(row) = ribbon
            .tab_rows()
            .into_iter()
            .find(|row| row.checked && row.enabled)
        {
            ribbon.on_menu(row.action);
        }
        let rows = ribbon.tab_rows();
        let shown: Vec<_> = rows.iter().filter(|row| row.checked).collect();
        assert_eq!(shown.len(), 1);
        assert!(!shown[0].enabled);
        // The contextual tabs are not listed.
        assert!(rows.len() < ribbon.modules.len());
    }

    #[test]
    fn panel_titles_toggle_and_the_menu_closes_on_a_pick() {
        let mut ribbon = Ribbon::default();
        ribbon.on_menu(RibbonMenuMsg::Open(
            Point::new(10.0, 10.0),
            MenuTarget::Ribbon,
        ));
        assert!(ribbon.menu_open());
        ribbon.on_menu(RibbonMenuMsg::Hover(Some(Submenu::Tabs)));
        assert_eq!(ribbon.menu.as_ref().unwrap().open, Some(Submenu::Tabs));
        assert!(ribbon.on_menu(RibbonMenuMsg::TogglePanelTitles));
        assert!(!ribbon.menu_open());
        assert!(!ribbon.layout.show_panel_titles);
        assert!(!ribbon.is_default_layout());
    }

    #[test]
    fn the_menu_stays_inside_the_window() {
        assert_eq!(
            place(Point::new(10.0, 20.0), (200.0, 100.0), (800.0, 600.0)),
            (10.0, 20.0)
        );
        assert_eq!(
            place(Point::new(700.0, 550.0), (200.0, 100.0), (800.0, 600.0)),
            (600.0, 500.0)
        );
    }

    #[test]
    fn a_right_clicked_tool_goes_on_the_toolbar_and_comes_off() {
        let mut ribbon = Ribbon::default();
        let hatch = QatItem::Command("HATCH".into());
        ribbon.on_menu(RibbonMenuMsg::Open(
            Point::new(10.0, 10.0),
            MenuTarget::Tool(hatch.clone()),
        ));
        assert!(ribbon.on_menu(RibbonMenuMsg::QatToggle(hatch.clone())));
        assert!(!ribbon.menu_open());
        assert!(ribbon.layout.qat_has(&hatch));
        assert!(ribbon.on_menu(RibbonMenuMsg::QatToggle(hatch.clone())));
        assert!(!ribbon.layout.qat_has(&hatch));
    }

    impl Ribbon {
        /// The labels of the open menu's rows.
        fn qat_menu_labels(&self) -> Vec<String> {
            let menu = self.menu.as_ref().expect("open menu");
            self.menu_rows(menu, false, false)
                .into_iter()
                .flatten()
                .map(|(row, _)| row.label)
                .collect()
        }
    }

    #[test]
    fn a_separator_is_offered_only_between_two_items_and_never_doubled() {
        let mut ribbon = Ribbon::default();
        let last = ribbon.layout.qat.len() - 1;
        let labels = |ribbon: &Ribbon, index: usize| -> Vec<String> {
            let mut ribbon_at = Ribbon {
                layout: ribbon.layout.clone(),
                ..Ribbon::default()
            };
            ribbon_at.on_menu(RibbonMenuMsg::Open(Point::ORIGIN, MenuTarget::Qat(index)));
            ribbon_at.qat_menu_labels()
        };
        let add = crate::tr!("ribbon-menu", "add-separator");
        assert!(labels(&ribbon, 0).contains(&add));
        assert!(!labels(&ribbon, last).contains(&add));
        // On a separator itself it offers none.
        ribbon.on_menu(RibbonMenuMsg::QatSeparatorAfter(0));
        assert_eq!(ribbon.layout.qat[1], QatItem::Separator);
        assert!(!labels(&ribbon, 1).contains(&add));

        // Even sent anyway, the toolbar never ends on or doubles one.
        let len = ribbon.layout.qat.len();
        ribbon.on_menu(RibbonMenuMsg::QatSeparatorAfter(len - 1));
        assert_ne!(ribbon.layout.qat.last(), Some(&QatItem::Separator));
        ribbon.on_menu(RibbonMenuMsg::QatSeparatorAfter(1));
        assert!(ribbon
            .layout
            .qat
            .windows(2)
            .all(|pair| pair != [QatItem::Separator, QatItem::Separator]));
    }

    #[test]
    fn a_separator_goes_after_an_item_and_comes_off_again() {
        let mut ribbon = Ribbon::default();
        // After Save (the third item).
        assert!(ribbon.on_menu(RibbonMenuMsg::QatSeparatorAfter(2)));
        assert_eq!(ribbon.layout.qat[3], QatItem::Separator);
        let len = ribbon.layout.qat.len();
        ribbon.on_menu(RibbonMenuMsg::QatSeparatorAfter(2));
        assert_eq!(ribbon.layout.qat.len(), len - 1);
        assert_ne!(ribbon.layout.qat[3], QatItem::Separator);

        // Removing an item by its place.
        let first = ribbon.layout.qat[0].clone();
        ribbon.on_menu(RibbonMenuMsg::QatRemove(0));
        assert!(!ribbon.layout.qat_has(&first));
        ribbon.on_menu(RibbonMenuMsg::QatBelow(true));
        assert!(ribbon.layout.qat_below);
    }

    #[test]
    fn a_floating_panel_is_listed_as_floating_and_unticking_docks_and_hides_it() {
        let mut ribbon = Ribbon::default();
        let draw = index(&ribbon, "draw");
        ribbon.select(draw);
        assert!(ribbon.float_panel("draw", "Modify", 400.0, 300.0));
        assert!(!ribbon.is_default_layout());
        let modify = ribbon
            .panel_rows()
            .into_iter()
            .find(|row| row.label.starts_with("Modify"))
            .unwrap();
        assert!(modify.checked);
        assert!(modify.label.ends_with("(floating)"), "{}", modify.label);
        assert!(!modify.label.contains('"'));

        assert!(ribbon.on_menu(modify.action));
        assert!(ribbon.layout.floating.is_empty());
        assert!(ribbon.layout.panel_hidden("draw", "Modify"));
    }

    #[test]
    fn reset_brings_the_floating_panels_back() {
        let mut ribbon = Ribbon::default();
        ribbon.float_panel("draw", "Draw", 400.0, 300.0);
        ribbon.float_panel("insert", "Block Definition", 500.0, 300.0);
        assert!(ribbon.on_menu(RibbonMenuMsg::Reset));
        assert!(ribbon.layout.floating.is_empty());
        assert!(ribbon.is_default_layout());
    }
}
