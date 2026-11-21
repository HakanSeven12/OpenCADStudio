//! Dock drawing: the edges (icon strip and shown group), floating pallets,
//! the + menu and the drag preview.

use crate::app::tab_document::DocumentTab;
use crate::app::{Message, OpenCADStudio};
use crate::t;
use crate::ui::dock::{DOCK_DIVIDER_W, DOCK_FLOAT_BAR_W, RAIL_ICON};
use iced::widget::{button, canvas, column, container, mouse_area, row, stack, text, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

/// One row of the edge (+) pallet menu: a built-in panel, or a panel a
/// plugin registered, identified by its numeric dock id and carrying the
/// title the plugin authored (a `PanelId::Plugin(_)` literal is not a title).
pub(in crate::app) enum MenuPanel {
    Builtin(crate::ui::dock::PanelId),
    Plugin(u32, String),
}

impl MenuPanel {
    /// The dockable panel id the menu row toggles.
    fn id(&self) -> crate::ui::dock::PanelId {
        match self {
            MenuPanel::Builtin(id) => *id,
            MenuPanel::Plugin(num, _) => crate::ui::dock::PanelId::Plugin(*num),
        }
    }

    /// Resolved menu text: built-ins localized, plugin titles as authored.
    pub(super) fn title(&self) -> String {
        match self {
            MenuPanel::Builtin(id) => t!(id.title()).into_owned(),
            MenuPanel::Plugin(_, title) => title.clone(),
        }
    }

    /// The generic plugin icon; built-ins keep their own.
    fn icon(&self) -> &'static [u8] {
        self.id().icon()
    }
}

impl OpenCADStudio {
    /// Everything the dock draws over the workspace (the row of edges and
    /// viewport): floating pallets, an open + menu, the drag preview, and
    /// the pointer capture that feeds drags.
    pub(in crate::app) fn dock_overlay<'a>(
        &'a self,
        workspace: Element<'a, Message>,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        use crate::app::config::DockSide;
        // Floating panels sit over the workspace, back to front.
        let mut layers: Vec<Element<'_, Message>> = vec![workspace];
        // Auto-hiding edges draw over the drawing, below floating panels.
        for side in [DockSide::Left, DockSide::Right] {
            if !self.dock.edge_auto_hide(side) {
                continue;
            }
            if let Some(edge) = self.dock_edge(side, tab) {
                // Opaque to the pointer, as a floating panel is: clicks on a
                // revealed group's empty areas don't reach the drawing.
                let edge = mouse_area(edge).interaction(iced::mouse::Interaction::Idle);
                layers.push(
                    container(edge)
                        .width(Fill)
                        .height(Fill)
                        .align_x(match side {
                            DockSide::Left => iced::alignment::Horizontal::Left,
                            DockSide::Right => iced::alignment::Horizontal::Right,
                        })
                        .into(),
                );
            }
        }
        // A docked edge's grab band, over the drawing beside its group.
        for side in [DockSide::Left, DockSide::Right] {
            if self.dock.edge_auto_hide(side) || !self.dock_edge_expanded(side) {
                continue;
            }
            let Some(gi) = self.dock_shown_group(side) else {
                continue;
            };
            let Some(&id) = self.dock_group_visible(side, gi).first() else {
                continue;
            };
            let inset = Space::new()
                .width(crate::ui::dock::DOCK_STRIP_W + self.dock_group_width(side, gi))
                .height(Fill);
            let grab = dock_divider(id, side);
            layers.push(
                container(match side {
                    DockSide::Left => row![inset, grab],
                    DockSide::Right => row![grab, inset],
                })
                .width(Fill)
                .height(Fill)
                .align_x(match side {
                    DockSide::Left => iced::alignment::Horizontal::Left,
                    DockSide::Right => iced::alignment::Horizontal::Right,
                })
                .into(),
            );
        }
        let movement = self.dock_drag.and_then(|d| d.movement());
        for f in &self.dock.floating {
            if !self.dock_panel_visible(f.id) {
                continue;
            }
            // A floating pallet being dragged moves itself: it draws where it
            // would land, its title bar on the side it would land on. Over a
            // dock zone it hides and the preview in the column shows instead.
            // Its layout only changes on release.
            let f = match movement {
                Some((panel, _, Some(target))) if panel == f.id => match target {
                    crate::ui::dock::DropTarget::Float { x, y, bar } => {
                        crate::ui::dock::FloatPanel { x, y, bar, ..*f }
                    }
                    _ => continue,
                },
                _ => *f,
            };
            layers.push(self.floating_panel(f, tab));
        }
        // An open pallet menu hangs beside its edge's + button, over a
        // catcher that closes it on any click elsewhere.
        if let Some(side) = self.dock_edge_menu {
            if !self.dock_visible_groups(side).is_empty() {
                // Checked = open and docked on this edge.
                let checked = self.pallet_menu_entries(side, tab);
                let (_, plus_top) = self.dock_strip_layout(side);
                let items = dock_edge_menu_items(side, &checked, self.dock.strip_names);
                let menu_w = crate::ui::kit::menu::width(&items);
                let strip = crate::ui::dock::DOCK_STRIP_W;
                let gap = crate::ui::style::kit_metrics::TOOL_GAP;
                let x = match side {
                    crate::app::config::DockSide::Left => strip + gap,
                    crate::app::config::DockSide::Right => {
                        self.dock_workspace_size().0 - strip - menu_w - gap
                    }
                };
                layers.push(
                    mouse_area(Space::new().width(Fill).height(Fill))
                        .on_press(Message::Dock(crate::ui::dock::DockMsg::EdgeMenu(None)))
                        .on_right_press(Message::Dock(crate::ui::dock::DockMsg::EdgeMenu(None)))
                        .into(),
                );
                let menu_h = crate::ui::kit::menu::height(&items);
                let y = crate::ui::dock::strip_menu_top(
                    self.dock_edge_menu_y.unwrap_or(plus_top),
                    menu_h,
                    self.dock_workspace_size().1,
                );
                layers.push(place_at(crate::ui::kit::menu::view(items), x.max(0.0), y));
            }
        }
        // An open floating title-bar menu, over its own click catcher.
        if let Some(menu) = self.dock_float_menu_overlay() {
            let close = Message::Dock(crate::ui::dock::DockMsg::FloatMenu(None));
            layers.push(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(close.clone())
                    .on_right_press(close)
                    .into(),
            );
            layers.push(menu);
        }
        // While a panel is dragged, preview where it lands, then cover
        // everything with a layer that takes the pointer: the moving window
        // must not react to it (hover highlights, its title bar's buttons,
        // their cursors) and the cursor stays a closed hand.
        if let Some((id, _, Some(target))) = movement {
            layers.push(self.dock_drop_preview(id, target));
            layers.push(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .interaction(iced::mouse::Interaction::Grabbing)
                    .into(),
            );
        }
        // Always a stack (and, below, always a mouse area), even with nothing
        // on top: iced keeps a widget's state only while the tree keeps its
        // shape, and a title bar's double-click needs its first click to
        // survive the drag that pressing the bar starts.
        let workspace: Element<'_, Message> = iced::widget::Stack::with_children(layers)
            .width(Fill)
            .height(Fill)
            .into();
        use crate::ui::dock::DockDrag;
        let capturing =
            self.dock_drag.is_some() || self.xref_col_drag.is_some() || self.xref_split_drag;
        let mut capture = mouse_area(workspace);
        if capturing {
            capture = capture
                .on_move(move |p| Message::Dock(crate::ui::dock::DockMsg::DragMove(p)))
                .on_release(Message::Dock(crate::ui::dock::DockMsg::DragRelease))
                .interaction(match self.dock_drag {
                    Some(DockDrag::FloatSize { edge, .. }) => edge.interaction(),
                    Some(DockDrag::Split { .. }) => iced::mouse::Interaction::ResizingVertically,
                    Some(DockDrag::Move { .. } | DockDrag::QuickProperties) => {
                        iced::mouse::Interaction::Grabbing
                    }
                    // Widths, the Layer Manager column and the xref drags.
                    _ => iced::mouse::Interaction::ResizingHorizontally,
                });
        }
        capture.into()
    }

    /// The edge (+) pallet menu rows for one edge: built-ins first in their
    /// historical order, then every registered plugin panel in title order.
    /// `checked` means open and docked on this edge — the same rule the
    /// built-ins show, so a plugin panel toggles like any pallet.
    pub(in crate::app) fn pallet_menu_entries(
        &self,
        side: crate::app::config::DockSide,
        tab: &DocumentTab,
    ) -> Vec<(MenuPanel, bool)> {
        use crate::ui::dock::PanelId;
        let edit_session = tab.active_block_edit_session().is_some();
        let mut entries: Vec<(MenuPanel, bool)> = PanelId::ALL
            .iter()
            .filter(|p| {
                !matches!(
                    **p,
                    PanelId::AuthoringPalettes | PanelId::ParametersManager
                ) || edit_session
            })
            .map(|p| {
                let here = self.dock.location(*p).map(|(s, _)| s) == Some(side);
                (MenuPanel::Builtin(*p), here && self.dock_panel_visible(*p))
            })
            .collect();
        // Plugin panels: title-ordered, keyed by the numeric dock id the
        // menu's toggle needs. `plugin_panel_by_num` is num -> id; one
        // reverse pass hands each row its number.
        let num_by_id: std::collections::HashMap<&str, u32> = self
            .plugin_panel_by_num
            .iter()
            .map(|(num, id)| (id.as_str(), *num))
            .collect();
        let mut plugins: Vec<(u32, String, bool)> = self
            .plugin_panels
            .iter()
            .filter_map(|(id, state)| {
                let num = *num_by_id.get(id.as_str())?;
                let here = self
                    .dock
                    .location(PanelId::Plugin(num))
                    .map(|(s, _)| s)
                    == Some(side);
                Some((
                    num,
                    state.def.title.clone(),
                    here && self.dock_panel_visible(PanelId::Plugin(num)),
                ))
            })
            .collect();
        plugins.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        // Abuse ceiling: one buggy plugin cannot widen the palette
        // indefinitely. The registration path reports the overflow.
        plugins.truncate(super::PLUGIN_PALLET_MENU_MAX_ROWS);
        entries.extend(
            plugins
                .into_iter()
                .map(|(num, title, visible)| (MenuPanel::Plugin(num, title), visible)),
        );
        entries
    }

    /// Build one edge: the vertical icon strip at the very edge plus the
    /// edge's shown group beside it. The strip lists every group as a grip
    /// bar over its pallets' icons, with a + button below; clicking an icon
    /// shows its group. The shown group's pallets stack top to bottom, each
    /// at its weighted share of the height, with a draggable splitter between
    /// neighbours. A group whose pallets all auto-hide shows only while one
    /// of its icons (or the group itself) is hovered; leaving the edge hides
    /// it again. `None` when nothing on the edge is open.
    ///
    /// An auto-hiding edge only holds room for its icon strip here: the edge
    /// itself draws over the workspace (see `dock_overlay`), so revealing a
    /// group covers the drawing instead of squeezing it.
    pub(in crate::app) fn build_edge_stack<'a>(
        &'a self,
        side: crate::app::config::DockSide,
        tab: &'a DocumentTab,
    ) -> Option<Element<'a, Message>> {
        if self.dock.edge_auto_hide(side) {
            self.dock_shown_group(side)?;
            return Some(
                Space::new()
                    .width(crate::ui::dock::DOCK_STRIP_W)
                    .height(Fill)
                    .into(),
            );
        }
        self.dock_edge(side, tab)
    }

    /// An edge as drawn: its icon strip and, when expanded, its shown group.
    fn dock_edge<'a>(
        &'a self,
        side: crate::app::config::DockSide,
        tab: &'a DocumentTab,
    ) -> Option<Element<'a, Message>> {
        let gi = self.dock_shown_group(side)?;
        let strip = self.dock_icon_strip(side);
        let column_el = self.dock_edge_expanded(side).then(|| {
            let width = self.dock_group_width(side, gi);
            let group = &self.dock.groups(side)[gi];
            let mut layers: Vec<Element<'_, Message>> = Vec::new();
            let mut prev: Option<usize> = None;
            for (pos, id) in group.panels.iter().enumerate() {
                if !self.dock_panel_visible(*id) {
                    continue;
                }
                if let Some(upper) = prev {
                    let active = matches!(
                        self.dock_drag,
                        Some(crate::ui::dock::DockDrag::Split { side: s, group, upper: u, .. })
                            if s == side && group == gi && u == upper
                    );
                    layers.push(dock_splitter(side, gi, upper, pos, width, active));
                }
                layers.push(
                    container(self.expanded_panel(*id, side, width, tab))
                        .height(Length::FillPortion(crate::ui::dock::portion(
                            group.weights[pos],
                        )))
                        .clip(true)
                        .into(),
                );
                prev = Some(pos);
            }
            column(layers).height(Fill).into()
        });

        let mut children: Vec<Element<'_, Message>> = Vec::new();
        match side {
            crate::app::config::DockSide::Left => {
                children.push(strip);
                children.extend(column_el);
            }
            crate::app::config::DockSide::Right => {
                children.extend(column_el);
                children.push(strip);
            }
        }
        // Not a `mouse_area`: the pointer on one of the pallet's own
        // dropdowns must not read as leaving the edge.
        Some(crate::ui::hover_zone::hover_zone(
            row(children).height(Fill),
            Message::Dock(crate::ui::dock::DockMsg::HoverStay),
            Message::Dock(crate::ui::dock::DockMsg::HoverExit),
        ))
    }

    /// An edge's vertical icon strip, on the window edge: a 30 px light part
    /// and a 1 px `edge` line on the palette side. Per group its open
    /// pallets' cells (icon, icon and name, or name, per `StripNames`),
    /// groups parted by a 1 px line, then a + cell opening the pallet menu.
    /// The group on screen takes the palette's body colour, running over
    /// the edge line into the palette, with `edge` lines above and below it.
    /// Hover lights a whole cell, square, up to the neighbouring group. The
    /// band along a group's window-side edge is its grip: invisible until
    /// hovered (or a drop target), it drags the whole group; an icon shows
    /// its group on click and drags just that pallet. The right edge
    /// mirrors the left; names read upward on both. Heights follow
    /// `dock::strip_layout`, which drop targeting uses too.
    fn dock_icon_strip(&self, side: crate::app::config::DockSide) -> Element<'_, Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{
            DockMsg, DropTarget, DOCK_STRIP_W, STRIP_GRIP_W, STRIP_PAD, STRIP_PLUS_GAP,
            STRIP_PLUS_H,
        };
        use crate::ui::style::{chrome, kit_metrics as km};
        let (layout, plus_top) = self.dock_strip_layout(side);
        let names = self.dock.strip_names;
        // Only a group actually on screen is marked as open: on an
        // auto-hiding edge nothing is, until a group is revealed.
        let shown = self
            .dock_shown_group(side)
            .filter(|_| self.dock_edge_expanded(side));
        let movement = self.dock_drag.and_then(|d| d.movement());
        let move_target = movement.and_then(|(_, _, target)| target);
        let dragging = move_target.is_some();
        // While a pallet or group is moved the cursor stays a closed hand,
        // over the tabs too.
        let moving = movement.is_some();
        let target_group = match move_target {
            Some(DropTarget::Join { side: s, group, .. }) if s == side => Some(group),
            _ => None,
        };
        // The light part and the edge line, in the order they sit from the
        // window edge inward.
        let body_and_edge = |body: Element<'static, Message>,
                             edge: Element<'static, Message>|
         -> Element<'static, Message> {
            match side {
                DockSide::Left => row![body, edge].into(),
                DockSide::Right => row![edge, body].into(),
            }
        };
        let window_side = match side {
            DockSide::Left => iced::alignment::Horizontal::Left,
            DockSide::Right => iced::alignment::Horizontal::Right,
        };
        let groups = self.dock_visible_groups(side);
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        for (n, &gi) in groups.iter().enumerate() {
            let is_open = shown == Some(gi);
            let lit =
                target_group == Some(gi) || (!dragging && self.dock_grip_lit == Some((side, gi)));
            let group_dragged =
                dragging && movement.is_some_and(|(_, group, _)| group == Some((side, gi)));
            let panels = self.dock_group_visible(side, gi);
            let Some(placed) = layout.iter().find(|g| g.group == gi) else {
                continue;
            };
            let row_h = placed.bottom() - placed.top;
            let count = panels.len();

            let mut cells: Vec<Element<'_, Message>> = Vec::new();
            for (k, (id, &cell_h)) in panels.into_iter().zip(&placed.cells).enumerate() {
                let faded = group_dragged
                    || (dragging
                        && movement
                            .is_some_and(|(panel, group, _)| group.is_none() && panel == id));
                let art = id.icon();
                let tint: fn(&Theme) -> Color = if is_open {
                    chrome::strong
                } else {
                    chrome::tab_text
                };
                let icon = || -> Element<'_, Message> {
                    if faded {
                        crate::ui::icons::themed_disabled(art, RAIL_ICON)
                    } else {
                        crate::ui::icons::tinted(art, RAIL_ICON, tint)
                    }
                };
                let name = || -> Element<'_, Message> {
                    iced::widget::canvas(crate::ui::dock::VerticalLabel {
                        text: t!(id.title()).to_uppercase(),
                        centered: true,
                    })
                    .width(Fill)
                    .height(Fill)
                    .into()
                };
                // The name reads upward, so it sits above the icon.
                let content: Element<'_, Message> = match (names.icons(), names.names()) {
                    (true, false) => container(icon()).center(Fill).into(),
                    (true, true) => column![
                        Space::new().height(km::STRIP_NAME_PAD),
                        name(),
                        Space::new().height(km::STRIP_NAME_GAP),
                        container(icon()).center_x(Fill).height(RAIL_ICON),
                        Space::new().height(km::STRIP_NAME_PAD),
                    ]
                    .into(),
                    _ => column![
                        Space::new().height(km::STRIP_NAME_PAD),
                        name(),
                        Space::new().height(km::STRIP_NAME_PAD),
                    ]
                    .into(),
                };
                // The group's padding belongs to its first and last cells,
                // so hover reaches the neighbouring group.
                let pad_top = if k == 0 { STRIP_PAD } else { 0.0 };
                let pad_bottom = if k + 1 == count { STRIP_PAD } else { 0.0 };
                let hovered = !dragging && self.dock_icon_hover == Some(id);
                let cell = container(container(content).width(Fill).height(Length::Fixed(cell_h)))
                    .padding(iced::Padding {
                        top: pad_top,
                        bottom: pad_bottom,
                        ..iced::Padding::ZERO
                    })
                    .width(Length::Fixed(km::STRIP_BODY_W))
                    .height(Length::Fixed(cell_h + pad_top + pad_bottom))
                    .style(move |theme: &Theme| container::Style {
                        background: hovered.then(|| Background::Color(chrome::hover(theme))),
                        ..Default::default()
                    });
                let cell = mouse_area(cell)
                    .on_press(Message::Dock(DockMsg::IconPress(id)))
                    .on_right_press(Message::Dock(DockMsg::EdgeMenuAtPointer(side)))
                    .on_enter(Message::Dock(DockMsg::Hover(id)))
                    .on_exit(Message::Dock(DockMsg::HoverEnd(id)))
                    .interaction(if moving {
                        iced::mouse::Interaction::Grabbing
                    } else {
                        iced::mouse::Interaction::Pointer
                    });
                cells.push(cell.into());
            }

            // The group grip: a band on the window side, invisible until
            // hovered or a drop target.
            let band = mouse_area(
                container(Space::new())
                    .width(Length::Fixed(STRIP_GRIP_W))
                    .height(Length::Fixed(row_h))
                    .style(move |theme: &Theme| container::Style {
                        background: lit.then(|| {
                            Background::Color(chrome::accent(theme).scale_alpha(GRIP_LIT))
                        }),
                        ..Default::default()
                    }),
            )
            .on_enter(Message::Dock(DockMsg::GripHover(side, gi)))
            .on_exit(Message::Dock(DockMsg::GripHoverEnd(side, gi)));
            // The band takes a press, and shows the open hand, only once it
            // has lit; until then a press goes to the tab under it.
            let grip_ready = !moving && self.dock_grip_lit == Some((side, gi));
            let band = if moving {
                band.interaction(iced::mouse::Interaction::Grabbing)
            } else if grip_ready {
                band.on_press(Message::Dock(DockMsg::GroupGrab(side, gi)))
                    .interaction(iced::mouse::Interaction::Grab)
            } else {
                band
            };
            let band = container(band)
                .width(Length::Fixed(km::STRIP_BODY_W))
                .height(Length::Fixed(row_h))
                .align_x(window_side);
            let body: Element<'_, Message> = stack![
                column(cells)
                    .width(Length::Fixed(DOCK_STRIP_W - km::line()))
                    .height(Length::Fixed(row_h)),
                band
            ]
            .into();
            // The open group runs over the edge line into the palette.
            let edge = Space::new().width(km::line()).height(Length::Fixed(row_h));
            let group: Element<'_, Message> = match side {
                DockSide::Left => row![body, edge].into(),
                DockSide::Right => row![edge, body].into(),
            };
            items.push(
                container(group)
                    .width(Length::Fixed(DOCK_STRIP_W))
                    .height(Length::Fixed(row_h))
                    .style(move |theme: &Theme| container::Style {
                        background: is_open.then(|| Background::Color(chrome::surface(theme))),
                        ..Default::default()
                    })
                    .into(),
            );
            // The line under the group: the dark edge around the open
            // group, the lighter divider between others.
            let next_open = groups.get(n + 1).is_some_and(|g| shown == Some(*g));
            let around_open = is_open || next_open;
            items.push(body_and_edge(
                container(
                    Space::new()
                        .width(DOCK_STRIP_W - km::line())
                        .height(km::line()),
                )
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(if around_open {
                        chrome::edge(theme)
                    } else {
                        chrome::strip_divider(theme)
                    })),
                    ..Default::default()
                })
                .into(),
                Space::new().width(km::line()).height(km::line()).into(),
            ));
        }
        let plus = button(
            container(crate::ui::icons::tinted(
                crate::ui::icons::PLUS,
                km::STRIP_PLUS_ICON,
                chrome::tab_text,
            ))
            .center(Fill),
        )
        .on_press(Message::Dock(DockMsg::EdgeMenu(Some(side))))
        .style(|theme: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| Background::Color(chrome::hover(theme))),
            ..Default::default()
        })
        .width(Length::Fixed(DOCK_STRIP_W - km::line()))
        .height(Length::Fixed(STRIP_PLUS_H))
        .padding(0);
        // The tabs clip above the +, so overflowing ones never push it away.
        let tabs = container(column(items))
            .height(Length::Fixed(plus_top - STRIP_PLUS_GAP))
            .clip(true);
        let strip = column![
            tabs,
            Space::new().height(STRIP_PLUS_GAP),
            body_and_edge(plus.into(), Space::new().width(km::line()).into()),
        ];
        let strip = mouse_area(strip).on_move(|p| Message::Dock(DockMsg::StripPointer(p)));
        // Underneath: the strip colour and the edge line along the palette
        // side, full height (the open group covers it).
        let base = body_and_edge(
            container(Space::new())
                .width(Length::Fixed(DOCK_STRIP_W - km::line()))
                .height(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::strip(theme))),
                    ..Default::default()
                })
                .into(),
            container(Space::new())
                .width(Length::Fixed(km::line()))
                .height(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::edge(theme))),
                    ..Default::default()
                })
                .into(),
        );
        container(stack![base, strip])
            .width(Length::Fixed(DOCK_STRIP_W))
            .height(Fill)
            .into()
    }

    /// The content of panel `id` at `width`. A `floating` panel leaves its
    /// title to the floating frame's vertical title bar.
    pub(in crate::app) fn panel_body<'a>(
        &'a self,
        id: crate::ui::dock::PanelId,
        width: f32,
        floating: bool,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        let chrome = crate::ui::dock::Chrome {
            auto_collapse: self.dock.auto_hides(id),
            floating,
            title_hovered: self.dock_title_hover == Some(id),
        };
        match id {
            crate::ui::dock::PanelId::Properties => tab.properties.view(width, chrome),
            crate::ui::dock::PanelId::GraphicAttributes => {
                crate::ui::window::graphic_attributes::view(
                    &self.graphic_attributes,
                    &tab.scene,
                    &tab.properties,
                    width,
                    chrome,
                    self.dock
                        .location(id)
                        .map_or(crate::app::config::DockSide::Right, |(side, _)| side),
                    matches!(
                        self.color_picker.as_ref().map(|picker| &picker.target),
                        Some(crate::app::ColorPickTarget::Gradient(_))
                    ),
                )
            }
            crate::ui::dock::PanelId::AuthoringPalettes => {
                crate::ui::window::authoring_palettes::view(
                    self.authoring_palette_tab,
                    width,
                    chrome,
                    self.visibility_placement_error(),
                    self.point_parameter_placement_error(),
                    self.parameter_placement_error("FLIP"),
                    if self.block_editor_preferences.action_bars {
                        Some("Use the Lookup parameter action bar, or set BACTIONBARMODE to 0.")
                    } else {
                        self.lookup_table_error()
                    },
                    self.properties_table_error(),
                    self.action_error(Some(crate::scene::block_actions::Kind::Move)),
                    self.action_error(Some(crate::scene::block_actions::Kind::Scale)),
                    self.action_error(Some(crate::scene::block_actions::Kind::Rotate)),
                    self.action_error(Some(crate::scene::block_actions::Kind::Flip)),
                    self.action_error(Some(crate::scene::block_actions::Kind::Stretch)),
                    self.action_error(Some(crate::scene::block_actions::Kind::PolarStretch)),
                    self.action_error(Some(crate::scene::block_actions::Kind::Array)),
                    self.parameter_placement_error("LINEAR"),
                )
            }
            crate::ui::dock::PanelId::ParametersManager => {
                let (definition, rows) = match tab.active_block_edit_session() {
                    Some(session) => (
                        session.br_handle,
                        crate::scene::block_parameters_manager::rows(
                            &tab.scene.document,
                            session.br_handle,
                        ),
                    ),
                    None => (document::Handle::NULL, Vec::new()),
                };
                crate::ui::window::parameters_manager::view(
                    &rows,
                    &tab.parameters_panel,
                    definition,
                    width,
                    chrome,
                )
            }
            crate::ui::dock::PanelId::BlockPalette => {
                crate::ui::window::block_palette::view(&self.block_palette, width, chrome)
            }
            crate::ui::dock::PanelId::ToolPalettes => crate::ui::window::tool_palettes::view(
                &self.tool_palettes,
                &tab.scene.document,
                &tab.scene.selected,
                width,
                chrome,
            ),
            crate::ui::dock::PanelId::ExternalReferences => {
                self.xref_manager
                    .view(width, chrome, tab.xref_missing, &tab.scene.document)
            }
            crate::ui::dock::PanelId::Browser => crate::ui::window::browser::view(
                &tab.scene.document,
                tab.sketch_session
                    .as_ref()
                    .map(|session| session.name.as_str()),
                width,
                chrome,
            ),
            crate::ui::dock::PanelId::NodeGraph => {
                tab.graph.panel(width, chrome, self.node_graph_links)
            }
            crate::ui::dock::PanelId::Count => crate::ui::window::count_palette::view(
                &self.count_palette,
                tab.count.as_ref(),
                &tab.scene.document,
                tab.id,
                tab.scene.geometry_epoch,
                width,
                chrome,
            ),
            crate::ui::dock::PanelId::PointCloudManager => crate::ui::window::pc_manager::view(
                &self.pc_manager,
                &tab.scene.document,
                &tab.scene.selected_handles_in_order(),
                width,
                chrome,
            ),
            crate::ui::dock::PanelId::SheetSetManager => {
                crate::ui::window::sheet_set::view(&self.sheet_set, width, chrome)
            }
            crate::ui::dock::PanelId::Layers => {
                let active_vp = tab
                    .scene
                    .active_viewport
                    .filter(|_| tab.scene.current_layout != "Model");
                tab.layers.view_panel(
                    self.layer_name_col_w,
                    &self.layer_col_w,
                    width,
                    chrome,
                    active_vp,
                )
            }
            crate::ui::dock::PanelId::Laywalk => self.layer_walk_view(tab, width, chrome),
            crate::ui::dock::PanelId::QuickCalc => self.quick_calc_view(tab, width, chrome),
            crate::ui::dock::PanelId::Assistant => self.assistant_view(width, chrome),
            crate::ui::dock::PanelId::CommandHistory => {
                self.command_line
                    .history_panel(&self.history_content, width, chrome)
            }
            crate::ui::dock::PanelId::Plugin(num) => {
                if let Some(panel_id) = self.plugin_panel_by_num.get(&num) {
                    if let Some(state) = self.plugin_panels.get(panel_id) {
                        state.view(num, width, chrome, self.mount_registry.as_ref())
                    } else {
                        Space::new().into()
                    }
                } else {
                    Space::new().into()
                }
            }
        }
    }

    /// A pallet of the shown group. On an auto-hiding edge, which draws
    /// over the drawing, its grab band for sizing the group comes along
    /// (so reaching for it keeps the edge open); a docked edge's band lies
    /// over the drawing instead (see `dock_overlay`). Hovering is handled by
    /// the enclosing edge region (see `build_edge_stack`), so the body stays
    /// fully interactive without fighting the region's hover tracking.
    fn expanded_panel<'a>(
        &'a self,
        id: crate::ui::dock::PanelId,
        side: crate::app::config::DockSide,
        width: f32,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        let panel = self.panel_body(id, width, false, tab);
        if !self.dock.edge_auto_hide(side) {
            return panel;
        }
        let divider = dock_divider(id, side);
        match side {
            crate::app::config::DockSide::Left => row![panel, divider].height(Fill).into(),
            crate::app::config::DockSide::Right => row![divider, panel].height(Fill).into(),
        }
    }

    /// A floating panel at its saved place: a vertical title bar on its side
    /// and the panel beside it, resized by dragging any side or corner. With
    /// its pin on, the panel hides down to the title bar until hovered.
    fn floating_panel<'a>(
        &'a self,
        f: crate::ui::dock::FloatPanel,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        use crate::ui::dock::DockMsg;
        let FloatFrame {
            x,
            y,
            bar_left,
            hidden,
        } = self.dock_float_frame(f);
        let hovered = self.dock_title_hover == Some(f.id);
        let bar = floating_title_bar(f.id, self.dock.auto_collapse(f.id), bar_left, hovered);
        let content: Element<'_, Message> = if hidden {
            bar
        } else {
            let body = container(self.panel_body(f.id, f.w, true, tab))
                .width(Length::Fixed(f.w))
                .height(Fill);
            if bar_left {
                row![bar, body].into()
            } else {
                row![body, bar].into()
            }
        };
        // Clipped, so a panel squeezed below its content's height never
        // draws outside its frame.
        // The content sits inside the 1 px contour: iced draws a border
        // under the content, which would otherwise cover it.
        let framed = container(content)
            .height(Length::Fixed(f.h))
            .padding(crate::ui::style::kit_metrics::line())
            .clip(true)
            .style(|theme: &Theme| container::Style {
                // The ribbon's dark 1 px contour, all the way round.
                border: Border {
                    color: crate::ui::style::chrome::page(theme),
                    width: crate::ui::style::kit_metrics::line(),
                    radius: 0.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 14.0,
                },
                ..Default::default()
            });
        // Every side and corner resizes the panel; hidden down to its title
        // bar, only the top and bottom do.
        let framed = stack![framed, float_resize_handles(f.id, hidden)];
        // The idle cursor marks the panel as opaque to the pointer, so clicks
        // on its empty areas don't fall through to the drawing below.
        let panel = mouse_area(framed)
            .on_press(Message::Dock(DockMsg::FloatRaise(f.id)))
            .interaction(iced::mouse::Interaction::Idle);
        // Enter / leave as on a docked edge: its dropdowns keep it shown.
        let panel = crate::ui::hover_zone::hover_zone(
            panel,
            Message::Dock(DockMsg::Hover(f.id)),
            Message::Dock(DockMsg::HoverExit),
        );
        place_at(panel, x, y)
    }

    /// Where floating panel `f` draws.
    fn dock_float_frame(&self, f: crate::ui::dock::FloatPanel) -> FloatFrame {
        let (ww, wh) = self.dock_workspace_size();
        // The bar keeps its side; only a drag against an edge moves it.
        let bar_left = f.bar == crate::app::config::DockSide::Left;
        let hidden = self.dock.auto_collapse(f.id) && self.dock_peek != Some(f.id);
        // A hidden panel keeps its title bar where it sits when shown.
        let x = if hidden && !bar_left { f.x + f.w } else { f.x };
        let frame = FloatFrame {
            x,
            y: f.y,
            bar_left,
            hidden,
        };
        // Keep the panel on screen when the window shrank since it was placed.
        FloatFrame {
            x: x.min(ww - frame.width(f)).max(0.0),
            y: f.y.min(wh - f.h).max(0.0),
            ..frame
        }
    }

    /// The open right-click menu of a floating title bar: Allow docking,
    /// Dock left and Dock right. It opens at the pointer, flipped to stay on
    /// screen.
    fn dock_float_menu_overlay(&self) -> Option<Element<'_, Message>> {
        use crate::app::config::DockSide;
        use crate::ui::dock::DockMsg;
        let id = self.dock_float_menu?;
        let f = self.dock.float_rect(id)?;
        if !self.dock_panel_visible(id) {
            return None;
        }
        let (ww, wh) = self.dock_workspace_size();
        let frame = self.dock_float_frame(f);
        // Where the right-click happened, in the workspace.
        let at = iced::Point::new(
            frame.bar_x(f) + self.dock_float_menu_at.x,
            frame.y + self.dock_float_menu_at.y,
        );
        use crate::ui::kit::menu::{self, Item, Mark};
        #[allow(unused_mut)]
        let mut items = vec![
            Item::new(
                t!("Allow docking"),
                Message::Dock(DockMsg::FloatDockingToggle(id)),
            )
            .mark(Mark::Check(f.docking)),
            Item::Separator,
            Item::new(
                t!("Dock left"),
                Message::Dock(DockMsg::DockTo(id, DockSide::Left)),
            ),
            Item::new(
                t!("Dock right"),
                Message::Dock(DockMsg::DockTo(id, DockSide::Right)),
            ),
        ];
        // Desktop: the pallet can leave the main window, onto another monitor.
        #[cfg(not(target_arch = "wasm32"))]
        items.push(
            Item::new(
                t!("Open in its own window"),
                Message::Dock(DockMsg::Detach(id)),
            )
            .icon(crate::ui::icons::DETACH),
        );
        // At the pointer, kept inside the workspace.
        let (menu_w, menu_h) = (menu::width(&items), menu::height(&items));
        let menu_x = if at.x + menu_w <= ww {
            at.x
        } else {
            at.x - menu_w
        };
        let menu_x = menu_x.clamp(0.0, (ww - menu_w).max(0.0));
        let menu_y = at.y.min(wh - menu_h).max(0.0);
        Some(place_at(menu::view(items), menu_x, menu_y))
    }

    /// The live drop preview for dragged panel `id`: a tint over the target
    /// edge plus a panel-sized ghost where it lands — stacked into a group
    /// (sized as that group will be), as a new group of its own (full
    /// height), or as a floating window (a pallet that already floats draws
    /// itself there instead, see `dock_overlay`).
    fn dock_drop_preview(
        &self,
        id: crate::ui::dock::PanelId,
        target: crate::ui::dock::DropTarget,
    ) -> Element<'_, Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::DropTarget;
        let (ww, _) = self.dock_workspace_size();
        let strip = crate::ui::dock::DOCK_STRIP_W;
        let tint = |side: DockSide, w: f32| -> Element<'_, Message> {
            let band = container(Space::new())
                .width(Length::Fixed(w + strip))
                .height(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        crate::ui::style::chrome::lit(theme).scale_alpha(0.35),
                    )),
                    ..Default::default()
                });
            container(band)
                .width(Fill)
                .height(Fill)
                .align_x(match side {
                    DockSide::Left => iced::alignment::Horizontal::Left,
                    DockSide::Right => iced::alignment::Horizontal::Right,
                })
                .into()
        };
        let mut layers: Vec<Element<'_, Message>> = Vec::new();
        let group_drag = self
            .dock_drag
            .and_then(|d| d.movement())
            .and_then(|(_, g, _)| g);
        let marks_side = match target {
            DropTarget::Edge { side, .. } | DropTarget::Join { side, .. } => Some(side),
            DropTarget::Float { x, y, .. } => {
                if self.dock.float_rect(id).is_none() {
                    let (w, h) = self.dock_float_size(id);
                    layers.push(place_at(dock_ghost(id, w + DOCK_FLOAT_BAR_W, h), x, y));
                }
                None
            }
        };
        // A single pallet also gets a ghost where it lands in the column; a
        // whole group shows only the strip feedback, as the column keeps
        // showing what is there.
        if let (Some(side), None) = (marks_side, group_drag) {
            if let Some((top, bottom, w)) = self.dock_landing(id, target) {
                let x = match side {
                    DockSide::Left => strip,
                    DockSide::Right => (ww - w - strip).max(0.0),
                };
                layers.push(tint(side, w));
                layers.push(place_at(dock_ghost(id, w, bottom - top), x, top));
            }
        }
        // On top: the strip's drop marks and a ghost of what is dragged
        // following the pointer.
        if let Some(side) = marks_side {
            layers.extend(self.dock_strip_drop_marks(side, target));
            if let Some(p) = self.dock_drag_last {
                let icons = match group_drag {
                    Some((s, g)) => self.dock_group_visible(s, g),
                    None => vec![id],
                };
                layers.push(place_at(dock_icon_ghost(&icons), p.x - 16.0, p.y - 15.0));
            }
        }
        iced::widget::Stack::with_children(layers)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Where docked pallet `id` would sit after dropping on `target`: its
    /// span (top, bottom) in its group as laid out after the drop, and the
    /// group's width. `None` for a floating target.
    fn dock_landing(
        &self,
        id: crate::ui::dock::PanelId,
        target: crate::ui::dock::DropTarget,
    ) -> Option<(f32, f32, f32)> {
        use crate::ui::dock::DropTarget;
        let mut after = self.dock.clone();
        match target {
            DropTarget::Edge { side, index } => {
                after.dock(id, side, index);
            }
            DropTarget::Join { side, group, index } => {
                after.join_group(id, side, group, index);
            }
            DropTarget::Float { .. } => return None,
        }
        let (side, gi) = after.location(id)?;
        let group = &after.groups(side)[gi];
        let (_, top, bottom) = self
            .dock_open_spans(group, Some(id))
            .into_iter()
            .find(|(i, _, _)| group.panels[*i] == id)?;
        Some((top, bottom, after.group_width_px(side, gi, self.win_size.0)))
    }

    /// The icon strip's drop marks for `target` on `side`: a short line with
    /// end caps between a group's icons (join that group there), or a
    /// full-width line on a divider with a "+ New group" badge.
    fn dock_strip_drop_marks(
        &self,
        side: crate::app::config::DockSide,
        target: crate::ui::dock::DropTarget,
    ) -> Vec<Element<'_, Message>> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, DOCK_STRIP_W};
        let (ww, _) = self.dock_workspace_size();
        let strip_x = match side {
            DockSide::Left => 0.0,
            DockSide::Right => ww - DOCK_STRIP_W,
        };
        let (layout, _) = self.dock_strip_layout(side);
        let mut marks: Vec<Element<'_, Message>> = Vec::new();
        match target {
            DropTarget::Join { group, index, .. } => {
                let Some(g) = layout.iter().find(|g| g.group == group) else {
                    return marks;
                };
                // Icons above the insertion point, counted as the strip shows
                // them (the dragged icon still sits in its old place).
                let dragged = self.dock_drag.and_then(|d| d.movement()).map(|(p, _, _)| p);
                let above = self.dock.groups(side)[group].panels[..index]
                    .iter()
                    .filter(|p| self.dock_panel_visible(**p) || Some(**p) == dragged)
                    .count();
                let y = g.cell_top(above);
                marks.push(place_at(dock_insert_line(), strip_x + 7.0, y - 3.5));
            }
            DropTarget::Edge { index, .. } => {
                let y = layout
                    .iter()
                    .find(|g| g.group >= index)
                    .map(|g| g.top)
                    .or_else(|| layout.last().map(|g| g.bottom()))
                    .unwrap_or(0.0);
                marks.push(place_at(
                    container(Space::new())
                        .width(Length::Fixed(DOCK_STRIP_W))
                        .height(Length::Fixed(4.0))
                        .style(dock_drop_line_style)
                        .into(),
                    strip_x,
                    (y - 2.0).max(0.0),
                ));
                const BADGE_W: f32 = 96.0;
                let badge = container(text(format!("+ {}", t!("New group"))).size(10))
                    .width(Length::Fixed(BADGE_W))
                    .center_x(Length::Fixed(BADGE_W))
                    .padding([2, 6])
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(crate::ui::style::chrome::accent(
                            theme,
                        ))),
                        text_color: Some(crate::ui::style::chrome::on_accent(theme)),
                        border: Border {
                            radius: 9.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                let badge_x = match side {
                    DockSide::Left => DOCK_STRIP_W + 6.0,
                    DockSide::Right => ww - DOCK_STRIP_W - 6.0 - BADGE_W,
                };
                marks.push(place_at(badge.into(), badge_x, (y - 9.0).max(0.0)));
            }
            DropTarget::Float { .. } => {}
        }
        marks
    }
}

/// The glowing accent used by the strip's drop lines.
fn dock_drop_line_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(crate::ui::style::chrome::accent(theme))),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        shadow: iced::Shadow {
            color: crate::ui::style::chrome::accent(theme).scale_alpha(0.8),
            offset: iced::Vector::new(0.0, 0.0),
            blur_radius: 6.0,
        },
        ..Default::default()
    }
}

/// The short insertion line with end caps shown between a group's icons.
fn dock_insert_line() -> Element<'static, Message> {
    let cap = || {
        container(Space::new())
            .width(Length::Fixed(3.0))
            .height(Length::Fixed(7.0))
            .style(dock_drop_line_style)
    };
    row![
        cap(),
        container(Space::new())
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(3.0))
            .style(dock_drop_line_style),
        cap(),
    ]
    .align_y(iced::Center)
    .height(Length::Fixed(7.0))
    .into()
}

/// The ghost following the pointer while pallets are dragged: their icons
/// in an accent-framed box.
fn dock_icon_ghost(icons: &[crate::ui::dock::PanelId]) -> Element<'static, Message> {
    let glyphs: Vec<Element<'static, Message>> = icons
        .iter()
        .map(|id| {
            container(crate::ui::icons::themed(id.icon(), 18.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(30.0))
                .into()
        })
        .collect();
    container(column(glyphs))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                crate::ui::style::chrome::accent(theme).scale_alpha(0.22),
            )),
            border: Border {
                color: crate::ui::style::chrome::accent(theme),
                width: 1.5,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Where a floating panel draws: its top-left corner, whether its title bar
/// is on the left, and whether it is hidden down to that bar (auto-hide, not
/// hovered).
#[derive(Debug, Clone, Copy)]
struct FloatFrame {
    x: f32,
    y: f32,
    bar_left: bool,
    hidden: bool,
}

impl FloatFrame {
    /// On-screen width of panel `f` in this frame: the bar, plus the panel
    /// unless hidden.
    fn width(&self, f: crate::ui::dock::FloatPanel) -> f32 {
        if self.hidden {
            DOCK_FLOAT_BAR_W
        } else {
            f.w + DOCK_FLOAT_BAR_W
        }
    }

    /// Left edge of the title bar.
    fn bar_x(&self, f: crate::ui::dock::FloatPanel) -> f32 {
        if self.bar_left {
            self.x
        } else {
            self.x + self.width(f) - DOCK_FLOAT_BAR_W
        }
    }
}

/// A floating panel's vertical title bar: close then the hide (auto-collapse)
/// pin at the top, the panel's icon at the bottom with its title reading
/// upward above it. Pressing the bar's free area drags the panel.
fn floating_title_bar(
    id: crate::ui::dock::PanelId,
    auto_collapse: bool,
    on_left: bool,
    hovered: bool,
) -> Element<'static, Message> {
    use crate::ui::dock::DockMsg;
    use crate::ui::style::{chrome, kit_metrics as km};
    let tip = if on_left {
        iced::widget::tooltip::Position::Left
    } else {
        iced::widget::tooltip::Position::Right
    };
    // In capitals, as on the palette strip.
    let title = canvas(crate::ui::dock::VerticalLabel {
        text: t!(id.title()).to_uppercase(),
        centered: false,
    })
    .width(Fill)
    .height(Fill);
    // Close and pin only show while the pointer is over the bar, as on a
    // docked title bar.
    let mut bar = column![].spacing(km::line());
    if hovered {
        bar = bar
            .push(crate::ui::dock::close_button_sized(id, tip, km::FLOAT_ICON))
            .push(crate::ui::dock::pin_button_sized(
                id,
                auto_collapse,
                tip,
                km::FLOAT_ICON,
            ));
    }
    let bar = bar
        .push(
            container(title)
                .width(Fill)
                .height(Fill)
                .padding([km::STRIP_NAME_PAD, 0.0]),
        )
        .push(
            container(crate::ui::icons::tinted(
                id.icon(),
                km::FLOAT_ICON,
                chrome::title_text,
            ))
            .center_x(Fill),
        )
        .padding([km::TOOL_GAP, 0.0])
        .align_x(iced::Center)
        .width(Length::Fixed(km::FLOAT_BAR_W))
        .height(Fill);
    // In the ribbon's title-strip colour, with a 1 px dark line against
    // the panel.
    let bar = container(bar).style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::title_bar(theme))),
        ..Default::default()
    });
    let line = container(Space::new().width(km::line()).height(Fill)).style(|theme: &Theme| {
        container::Style {
            background: Some(Background::Color(chrome::page(theme))),
            ..Default::default()
        }
    });
    let bar: Element<'static, Message> = if on_left {
        row![bar, line].into()
    } else {
        row![line, bar].into()
    };
    mouse_area(bar)
        .on_press(Message::Dock(DockMsg::DockGrab(id)))
        .on_right_press(Message::Dock(DockMsg::FloatMenu(Some(id))))
        .on_enter(Message::Dock(DockMsg::TitleHover(id)))
        .on_move(|p| Message::Dock(DockMsg::FloatBarPointer(p)))
        .on_exit(Message::Dock(DockMsg::TitleHoverEnd(id)))
        // Double-click docks it against the edge its title bar faces.
        .on_double_click(Message::Dock(DockMsg::DockTo(
            id,
            if on_left {
                crate::app::config::DockSide::Left
            } else {
                crate::app::config::DockSide::Right
            },
        )))
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// Thickness of the band along a floating panel's sides that resizes it.
const FLOAT_EDGE_W: f32 = 5.0;
/// Side length of the square in a floating panel's corners that resizes it
/// both ways.
const FLOAT_CORNER_W: f32 = 12.0;

/// The invisible resize bands over floating panel `id`'s frame: one along
/// each side and a square in each corner (with `hidden`, the panel shows
/// only its title bar, just the top and bottom). Between them the layer is
/// empty, so the panel below gets the pointer.
fn float_resize_handles(id: crate::ui::dock::PanelId, hidden: bool) -> Element<'static, Message> {
    use crate::ui::dock::{DockMsg, FloatEdge};
    let handle = |edge: FloatEdge, w: Length, h: Length| -> Element<'static, Message> {
        mouse_area(Space::new().width(w).height(h))
            .on_press(Message::Dock(DockMsg::FloatResizeGrab(id, edge)))
            .interaction(edge.interaction())
            .into()
    };
    let band = |edge: FloatEdge, top: bool| -> Element<'static, Message> {
        container(handle(edge, Fill, Length::Fixed(FLOAT_EDGE_W)))
            .width(Fill)
            .height(Length::Fixed(FLOAT_CORNER_W))
            .align_y(if top {
                iced::alignment::Vertical::Top
            } else {
                iced::alignment::Vertical::Bottom
            })
            .into()
    };
    if hidden {
        return column![
            band(FloatEdge::N, true),
            Space::new().width(Fill).height(Fill),
            band(FloatEdge::S, false),
        ]
        .into();
    }
    let corner = Length::Fixed(FLOAT_CORNER_W);
    let side = Length::Fixed(FLOAT_EDGE_W);
    column![
        row![
            handle(FloatEdge::NW, corner, corner),
            band(FloatEdge::N, true),
            handle(FloatEdge::NE, corner, corner),
        ],
        row![
            handle(FloatEdge::W, side, Fill),
            Space::new().width(Fill).height(Fill),
            handle(FloatEdge::E, side, Fill),
        ]
        .height(Fill),
        row![
            handle(FloatEdge::SW, corner, corner),
            band(FloatEdge::S, false),
            handle(FloatEdge::SE, corner, corner),
        ],
    ]
    .into()
}

/// Grabbable separator between a docked pallet and the viewport: drag to
/// size its group, double-click to reset the width.
fn dock_divider(
    id: crate::ui::dock::PanelId,
    side: crate::app::config::DockSide,
) -> Element<'static, Message> {
    use crate::ui::style::{chrome, kit_metrics as km};
    // The grab band lies over the drawing's edge, beside the palette, and
    // shows nothing: the palette runs up to the drawing as a floating one
    // does. Under the pointer its line shows in the accent, against the
    // palette.
    let band = |lit: bool| -> Element<'static, Message> {
        let line = container(Space::new())
            .width(Length::Fixed(km::DIVIDER_LINE))
            .height(Fill)
            .style(move |theme: &Theme| container::Style {
                background: lit.then(|| Background::Color(chrome::accent(theme))),
                ..Default::default()
            });
        let rest = Space::new()
            .width(Length::Fixed(DOCK_DIVIDER_W - km::DIVIDER_LINE))
            .height(Fill);
        match side {
            crate::app::config::DockSide::Left => row![line, rest].into(),
            crate::app::config::DockSide::Right => row![rest, line].into(),
        }
    };
    let line = iced::widget::hover(band(false), band(true));
    let grab = Message::Dock(crate::ui::dock::DockMsg::ResizeGrab(id));
    let reset = Message::Dock(crate::ui::dock::DockMsg::WidthReset(id));
    mouse_area(line)
        .on_press(grab)
        .on_double_click(reset)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

/// Opacity of the accent on a lit group grip.
const GRIP_LIT: f32 = 0.55;

/// The pallet menu of an edge's + button and of a right-click on the strip:
/// one item per pallet with its icon, its name and a check mark when it is
/// open on this edge, then the three strip modes as choices. `checked`
/// holds each pallet with that state.
fn dock_edge_menu_items(
    side: crate::app::config::DockSide,
    checked: &[(MenuPanel, bool)],
    names: crate::ui::dock::StripNames,
) -> Vec<crate::ui::kit::menu::Item> {
    use crate::ui::dock::{DockMsg, StripNames};
    use crate::ui::kit::menu::{Item, Mark};
    let mut items: Vec<Item> = checked
        .iter()
        .map(|(entry, checked)| {
            Item::new(
                entry.title(),
                Message::Dock(DockMsg::EdgeMenuToggle(side, entry.id())),
            )
            .mark(Mark::Check(*checked))
            .icon(entry.icon())
        })
        .collect();
    items.push(Item::Separator);
    for (label, mode) in [
        (crate::tr!("dock", "icons-only"), StripNames::Icons),
        (
            crate::tr!("dock", "icons-and-names"),
            StripNames::IconsAndNames,
        ),
        (crate::tr!("dock", "names-only"), StripNames::NamesOnly),
    ] {
        items.push(
            Item::new(label, Message::Dock(DockMsg::StripNames(mode)))
                .mark(Mark::Radio(mode == names)),
        );
    }
    items
}

/// Draggable bar between two stacked pallets of group `gi`; double-click
/// gives every pallet of the group the same height again. It spans exactly
/// the pallets' width (panel plus its divider): a `Fill` width would stretch
/// the whole edge column across the workspace and squeeze the drawing view.
fn dock_splitter(
    side: crate::app::config::DockSide,
    gi: usize,
    upper: usize,
    lower: usize,
    col_w: f32,
    active: bool,
) -> Element<'static, Message> {
    let line = canvas(crate::ui::dock::SplitterGrip { active })
        .width(Length::Fixed(col_w))
        .height(Length::Fixed(crate::ui::style::kit_metrics::SPLITTER_H));
    mouse_area(line)
        .on_press(Message::Dock(crate::ui::dock::DockMsg::SplitGrab(
            side, gi, upper, lower,
        )))
        .on_double_click(Message::Dock(crate::ui::dock::DockMsg::SplitReset(
            side, gi,
        )))
        .interaction(iced::mouse::Interaction::ResizingVertically)
        .into()
}

/// The blue drop ghost: a `w`×`h` outline of where pallet `id` lands, with
/// its name as the header.
fn dock_ghost(id: crate::ui::dock::PanelId, w: f32, h: f32) -> Element<'static, Message> {
    let header = container(
        text(t!(id.title()))
            .size(12)
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::advanced::text::Ellipsis::End),
    )
    .width(Fill)
    .padding([5, 8])
    .clip(true)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(crate::ui::style::chrome::accent(theme))),
        text_color: Some(crate::ui::style::chrome::on_accent(theme)),
        ..Default::default()
    });
    container(column![header, Space::new()])
        .width(Length::Fixed(w.max(1.0)))
        .height(Length::Fixed(h.max(1.0)))
        .clip(true)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                crate::ui::style::chrome::surface(theme).scale_alpha(0.85),
            )),
            border: Border {
                color: crate::ui::style::chrome::accent(theme),
                width: 2.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Position `content` with its top-left corner at workspace point (`x`, `y`).
fn place_at(content: Element<'_, Message>, x: f32, y: f32) -> Element<'_, Message> {
    container(content)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Left)
        .align_y(iced::alignment::Vertical::Top)
        .padding(iced::Padding {
            top: y.max(0.0),
            right: 0.0,
            bottom: 0.0,
            left: x.max(0.0),
        })
        .into()
}
