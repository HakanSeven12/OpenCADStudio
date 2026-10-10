//! Dock state changes: drags, hover, open / close, and the queries the view
//! and drop targeting share.

use crate::app::{Message, OpenCADStudio};

/// Pointer travel (px) before pressing a title bar or icon becomes a drag,
/// so a plain click never moves anything.
const DOCK_DRAG_THRESHOLD: f32 = 5.0;

/// Resolves once the dock hover delay has passed (a sleeping helper thread;
/// the web build has none and resolves at once).
fn hover_delay() -> impl std::future::Future<Output = ()> {
    #[cfg(not(target_arch = "wasm32"))]
    let rx = {
        let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(crate::ui::dock::DOCK_HOVER_DELAY);
            let _ = tx.send(());
        });
        rx
    };
    async move {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = rx.await;
    }
}

impl OpenCADStudio {
    /// Dock chrome interaction (grab / pin / resize / hover / move) applied to
    /// whichever panel the message names.
    pub(crate) fn on_dock(&mut self, m: crate::ui::dock::DockMsg) -> iced::Task<Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockDrag, DockMsg, DropTarget, FloatPanel};
        match m {
            DockMsg::DockGrab(id) => self.dock_begin(DockDrag::moving(id, None), Some(id)),
            DockMsg::IconPress(id) => {
                // An icon press shows the icon's group and may also start
                // dragging the pallet; a plain click never passes the drag
                // threshold.
                if self.dock.show_group_of(id) {
                    self.save_config();
                }
                self.dock_begin(DockDrag::moving(id, None), Some(id));
            }
            DockMsg::GroupGrab(side, gi) => {
                if let Some(&id) = self.dock_group_visible(side, gi).first() {
                    self.dock_begin(DockDrag::moving(id, Some((side, gi))), Some(id));
                }
            }
            DockMsg::ResizeGrab(id) => self.dock_begin(DockDrag::Width(id), Some(id)),
            DockMsg::SplitGrab(side, group, upper, lower) => self.dock_begin(
                DockDrag::Split {
                    side,
                    group,
                    upper,
                    lower,
                },
                None,
            ),
            DockMsg::FloatResizeGrab(panel, from_left) => {
                self.dock.raise_float(panel);
                self.dock_begin(DockDrag::FloatSize { panel, from_left }, None);
            }
            DockMsg::SplitReset(side, gi) => {
                self.dock.reset_splits(side, gi);
                self.save_config();
            }
            DockMsg::WidthReset(id) => {
                if let Some((side, gi)) = self.dock.location(id) {
                    self.dock.reset_group_width(side, gi);
                } else {
                    self.dock.reset_width(id);
                }
                self.save_config();
            }
            DockMsg::TitleHover(id) => self.dock_title_hover = Some(id),
            DockMsg::TitleHoverEnd(id) => {
                if self.dock_title_hover == Some(id) {
                    self.dock_title_hover = None;
                }
            }
            DockMsg::GripHover(side, gi) => self.dock_grip_hover = Some((side, gi)),
            DockMsg::GripHoverEnd(side, gi) => {
                if self.dock_grip_hover == Some((side, gi)) {
                    self.dock_grip_hover = None;
                }
            }
            DockMsg::FloatRaise(id) => self.dock.raise_float(id),
            DockMsg::FloatOut(id) => {
                // Float beside the edge it was docked on, near the top.
                let (ww, _) = self.dock_workspace_size();
                let (w, h) = self.dock_float_size(id);
                let strip = crate::ui::dock::DOCK_STRIP_W;
                let x = match self.dock.location(id) {
                    Some((DockSide::Right, _)) => {
                        ww - strip - self.dock_column_width(DockSide::Right) - w - 60.0
                    }
                    _ => strip + self.dock_column_width(DockSide::Left) + 40.0,
                };
                self.dock_drag = None;
                if self.dock.float(FloatPanel::new(id, x.max(0.0), 40.0, w, h)) {
                    self.save_config();
                }
            }
            DockMsg::DockTo(id, side) => {
                self.dock_drag = None;
                self.dock_float_menu = None;
                if self.dock.dock(id, side, usize::MAX) {
                    self.save_config();
                }
            }
            DockMsg::FloatMenu(id) => {
                self.dock_float_menu = id;
                self.dock_float_menu_at = self.dock_float_pointer;
                if let Some(id) = id {
                    self.dock.raise_float(id);
                }
            }
            DockMsg::FloatBarPointer(p) => self.dock_float_pointer = p,
            DockMsg::FloatDockingToggle(id) => {
                self.dock_float_menu = None;
                if self.dock.toggle_float_docking(id) {
                    self.save_config();
                }
            }
            DockMsg::EdgeMenu(side) => self.dock_edge_menu = side,
            DockMsg::EdgeMenuToggle(side, id) => {
                // Checked = open and docked on this edge: unchecking hides it.
                // Anything else (closed, floating, on the other edge) moves
                // here and opens, in its own group here if it has one, else
                // as a new group; adding a pallet ends the menu's job.
                let here = self.dock.location(id).map(|(s, _)| s) == Some(side);
                if !here {
                    self.dock.dock(id, side, usize::MAX);
                } else if self.dock_panel_visible(id) {
                    let task = self.dock_set_open(id, false);
                    self.save_config();
                    return task;
                }
                self.dock_edge_menu = None;
                let task = self.dock_set_open(id, true);
                self.save_config();
                return task;
            }
            DockMsg::AutoCollapseToggle(id) => {
                // Docked: auto-hide for the whole edge; floating: this pallet.
                if self.dock.toggle_auto_hide(id) {
                    self.dock_peek = None;
                }
                self.save_config();
            }
            DockMsg::Close(id) => return self.dock_set_open(id, false),
            DockMsg::Hover(id) => {
                self.dock_icon_hover = Some(id);
                // Only an auto-hiding edge (or floating pallet) reacts to
                // hover: after a short rest it reveals the hovered pallet's
                // group. Otherwise groups switch on click. Ignored mid-drag,
                // when the pointer is over the preview rather than the strip.
                if self.dock_drag.is_none() && self.dock.auto_hides(id) {
                    if self.dock_peek == Some(id) {
                        self.dock_hover_cancel();
                    } else {
                        return self.dock_hover_arm(Some(id));
                    }
                }
            }
            DockMsg::HoverEnd(id) => {
                if self.dock_icon_hover == Some(id) {
                    self.dock_icon_hover = None;
                }
            }
            DockMsg::HoverExit => {
                if self.dock_drag.is_none()
                    && self.dock_peek.is_some_and(|id| self.dock.auto_hides(id))
                {
                    return self.dock_hover_arm(None);
                }
                // Left before a pending reveal ran out: it never happens.
                self.dock_hover_cancel();
            }
            DockMsg::HoverStay => {
                if self.dock_hover_pending == Some(None) {
                    self.dock_hover_cancel();
                }
            }
            DockMsg::HoverSettled(gen) => {
                if gen == self.dock_hover_gen && self.dock_drag.is_none() {
                    match self.dock_hover_pending.take() {
                        Some(Some(id)) if self.dock.auto_hides(id) => self.dock_reveal(id),
                        Some(None) if self.dock_peek.is_some_and(|id| self.dock.auto_hides(id)) => {
                            self.dock_peek = None;
                        }
                        _ => {}
                    }
                }
            }
            DockMsg::DragMove(point) => self.dock_drag_move(point),
            DockMsg::DragRelease => {
                let mut changed = false;
                match self.dock_drag.take() {
                    Some(DockDrag::Move {
                        group: Some((side, gi)),
                        target: Some(DropTarget::Edge { side: to, index }),
                        ..
                    }) => changed = self.dock.move_group(side, gi, to, index),
                    Some(DockDrag::Move {
                        panel,
                        group: None,
                        target: Some(target),
                        ..
                    }) => {
                        changed = match target {
                            DropTarget::Edge { side, index } => self.dock.dock(panel, side, index),
                            DropTarget::Join { side, group, index } => {
                                self.dock.join_group(panel, side, group, index)
                            }
                            DropTarget::Float { x, y } => {
                                let (w, h) = self.dock_float_size(panel);
                                // A floating panel keeps its docking choice.
                                let docking = self.dock.float_rect(panel).is_none_or(|f| f.docking);
                                self.dock.float(FloatPanel {
                                    docking,
                                    ..FloatPanel::new(panel, x, y, w, h)
                                })
                            }
                        };
                    }
                    Some(DockDrag::Move { .. } | DockDrag::LayerColumn) | None => {}
                    Some(DockDrag::Width(_) | DockDrag::Split { .. } | DockDrag::FloatSize { .. }) => {
                        changed = true;
                    }
                }
                if changed {
                    self.save_config();
                }
                self.dock_drag_last = None;
                self.xref_col_drag = None;
                self.xref_col_last = None;
                self.xref_split_drag = false;
            }
        }
        iced::Task::none()
    }

    /// Pointer motion while something in the dock is dragged.
    fn dock_drag_move(&mut self, point: iced::Point) {
        // NOTE: xref column drags intentionally do NOT ride this path: its
        // points live in workspace space while the header tracker reports
        // header-local points, and mixing the two produced a one-time jump
        // plus a stuck drag. Column moves arrive via Message::XrefColMove
        // only.
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockDrag, DropTarget};
        let last = self.dock_drag_last.replace(point);
        let Some(drag) = self.dock_drag else {
            return;
        };
        match drag {
            DockDrag::Move {
                panel,
                group,
                origin,
                target,
                grab,
            } => {
                let origin = origin.unwrap_or(point);
                let moved = (point.x - origin.x).hypot(point.y - origin.y);
                let started = target.is_some() || moved >= DOCK_DRAG_THRESHOLD;
                // Hold a floating panel where it was grabbed; a docked one by
                // its title bar.
                let grab = if target.is_none() && started {
                    match self.dock.float_rect(panel) {
                        Some(f) => iced::Vector::new(origin.x - f.x, origin.y - f.y),
                        None => iced::Vector::new(
                            (self.dock.width(panel, self.win_size.0) * 0.5).min(120.0),
                            14.0,
                        ),
                    }
                } else {
                    grab
                };
                let target = if started {
                    // A whole group only moves between group positions: it
                    // cannot join another group or float.
                    // A floating panel that may not dock only floats.
                    let found = match self.dock.float_rect(panel) {
                        Some(f) if !f.docking => self.dock_float_target(point, grab),
                        _ => self.dock_drop_target(point, grab),
                    };
                    match (group, found) {
                        (Some(_), found @ DropTarget::Edge { .. }) | (None, found) => Some(found),
                        (Some(_), _) => None,
                    }
                } else {
                    target
                };
                self.dock_drag = Some(DockDrag::Move {
                    panel,
                    group,
                    origin: Some(origin),
                    target,
                    grab,
                });
            }
            DockDrag::Split {
                side,
                group,
                upper,
                lower,
            } => {
                let Some(last) = last else { return };
                let Some(weights) = self.dock.groups(side).get(group).map(|g| &g.weights) else {
                    return;
                };
                let (_, avail) = self.dock_workspace_size();
                let total: f32 = self
                    .dock_slot_spans(side)
                    .iter()
                    .filter_map(|(i, _, _)| weights.get(*i))
                    .sum();
                if avail > 0.0 && total > 0.0 {
                    let per_px = total / avail;
                    self.dock.shift_split(
                        side,
                        group,
                        upper,
                        lower,
                        (point.y - last.y) * per_px,
                        crate::ui::dock::GROUP_MIN_H * per_px,
                    );
                }
            }
            DockDrag::FloatSize { panel, from_left } => {
                let (Some(last), Some(f)) = (last, self.dock.float_rect(panel)) else {
                    return;
                };
                let dx = point.x - last.x;
                self.dock.resize_float(
                    panel,
                    if from_left { f.w - dx } else { f.w + dx },
                    f.h + point.y - last.y,
                    from_left,
                );
            }
            DockDrag::LayerColumn => {
                let Some(last) = last else { return };
                self.layer_name_col_w =
                    (self.layer_name_col_w + point.x - last.x).clamp(60.0, 640.0);
            }
            DockDrag::Width(id) => {
                let Some(last) = last else { return };
                let dx = point.x - last.x;
                match self.dock.location(id) {
                    // The divider sizes the panel's group.
                    Some((side, gi)) => {
                        let delta = if side == DockSide::Left { dx } else { -dx };
                        let cur = self.dock_group_width(side, gi) + delta;
                        self.dock.set_group_width(side, gi, cur);
                    }
                    None => {
                        let cur = self.dock.settings(id).width + dx;
                        self.dock.set_width(id, cur);
                    }
                }
            }
        }
    }

    /// Start `drag`, peeking `peek` (a pallet being dragged or resized stays
    /// shown on an auto-hiding edge).
    fn dock_begin(&mut self, drag: crate::ui::dock::DockDrag, peek: Option<crate::ui::dock::PanelId>) {
        if let crate::ui::dock::DockDrag::Move { panel, .. } = drag {
            self.dock.raise_float(panel);
        }
        self.dock_drag = Some(drag);
        self.dock_drag_last = None;
        self.dock_edge_menu = None;
        self.dock_float_menu = None;
        if peek.is_some() {
            self.dock_peek = peek;
        }
    }

    /// Bring `id` into view: show its group on its edge and, where it
    /// auto-hides, keep it revealed until the pointer leaves.
    /// Arm a delayed reveal (`Some`) or hide (`None`); it applies when its
    /// timer reports back unless a newer hover superseded it.
    fn dock_hover_arm(&mut self, target: Option<crate::ui::dock::PanelId>) -> iced::Task<Message> {
        self.dock_hover_gen = self.dock_hover_gen.wrapping_add(1);
        self.dock_hover_pending = Some(target);
        let gen = self.dock_hover_gen;
        iced::Task::perform(hover_delay(), move |()| {
            Message::Dock(crate::ui::dock::DockMsg::HoverSettled(gen))
        })
    }

    /// Drop any pending delayed reveal / hide.
    fn dock_hover_cancel(&mut self) {
        self.dock_hover_gen = self.dock_hover_gen.wrapping_add(1);
        self.dock_hover_pending = None;
    }

    pub(crate) fn dock_reveal(&mut self, id: crate::ui::dock::PanelId) {
        self.dock.show_group_of(id);
        self.dock_peek = Some(id);
    }

    /// Open or close pallet `id`, running its usual open path (refreshes,
    /// ribbon highlight) or close path. An opened pallet is revealed where
    /// the layout has it.
    pub(crate) fn dock_set_open(
        &mut self,
        id: crate::ui::dock::PanelId,
        open: bool,
    ) -> iced::Task<Message> {
        use crate::ui::dock::{DockDrag, PanelId};
        let mut task = iced::Task::none();
        if open {
            if !self.dock_panel_visible(id) {
                match id {
                    PanelId::Properties => {
                        self.show_properties = true;
                        self.ribbon.set_properties(true);
                    }
                    PanelId::BlockPalette => self.open_blocks_palette(None),
                    PanelId::ExternalReferences => {
                        self.show_external_references = true;
                        self.refresh_xref_manager();
                    }
                    PanelId::Browser => self.show_browser = true,
                    PanelId::NodeGraph => {
                        task = self.on_graph(crate::ui::node_graph::GraphMsg::Toggle);
                    }
                    PanelId::PointCloudManager => self.pc_manager.show = true,
                    PanelId::Count => self.set_count_palette(true),
                    PanelId::SheetSetManager => self.show_sheet_set_manager(true),
                    PanelId::Layers => {
                        self.sync_ribbon_layers();
                        self.show_layers = true;
                    }
                }
            }
            self.dock_reveal(id);
            return task;
        }
        match id {
            PanelId::BlockPalette => {
                self.show_block_palette = false;
                self.block_palette.placing = None;
            }
            PanelId::ExternalReferences => self.show_external_references = false,
            PanelId::Browser => self.show_browser = false,
            PanelId::NodeGraph => self.show_node_graph = false,
            PanelId::PointCloudManager => self.pc_manager.show = false,
            PanelId::Count => {
                self.count_palette.show = false;
                self.ribbon.set_count_palette(false);
            }
            PanelId::SheetSetManager => {
                self.sheet_set.show = false;
                self.ribbon.set_sheet_set(false);
            }
            PanelId::Properties => {
                self.show_properties = false;
                self.ribbon.set_properties(false);
            }
            PanelId::Layers => {
                self.show_layers = false;
                self.ribbon.deactivate_tool_if("LAYERS");
            }
        }
        self.dock_unpeek(id);
        // A closed pallet cannot stay held by the pointer.
        let held = match self.dock_drag {
            Some(DockDrag::Move { panel, .. } | DockDrag::Width(panel)) => panel == id,
            _ => false,
        };
        if held {
            self.dock_drag = None;
        }
        task
    }

    /// Show `id` where the layout has it, first docking it on the right edge
    /// when it has no place yet (opened for the first time, or never moved).
    pub(crate) fn dock_open_at_default(&mut self, id: crate::ui::dock::PanelId) {
        if !self.dock.is_placed(id) {
            self.dock.dock(id, crate::app::config::DockSide::Right, usize::MAX);
        }
        self.dock_reveal(id);
    }

    /// Open the Layer Manager pallet (LAYERS). The first time it floats over
    /// the middle of the drawing, where the dialog it replaced used to open;
    /// after that it opens wherever the user left it.
    pub(crate) fn open_layers_pallet(&mut self) {
        use crate::ui::dock::{FloatPanel, PanelId};
        self.sync_ribbon_layers();
        self.show_layers = true;
        if !self.dock.is_placed(PanelId::Layers) {
            let (ww, wh) = self.dock_workspace_size();
            let (w, h) = self.dock_float_size(PanelId::Layers);
            let (x, y) = (((ww - w) * 0.5).max(0.0), ((wh - h) * 0.5).max(0.0));
            self.dock.float(FloatPanel::new(PanelId::Layers, x, y, w, h));
            self.save_config();
        }
        self.dock.raise_float(PanelId::Layers);
        self.dock_reveal(PanelId::Layers);
    }

    /// Stop revealing `id` (it closed).
    pub(crate) fn dock_unpeek(&mut self, id: crate::ui::dock::PanelId) {
        if self.dock_peek == Some(id) {
            self.dock_peek = None;
        }
    }

    /// Whether `id` is currently rendered (not closed, not on the start screen /
    /// clean-screen viewport). Mirrors the visibility filter the edge column
    /// renderer applies before building each side's stack.
    pub(crate) fn dock_panel_visible(&self, id: crate::ui::dock::PanelId) -> bool {
        use crate::ui::dock::PanelId;
        if self.tabs[self.active_tab].is_start || self.clean_screen {
            return false;
        }
        match id {
            PanelId::Properties => self.show_properties,
            PanelId::BlockPalette => self.show_block_palette,
            PanelId::ExternalReferences => self.show_external_references,
            PanelId::Browser => self.show_browser,
            PanelId::NodeGraph => self.show_node_graph,
            PanelId::PointCloudManager => self.pc_manager.show,
            PanelId::Count => self.count_palette.show,
            PanelId::SheetSetManager => self.sheet_set.show,
            PanelId::Layers => self.show_layers,
        }
    }

    /// Size of the workspace the dock lays out in: window width by viewport
    /// height.
    pub(crate) fn dock_workspace_size(&self) -> (f32, f32) {
        (
            self.win_size.0,
            self.tabs[self.active_tab].scene.selection.borrow().view.vp_size.1,
        )
    }

    /// The open pallets of group `gi` on `side`, top → bottom. Closed
    /// pallets keep their place but take no space, so a group with none open
    /// is not drawn at all.
    pub(crate) fn dock_group_visible(
        &self,
        side: crate::app::config::DockSide,
        gi: usize,
    ) -> Vec<crate::ui::dock::PanelId> {
        // A stale index (a message from before the layout changed) reads as
        // an empty group rather than a crash.
        self.dock
            .groups(side)
            .get(gi)
            .into_iter()
            .flat_map(|g| g.panels.iter().copied())
            .filter(|id| self.dock_panel_visible(*id))
            .collect()
    }

    /// Indices of the groups on `side` with an open pallet (listed in the
    /// icon strip).
    pub(crate) fn dock_visible_groups(&self, side: crate::app::config::DockSide) -> Vec<usize> {
        (0..self.dock.groups(side).len())
            .filter(|gi| !self.dock_group_visible(side, *gi).is_empty())
            .collect()
    }

    /// The group `side` draws: the one it shows, or the first with an open
    /// pallet when that one has none.
    pub(crate) fn dock_shown_group(&self, side: crate::app::config::DockSide) -> Option<usize> {
        let visible = self.dock_visible_groups(side);
        let shown = self.dock.shown(side);
        if visible.contains(&shown) {
            Some(shown)
        } else {
            visible.first().copied()
        }
    }

    /// Whether `side` draws its shown group beside the icon strip: always,
    /// unless the edge auto-hides and none of the group's icons is hovered.
    pub(crate) fn dock_edge_expanded(&self, side: crate::app::config::DockSide) -> bool {
        let Some(gi) = self.dock_shown_group(side) else {
            return false;
        };
        !self.dock.edge_auto_hide(side)
            || self
                .dock_peek
                .is_some_and(|e| self.dock_group_visible(side, gi).contains(&e))
    }

    /// On-screen width of group `gi` on `side`.
    pub(crate) fn dock_group_width(&self, side: crate::app::config::DockSide, gi: usize) -> f32 {
        self.dock.group_width_px(side, gi, self.win_size.0)
    }

    /// Width of the pallet column beside `side`'s icon strip: the shown
    /// group's width, or nothing when it is hidden.
    pub(crate) fn dock_column_width(&self, side: crate::app::config::DockSide) -> f32 {
        match self.dock_shown_group(side) {
            Some(gi) if self.dock_edge_expanded(side) => self.dock_group_width(side, gi),
            _ => 0.0,
        }
    }

    /// The icon strip of `side`: each listed group's place, and the top of
    /// the + button.
    pub(crate) fn dock_strip_layout(
        &self,
        side: crate::app::config::DockSide,
    ) -> (Vec<crate::ui::dock::StripGroup>, f32) {
        let groups: Vec<(usize, usize)> = self
            .dock_visible_groups(side)
            .into_iter()
            .map(|gi| (gi, self.dock_group_visible(side, gi).len()))
            .collect();
        crate::ui::dock::strip_layout(&groups)
    }

    /// The open pallets of `side`'s shown group, each as (position in the
    /// group, top, bottom) in the workspace.
    pub(crate) fn dock_slot_spans(
        &self,
        side: crate::app::config::DockSide,
    ) -> Vec<(usize, f32, f32)> {
        let Some(gi) = self.dock_shown_group(side) else {
            return Vec::new();
        };
        let (_, avail) = self.dock_workspace_size();
        let group = &self.dock.groups(side)[gi];
        let open: Vec<usize> = (0..group.panels.len())
            .filter(|i| self.dock_panel_visible(group.panels[*i]))
            .collect();
        let weights: Vec<f32> = open.iter().map(|i| group.weights[*i]).collect();
        open.into_iter()
            .zip(crate::ui::dock::slot_spans(&weights, avail))
            .map(|(i, (top, bottom))| (i, top, bottom))
            .collect()
    }

    /// Size a panel takes when floated: its current floating size, or its
    /// docked width by a comfortable share of the workspace height.
    pub(crate) fn dock_float_size(&self, id: crate::ui::dock::PanelId) -> (f32, f32) {
        if let Some(f) = self.dock.float_rect(id) {
            return (f.w, f.h);
        }
        let (_, avail) = self.dock_workspace_size();
        (
            self.dock.width(id, self.win_size.0),
            (avail * 0.6).clamp(crate::ui::dock::FLOAT_MIN_H, 520.0),
        )
    }

    /// Where a dragged pallet lands if released at workspace point `p`:
    /// on an edge's icon strip it joins a group between its icons or starts a
    /// new group; over the shown group's pallets it stacks above / below the
    /// one under the pointer; on an empty edge it starts a group; anywhere
    /// else it floats, held at `grab` from its top-left corner.
    pub(crate) fn dock_drop_target(
        &self,
        p: iced::Point,
        grab: iced::Vector,
    ) -> crate::ui::dock::DropTarget {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, StripHit, DOCK_EDGE_ZONE, DOCK_STRIP_W};
        let (ww, _) = self.dock_workspace_size();
        for side in [DockSide::Left, DockSide::Right] {
            let listed = !self.dock_visible_groups(side).is_empty();
            let from_edge = match side {
                DockSide::Left => p.x,
                DockSide::Right => ww - p.x,
            };
            if !listed {
                if from_edge <= DOCK_EDGE_ZONE {
                    return DropTarget::Edge {
                        side,
                        index: self.dock.groups(side).len(),
                    };
                }
                continue;
            }
            // Over the icon strip.
            if from_edge <= DOCK_STRIP_W {
                let (layout, _) = self.dock_strip_layout(side);
                return match crate::ui::dock::strip_hit(p.y, &layout) {
                    StripHit::NewGroup(at) => DropTarget::Edge {
                        side,
                        index: at.unwrap_or(self.dock.groups(side).len()),
                    },
                    StripHit::Join { group, index } => {
                        // Map the gap among open pallets to a position among
                        // all of the group's pallets.
                        let open = self.dock_group_visible(side, group);
                        let all = &self.dock.groups(side)[group].panels;
                        let index = open
                            .get(index)
                            .and_then(|t| all.iter().position(|a| a == t))
                            .unwrap_or(all.len());
                        DropTarget::Join { side, group, index }
                    }
                };
            }
            // Over the shown group's pallets: above or below the one there.
            if from_edge <= DOCK_STRIP_W + self.dock_column_width(side) {
                let gi = self.dock_shown_group(side).expect("listed edge");
                let spans = self.dock_slot_spans(side);
                if let Some(&(pos, top, bottom)) = spans
                    .iter()
                    .find(|(_, _, bottom)| p.y < *bottom)
                    .or(spans.last())
                {
                    let index = if p.y < (top + bottom) * 0.5 { pos } else { pos + 1 };
                    return DropTarget::Join {
                        side,
                        group: gi,
                        index,
                    };
                }
            }
        }
        self.dock_float_target(p, grab)
    }

    /// Floating at workspace point `p`, held at `grab` from its top-left
    /// corner (kept on the workspace).
    pub(crate) fn dock_float_target(
        &self,
        p: iced::Point,
        grab: iced::Vector,
    ) -> crate::ui::dock::DropTarget {
        let (ww, wh) = self.dock_workspace_size();
        let x = (p.x - grab.x).clamp(0.0, (ww - 60.0).max(0.0));
        let y = (p.y - grab.y).clamp(0.0, (wh - 30.0).max(0.0));
        crate::ui::dock::DropTarget::Float { x, y }
    }
}
