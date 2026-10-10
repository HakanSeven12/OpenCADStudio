//! Dock drawing: the edges (icon strip and shown group), floating pallets,
//! the + menu and the drag preview.

use crate::app::document::DocumentTab;
use crate::app::{Message, OpenCADStudio};
use crate::t;
use crate::ui::dock::DOCK_DIVIDER_W;
use iced::widget::{button, canvas, column, container, mouse_area, row, stack, text, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

impl OpenCADStudio {
    /// Everything the dock draws over the workspace (the row of edges and
    /// viewport): floating pallets, an open + menu, the drag preview, and
    /// the pointer capture that feeds drags.
    pub(in crate::app) fn dock_overlay<'a>(
        &'a self,
        workspace: Element<'a, Message>,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        use crate::ui::dock::DockMsg;
        // Floating panels sit over the workspace, back to front.
        let mut layers: Vec<Element<'_, Message>> = vec![workspace];
        for f in &self.dock.floating {
            // Once a window drag is under way only its preview shows it.
            let dragged = matches!(
                self.dock_drag,
                Some(crate::ui::dock::DockDrag::Move { panel, window: true, target: Some(_), .. })
                    if f.contains(panel)
            );
            let visible = self.dock_float_visible(f);
            if !visible.is_empty() && !dragged {
                layers.push(self.floating_panel(f, visible, tab));
            }
        }
        // An open pallet menu hangs beside its edge's + button, over a
        // catcher that closes it on any click elsewhere.
        if let Some(side) = self.dock_edge_menu {
            if !self.dock_visible_groups(side).is_empty() {
                // Checked = open and docked on this edge.
                let checked: Vec<_> = crate::ui::dock::PanelId::ALL
                    .iter()
                    .map(|p| {
                        let here = self.dock.location(*p).map(|(s, _)| s) == Some(side);
                        (*p, here && self.dock_panel_visible(*p))
                    })
                    .collect();
                let (_, plus_top) = self.dock_strip_layout(side);
                let strip = crate::ui::dock::DOCK_STRIP_W;
                let x = match side {
                    crate::app::config::DockSide::Left => strip + 4.0,
                    crate::app::config::DockSide::Right => {
                        self.dock_workspace_size().0 - strip - DOCK_MENU_W - 4.0
                    }
                };
                layers.push(
                    mouse_area(Space::new().width(Fill).height(Fill))
                        .on_press(Message::Dock(DockMsg::EdgeMenu(None)))
                        .on_right_press(Message::Dock(DockMsg::EdgeMenu(None)))
                        .into(),
                );
                let toggle = move |id| Message::Dock(DockMsg::EdgeMenuToggle(side, id));
                layers.push(place_at(dock_pallet_menu(&checked, toggle), x.max(0.0), plus_top));
            }
        }
        if let Some(menu) = self.dock_float_menu_overlay() {
            layers.push(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(Message::Dock(DockMsg::FloatMenu(None)))
                    .on_right_press(Message::Dock(DockMsg::FloatMenu(None)))
                    .into(),
            );
            layers.push(menu);
        }
        // While a panel is dragged, preview where it lands.
        if let Some((id, _, Some(target))) = self.dock_drag.and_then(|d| d.movement()) {
            layers.push(self.dock_drop_preview(id, target));
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
        let capturing = self.dock_drag.is_some()
            || self.xref_col_drag.is_some()
            || self.xref_split_drag;
        let mut capture = mouse_area(workspace);
        if capturing {
            capture = capture
                .on_move(move |p| Message::Dock(crate::ui::dock::DockMsg::DragMove(p)))
                .on_release(Message::Dock(crate::ui::dock::DockMsg::DragRelease))
                .interaction(match self.dock_drag {
                    Some(DockDrag::FloatSize { from_left: true, .. }) => {
                        iced::mouse::Interaction::ResizingDiagonallyUp
                    }
                    Some(DockDrag::FloatSize { from_left: false, .. }) => {
                        iced::mouse::Interaction::ResizingDiagonallyDown
                    }
                    Some(DockDrag::Split { .. }) => iced::mouse::Interaction::ResizingVertically,
                    Some(DockDrag::Move { .. }) => iced::mouse::Interaction::Grabbing,
                    // Widths, the Layer Manager column and the xref drags.
                    _ => iced::mouse::Interaction::ResizingHorizontally,
                });
        }
        capture.into()
    }

    /// Build one edge: the vertical icon strip at the very edge plus the
    /// edge's shown group beside it. The strip lists every group as a grip
    /// bar over its pallets' icons, with a + button below; clicking an icon
    /// shows its group. The shown group's pallets stack top to bottom, each
    /// at its weighted share of the height, with a draggable splitter between
    /// neighbours. A group whose pallets all auto-hide shows only while one
    /// of its icons (or the group itself) is hovered; leaving the edge hides
    /// it again. `None` when nothing on the edge is open.
    pub(in crate::app) fn build_edge_stack<'a>(
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
                    layers.push(dock_splitter(side, gi, upper, pos, width));
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
        Some(
            mouse_area(row(children).height(Fill))
                .on_enter(Message::Dock(crate::ui::dock::DockMsg::HoverStay))
                .on_exit(Message::Dock(crate::ui::dock::DockMsg::HoverExit))
                .into(),
        )
    }

    /// An edge's vertical icon strip: per group its open pallets' icons in a
    /// row ended by a hairline divider, then a + button opening the pallet
    /// menu. The band along a group's window-side edge is its grip: it
    /// carries the blue bar of the shown group, lights up while hovered (or
    /// while it is a drop target) and drags the whole group; an icon shows its
    /// group on click and drags just that pallet. Heights follow
    /// `dock::strip_layout`, which drop targeting uses too.
    fn dock_icon_strip(&self, side: crate::app::config::DockSide) -> Element<'_, Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{
            DockMsg, DropTarget, DOCK_STRIP_W, STRIP_CELL_H, STRIP_DIVIDER_H, STRIP_GRIP_W,
            STRIP_PAD, STRIP_PLUS_GAP,
        };
        // Only a group actually on screen is marked as shown: on an
        // auto-hiding edge nothing is, until a group is revealed.
        let shown = self
            .dock_shown_group(side)
            .filter(|_| self.dock_edge_expanded(side));
        let movement = self.dock_drag.and_then(|d| d.movement());
        let move_target = movement.and_then(|(_, _, target)| target);
        let dragging = move_target.is_some();
        let target_group = match move_target {
            Some(DropTarget::Join { side: s, group, .. }) if s == side => Some(group),
            _ => None,
        };
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        for gi in self.dock_visible_groups(side) {
            let is_shown = shown == Some(gi);
            let lit = target_group == Some(gi)
                || (!dragging && self.dock_grip_hover == Some((side, gi)));
            let group_dragged =
                dragging && movement.is_some_and(|(_, group, _)| group == Some((side, gi)));
            let panels = self.dock_group_visible(side, gi);
            let row_h = 2.0 * STRIP_PAD + panels.len() as f32 * STRIP_CELL_H;

            let mut icons: Vec<Element<'_, Message>> = vec![Space::new().height(STRIP_PAD).into()];
            for id in panels {
                let faded = group_dragged
                    || (dragging
                        && movement.is_some_and(|(panel, group, _)| group.is_none() && panel == id));
                let glyph = if faded {
                    crate::ui::icons::themed_disabled(id.icon(), 20.0)
                } else if is_shown {
                    crate::ui::icons::themed(id.icon(), 20.0)
                } else {
                    crate::ui::icons::themed_secondary(id.icon(), 20.0)
                };
                let hovered = !dragging && self.dock_icon_hover == Some(id);
                let cell = container(glyph)
                    .center_x(Length::Fixed(STRIP_CELL_H))
                    .center_y(Length::Fixed(STRIP_CELL_H - 2.0))
                    .style(move |theme: &Theme| container::Style {
                        background: hovered
                            .then(|| Background::Color(theme.palette().background.weak.color)),
                        border: Border {
                            radius: 5.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                let cell = mouse_area(
                    container(cell)
                        .center_x(Length::Fixed(DOCK_STRIP_W))
                        .center_y(Length::Fixed(STRIP_CELL_H)),
                )
                .on_press(Message::Dock(DockMsg::IconPress(id)))
                .on_enter(Message::Dock(DockMsg::Hover(id)))
                .on_exit(Message::Dock(DockMsg::HoverEnd(id)))
                .interaction(iced::mouse::Interaction::Pointer);
                icons.push(cell.into());
            }

            // The group grip: a band on the window side holding the edge bar.
            let bar_w = if lit { 5.0 } else { 3.0 };
            let bar_inset = 5.0;
            let bar = container(
                Space::new()
                    .width(bar_w)
                    .height(Length::Fixed(row_h - 2.0 * bar_inset)),
            )
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let (color, glow) = if lit {
                    (Some(palette.primary.weak.color), true)
                } else if is_shown {
                    (Some(palette.primary.base.color), false)
                } else {
                    (None, false)
                };
                container::Style {
                    background: color.map(Background::Color),
                    border: Border {
                        radius: match side {
                            DockSide::Left => iced::border::Radius::default().right(3.0),
                            DockSide::Right => iced::border::Radius::default().left(3.0),
                        },
                        ..Default::default()
                    },
                    shadow: if glow {
                        iced::Shadow {
                            color: palette.primary.base.color.scale_alpha(0.55),
                            offset: iced::Vector::new(0.0, 0.0),
                            blur_radius: 8.0,
                        }
                    } else {
                        iced::Shadow::default()
                    },
                    ..Default::default()
                }
            });
            let band = mouse_area(
                container(bar)
                    .width(Length::Fixed(STRIP_GRIP_W))
                    .height(Length::Fixed(row_h))
                    .align_y(iced::alignment::Vertical::Center)
                    .align_x(match side {
                        DockSide::Left => iced::alignment::Horizontal::Left,
                        DockSide::Right => iced::alignment::Horizontal::Right,
                    }),
            )
            .on_press(Message::Dock(DockMsg::GroupGrab(side, gi)))
            .on_double_click(Message::Dock(DockMsg::GroupFloat(side, gi)))
            .on_enter(Message::Dock(DockMsg::GripHover(side, gi)))
            .on_exit(Message::Dock(DockMsg::GripHoverEnd(side, gi)))
            .interaction(iced::mouse::Interaction::Grab);
            let band = container(band)
                .width(Fill)
                .height(Length::Fixed(row_h))
                .align_x(match side {
                    DockSide::Left => iced::alignment::Horizontal::Left,
                    DockSide::Right => iced::alignment::Horizontal::Right,
                });
            items.push(
                // A stack is as tall as its first layer, so the icon column
                // spans the whole row; the edge bar then centres on the row.
                container(stack![column(icons).height(Length::Fixed(row_h)), band])
                    .width(Length::Fixed(DOCK_STRIP_W))
                    .height(Length::Fixed(row_h))
                    .style(move |theme: &Theme| container::Style {
                        background: lit.then(|| {
                            Background::Color(theme.palette().primary.weak.color.scale_alpha(0.12))
                        }),
                        ..Default::default()
                    })
                    .into(),
            );
            items.push(
                container(Space::new())
                    .width(Fill)
                    .height(Length::Fixed(STRIP_DIVIDER_H))
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(
                            theme.palette().background.neutral.color,
                        )),
                        ..Default::default()
                    })
                    .into(),
            );
        }
        items.push(Space::new().height(STRIP_PLUS_GAP).into());
        let plus = button(crate::ui::icons::themed_secondary(crate::ui::icons::PLUS, 14.0))
            .on_press(Message::Dock(DockMsg::EdgeMenu(Some(side))))
            .style(|theme: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        theme.palette().background.weak.color
                    }
                    _ => theme.palette().background.weakest.color,
                })),
                border: Border {
                    radius: 14.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .padding(7);
        items.push(container(plus).center_x(Length::Fixed(DOCK_STRIP_W)).into());
        container(column(items))
            .width(Length::Fixed(DOCK_STRIP_W))
            .height(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
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
            crate::ui::dock::PanelId::BlockPalette => {
                crate::ui::window::block_palette::view(&self.block_palette, width, chrome)
            }
            crate::ui::dock::PanelId::ExternalReferences => self.xref_manager.view(
                width,
                chrome,
                tab.xref_missing,
                &tab.scene.document,
            ),
            crate::ui::dock::PanelId::Browser => crate::ui::window::browser::view(
                &tab.scene.document,
                tab.sketch_session.as_ref().map(|session| session.name.as_str()),
                width,
                chrome,
            ),
            crate::ui::dock::PanelId::NodeGraph => tab.graph.panel(width, chrome),
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
                tab.layers.view_panel(self.layer_name_col_w, width, chrome)
            }
        }
    }

    /// A pallet of the shown group: its body plus a grabbable divider
    /// against the viewport (sizing the whole group). Hovering is handled by
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
        let divider = dock_divider(id);
        match side {
            crate::app::config::DockSide::Left => row![panel, divider].height(Fill).into(),
            crate::app::config::DockSide::Right => row![divider, panel].height(Fill).into(),
        }
    }

    /// Where floating window `f` draws: its top-left corner, whether its
    /// title bar is on the left, and whether it is hidden down to that bar
    /// (auto-hide, not hovered).
    fn dock_float_frame(&self, f: &crate::ui::dock::FloatPanel) -> (f32, f32, bool, bool) {
        let (ww, wh) = self.dock_workspace_size();
        // The bar faces the nearer workspace edge; while resizing, it stays
        // opposite the grip being dragged so it cannot flip mid-drag.
        let bar_left = match self.dock_drag {
            Some(crate::ui::dock::DockDrag::FloatSize { panel, from_left }) if f.contains(panel) => {
                !from_left
            }
            _ => f.x + (f.w + DOCK_FLOAT_BAR_W) * 0.5 < ww * 0.5,
        };
        let hidden = self.dock.auto_hides(f.id()) && !self.dock_peek.is_some_and(|p| f.contains(p));
        let total_w = if hidden { DOCK_FLOAT_BAR_W } else { f.w + DOCK_FLOAT_BAR_W };
        // A hidden window keeps its title bar where it sits when shown.
        let x = if hidden && !bar_left { f.x + f.w } else { f.x };
        // Keep the window on screen when the workspace shrank since it was
        // placed.
        let x = x.min(ww - total_w).max(0.0);
        let y = f.y.min(wh - f.h).max(0.0);
        (x, y, bar_left, hidden)
    }

    /// A floating window at its saved place: a vertical title bar on the
    /// side facing the nearer workspace edge, its open pallets (`visible`)
    /// stacked beside it, and a corner grip to resize it. With its pin on,
    /// the window hides down to the title bar until hovered.
    fn floating_panel<'a>(
        &'a self,
        f: &'a crate::ui::dock::FloatPanel,
        visible: Vec<crate::ui::dock::PanelId>,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        use crate::ui::dock::DockMsg;
        let (x, y, bar_left, hidden) = self.dock_float_frame(f);
        // Messages name the window by its first open pallet.
        let anchor = visible[0];
        let hovered = self.dock_title_hover.is_some_and(|p| f.contains(p));
        let bar = floating_title_bar(&visible, self.dock.auto_hides(anchor), bar_left, hovered);
        let content: Element<'_, Message> = if hidden {
            bar
        } else {
            // The resize grip sits in the bottom corner away from the bar.
            let grip_left = !bar_left;
            let grip_icon = crate::ui::icons::themed_secondary(
                if grip_left {
                    crate::ui::icons::RESIZE_LEFT
                } else {
                    crate::ui::icons::RESIZE
                },
                12.0,
            );
            let grip = mouse_area(
                container(grip_icon)
                    .center_x(Length::Fixed(14.0))
                    .center_y(Length::Fixed(14.0)),
            )
            .on_press(Message::Dock(DockMsg::FloatResizeGrab(anchor, grip_left)))
            .interaction(if grip_left {
                iced::mouse::Interaction::ResizingDiagonallyUp
            } else {
                iced::mouse::Interaction::ResizingDiagonallyDown
            });
            // The pallets stack like a docked group, with splitters between.
            let mut pallets: Vec<Element<'_, Message>> = Vec::new();
            let mut prev: Option<usize> = None;
            for id in &visible {
                let pos = f.group.panels.iter().position(|p| p == id).unwrap_or(0);
                if let Some(upper) = prev {
                    pallets.push(float_splitter(anchor, upper, pos, f.w));
                }
                let weight = f.group.weights.get(pos).copied().unwrap_or(1.0);
                pallets.push(
                    container(self.panel_body(*id, f.w, true, tab))
                        .height(Length::FillPortion(crate::ui::dock::portion(weight)))
                        .clip(true)
                        .into(),
                );
                prev = Some(pos);
            }
            let body = stack![
                column(pallets).height(Fill),
                container(grip)
                    .width(Fill)
                    .height(Fill)
                    .align_x(if grip_left {
                        iced::alignment::Horizontal::Left
                    } else {
                        iced::alignment::Horizontal::Right
                    })
                    .align_y(iced::alignment::Vertical::Bottom),
            ]
            .width(Length::Fixed(f.w))
            .height(Fill);
            if bar_left {
                row![bar, body].into()
            } else {
                row![body, bar].into()
            }
        };
        // Clipped, so a pallet squeezed below its content's height never
        // draws outside the window.
        let framed = container(content)
            .height(Length::Fixed(f.h))
            .clip(true)
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.strong.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 14.0,
                },
                ..Default::default()
            });
        // The idle cursor marks the window as opaque to the pointer, so
        // clicks on its empty areas don't fall through to the drawing below.
        let panel = mouse_area(framed)
            .on_press(Message::Dock(DockMsg::FloatRaise(anchor)))
            .on_enter(Message::Dock(DockMsg::Hover(anchor)))
            .on_exit(Message::Dock(DockMsg::HoverExit))
            .interaction(iced::mouse::Interaction::Idle);
        place_at(panel.into(), x, y)
    }

    /// The open right-click menu of a floating window: Pallets (a submenu
    /// to add or hide pallets in the window), Allow docking, Dock left and
    /// Dock right. It opens at the pointer, flipped to stay on screen.
    fn dock_float_menu_overlay(&self) -> Option<Element<'_, Message>> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        let anchor = self.dock_float_menu?;
        let f = self.dock.float_rect(anchor)?;
        if self.dock_float_visible(f).is_empty() {
            return None;
        }
        let (ww, wh) = self.dock_workspace_size();
        let (x, y, bar_left, hidden) = self.dock_float_frame(f);
        let total_w = if hidden { DOCK_FLOAT_BAR_W } else { f.w + DOCK_FLOAT_BAR_W };
        let bar_x = if bar_left { x } else { x + total_w - DOCK_FLOAT_BAR_W };
        // Where the right-click happened, in the workspace.
        let at = iced::Point::new(bar_x + self.dock_float_menu_at.x, y + self.dock_float_menu_at.y);
        let max_x = (ww - DOCK_MENU_W).max(0.0);
        let menu_x = if at.x + DOCK_MENU_W <= ww { at.x } else { at.x - DOCK_MENU_W };
        let menu_x = menu_x.clamp(0.0, max_x);
        let menu_y = at.y.min(wh - FLOAT_MENU_H).max(0.0);
        let open = self.dock_float_menu_pallets;
        // Hovering a row opens (Pallets) or closes (the others) the submenu.
        let item = |label: Element<'static, Message>, press: DockMsg, sub: bool| -> Element<'static, Message> {
            mouse_area(
                button(label)
                    .on_press(Message::Dock(press))
                    .style(menu_row_style)
                    .padding([5, 8])
                    .width(Fill),
            )
            .on_enter(Message::Dock(DockMsg::FloatMenuPallets(sub)))
            .into()
        };
        let pallets = row![
            text(t!("Pallets")).size(12).width(Fill),
            text("\u{203A}").size(13),
        ]
        .align_y(iced::Center);
        let separator = container(Space::new())
            .width(Fill)
            .height(Length::Fixed(1.0))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            });
        let check: Element<'static, Message> = if f.docking {
            crate::ui::icons::themed(crate::ui::icons::CHECK, 12.0)
        } else {
            Space::new().width(12).height(12).into()
        };
        let docking = row![text(t!("Allow docking")).size(12).width(Fill), check]
            .align_y(iced::Center);
        let main = column![
            item(pallets.into(), DockMsg::FloatMenuPallets(!open), true),
            container(separator).padding([3, 4]),
            item(docking.into(), DockMsg::FloatDockingToggle(anchor), false),
            item(
                text(t!("Dock left")).size(12).into(),
                DockMsg::DockTo(anchor, DockSide::Left),
                false
            ),
            item(
                text(t!("Dock right")).size(12).into(),
                DockMsg::DockTo(anchor, DockSide::Right),
                false
            ),
        ]
        .spacing(1)
        .padding(4);
        let mut layers = vec![place_at(menu_frame(main.into()), menu_x, menu_y)];
        if open {
            let checked: Vec<_> = PanelId::ALL
                .iter()
                .map(|p| (*p, f.contains(*p) && self.dock_panel_visible(*p)))
                .collect();
            let toggle = move |id| Message::Dock(DockMsg::FloatMenuToggle(anchor, id));
            // To the right of the menu, or its left when there is no room.
            let sub_x = if menu_x + 2.0 * DOCK_MENU_W + 2.0 <= ww {
                menu_x + DOCK_MENU_W + 2.0
            } else {
                menu_x - DOCK_MENU_W - 2.0
            }
            .clamp(0.0, max_x);
            let sub_y = menu_y.min(wh - PALLET_MENU_H).max(0.0);
            layers.push(place_at(dock_pallet_menu(&checked, toggle), sub_x, sub_y));
        }
        Some(
            iced::widget::Stack::with_children(layers)
                .width(Fill)
                .height(Fill)
                .into(),
        )
    }

    /// The live drop preview for dragged panel `id`: a tint over the target
    /// edge plus a panel-sized ghost where it lands — stacked into a group
    /// (sized as that group will be), as a new group of its own (full
    /// height), or as a floating window.
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
                        theme.palette().primary.weak.color.scale_alpha(0.35),
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
        let group_drag = self.dock_drag.and_then(|d| d.movement()).and_then(|(_, g, _)| g);
        // The pallets that move: a docked group's, a floating window's, or
        // just `id`.
        let window_drag = matches!(
            self.dock_drag,
            Some(crate::ui::dock::DockDrag::Move { window: true, .. })
        );
        let moving = match (group_drag, self.dock.float_rect(id)) {
            (Some((s, g)), _) => self.dock_group_visible(s, g),
            (None, Some(f)) if window_drag => self.dock_float_visible(f),
            _ => vec![id],
        };
        let marks_side = match target {
            DropTarget::Edge { side, .. } | DropTarget::Join { side, .. } => Some(side),
            DropTarget::Float { x, y } => {
                let (w, h) = match group_drag {
                    Some((s, g)) => (self.dock_group_width(s, g), self.dock_default_float_h()),
                    None => self.dock_float_size(id),
                };
                layers.push(place_at(dock_ghost(id, w + DOCK_FLOAT_BAR_W, h), x, y));
                None
            }
        };
        // A single pallet also gets a ghost where it lands in the column;
        // several show only the strip feedback, as the column keeps showing
        // what is there.
        if let (Some(side), None, [_]) = (marks_side, group_drag, moving.as_slice()) {
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
                layers.push(place_at(dock_icon_ghost(&moving), p.x - 16.0, p.y - 15.0));
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
        let open: Vec<usize> = (0..group.panels.len())
            .filter(|i| group.panels[*i] == id || self.dock_panel_visible(group.panels[*i]))
            .collect();
        let weights: Vec<f32> = open.iter().map(|i| group.weights[*i]).collect();
        let at = open.iter().position(|i| group.panels[*i] == id)?;
        let (_, avail) = self.dock_workspace_size();
        let (top, bottom) = *crate::ui::dock::slot_spans(&weights, avail).get(at)?;
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
        use crate::ui::dock::{DropTarget, DOCK_STRIP_W, STRIP_CELL_H};
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
                let y = g.icons_top + above as f32 * STRIP_CELL_H;
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
                        background: Some(Background::Color(theme.palette().primary.base.color)),
                        text_color: Some(theme.palette().primary.base.text),
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
        background: Some(Background::Color(theme.palette().primary.base.color)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        shadow: iced::Shadow {
            color: theme.palette().primary.base.color.scale_alpha(0.8),
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
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.primary.base.color.scale_alpha(0.22))),
                border: Border {
                    color: palette.primary.base.color,
                    width: 1.5,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

/// Canvas that draws a floating title bar's label, reading upward from the
/// bar's bottom end.
struct VBarLabel {
    text: String,
}

impl canvas::Program<Message> for VBarLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.with_save(|frame| {
            frame.translate(iced::Vector::new(bounds.width / 2.0, bounds.height / 2.0));
            frame.rotate(iced::Radians(-std::f32::consts::FRAC_PI_2));
            // After the rotation the text runs along local +x (upward), so
            // the bar's bottom end sits at -height/2.
            frame.fill_text(canvas::Text {
                content: self.text.clone(),
                position: iced::Point::new(-bounds.height / 2.0, 0.0),
                color: theme.palette().background.base.text.scale_alpha(0.72),
                size: iced::Pixels(13.0),
                align_x: iced::advanced::text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                shaping: iced::advanced::text::Shaping::Advanced,
                ..Default::default()
            });
        });
        vec![frame.into_geometry()]
    }
}

/// Width of a floating panel's vertical title bar.
const DOCK_FLOAT_BAR_W: f32 = 28.0;

/// A floating window's vertical title bar: close then the hide (auto-hide)
/// pin at the top; at the bottom the pallet's icon with its title reading
/// upward, or for a group each pallet's icon (press one to drag that pallet
/// out). Pressing the bar drags the window, double-clicking docks it against
/// the edge the bar faces, right-clicking opens its menu.
fn floating_title_bar(
    visible: &[crate::ui::dock::PanelId],
    auto_collapse: bool,
    on_left: bool,
    hovered: bool,
) -> Element<'static, Message> {
    use crate::ui::dock::DockMsg;
    let anchor = visible[0];
    let tip = if on_left {
        iced::widget::tooltip::Position::Left
    } else {
        iced::widget::tooltip::Position::Right
    };
    // Close and pin only show while the pointer is over the bar, as on a
    // docked title bar.
    let mut bar = column![].spacing(2);
    if hovered {
        bar = bar
            .push(crate::ui::dock::close_button(DockMsg::FloatClose(anchor), tip))
            .push(crate::ui::dock::pin_button(anchor, auto_collapse, tip));
    }
    if let [id] = visible {
        let title = canvas(VBarLabel {
            text: t!(id.title()).into_owned(),
        })
        .width(Fill)
        .height(Fill);
        bar = bar
            .push(container(title).width(Fill).height(Fill).padding([6, 0]))
            .push(container(crate::ui::icons::themed(id.icon(), 16.0)).center_x(Fill));
    } else {
        bar = bar.push(Space::new().height(Fill));
        for id in visible {
            let icon = mouse_area(
                container(crate::ui::icons::themed(id.icon(), 16.0))
                    .center_x(Fill)
                    .center_y(Length::Fixed(24.0)),
            )
            .on_press(Message::Dock(DockMsg::IconPress(*id)))
            .interaction(iced::mouse::Interaction::Grab);
            bar = bar.push(
                iced::widget::tooltip(icon, text(t!(id.title())).size(10), tip).gap(4),
            );
        }
    }
    let bar = bar
        .padding([4, 0])
        .align_x(iced::Center)
        .width(Length::Fixed(DOCK_FLOAT_BAR_W))
        .height(Fill);
    mouse_area(container(bar).style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        ..Default::default()
    }))
    .on_press(Message::Dock(DockMsg::FloatGrab(anchor)))
    .on_right_press(Message::Dock(DockMsg::FloatMenu(Some(anchor))))
    .on_enter(Message::Dock(DockMsg::TitleHover(anchor)))
    .on_move(|p| Message::Dock(DockMsg::FloatBarPointer(p)))
    .on_exit(Message::Dock(DockMsg::TitleHoverEnd(anchor)))
    // Double-click docks it against the edge its title bar faces.
    .on_double_click(Message::Dock(DockMsg::DockTo(
        anchor,
        if on_left {
            crate::app::config::DockSide::Left
        } else {
            crate::app::config::DockSide::Right
        },
    )))
    .interaction(iced::mouse::Interaction::Grab)
    .into()
}

/// Draggable bar between floating pallets `upper` and `lower` of the window
/// holding `anchor`; double-click evens their heights.
fn float_splitter(
    anchor: crate::ui::dock::PanelId,
    upper: usize,
    lower: usize,
    w: f32,
) -> Element<'static, Message> {
    use crate::ui::dock::DockMsg;
    let line = container(Space::new())
        .width(Length::Fixed(w))
        .height(Length::Fixed(5.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        });
    mouse_area(line)
        .on_press(Message::Dock(DockMsg::FloatSplitGrab(anchor, upper, lower)))
        .on_double_click(Message::Dock(DockMsg::FloatSplitReset(anchor)))
        .interaction(iced::mouse::Interaction::ResizingVertically)
        .into()
}

/// Grabbable separator between a docked pallet and the viewport: drag to
/// size its group, double-click to reset the width.
fn dock_divider(id: crate::ui::dock::PanelId) -> Element<'static, Message> {
    let line = container(Space::new())
        .width(Length::Fixed(DOCK_DIVIDER_W))
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        });
    let grab = Message::Dock(crate::ui::dock::DockMsg::ResizeGrab(id));
    let reset = Message::Dock(crate::ui::dock::DockMsg::WidthReset(id));
    mouse_area(line)
        .on_press(grab)
        .on_double_click(reset)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}


/// Width of the icon strip's pallet menu.
const DOCK_MENU_W: f32 = 230.0;
/// Rough heights of a floating window's menu and of a pallet menu, to keep
/// them on screen.
const FLOAT_MENU_H: f32 = 150.0;
const PALLET_MENU_H: f32 = 270.0;

/// A pallet menu (an edge's + button, a floating window's Pallets
/// submenu), styled like the right-click menu: one row per pallet with its
/// icon, its name and a check mark when it is open there. `checked` holds
/// each pallet with that state; picking one sends `toggle`.
fn dock_pallet_menu(
    checked: &[(crate::ui::dock::PanelId, bool)],
    toggle: impl Fn(crate::ui::dock::PanelId) -> Message,
) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = checked
        .iter()
        .map(|(id, checked)| {
            let id = *id;
            let check: Element<'static, Message> = if *checked {
                crate::ui::icons::themed(crate::ui::icons::CHECK, 12.0)
            } else {
                Space::new().width(12).height(12).into()
            };
            button(
                row![
                    crate::ui::icons::themed(id.icon(), 16.0),
                    text(t!(id.title())).size(12).width(Fill),
                    check,
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .on_press(toggle(id))
            .style(menu_row_style)
            .padding([5, 8])
            .width(Fill)
            .into()
        })
        .collect();
    menu_frame(column(rows).spacing(1).padding(4).into())
}

/// A menu row button: transparent until hovered.
fn menu_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| Background::Color(palette.background.weak.color)),
        text_color: palette.background.base.text,
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The raised box a dock menu sits in.
fn menu_frame(rows: Element<'static, Message>) -> Element<'static, Message> {
    container(rows)
        .width(Length::Fixed(DOCK_MENU_W))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        })
        .into()
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
) -> Element<'static, Message> {
    let line = container(Space::new())
        .width(Length::Fixed(col_w + DOCK_DIVIDER_W))
        .height(Length::Fixed(5.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        });
    mouse_area(line)
        .on_press(Message::Dock(crate::ui::dock::DockMsg::SplitGrab(side, gi, upper, lower)))
        .on_double_click(Message::Dock(crate::ui::dock::DockMsg::SplitReset(side, gi)))
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
    .style(|theme: &Theme| {
        let palette = theme.palette();
        container::Style {
            background: Some(Background::Color(palette.primary.base.color)),
            text_color: Some(palette.primary.base.text),
            ..Default::default()
        }
    });
    container(column![header, Space::new()])
        .width(Length::Fixed(w.max(1.0)))
        .height(Length::Fixed(h.max(1.0)))
        .clip(true)
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(
                    palette.background.base.color.scale_alpha(0.85),
                )),
                border: Border {
                    color: palette.primary.base.color,
                    width: 2.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }
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
