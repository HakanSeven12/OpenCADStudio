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

use super::widgets::{make_tip, tip_style};
use super::{extras, Ribbon};
use crate::app::Message;
use crate::ui::icons;
use crate::ui::popup::context_menu::{MENU_PAD_TOP, MENU_ROW_H, MENU_SEP_H, MENU_WIDTH};

/// Size of the check mark at the end of a checked row, as in the viewport
/// menu.
const CHECK_SIZE: f32 = 10.0;
const CARET_SIZE: f32 = 9.0;
/// How far a list overlaps the menu it opens from.
const SUBMENU_OVERLAP: f32 = 2.0;

/// The lists the menu opens beside itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submenu {
    Tabs,
    Panels,
}

#[derive(Clone, Debug)]
pub enum RibbonMenuMsg {
    /// Open the menu at this window point.
    Open(Point),
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
    /// Run RIBBONCLOSE (handled by the app).
    CloseRibbon,
}

#[derive(Clone, Debug)]
pub(super) struct MenuState {
    at: Point,
    open: Option<Submenu>,
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
            RibbonMenuMsg::Open(at) => {
                self.close_dropdown();
                self.menu = Some(MenuState { at, open: None });
                false
            }
            RibbonMenuMsg::Close | RibbonMenuMsg::ShowMenuBar(_) | RibbonMenuMsg::CloseRibbon => {
                self.menu = None;
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
                Row {
                    label: crate::t!(group.title).into_owned(),
                    checked,
                    // The last panel shown stays.
                    enabled: !(checked && shown == 1),
                    reason: None,
                    action: RibbonMenuMsg::TogglePanel(extras::panel_key(id, group.title)),
                }
            })
            .collect()
    }

    /// The menu and its open list over the whole window, with a backdrop
    /// that closes it on any click outside.
    pub fn menu_overlay(
        &self,
        win: (f32, f32),
        show_menu_bar: bool,
    ) -> Option<Element<'_, Message>> {
        let menu = self.menu.as_ref()?;
        let is_default = self.is_default_layout();
        // The rows; `None` is a separator.
        let rows: Vec<Option<(Row, Option<Submenu>)>> = vec![
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
                    label: crate::tr!("ribbon-menu", "close-ribbon"),
                    checked: false,
                    enabled: true,
                    reason: None,
                    action: RibbonMenuMsg::CloseRibbon,
                },
                None,
            )),
        ];

        let height = list_height(rows.iter().map(Option::is_some));
        let (left, top) = place(menu.at, (MENU_WIDTH, height), win);

        // Where the open list's row sits, for placing the list beside it.
        let mut open_row_y = None;
        let mut y = MENU_PAD_TOP;
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
            placed(panel(items), left, top),
        ];

        if let (Some(list), Some(row_y)) = (menu.open, open_row_y) {
            let rows = match list {
                Submenu::Tabs => self.tab_rows(),
                Submenu::Panels => self.panel_rows(),
            };
            let width = list_width(&rows);
            let height = list_height(rows.iter().map(|_| true));
            // Beside the menu on the right, or on the left when the right
            // has no room; its first row level with the row that opened it.
            let right = left + MENU_WIDTH - SUBMENU_OVERLAP;
            let x = if right + width <= win.0 {
                right
            } else {
                (left - width + SUBMENU_OVERLAP).max(0.0)
            };
            let y = (top + row_y - MENU_PAD_TOP).min((win.1 - height).max(0.0));
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
    2.0 * MENU_PAD_TOP
        + rows
            .map(|item| if item { MENU_ROW_H } else { MENU_SEP_H })
            .sum::<f32>()
}

/// A list is as wide as the menu, or wider for long labels.
fn list_width(rows: &[Row]) -> f32 {
    let longest = rows
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(0) as f32;
    MENU_WIDTH.max(longest * 7.0 + 48.0)
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

fn panel(items: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    panel_of_width(items, MENU_WIDTH)
}

/// The menu's frame, as the viewport menu draws it.
fn panel_of_width(items: Vec<Element<'_, Message>>, width: f32) -> Element<'_, Message> {
    container(column(items).width(Fill))
        .style(container::bordered_box)
        .padding([MENU_PAD_TOP as u16, 0])
        .width(Length::Fixed(width))
        .into()
}

/// A 1 px line across the menu, inset at the sides.
fn separator<'a>() -> Element<'a, Message> {
    let line =
        container(Space::new().width(Fill).height(1)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        });
    container(line)
        .padding(Padding {
            top: (MENU_SEP_H - 1.0) / 2.0,
            bottom: (MENU_SEP_H - 1.0) / 2.0,
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
    left: 28.0,
};

fn item_row<'a>(row: Row) -> Element<'a, Message> {
    let enabled = row.enabled;
    let mut content = row![text(row.label).size(12)].align_y(iced::Center);
    if row.checked {
        content = content.push(Space::new().width(Fill)).push(if enabled {
            icons::themed(icons::CHECK, CHECK_SIZE)
        } else {
            icons::themed_disabled(icons::CHECK, CHECK_SIZE)
        });
    }
    let mut item = button(content)
        .padding(ROW_PADDING)
        .width(Fill)
        .height(Length::Fixed(MENU_ROW_H))
        .style(row_style);
    if enabled {
        item = item.on_press(Message::RibbonMenu(row.action));
    }
    match row.reason {
        Some(reason) => tooltip(item, make_tip(reason), tooltip::Position::Right)
            .delay(Duration::from_millis(400))
            .style(tip_style)
            .into(),
        None => item.into(),
    }
}

/// A row that opens `list` beside the menu when hovered or clicked.
fn submenu_row<'a>(label: String, open: bool, list: Submenu) -> Element<'a, Message> {
    let content = row![
        text(label).size(12),
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
    let palette = theme.palette();
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Background::Color(palette.background.weak.color)),
            palette.background.weak.text,
        ),
        button::Status::Disabled => (None, palette.background.base.text.scale_alpha(0.42)),
        button::Status::Active => (None, palette.background.base.text),
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

/// Opens the ribbon menu where `child` is right-clicked. It reports the
/// press only, so moving the pointer over the ribbon sends nothing.
pub(super) struct RightPress<'a> {
    child: Element<'a, Message>,
}

impl<'a> RightPress<'a> {
    pub(super) fn new(child: impl Into<Element<'a, Message>>) -> Self {
        Self {
            child: child.into(),
        }
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
                    shell.publish(Message::RibbonMenu(RibbonMenuMsg::Open(at)));
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
        ribbon.on_menu(RibbonMenuMsg::Open(Point::new(10.0, 10.0)));
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
}
