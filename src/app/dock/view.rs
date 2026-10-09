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
        // Floating panels sit over the workspace, back to front.
        let mut layers: Vec<Element<'_, Message>> = vec![workspace];
        for f in &self.dock.floating {
            // Once a drag is under way only its preview shows the panel.
            let dragged = self
                .dock_drag
                .and_then(|d| d.movement())
                .is_some_and(|(panel, _, target)| panel == f.id && target.is_some());
            if self.dock_panel_visible(f.id) && !dragged {
                layers.push(self.floating_panel(*f, tab));
            }
        }
        // An open pallet menu hangs beside its edge's + button, over a
        // catcher that closes it on any click elsewhere.
        if let Some(side) = self.dock_edge_menu {
            if !self.dock_visible_groups(side).is_empty() {
                // Checked = open and docked on this edge.
                let shown: Vec<_> = crate::ui::dock::PanelId::ALL
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
                        .on_press(Message::Dock(crate::ui::dock::DockMsg::EdgeMenu(None)))
                        .on_right_press(Message::Dock(crate::ui::dock::DockMsg::EdgeMenu(None)))
                        .into(),
                );
                layers.push(place_at(dock_edge_menu(side, &shown), x.max(0.0), plus_top));
            }
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
        let shown = self.dock_shown_group(side);
        let movement = self.dock_drag.and_then(|d| d.movement());
        let move_target = movement.and_then(|(_, _, target)| target);
        let dragging = move_target.is_some();
        let target_group = match move_target {
            Some(DropTarget::Join { side: s, group, .. }) if s == side => Some(group),
            _ => None,
        };
        let tip_side = match side {
            DockSide::Left => iced::widget::tooltip::Position::Right,
            DockSide::Right => iced::widget::tooltip::Position::Left,
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
                .on_enter(Message::Dock(DockMsg::Hover(Some(id))))
                .on_exit(Message::Dock(DockMsg::Hover(None)))
                .interaction(iced::mouse::Interaction::Pointer);
                icons.push(cell.into());
            }

            // The group grip: a band on the window side holding the edge bar.
            // Explicit heights: the tooltip around the band sizes it to its
            // content, so a `Fill` bar would stop short of the row's bottom.
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
            .on_enter(Message::Dock(DockMsg::GripHover(Some((side, gi)))))
            .on_exit(Message::Dock(DockMsg::GripHover(None)))
            .interaction(iced::mouse::Interaction::Grab);
            let band = iced::widget::tooltip(
                band,
                text(t!("Drag to move group")).size(10),
                tip_side,
            )
            .gap(4);
            let band = container(band)
                .width(Fill)
                .height(Length::Fixed(row_h))
                .align_x(match side {
                    DockSide::Left => iced::alignment::Horizontal::Left,
                    DockSide::Right => iced::alignment::Horizontal::Right,
                });
            items.push(
                container(stack![column(icons), band])
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

    /// A floating panel at its saved place: a vertical title bar on the side
    /// facing the nearer workspace edge, the panel beside it, and a corner
    /// grip to resize it. With its pin on, the panel hides down to the title
    /// bar until hovered.
    fn floating_panel<'a>(
        &'a self,
        f: crate::ui::dock::FloatPanel,
        tab: &'a DocumentTab,
    ) -> Element<'a, Message> {
        use crate::ui::dock::DockMsg;
        let (ww, wh) = self.dock_workspace_size();
        // The bar faces the nearer workspace edge; while resizing, it stays
        // opposite the grip being dragged so it cannot flip mid-drag.
        let bar_left = match self.dock_drag {
            Some(crate::ui::dock::DockDrag::FloatSize { panel, from_left }) if panel == f.id => {
                !from_left
            }
            _ => f.x + (f.w + DOCK_FLOAT_BAR_W) * 0.5 < ww * 0.5,
        };
        let hidden = self.dock.auto_collapse(f.id) && self.dock_peek != Some(f.id);
        let bar = floating_title_bar(f.id, self.dock.auto_collapse(f.id), bar_left);
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
            .on_press(Message::Dock(DockMsg::FloatResizeGrab(f.id, grip_left)))
            .interaction(if grip_left {
                iced::mouse::Interaction::ResizingDiagonallyUp
            } else {
                iced::mouse::Interaction::ResizingDiagonallyDown
            });
            let body = stack![
                self.panel_body(f.id, f.w, true, tab),
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
        let total_w = if hidden { DOCK_FLOAT_BAR_W } else { f.w + DOCK_FLOAT_BAR_W };
        let framed = container(content)
            .height(Length::Fixed(f.h))
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
        // The idle cursor marks the panel as opaque to the pointer, so clicks
        // on its empty areas don't fall through to the drawing below.
        let panel = mouse_area(framed)
            .on_press(Message::Dock(DockMsg::FloatRaise(f.id)))
            .on_enter(Message::Dock(DockMsg::Hover(Some(f.id))))
            .on_exit(Message::Dock(DockMsg::HoverExit))
            .interaction(iced::mouse::Interaction::Idle);
        // A hidden panel keeps its title bar where it sits when shown.
        let x = if hidden && !bar_left { f.x + f.w } else { f.x };
        // Keep the panel on screen when the window shrank since it was placed.
        let x = x.min(ww - total_w).max(0.0);
        let y = f.y.min(wh - f.h).max(0.0);
        place_at(panel.into(), x, y)
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
        let (ww, avail) = self.dock_workspace_size();
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
        let docked = match target {
            DropTarget::Edge { side, index } => {
                let mut after = self.dock.clone();
                after.dock(id, side, index);
                Some((side, after))
            }
            DropTarget::Join { side, group, index } => {
                let mut after = self.dock.clone();
                after.join_group(id, side, group, index);
                Some((side, after))
            }
            DropTarget::Float { x, y } => {
                let (w, h) = self.dock_float_size(id);
                layers.push(place_at(
                    dock_ghost(id, w + DOCK_FLOAT_BAR_W, h),
                    x,
                    y,
                ));
                None
            }
        };
        // A whole group shows only the strip feedback and its ghost; the
        // column keeps showing what is there.
        let group_drag = self.dock_drag.and_then(|d| d.movement()).and_then(|(_, g, _)| g);
        let docked = docked.map(|(side, after)| (side, group_drag.is_none().then_some(after)));
        let marks_side = docked.as_ref().map(|(side, _)| *side);
        if let Some((side, Some(after))) = docked {
            // Lay the group out as it will be after the drop.
            if let Some((_, gi)) = after.location(id) {
                let group = &after.groups(side)[gi];
                let open: Vec<usize> = (0..group.panels.len())
                    .filter(|i| group.panels[*i] == id || self.dock_panel_visible(group.panels[*i]))
                    .collect();
                let weights: Vec<f32> = open.iter().map(|i| group.weights[*i]).collect();
                let spans = crate::ui::dock::slot_spans(&weights, avail);
                let at = open
                    .iter()
                    .position(|i| group.panels[*i] == id)
                    .expect("dropped panel");
                let (top, bottom) = spans[at];
                let w = group
                    .panels
                    .iter()
                    .map(|p| after.width(*p, self.win_size.0))
                    .fold(0.0, f32::max);
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

/// Canvas that draws a label rotated 90° (for a collapsed panel's bar).
struct VBarLabel {
    text: String,
    clockwise: bool,
    /// Start the text at the reading-start end of the bar instead of
    /// centring it.
    from_start: bool,
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
            frame.rotate(iced::Radians(if self.clockwise {
                std::f32::consts::FRAC_PI_2
            } else {
                -std::f32::consts::FRAC_PI_2
            }));
            // After the rotation the text runs along local +x, so the
            // reading start sits at -height/2.
            let (position, align_x) = if self.from_start {
                (
                    iced::Point::new(-bounds.height / 2.0, 0.0),
                    iced::advanced::text::Alignment::Left,
                )
            } else {
                (iced::Point::ORIGIN, iced::advanced::text::Alignment::Center)
            };
            frame.fill_text(canvas::Text {
                content: self.text.clone(),
                position,
                color: theme.palette().background.base.text.scale_alpha(0.72),
                size: iced::Pixels(13.0),
                align_x,
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

/// A floating panel's vertical title bar: close then the hide (auto-collapse)
/// pin at the top, the panel's icon at the bottom with its title reading
/// upward above it. Pressing the bar's free area drags the panel.
fn floating_title_bar(
    id: crate::ui::dock::PanelId,
    auto_collapse: bool,
    on_left: bool,
) -> Element<'static, Message> {
    use crate::ui::dock::DockMsg;
    let tip = if on_left {
        iced::widget::tooltip::Position::Left
    } else {
        iced::widget::tooltip::Position::Right
    };
    let close = crate::ui::dock::close_button(id, tip);
    let pin = crate::ui::dock::pin_button(id, auto_collapse, tip);
    let title = canvas(VBarLabel {
        text: id.title().to_string(),
        clockwise: false,
        from_start: true,
    })
    .width(Fill)
    .height(Fill);
    let bar = column![
        close,
        pin,
        container(title).width(Fill).height(Fill).padding([6, 0]),
        container(crate::ui::icons::themed(id.icon(), 16.0)).center_x(Fill),
    ]
    .spacing(2)
    .padding([4, 0])
    .align_x(iced::Center)
    .width(Length::Fixed(DOCK_FLOAT_BAR_W))
    .height(Fill);
    mouse_area(container(bar).style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        ..Default::default()
    }))
    .on_press(Message::Dock(DockMsg::DockGrab(id)))
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

/// Grabbable separator for a docked panel managed by the general dock. Same
/// visual as the previous per-panel divider but emits generic dock messages.
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

/// The pallet menu of an edge's + button, styled like the right-click menu:
/// one row per pallet with its icon, its name and a check mark when it is
/// open on this edge. `shown` holds each pallet with that state.
fn dock_edge_menu(
    side: crate::app::config::DockSide,
    shown: &[(crate::ui::dock::PanelId, bool)],
) -> Element<'static, Message> {
    use crate::ui::dock::DockMsg;
    let rows: Vec<Element<'static, Message>> = shown
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
                    text(id.title()).size(12).width(Fill),
                    check,
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .on_press(Message::Dock(DockMsg::EdgeMenuToggle(side, id)))
            .style(|theme: &Theme, status| {
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
            })
            .padding([5, 8])
            .width(Fill)
            .into()
        })
        .collect();
    container(column(rows).spacing(1).padding(4))
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
        text(id.title())
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
