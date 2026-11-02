//! Dock behaviour through the app: drags, hover, menus, and the real view.

use crate::app::{Message, OpenCADStudio};

fn fresh() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    // These tests drive dock controls, not the hardware warning dialog.
    // Software snapshots intentionally have no scene GPU adapter.
    app.gpu_warning_silenced = crate::scene::pipeline::gpu_status()
        .identity()
        .or_else(|| crate::scene::pipeline::GpuStatus::NoRenderer.identity())
        .unwrap();
    app.pending_startup_modals
        .retain(|modal| *modal != crate::app::ModalKind::GpuWarning);
    if app.active_modal == Some(crate::app::ModalKind::GpuWarning) {
        app.active_modal = None;
    }
    app
}

#[test]
fn blockpalette_pin_toggles_autocollapse_and_close_hides() {
    let mut app = fresh();
    app.show_block_palette = true;
    let id = crate::ui::dock::PanelId::BlockPalette;
    let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
    assert!(app.dock.auto_hides(id), "pin enables auto-hide");
    let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
    assert!(!app.dock.auto_hides(id), "second pin disables auto-hide");
    let _ = app.on_dock(crate::ui::dock::DockMsg::Close(id));
    assert!(!app.show_block_palette, "close dismisses the sidebar");
}

/// A hermetic app with a 1600×900 workspace and the default dock layout
/// (Properties left, block palette right), both shown.
/// Run `f` on a thread with the stack the application's own threads get:
/// building and driving the real view is deeper than a default test thread.
/// One at a time: two headless renderers made in parallel crash the process.
fn with_stack(f: impl FnOnce() + Send + 'static) {
    let _one = crate::headless_ui_lock();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
}

fn dock_app() -> OpenCADStudio {
    let mut app = fresh();
    // Tests load the user's persisted config; reset the dock to a known
    // state so this stays hermetic.
    app.dock = Default::default();
    app.dock.ensure_settings();
    app.show_properties = true;
    app.show_block_palette = true;
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 900.0);
    app.win_size = (1600.0, 900.0).into();
    app
}

/// Where the current dock drag would land, if it has started.
fn drag_target(app: &OpenCADStudio) -> Option<crate::ui::dock::DropTarget> {
    app.dock_drag
        .and_then(|d| d.movement())
        .and_then(|(_, _, target)| target)
}

/// Press `id`'s title bar at `from`, drag to `to`, and return the target
/// shown just before release.
fn drag_panel(
    app: &mut OpenCADStudio,
    id: crate::ui::dock::PanelId,
    from: iced::Point,
    to: iced::Point,
) -> Option<crate::ui::dock::DropTarget> {
    use crate::ui::dock::DockMsg;
    let _ = app.on_dock(DockMsg::DockGrab(id));
    let _ = app.on_dock(DockMsg::DragMove(from));
    let _ = app.on_dock(DockMsg::DragMove(to));
    let target = drag_target(app);
    let _ = app.on_dock(DockMsg::DragRelease);
    target
}

#[test]
fn dropping_on_a_pallets_top_or_bottom_half_stacks_above_or_below_it() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DropTarget, PanelId};
    let mut app = dock_app();
    let id = PanelId::BlockPalette;
    assert_eq!(app.dock.location(id), Some((DockSide::Right, 0)));
    let target = drag_panel(
        &mut app,
        id,
        iced::Point::new(1500.0, 10.0),
        iced::Point::new(150.0, 300.0),
    );
    assert_eq!(
        target,
        Some(DropTarget::Join {
            side: DockSide::Left,
            group: 0,
            index: 0
        })
    );
    assert_eq!(
        app.dock.left[0].panels,
        vec![PanelId::BlockPalette, PanelId::Properties]
    );
    assert!(app.dock.right.is_empty());
    // The bottom half of a pallet stacks below it.
    let target = drag_panel(
        &mut app,
        id,
        iced::Point::new(150.0, 100.0),
        iced::Point::new(150.0, 800.0),
    );
    assert_eq!(
        target,
        Some(DropTarget::Join {
            side: DockSide::Left,
            group: 0,
            index: 2
        })
    );
    assert_eq!(
        app.dock.left[0].panels,
        vec![PanelId::Properties, PanelId::BlockPalette]
    );
}

#[test]
fn dock_dragging_a_lone_pallet_over_its_own_group_is_a_no_op() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DropTarget, PanelId};
    let mut app = dock_app();
    let before = app.dock.clone();
    // The right edge's only group holds just the block palette; drag it
    // around over itself.
    let target = drag_panel(
        &mut app,
        PanelId::BlockPalette,
        iced::Point::new(1500.0, 10.0),
        iced::Point::new(1450.0, 600.0),
    );
    assert_eq!(
        target,
        Some(DropTarget::Join {
            side: DockSide::Right,
            group: 0,
            index: 1
        })
    );
    assert_eq!(app.dock, before);
}

#[test]
fn dock_click_without_drag_changes_nothing() {
    let mut app = dock_app();
    let before = app.dock.clone();
    let id = crate::ui::dock::PanelId::BlockPalette;
    let p = iced::Point::new(1500.0, 10.0);
    let target = drag_panel(&mut app, id, p, iced::Point::new(1502.0, 11.0));
    assert_eq!(target, None, "movement under the threshold is a click");
    assert_eq!(app.dock, before);
}

#[test]
fn dock_drop_below_the_strip_groups_starts_a_new_shown_group() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DropTarget, PanelId};
    let mut app = dock_app();
    let (_, plus_top) = app.dock_strip_layout(DockSide::Left);
    let target = drag_panel(
        &mut app,
        PanelId::BlockPalette,
        iced::Point::new(1500.0, 10.0),
        iced::Point::new(18.0, plus_top + 10.0),
    );
    assert_eq!(
        target,
        Some(DropTarget::Edge {
            side: DockSide::Left,
            index: 1
        })
    );
    assert_eq!(app.dock.left.len(), 2);
    assert_eq!(app.dock.left[1].panels, vec![PanelId::BlockPalette]);
    // The new group is the one shown: the column holds only the palette.
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
    assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(0, 0.0, 900.0)]);
}

#[test]
fn dock_icon_press_switches_the_shown_group() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.show_browser = true;
    app.dock.left = vec![
        DockGroup::stack(vec![PanelId::Properties, PanelId::BlockPalette]),
        DockGroup::single(PanelId::Browser),
    ];
    app.dock.right.clear();
    // Group 0 shows both stacked pallets.
    assert_eq!(app.dock_slot_spans(DockSide::Left).len(), 2);
    let _ = app.on_dock(DockMsg::IconPress(PanelId::Browser));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
    assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(0, 0.0, 900.0)]);
    // Each group keeps its own width.
    app.dock.set_group_width(DockSide::Left, 1, 400.0);
    assert_eq!(app.dock_column_width(DockSide::Left), 400.0);
    let _ = app.on_dock(DockMsg::IconPress(PanelId::BlockPalette));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock_column_width(DockSide::Left), 260.0);
}

#[test]
fn dock_drop_over_the_viewport_floats_the_panel() {
    use crate::ui::dock::{DropTarget, PanelId};
    let mut app = dock_app();
    let target = drag_panel(
        &mut app,
        PanelId::BlockPalette,
        iced::Point::new(1500.0, 10.0),
        iced::Point::new(800.0, 400.0),
    );
    assert!(matches!(target, Some(DropTarget::Float { .. })));
    assert!(app.dock.right.is_empty());
    let f = app
        .dock
        .float_rect(PanelId::BlockPalette)
        .expect("floating");
    assert!(f.x > 600.0 && f.x < 800.0 && f.y > 350.0 && f.y < 400.0);
    // Dragging a floating panel keeps the grab point under the pointer.
    let target = drag_panel(
        &mut app,
        PanelId::BlockPalette,
        iced::Point::new(f.x + 20.0, f.y + 10.0),
        iced::Point::new(f.x + 120.0, f.y + 60.0),
    );
    assert_eq!(
        target,
        Some(DropTarget::Float {
            x: f.x + 100.0,
            y: f.y + 50.0,
            bar: crate::app::config::DockSide::Right,
        })
    );
    // Opening a floating panel must not re-dock it.
    assert!(app.dock.is_placed(PanelId::BlockPalette));
}

#[test]
fn dock_dragging_an_icon_into_another_group_lands_between_its_icons() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, DropTarget, PanelId, STRIP_CELL_H};
    let mut app = dock_app();
    app.show_browser = true;
    app.dock.left = vec![
        DockGroup::stack(vec![PanelId::Properties, PanelId::Browser]),
        DockGroup::single(PanelId::BlockPalette),
    ];
    app.dock.right.clear();
    let (layout, _) = app.dock_strip_layout(DockSide::Left);
    // Drag the Blocks icon (group 1) to between Properties and Browser.
    let _ = app.on_dock(DockMsg::IconPress(PanelId::BlockPalette));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(
        20.0,
        layout[1].icons_top + 10.0,
    )));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(
        20.0,
        layout[0].icons_top + STRIP_CELL_H + 2.0,
    )));
    assert_eq!(
        drag_target(&app),
        Some(DropTarget::Join {
            side: DockSide::Left,
            group: 0,
            index: 1
        })
    );
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock.left.len(), 1, "the emptied group goes away");
    assert_eq!(
        app.dock.left[0].panels,
        vec![PanelId::Properties, PanelId::BlockPalette, PanelId::Browser]
    );
}

#[test]
fn dock_grip_drag_moves_a_whole_group() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.show_browser = true;
    app.dock.left = vec![
        DockGroup::stack(vec![PanelId::Properties, PanelId::Browser]),
        DockGroup::single(PanelId::BlockPalette),
    ];
    app.dock.right.clear();
    let (layout, plus_top) = app.dock_strip_layout(DockSide::Left);
    let _ = app.on_dock(DockMsg::GroupGrab(DockSide::Left, 0));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(
        18.0,
        layout[0].top + 2.0,
    )));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, plus_top + 10.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock.left[0].panels, vec![PanelId::BlockPalette]);
    assert_eq!(
        app.dock.left[1].panels,
        vec![PanelId::Properties, PanelId::Browser]
    );
    // A group dropped over the viewport does not float or split up.
    let before = app.dock.clone();
    let _ = app.on_dock(DockMsg::GroupGrab(DockSide::Left, 1));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, 300.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(800.0, 400.0)));
    assert_eq!(drag_target(&app), None);
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock, before);
}

#[test]
fn dock_closed_pallets_take_no_room_and_empty_groups_hide() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, PanelId};
    let mut app = dock_app();
    app.dock.left = vec![
        DockGroup::stack(vec![PanelId::BlockPalette, PanelId::Properties]),
        DockGroup::single(PanelId::Browser),
    ];
    app.show_block_palette = false;
    app.show_browser = false;
    // Only Properties is open: one listed group, full height.
    assert_eq!(app.dock_visible_groups(DockSide::Left), vec![0]);
    assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(1, 0.0, 900.0)]);
    // A shown group with nothing open falls back to one that has.
    app.dock.show_group(DockSide::Left, 1);
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(0));
}

#[test]
fn dock_splitter_drag_changes_pallet_heights() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.dock.left = vec![DockGroup::stack(vec![
        PanelId::BlockPalette,
        PanelId::Properties,
    ])];
    app.dock.right.clear();
    let _ = app.on_dock(DockMsg::SplitGrab(DockSide::Left, 0, 0, 1));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 450.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 675.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    // 225 px of a 900 px edge holding 2 weight units = 0.5 units.
    let w = &app.dock.left[0].weights;
    assert!((w[0] - 1.5).abs() < 1e-4 && (w[1] - 0.5).abs() < 1e-4);
    let _ = app.on_dock(DockMsg::SplitReset(DockSide::Left, 0));
    assert_eq!(app.dock.left[0].weights, vec![1.0, 1.0]);
}

#[test]
fn dock_hover_reveals_groups_only_on_an_auto_hiding_edge() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.show_browser = true;
    app.dock.left.push(DockGroup::single(PanelId::Browser));
    // The pin on any docked pallet turns auto-hide on for its edge.
    let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::Properties));
    assert!(app.dock.edge_auto_hide(DockSide::Left));
    assert!(!app.dock.edge_auto_hide(DockSide::Right));
    assert!(!app.dock_edge_expanded(DockSide::Left));
    // Hovering an icon flies its group out.
    let _ = app.on_dock(DockMsg::Hover(PanelId::Browser));
    // ...but only once the pointer rests: the reveal waits for the delay.
    assert!(!app.dock_edge_expanded(DockSide::Left));
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert!(app.dock_edge_expanded(DockSide::Left));
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
    // Leaving and coming back within the delay keeps it open.
    let _ = app.on_dock(DockMsg::HoverExit);
    let stale = app.dock_hover_gen;
    let _ = app.on_dock(DockMsg::HoverStay);
    let _ = app.on_dock(DockMsg::HoverSettled(stale));
    assert!(app.dock_edge_expanded(DockSide::Left));
    let _ = app.on_dock(DockMsg::HoverExit);
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert!(!app.dock_edge_expanded(DockSide::Left));
    // Without auto-hide, hovering does not switch groups (clicks do).
    let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::Properties));
    let _ = app.on_dock(DockMsg::Hover(PanelId::Properties));
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
    assert!(app.dock_edge_expanded(DockSide::Left));
}

#[test]
fn dock_auto_hide_waits_for_an_open_layer_color_dropdown() {
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.dock.left.push(DockGroup::single(PanelId::Layers));
    let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::Layers));
    let _ = app.on_dock(DockMsg::Hover(PanelId::Layers));
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert_eq!(app.dock_peek, Some(PanelId::Layers));
    // Opening a row's colour swatch, then moving onto its dropdown, reads
    // as the pointer leaving the edge: the panel must stay out.
    let _ = app.update(Message::LayerColorPickerToggle("0".to_string()));
    assert!(app.tabs[app.active_tab].layers.color_picker_row.is_some());
    let _ = app.on_dock(DockMsg::HoverExit);
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert_eq!(app.dock_peek, Some(PanelId::Layers));
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert_eq!(app.dock_peek, Some(PanelId::Layers));
    // Once the dropdown closes with the pointer still away, it hides.
    let _ = app.update(Message::LayerColorPickerToggle("0".to_string()));
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert_eq!(app.dock_peek, None);
}

#[test]
fn dock_float_resize_grows_the_panel() {
    use crate::ui::dock::{DockMsg, FloatEdge, FloatPanel, PanelId};
    let mut app = dock_app();
    app.dock.float(FloatPanel::new(
        PanelId::BlockPalette,
        100.0,
        100.0,
        260.0,
        300.0,
    ));
    let resize = |app: &mut OpenCADStudio, edge: FloatEdge, from: (f32, f32), to: (f32, f32)| {
        let _ = app.on_dock(DockMsg::FloatResizeGrab(PanelId::BlockPalette, edge));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(from.0, from.1)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(to.0, to.1)));
        let _ = app.on_dock(DockMsg::DragRelease);
        app.dock.float_rect(PanelId::BlockPalette).unwrap()
    };
    let f = resize(&mut app, FloatEdge::SE, (360.0, 400.0), (400.0, 450.0));
    assert_eq!((f.x, f.y, f.w, f.h), (100.0, 100.0, 300.0, 350.0));
    // The bottom-left corner grows the panel leftward, keeping its right edge.
    let f = resize(&mut app, FloatEdge::SW, (100.0, 450.0), (60.0, 450.0));
    assert_eq!((f.x, f.w, f.h), (60.0, 340.0, 350.0));
    // The top side grows it upward, keeping its bottom edge.
    let f = resize(&mut app, FloatEdge::N, (200.0, 100.0), (200.0, 70.0));
    assert_eq!((f.y, f.h), (70.0, 380.0));
    // The right side only changes the width.
    let f = resize(&mut app, FloatEdge::E, (428.0, 200.0), (408.0, 260.0));
    assert_eq!((f.x, f.y, f.w, f.h), (60.0, 70.0, 320.0, 380.0));
    // Past the minimum the size stops, and going back resumes from the
    // pointer, not from where the panel stopped.
    let _ = app.on_dock(DockMsg::FloatResizeGrab(
        PanelId::BlockPalette,
        FloatEdge::E,
    ));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(408.0, 200.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 200.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(398.0, 200.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert_eq!(f.w, 310.0);
}

#[test]
fn dock_and_window_changes_set_the_viewport_size_before_layout() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    let i = app.active_tab;
    let vp = |app: &OpenCADStudio| app.tabs[i].scene.selection.borrow().view.vp_size;
    let room = |app: &OpenCADStudio| {
        let (l, r) = app.dock_edge_insets();
        l + r
    };
    let _ = app.update(Message::Dock(DockMsg::HoverExit));
    // Sizing a docked group's width: the viewport loses what the group gains.
    let (w0, h0) = vp(&app);
    let room0 = room(&app);
    let _ = app.update(Message::Dock(DockMsg::ResizeGrab(PanelId::BlockPalette)));
    let _ = app.update(Message::Dock(DockMsg::DragMove(iced::Point::new(
        1300.0, 300.0,
    ))));
    let _ = app.update(Message::Dock(DockMsg::DragMove(iced::Point::new(
        1240.0, 300.0,
    ))));
    assert_eq!(room(&app) - room0, 60.0);
    assert_eq!(vp(&app), (w0 - 60.0, h0));
    let _ = app.update(Message::Dock(DockMsg::DragRelease));
    // Auto-hide on the left: the viewport gains the group's width.
    let (w1, _) = vp(&app);
    let room1 = room(&app);
    let _ = app.update(Message::Dock(DockMsg::AutoCollapseToggle(
        PanelId::Properties,
    )));
    assert!(app.dock.edge_auto_hide(DockSide::Left));
    assert_eq!(vp(&app), (w1 + room1 - room(&app), h0));
    // A window resize changes it by as much as the window (no pan there).
    let (w2, h2) = vp(&app);
    let target = app.tabs[i].scene.camera.borrow().target;
    let _ = app.update(Message::WindowResized(1500.0, 850.0));
    assert_eq!(vp(&app), (w2 - 100.0, h2 - 50.0));
    assert_eq!(app.tabs[i].scene.camera.borrow().target, target);
}

#[test]
fn floating_title_bar_starts_on_the_docked_side() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::Properties));
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::BlockPalette));
    let left = app.dock.float_rect(PanelId::Properties).unwrap();
    let right = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert_eq!(left.bar, DockSide::Left);
    assert_eq!(right.bar, DockSide::Right);
}

#[test]
fn floating_title_bar_only_flips_against_docked_pallets() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    // Float the block palette out of the right edge: its bar is on the right.
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::BlockPalette));
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert_eq!(f.bar, DockSide::Right);
    // Across the middle of the screen it keeps its side.
    let grab = iced::Point::new(f.x + 100.0, f.y + 10.0);
    drag_panel(
        &mut app,
        PanelId::BlockPalette,
        grab,
        iced::Point::new(500.0, 300.0),
    );
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert!(f.x + f.w < 800.0, "moved left of the middle");
    assert_eq!(f.bar, DockSide::Right);
    // Against the docked Properties column (strip + 250 px) it flips.
    let grab = iced::Point::new(f.x + 100.0, f.y + 10.0);
    drag_panel(
        &mut app,
        PanelId::BlockPalette,
        grab,
        iced::Point::new(380.0, 300.0),
    );
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert!(f.x <= 290.0, "frame touches the docked column: {}", f.x);
    assert_eq!(f.bar, DockSide::Left);
    // Back in the open it stays on the left.
    let grab = iced::Point::new(f.x + 100.0, f.y + 10.0);
    drag_panel(
        &mut app,
        PanelId::BlockPalette,
        grab,
        iced::Point::new(1000.0, 300.0),
    );
    assert_eq!(
        app.dock.float_rect(PanelId::BlockPalette).unwrap().bar,
        DockSide::Left
    );
}

#[test]
fn floating_title_bar_without_docked_pallets_flips_at_the_screen_edge() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, DropTarget, FloatPanel, PanelId};
    let mut app = dock_app();
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::Properties));
    // Nothing is docked on the left any more.
    assert!(app.dock_shown_group(DockSide::Left).is_none());
    app.dock.float(FloatPanel {
        bar: DockSide::Right,
        ..FloatPanel::new(PanelId::Properties, 400.0, 100.0, 250.0, 300.0)
    });
    // Near the left edge, not touching it: no flip.
    let target = drag_panel(
        &mut app,
        PanelId::Properties,
        iced::Point::new(500.0, 110.0),
        iced::Point::new(150.0, 110.0),
    );
    assert!(
        matches!(target, Some(DropTarget::Float { x, bar: DockSide::Right, .. }) if x == 50.0),
        "{target:?}"
    );
    // The frame touching the screen's left edge flips it, before the
    // pointer reaches the edge zone (no drop preview yet).
    let target = drag_panel(
        &mut app,
        PanelId::Properties,
        iced::Point::new(150.0, 110.0),
        iced::Point::new(90.0, 110.0),
    );
    assert!(
        matches!(target, Some(DropTarget::Float { x, bar: DockSide::Left, .. }) if x == 0.0),
        "{target:?}"
    );
    // Against the right edge's docked pallets it flips back.
    let target = drag_panel(
        &mut app,
        PanelId::Properties,
        iced::Point::new(100.0, 110.0),
        iced::Point::new(1150.0, 110.0),
    );
    assert!(
        matches!(
            target,
            Some(DropTarget::Float {
                bar: DockSide::Right,
                ..
            })
        ),
        "{target:?}"
    );
    app.dock.float(FloatPanel {
        bar: DockSide::Right,
        ..FloatPanel::new(PanelId::Properties, 0.0, 100.0, 250.0, 300.0)
    });
    // The pointer in the edge zone shows the preview; backing out of it
    // keeps the flipped side.
    let _ = app.on_dock(DockMsg::DockGrab(PanelId::Properties));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 110.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(20.0, 110.0)));
    assert!(matches!(
        drag_target(&app),
        Some(DropTarget::Edge {
            side: DockSide::Left,
            ..
        })
    ));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(300.0, 110.0)));
    assert!(matches!(
        drag_target(&app),
        Some(DropTarget::Float {
            bar: DockSide::Left,
            ..
        })
    ));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(
        app.dock.float_rect(PanelId::Properties).unwrap().bar,
        DockSide::Left
    );
}

#[test]
fn floating_title_bar_without_docking_flips_at_the_screen_edge() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{FloatPanel, PanelId};
    let mut app = dock_app();
    app.dock.float(FloatPanel {
        docking: false,
        ..FloatPanel::new(PanelId::Count, 600.0, 100.0, 280.0, 300.0)
    });
    app.count_palette.show = true;
    // Against the docked Properties column it does not flip...
    drag_panel(
        &mut app,
        PanelId::Count,
        iced::Point::new(700.0, 110.0),
        iced::Point::new(350.0, 110.0),
    );
    let f = app.dock.float_rect(PanelId::Count).unwrap();
    assert!(f.x > 0.0 && f.x < 290.0);
    assert_eq!(f.bar, DockSide::Left);
    // ...nor at the right docked column; only at the screen edge.
    drag_panel(
        &mut app,
        PanelId::Count,
        iced::Point::new(f.x + 100.0, 110.0),
        iced::Point::new(1300.0, 110.0),
    );
    let f = app.dock.float_rect(PanelId::Count).unwrap();
    assert!(f.x + f.w + crate::ui::dock::DOCK_FLOAT_BAR_W < 1600.0);
    assert_eq!(f.bar, DockSide::Left);
    drag_panel(
        &mut app,
        PanelId::Count,
        iced::Point::new(f.x + 100.0, 110.0),
        iced::Point::new(1590.0, 110.0),
    );
    assert_eq!(
        app.dock.float_rect(PanelId::Count).unwrap().bar,
        DockSide::Right
    );
}

/// The screen x (in the window) of the model camera's target, measured
/// against the current docked edges.
fn target_screen_x(app: &OpenCADStudio, probe: glam::DVec3) -> f64 {
    let i = app.active_tab;
    let cam = app.tabs[i].scene.camera.borrow();
    let (left, right) = app.dock_edge_insets();
    let vw = f64::from(app.win_size.0 - left - right);
    let wpp = f64::from(2.0 * cam.ortho_size()) / 900.0;
    let cam_right = (cam.rotation * glam::Vec3::X).as_dvec3();
    f64::from(left) + vw * 0.5 + (probe - cam.target).dot(cam_right) / wpp
}

#[test]
fn docking_or_auto_hiding_keeps_the_drawing_in_place() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    let _ = app.update(Message::Dock(DockMsg::HoverExit));
    let probe =
        app.tabs[app.active_tab].scene.camera.borrow().target + glam::DVec3::new(3.0, 0.0, 0.0);
    let at = target_screen_x(&app, probe);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-3;
    // Auto-hide on the left: the edge shrinks to its strip.
    let before = app.dock_edge_insets();
    let _ = app.update(Message::Dock(DockMsg::AutoCollapseToggle(
        PanelId::Properties,
    )));
    assert!(app.dock.edge_auto_hide(DockSide::Left));
    assert_ne!(app.dock_edge_insets(), before);
    assert!(close(target_screen_x(&app, probe), at));
    // Revealing the auto-hidden group draws over the drawing: no change.
    let insets = app.dock_edge_insets();
    let _ = app.update(Message::Dock(DockMsg::Hover(PanelId::Properties)));
    let _ = app.update(Message::Dock(DockMsg::HoverSettled(app.dock_hover_gen)));
    assert!(app.dock_edge_expanded(DockSide::Left));
    assert_eq!(app.dock_edge_insets(), insets);
    // Floating and docking the right pallet.
    let _ = app.update(Message::Dock(DockMsg::FloatOut(PanelId::BlockPalette)));
    assert_eq!(app.dock_edge_insets().1, 0.0);
    assert!(close(target_screen_x(&app, probe), at));
    let _ = app.update(Message::Dock(DockMsg::DockTo(
        PanelId::BlockPalette,
        DockSide::Right,
    )));
    assert!(app.dock_edge_insets().1 > 0.0);
    assert!(close(target_screen_x(&app, probe), at));
    // Sizing a docked group's width.
    let _ = app.update(Message::Dock(DockMsg::ResizeGrab(PanelId::BlockPalette)));
    let _ = app.update(Message::Dock(DockMsg::DragMove(iced::Point::new(
        1300.0, 300.0,
    ))));
    let _ = app.update(Message::Dock(DockMsg::DragMove(iced::Point::new(
        1250.0, 300.0,
    ))));
    let _ = app.update(Message::Dock(DockMsg::DragRelease));
    assert!(close(target_screen_x(&app, probe), at));
}

#[test]
fn dock_edge_menu_adds_and_hides_pallets() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    app.show_browser = false;
    let _ = app.on_dock(DockMsg::EdgeMenu(Some(DockSide::Left)));
    // Checking a closed pallet opens it as a new, shown group here.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
    assert!(app.show_browser);
    assert_eq!(app.dock.left.len(), 2);
    assert_eq!(app.dock.left[1].panels, vec![PanelId::Browser]);
    assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
    assert_eq!(app.dock_edge_menu, None, "the menu closes after adding");
    let _ = app.on_dock(DockMsg::EdgeMenu(Some(DockSide::Left)));
    // Unchecking hides it but keeps its place.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
    assert!(!app.show_browser);
    assert_eq!(
        app.dock.location(PanelId::Browser),
        Some((DockSide::Left, 1))
    );
    // Opening it again from the same edge reuses that place.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
    assert_eq!(app.dock.left.len(), 2);
    // A pallet open on the other edge is not checked here: picking it
    // moves it over instead of hiding it.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(
        DockSide::Left,
        PanelId::BlockPalette,
    ));
    assert!(app.show_block_palette);
    assert!(app.dock.right.is_empty());
    assert_eq!(
        app.dock.location(PanelId::BlockPalette),
        Some((DockSide::Left, 2))
    );
}

#[test]
fn dock_divider_resizes_only_its_group() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockGroup, DockMsg, PanelId};
    let mut app = dock_app();
    app.dock.left = vec![
        DockGroup::stack(vec![PanelId::Properties, PanelId::BlockPalette]),
        DockGroup::single(PanelId::Count),
    ];
    app.dock.right.clear();
    let _ = app.on_dock(DockMsg::ResizeGrab(PanelId::Properties));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(300.0, 100.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(330.0, 100.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    // The group was 260 wide (its widest pallet); it is now 290, while
    // the other group and the pallets' own widths are untouched.
    assert_eq!(app.dock_group_width(DockSide::Left, 0), 290.0);
    assert_eq!(app.dock_group_width(DockSide::Left, 1), 280.0);
    assert_eq!(app.dock.settings(PanelId::Properties).width, 250.0);
}

#[test]
fn layers_command_opens_a_floating_pallet_and_toggles_it_closed() {
    use crate::ui::dock::PanelId;
    let mut app = dock_app();
    let _ = app.update(Message::ToggleLayers);
    assert!(app.show_layers);
    assert!(app.active_modal.is_none(), "no modal backdrop");
    let f = app
        .dock
        .float_rect(PanelId::Layers)
        .expect("floats on first use");
    assert!(f.x > 0.0 && f.y >= 0.0);
    let _ = app.update(Message::ToggleLayers);
    assert!(!app.show_layers);
    // Its place is kept for next time.
    assert!(app.dock.float_rect(PanelId::Layers).is_some());
}

/// A headless renderer for driving the real view.
fn headless_renderer() -> iced::Renderer {
    iced_test::futures::futures::executor::block_on(
        <iced::Renderer as iced_test::core::renderer::Headless>::new(
            iced_test::core::renderer::Settings::default(),
            None,
        ),
    )
    .expect("headless renderer")
}

/// Size the real view is laid out at.
const VIEW_SIZE: iced::Size = iced::Size::new(1600.0, 900.0);

/// Centre of the first text `label` in the real view.
fn find_in_view(app: &OpenCADStudio, label: &str) -> iced::Point {
    try_find_in_view(app, label).unwrap_or_else(|| panic!("{label} not found"))
}

/// Centre of the first text `label` in the real view, if it is there.
fn try_find_in_view(app: &OpenCADStudio, label: &str) -> Option<iced::Point> {
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless_renderer();
    let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, Cache::default(), &mut renderer);
    let mut find = Selector::find(label);
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    match widget::Operation::finish(&find) {
        widget::operation::Outcome::Some(Some(target)) => {
            target.visible_bounds().map(|b| b.center())
        }
        _ => None,
    }
}

/// Drive the real view through iced's `UserInterface` with the pointer at
/// `at`: feed `events` one at a time, and between events hand the
/// published messages to the app and rebuild the view while keeping the
/// widget-tree cache, as the runtime does.
fn drive_view(app: &mut OpenCADStudio, at: iced::Point, events: &[iced::Event]) {
    use iced_test::core::{mouse, shell, window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut renderer = headless_renderer();
    let mut cache = Cache::default();
    let cursor = mouse::Cursor::Available(at);
    for event in events {
        let mut messages = Vec::new();
        {
            let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, cache, &mut renderer);
            let _ = ui.update(
                &window::Headless,
                &shell::Waker::noop(),
                std::slice::from_ref(event),
                cursor,
                &mut renderer,
                &mut messages,
            );
            cache = ui.into_cache();
        }
        for message in messages {
            let _ = app.update(message);
        }
    }
}

/// The hover messages an open dropdown inside a hover zone (`zone`) or a
/// `mouse_area` publishes as the pointer goes from the field onto the
/// dropdown's list, then out of the area, each tagged with where the pointer
/// was.
fn hover_messages_over_an_open_dropdown(zone: bool) -> Vec<(&'static str, &'static str)> {
    use crate::ui::dock::DockMsg;
    use iced::widget::{button, column, container, mouse_area, text};
    use iced_test::core::{mouse, shell, window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let view = || -> iced::Element<'static, Message> {
        let field = button(text("ByLayer")).width(120).height(20);
        // A pointer-wanting list, like a linetype combo's, under the field.
        let list = button(column![text("Continuous"), text("Dashed")])
            .on_press(Message::Dock(DockMsg::HoverEnd(
                crate::ui::dock::PanelId::Layers,
            )))
            .width(120)
            .height(100);
        let dd = iced_aw::DropDown::new(field, list, true)
            .alignment(iced_aw::drop_down::Alignment::Bottom);
        let area = container(dd).width(300).height(400);
        let (enter, exit) = (
            Message::Dock(DockMsg::HoverStay),
            Message::Dock(DockMsg::HoverExit),
        );
        if zone {
            crate::ui::hover_zone::hover_zone(area, enter, exit)
        } else {
            mouse_area(area).on_enter(enter).on_exit(exit).into()
        }
    };
    let mut renderer = headless_renderer();
    let mut cache = Cache::default();
    let mut seen = Vec::new();
    // On the field, on the open list (both inside the area), then outside.
    for (place, x, y) in [
        ("field", 60.0, 10.0),
        ("list", 60.0, 70.0),
        ("list", 60.0, 90.0),
        ("outside", 500.0, 70.0),
    ] {
        let at = iced::Point::new(x, y);
        let mut messages = Vec::new();
        let mut ui = UserInterface::build(view(), VIEW_SIZE, cache, &mut renderer);
        let _ = ui.update(
            &window::Headless,
            &shell::Waker::noop(),
            &[iced::Event::Mouse(mouse::Event::CursorMoved {
                position: at,
            })],
            mouse::Cursor::Available(at),
            &mut renderer,
            &mut messages,
        );
        cache = ui.into_cache();
        seen.extend(messages.into_iter().filter_map(|m| match m {
            Message::Dock(DockMsg::HoverStay) => Some(("enter", place)),
            Message::Dock(DockMsg::HoverExit) => Some(("exit", place)),
            _ => None,
        }));
    }
    seen
}

#[test]
fn an_open_dropdown_in_a_hover_zone_does_not_read_as_leaving() {
    with_stack(|| {
        // The bug: a mouse area reads the pointer on the list as gone.
        assert_eq!(
            hover_messages_over_an_open_dropdown(false),
            [("enter", "field"), ("exit", "list")]
        );
        assert_eq!(
            hover_messages_over_an_open_dropdown(true),
            [("enter", "field"), ("exit", "outside")]
        );
    });
}

/// Bounds of the field (text `ByLayer`) and of its open list (text `Opaque`)
/// for a `drop_down_below` in a scrollable that, like a pallet, sits away
/// from the window's top-left corner.
fn drop_down_below_in_an_offset_scrollable() -> (iced::Rectangle, iced::Rectangle) {
    use iced::widget::{button, column, container, row, scrollable, text, Space};
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let view = || -> iced::Element<'static, Message> {
        let field = container(text("ByLayer")).width(200).height(20);
        let list = column![
            button(text("Opaque")).width(iced::Fill),
            button(text("50")).width(iced::Fill),
        ];
        let dd = crate::ui::color_select::drop_down_below(
            field.into(),
            list.into(),
            None,
            iced::Length::Shrink,
            Message::PropEditChoiceToggle,
        );
        let pallet = scrollable(column![Space::new().height(300), dd]).height(500);
        row![
            Space::new().width(1000),
            container(pallet).width(300).height(iced::Fill)
        ]
        .into()
    };
    let mut renderer = headless_renderer();
    let mut ui = UserInterface::build(view(), VIEW_SIZE, Cache::default(), &mut renderer);
    let mut find = |label: &str| {
        let mut find = Selector::find(label);
        ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
        match widget::Operation::finish(&find) {
            widget::operation::Outcome::Some(Some(target)) => target.bounds(),
            _ => panic!("{label} not found"),
        }
    };
    (find("ByLayer"), find("Opaque"))
}

#[test]
fn a_drop_down_in_a_pallet_opens_right_below_its_field() {
    with_stack(|| {
        let (field, item) = drop_down_below_in_an_offset_scrollable();
        assert!(
            item.y >= field.y + field.height && item.y < field.y + field.height + 20.0,
            "list starts below the field: field {field:?}, first item {item:?}"
        );
        assert!(
            item.x >= field.x - 1.0 && item.x < field.x + field.width,
            "list lines up with the field: field {field:?}, first item {item:?}"
        );
    });
}

#[test]
fn docked_title_bar_double_click_floats_the_pallet_in_the_real_view() {
    // Building the real view is deeper than a default test thread's stack.
    with_stack(|| {
        use crate::ui::dock::PanelId;
        let mut app = dock_app();
        let press =
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let release = iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ));
        // The block palette's title text sits in its docked title bar.
        let title = find_in_view(&app, "Blocks");
        drive_view(
            &mut app,
            title,
            &[press.clone(), release.clone(), press, release],
        );
        assert!(
            app.dock.float_rect(PanelId::BlockPalette).is_some(),
            "a double-click on a docked title bar floats the pallet"
        );
    });
}

#[test]
fn dragging_a_floating_pallet_moves_the_real_window_in_the_real_view() {
    // Building the real view is deeper than a default test thread's stack.
    with_stack(|| {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, DropTarget, FloatPanel, PanelId};
        let mut app = dock_app();
        app.dock.float(FloatPanel::new(
            PanelId::BlockPalette,
            500.0,
            200.0,
            260.0,
            400.0,
        ));
        let saved = app.dock.float_rect(PanelId::BlockPalette).unwrap();
        // Some text inside the pallet marks where its window is drawn.
        let label = "Current Drawing";
        let before = find_in_view(&app, label);
        let _ = app.on_dock(DockMsg::DockGrab(PanelId::BlockPalette));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(510.0, 210.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(610.0, 260.0)));
        assert!(matches!(
            drag_target(&app),
            Some(DropTarget::Float { x, y, .. }) if x == 600.0 && y == 250.0
        ));
        // The window itself follows the pointer; no ghost with its title.
        let during = find_in_view(&app, label);
        assert_eq!(
            (during.x - before.x, during.y - before.y),
            (100.0, 50.0),
            "the real window moves with the pointer"
        );
        assert!(
            try_find_in_view(&app, "Block Palette").is_none(),
            "no drop ghost for a pallet that already floats"
        );
        // The layout only changes on release.
        assert_eq!(app.dock.float_rect(PanelId::BlockPalette), Some(saved));
        // Over a dock zone the window hides and the column preview shows.
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(20.0, 300.0)));
        assert!(matches!(
            drag_target(&app),
            Some(
                DropTarget::Join {
                    side: DockSide::Left,
                    ..
                } | DropTarget::Edge {
                    side: DockSide::Left,
                    ..
                }
            )
        ));
        assert!(try_find_in_view(&app, label).is_none());
        assert!(
            try_find_in_view(&app, "Block Palette").is_some(),
            "column ghost"
        );
        // Back in the open it shows again, and lands where it was drawn.
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(610.0, 260.0)));
        let during = find_in_view(&app, label);
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(find_in_view(&app, label), during);
        let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
        assert_eq!((f.x, f.y), (600.0, 250.0));
    });
}

#[test]
fn hovering_a_strip_icon_reveals_an_auto_hiding_edge_in_the_real_view() {
    // Building the real view is deeper than a default test thread's stack.
    with_stack(|| {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId, STRIP_CELL_H, STRIP_PAD};
        let mut app = dock_app();
        // Mirror the layout onto the left so the block palette's title marks
        // where the workspace starts, then auto-hide the right edge.
        let title = find_in_view(&app, "Blocks");
        let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::BlockPalette));
        assert!(!app.dock_edge_expanded(DockSide::Right));
        // The title bar text sits ~13 px below the workspace top; the strip's
        // first icon is centred STRIP_PAD + half a cell below it.
        let workspace_top = title.y - 13.0;
        let icon = iced::Point::new(
            VIEW_SIZE.width - crate::ui::dock::DOCK_STRIP_W / 2.0,
            workspace_top + STRIP_PAD + STRIP_CELL_H / 2.0,
        );
        drive_view(
            &mut app,
            icon,
            &[iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: icon,
            })],
        );
        let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
        assert!(
            app.dock_edge_expanded(DockSide::Right),
            "hovering the icon reveals the auto-hiding edge"
        );
    });
}

#[test]
fn layers_ribbon_button_opens_the_pallet_without_staying_highlighted() {
    let mut app = dock_app();
    // The ribbon's LAYERS button runs the LAYERS command.
    let click = || Message::RibbonToolClick {
        tool_id: "LAYERS".to_string(),
        event: crate::modules::ModuleEvent::Command("LAYERS".to_string()),
    };
    let _ = app.update(click());
    assert!(app.show_layers);
    assert_eq!(app.ribbon.active_tool(), None);
    // Pressing it again keeps it open, like the Blocks palette's button.
    let _ = app.update(click());
    assert!(app.show_layers);
}

#[test]
fn dock_double_clicks_float_and_dock_panels() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::Properties));
    assert!(app.dock.left.is_empty());
    let f = app.dock.float_rect(PanelId::Properties).expect("floating");
    assert!(f.x > 0.0, "floats clear of the left edge");
    let _ = app.on_dock(DockMsg::DockTo(PanelId::Properties, DockSide::Right));
    assert!(app.dock.floating.is_empty());
    assert_eq!(
        app.dock.location(PanelId::Properties),
        Some((DockSide::Right, 1))
    );
}

#[test]
fn blockpalette_width_reset() {
    use crate::app::config::DockSide;
    let mut app = dock_app();
    let id = crate::ui::dock::PanelId::BlockPalette;
    app.dock.set_group_width(DockSide::Right, 0, 500.0);
    let _ = app.on_dock(crate::ui::dock::DockMsg::WidthReset(id));
    assert_eq!(app.dock.group_width(DockSide::Right, 0), 260.0);
}

#[test]
fn floating_menu_docks_and_allow_docking_keeps_a_drag_floating() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, DropTarget, PanelId};
    let mut app = dock_app();
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::BlockPalette));
    // With Allow docking off, a drag onto the left edge's strip stays
    // floating.
    let _ = app.on_dock(DockMsg::FloatMenu(Some(PanelId::BlockPalette)));
    let _ = app.on_dock(DockMsg::FloatDockingToggle(PanelId::BlockPalette));
    assert_eq!(app.dock_float_menu, None);
    assert!(!app.dock.float_rect(PanelId::BlockPalette).unwrap().docking);
    let _ = app.on_dock(DockMsg::DockGrab(PanelId::BlockPalette));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(500.0, 300.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(10.0, 60.0)));
    assert!(matches!(drag_target(&app), Some(DropTarget::Float { .. })));
    let _ = app.on_dock(DockMsg::DragRelease);
    let f = app
        .dock
        .float_rect(PanelId::BlockPalette)
        .expect("still floating");
    assert!(!f.docking, "moving it keeps the choice");
    // The menu's Dock left still docks it.
    let _ = app.on_dock(DockMsg::FloatMenu(Some(PanelId::BlockPalette)));
    let _ = app.on_dock(DockMsg::DockTo(PanelId::BlockPalette, DockSide::Left));
    assert_eq!(app.dock_float_menu, None);
    assert_eq!(
        app.dock.location(PanelId::BlockPalette).map(|(s, _)| s),
        Some(DockSide::Left)
    );
}

#[test]
fn visibility_ribbon_real_controls_disable_enable_and_open_manager() {
    with_stack(|| {
        use crate::ui::window::block_visibility::Action as A;
        let mut app = authoring_app();
        app.ribbon.set_block_editing(true);
        app.ribbon
            .set_collapse_mode(crate::ui::ribbon::CollapseMode::Compact);
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let point = authoring_control(&app, "BVSTATE_MANAGER");
        authoring_click(&mut app, point);
        assert!(app.block_visibility.is_none());
        assert_eq!(app.tabs[i].scene.document, before);
        authoring_capture(&app, "visibility-ribbon-disabled");
        let _ = app.run_command_line("BPARAMETER VISIBILITY 0.5,0.5");
        let target = app.block_visibility_context().unwrap().target;
        let point = authoring_control(&app, "BVSTATE_MANAGER");
        authoring_click(&mut app, point);
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockVisibility)
        );
        let _ = app.update(Message::BlockVisibility(A::Name("Alternate".into())));
        let _ = app.update(Message::BlockVisibility(A::New));
        authoring_capture(&app, "visibility-states-dialog");
        let point = find_in_view(&app, "OK");
        authoring_click(&mut app, point);
        assert!(app.block_visibility.is_none());
        assert_eq!(app.block_visibility_context().unwrap().names.len(), 2);
        let messages = {
            let mut simulator = iced_test::Simulator::with_size(
                iced::Settings::default(),
                VIEW_SIZE,
                app.view_main(),
            );
            let target = simulator
                .click(iced::widget::Id::new("BVSTATE_CURRENT"))
                .expect("current-state selector");
            let bounds = target.visible_bounds().unwrap();
            authoring_snapshot(
                &mut simulator,
                &app.active_theme,
                "visibility-state-dropdown",
            );
            // Native pick-list options are painted text, not Text widgets.
            // Pick the second row using the measured selector/row height.
            let position = iced::Point::new(bounds.center_x(), bounds.y + bounds.height * 2.5);
            simulator.point_at(position);
            simulator.simulate([
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }),
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )),
            ]);
            simulator.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(
            app.block_visibility_context().unwrap().current.as_deref(),
            Some("Alternate")
        );
        let point = authoring_control(&app, "BVMODE_TOGGLE");
        authoring_click(&mut app, point);
        assert!(app.block_visibility_mode);
        authoring_capture(&app, "visibility-ribbon-enabled");
        app.ribbon
            .set_collapse_mode(crate::ui::ribbon::CollapseMode::Collapsed);
        let point = find_in_view(&app, "Visibility");
        authoring_click(&mut app, point);
        let messages = {
            let mut simulator = iced_test::Simulator::with_size(
                iced::Settings::default(),
                VIEW_SIZE,
                app.view_main(),
            );
            simulator
                .click("Visibility States")
                .expect("manager in flyout");
            simulator.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.block_visibility.as_ref().unwrap().target, target);
    });
}

#[test]
fn point_parameter_placement_starts_from_the_actual_palette_row() {
    with_stack(|| {
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let point = authoring_control(&app, "authoring-tool-point");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().unwrap().name(),
            "BPARAMETER"
        );
        let _ = app.update(Message::CommandInput("0.25,0.25".into()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(app.tabs[i].scene.document, before);
        let _ = app.update(Message::CommandInput("0.65,0.15".into()));
        let _ = app.update(Message::CommandSubmit);
        assert!(
            app.tabs[i].scene.preview_wires.is_empty(),
            "placement clears its rubber-band preview"
        );
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        assert_eq!(
            crate::scene::block_point_parameter::parameters(
                &app.tabs[i].scene.document,
                definition
            )
            .len(),
            1
        );
        assert!(
            app.visibility_placement_error().is_none(),
            "Visibility can join the Point graph"
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
        authoring_capture(&app, "point-parameter-placed");
        let point = authoring_control(&app, "authoring-tool-point");
        authoring_click(&mut app, point);
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
    });
}

#[test]
fn distance_parameter_batch_starts_from_actual_palette_rows() {
    with_stack(|| {
        for (id, name) in [
            ("authoring-tool-linear", "linear"),
            ("authoring-tool-polar", "polar"),
            ("authoring-tool-xy", "xy"),
        ] {
            let mut app = authoring_app();
            let _ = app.dispatch_command("BAUTHORPALETTE");
            let point = authoring_control(&app, id);
            authoring_click(&mut app, point);
            let i = app.active_tab;
            let before = app.tabs[i].scene.document.clone();
            for text in ["0.25,-0.15", "0.65,0.15"] {
                let _ = app.update(Message::CommandInput(text.into()));
                let _ = app.update(Message::CommandSubmit);
                assert_eq!(app.tabs[i].scene.document, before);
            }
            // XY asks for its grip count and labels itself; Linear and Polar
            // take the label point.
            let last = if name == "xy" { "1" } else { "0.3,0.2" };
            let _ = app.update(Message::CommandInput(last.into()));
            let _ = app.update(Message::CommandSubmit);
            let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
            let found = crate::scene::block_distance_parameter::parameters(
                &app.tabs[i].scene.document,
                definition,
            );
            assert_eq!(found.len(), 1);
            assert!(found[0].kind.name().eq_ignore_ascii_case(name));
            assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
            assert!(app.tabs[i].scene.preview_wires.is_empty());
            let _ = app.run_command_line("ZOOM E");
            authoring_capture(&app, &format!("{name}-parameter-placed"));
        }
    });
}

#[test]
fn orientation_parameter_batch_starts_from_actual_palette_rows() {
    with_stack(|| {
        for (id, name, inputs) in [
            (
                "authoring-tool-rotation",
                "rotation",
                vec!["0.35,-0.12", "0.65,-0.12", "0.35,0.2", "0.3,0.35", ""],
            ),
            (
                "authoring-tool-alignment",
                "alignment",
                vec!["0.25,-0.1", "0.6,0.15"],
            ),
            (
                "authoring-tool-flip",
                "flip",
                vec!["0.35,-0.15", "0.35,0.15", "0.6,0.15"],
            ),
        ] {
            let mut app = authoring_app();
            let _ = app.dispatch_command("BAUTHORPALETTE");
            let point = authoring_control(&app, id);
            authoring_click(&mut app, point);
            let i = app.active_tab;
            let before = app.tabs[i].scene.document.clone();
            for text in &inputs[..inputs.len() - 1] {
                let _ = app.update(Message::CommandInput((*text).into()));
                let _ = app.update(Message::CommandSubmit);
                assert_eq!(app.tabs[i].scene.document, before);
            }
            let _ = app.update(Message::CommandInput(inputs.last().unwrap().to_string()));
            let _ = app.update(Message::CommandSubmit);
            let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
            let found = crate::scene::block_orientation_parameter::parameters(
                &app.tabs[i].scene.document,
                definition,
            );
            assert_eq!(found.len(), 1);
            assert!(found[0].kind.name().eq_ignore_ascii_case(name));
            assert!(app.tabs[i].scene.preview_wires.is_empty());
            assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
            let _ = app.run_command_line("ZOOM E");
            authoring_capture(&app, &format!("{name}-parameter-placed"));
        }
    });
}

fn authoring_app() -> OpenCADStudio {
    use codec::{
        entities::{Insert, Line},
        types::Vector3,
        EntityType,
    };
    let mut app = dock_app();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "AUTHOR",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("AUTHOR", Vector3::ZERO)))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    app
}

#[test]
fn block_actions_palette_controls_require_compatible_parameters_and_start_commands() {
    with_stack(|| {
        use crate::ui::window::authoring_palettes::Tab;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let point = authoring_control(&app, Tab::Actions.widget_id());
        authoring_click(&mut app, point);
        for id in ["authoring-tool-move-action", "authoring-tool-scale-action"] {
            let point = authoring_control(&app, id);
            authoring_click(&mut app, point);
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
        }
        let _ = app.run_command_line("BPARAMETER POINT 0.5,0.5 0.7,0.7");
        let point = authoring_control(&app, "authoring-tool-scale-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let point = authoring_control(&app, "authoring-tool-move-action");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
            "BACTION"
        );
        let _ = app.update(Message::CommandEscape);
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,0 0.5,0.3");
        let point = authoring_control(&app, "authoring-tool-scale-action");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
            "BACTION"
        );
        let _ = app.update(Message::CommandEscape);
        authoring_capture(&app, "move-scale-actions-enabled");
        let i = app.active_tab;
        let targets = app.tabs[i]
            .scene
            .document
            .block_records
            .get("AUTHOR")
            .unwrap()
            .entity_handles
            .clone();
        let point = authoring_control(&app, "authoring-tool-scale-action");
        authoring_click(&mut app, point);
        let _ = app.feed_command(crate::command::StepInput::Point(glam::DVec3::X));
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .is_selection_gathering(),
            "picking the parameter endpoint starts object selection"
        );
        let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let _ = app.update(Message::CommandInput("0.8,0.5".into()));
        let _ = app.update(Message::CommandSubmit);
        assert!(app.tabs[i].active_cmd.is_none());
        let _ = app.run_command_line("ZOOM E");
        authoring_capture(&app, "scale-action-placed");
        let _ = app.dispatch_command("BTESTBLOCK");
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-0.2, -0.4, 0.),
            glam::DVec3::new(1.2, 0.6, 0.),
            1600. / 900.,
        );
        let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
        app.tabs[i].scene.select_entity(insert, true);
        app.refresh_properties();
        assert!(app.tabs[i]
            .selected_grips
            .iter()
            .any(|g| crate::app::block_actions::is_test_grip(g.id)));
        authoring_capture(&app, "scale-action-test-grip");
        // Drawing a software snapshot updates the canvas size. Use the same
        // current tile as the viewport input path, including when captures run.
        let (vw, vh) = app.tabs[i].scene.selection.borrow().view.vp_size;
        let tile = app.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let bounds = iced::Rectangle {
            x: 0.,
            y: 0.,
            width: tile.width,
            height: tile.height,
        };
        let grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| crate::app::block_actions::is_test_grip(g.id))
            .unwrap();
        let screen = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(grip.world, bounds)
            .unwrap();
        app.cursor_pos = iced::Point::new(screen.x + tile.x, screen.y + tile.y);
        app.tabs[i].scene.selection.borrow_mut().input.last_move_pos = Some(app.cursor_pos);
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "a running command consumes the test grip"
        );
        let hit = crate::scene::pick::grip::find_hit_grip(
            iced::Point::new(screen.x, screen.y),
            &app.tabs[i].selected_grips,
            &app.tabs[i].scene.camera.borrow(),
            bounds,
        );
        assert!(
            hit.is_some_and(|(_, id, _, _)| crate::app::block_actions::is_test_grip(id)),
            "test grip hit failed: {hit:?}; bounds={bounds:?}, cursor={:?}",
            app.cursor_pos
        );
        let _ = app.update(Message::ViewportLeftPressAt(app.cursor_pos));
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|c|c.name()),
            Some("BTESTBLOCK"),
            "viewport click routes the parameter grip to the test command: modal={:?}, active grip={:?}, selected={:?}, navigation={:?}, ucs={}",app.active_modal,app.tabs[i].active_grip,app.tabs[i].selected_handle,(app.tabs[i].orbit_mode,app.tabs[i].pan_mode,app.tabs[i].zoom_dynamic_mode),app.ucs_icon_selected
        );
        let _ = app.update(Message::CommandInput("2,0".into()));
        let _ = app.update(Message::CommandSubmit);
        let line = app.tabs[i]
            .scene
            .document
            .entities_in_block("AUTHOR")
            .find_map(|e| {
                if let codec::EntityType::Line(l) = e {
                    Some(l)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(line.end, codec::types::Vector3::new(2., 0., 0.));
    });
}

#[test]
fn block_actions_set_ribbon_flyout_starts_action_selection_and_uses_preselected_marker() {
    with_stack(|| {
        use crate::scene::block_actions::{self as actions, Kind};
        let mut app = authoring_app();
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let i = app.active_tab;
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
        let targets = app.tabs[i]
            .scene
            .document
            .block_records
            .get("AUTHOR")
            .unwrap()
            .entity_handles
            .clone();
        let a = actions::create(
            &mut app.tabs[i].scene.document,
            def,
            Kind::Move,
            p,
            &targets,
            glam::DVec3::new(3., 4., 0.),
        )
        .unwrap();
        app.tabs[i].selected_authoring = Some(a);
        app.ribbon
            .set_collapse_mode(crate::ui::ribbon::CollapseMode::Collapsed);
        // The top-level Manage tab has the same label as this panel opener.
        // Open the existing panel, then click the new command in its real flyout.
        let _ = app.update(Message::ToggleRibbonPanel("Manage".into()));
        assert_eq!(app.ribbon.collapsed_open.as_deref(), Some("Manage"));
        let messages = {
            let mut simulator = iced_test::Simulator::with_size(
                iced::Settings::default(),
                VIEW_SIZE,
                app.view_main(),
            );
            simulator
                .click("Action Selection")
                .expect("action selection in Manage flyout");
            simulator.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().unwrap().name(),
            "BACTIONSET"
        );
        assert!(app.tabs[i]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("New/Add/Remove"));
        let _ = app.feed_command(crate::command::StepInput::Enter);
        assert!(app.tabs[i]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
    });
}

#[test]
fn block_actions_stretch_palette_requires_distance_parameter() {
    with_stack(|| {
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        app.authoring_palette_tab = crate::ui::window::authoring_palettes::Tab::Actions;
        let point = authoring_control(&app, "authoring-tool-stretch-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
        let point = authoring_control(&app, "authoring-tool-stretch-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Stretch"));
    });
}

#[test]
fn block_actions_array_palette_requires_distance_parameter() {
    with_stack(|| {
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        app.authoring_palette_tab = crate::ui::window::authoring_palettes::Tab::Actions;
        let point = authoring_control(&app, "authoring-tool-array-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let _ = app.run_command_line("BPARAMETER POINT 0.5,0.5 0.7,0.7");
        let point = authoring_control(&app, "authoring-tool-array-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
        let point = authoring_control(&app, "authoring-tool-array-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Array"));
    });
}

#[test]
fn block_actions_rotate_flip_palette_requires_matching_parameters() {
    with_stack(|| {
        use crate::ui::window::authoring_palettes::Tab;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let point = authoring_control(&app, Tab::Actions.widget_id());
        authoring_click(&mut app, point);
        for id in ["authoring-tool-rotate-action", "authoring-tool-flip-action"] {
            let point = authoring_control(&app, id);
            authoring_click(&mut app, point);
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
        }
        let _ = app.run_command_line("BPARAMETER ROTATION 0,0 2 0 5,5");
        let point = authoring_control(&app, "authoring-tool-flip-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let point = authoring_control(&app, "authoring-tool-rotate-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Rotate"));
        let _ = app.update(Message::CommandEscape);
        let _ = app.run_command_line("BPARAMETER FLIP 0,0 2,0 -1,1");
        let point = authoring_control(&app, "authoring-tool-flip-action");
        authoring_click(&mut app, point);
        assert!(app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Flip"));
    });
}

#[test]
fn block_properties_table_offers_to_discard_and_close_on_an_unusable_cell() {
    with_stack(|| {
        use crate::ui::window::block_properties_table::Action as A;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,0 0,0.3");
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let history = app.tabs[i].history.undo_stack.len();
        let _ = app.dispatch_command("BPROPERTIESTABLE");
        assert!(
            app.block_properties_table.is_some(),
            "the dialog is open before the refusal"
        );
        let _ = app.update(Message::BlockPropertiesTable(A::Property(0)));
        let _ = app.update(Message::BlockPropertiesTable(A::AddProperty));
        let _ = app.update(Message::BlockPropertiesTable(A::AddRow));
        let _ = app.update(Message::BlockPropertiesTable(A::Cell(0, 0, "wide".into())));
        let _ = app.update(Message::BlockPropertiesTable(A::Apply));
        let state = app
            .block_properties_table
            .as_ref()
            .expect("an unusable cell is refused");
        assert_eq!(
            state.error.as_deref(),
            Some("Each cell must be a finite number."),
            "the refusal says why"
        );
        assert!(state.confirm_close, "and offers a way out");
        assert!(
            try_find_in_view(&app, "Discard & close").is_some(),
            "the offer is on screen, not only in the state"
        );
        let _ = app.update(Message::BlockPropertiesTable(A::KeepEditing));
        let state = app
            .block_properties_table
            .as_ref()
            .expect("keeping editing leaves the work in place");
        assert!(!state.confirm_close, "the offer is gone");
        assert!(state.error.is_some(), "the diagnosis stays where it was read");
        assert_eq!(state.rows[0][0], "wide");
        assert!(
            try_find_in_view(&app, "Discard & close").is_none(),
            "and the screen agrees"
        );
        let _ = app.update(Message::BlockPropertiesTable(A::Apply));
        assert!(app
            .block_properties_table
            .as_ref()
            .expect("the refusal offers the way out again")
            .confirm_close);
        let _ = app.update(Message::BlockPropertiesTable(A::DiscardClose));
        assert!(app.block_properties_table.is_none());
        assert!(app.active_modal.is_none(), "the dialog actually leaves");
        assert!(app.tabs[i].scene.document == before, "nothing was applied");
        assert_eq!(app.tabs[i].history.undo_stack.len(), history);
    });
}

#[test]
fn lookup_basepoint_palette_and_table_dialog_real_controls() {
    with_stack(|| {
        use crate::ui::window::{authoring_palettes::Tab, block_lookup::Action as A};
        let mut app = authoring_app();
        // The palette's lookup-table tool is for palette mode; with action
        // bars on it points to the Lookup parameter's bar instead.
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let _ = app.dispatch_command("BAUTHORPALETTE");
        for (id, point_text) in [
            ("authoring-tool-basepoint", "0.2,0.1"),
            ("authoring-tool-lookup", "0.5,0.2"),
        ] {
            let point = authoring_control(&app, id);
            authoring_click(&mut app, point);
            assert_eq!(
                app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
                "BPARAMETER"
            );
            let _ = app.update(Message::CommandInput(point_text.into()));
            let _ = app.update(Message::CommandSubmit);
            if id == "authoring-tool-lookup" {
                let _ = app.update(Message::CommandInput(String::new()));
                let _ = app.update(Message::CommandSubmit);
            }
        }
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,0 0,0.3");
        authoring_capture(&app, "lookup-basepoint-placed");
        let point = authoring_control(&app, Tab::Actions.widget_id());
        authoring_click(&mut app, point);
        let point = authoring_control(&app, "authoring-tool-lookup-table");
        authoring_click(&mut app, point);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::BlockLookup));
        let _ = app.update(Message::BlockLookup(A::Property(0)));
        let point = find_in_view(&app, "Add Property");
        authoring_click(&mut app, point);
        let point = find_in_view(&app, "Add Row");
        authoring_click(&mut app, point);
        assert_eq!(
            app.block_lookup.as_ref().unwrap().drafts[0]
                .edited
                .row_count,
            1
        );
        let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Small".into())));
        let point = find_in_view(&app, "Audit");
        authoring_click(&mut app, point);
        assert!(app.block_lookup.as_ref().unwrap().notice.is_some());
        authoring_capture(&app, "lookup-property-table");
        let point = find_in_view(&app, "OK");
        authoring_click(&mut app, point);
        assert!(app.block_lookup.is_none());
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter =
            crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[0].0;
        assert!(crate::scene::block_lookup::table(
            &app.tabs[i].scene.document,
            definition,
            parameter
        )
        .unwrap()
        .is_some());
    });
}

#[test]
fn block_properties_table_palette_dialog_authors_rows() {
    with_stack(|| {
        use crate::ui::window::{authoring_palettes::Tab, block_properties_table::Action as A};
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,0 0,0.3");
        let point = authoring_control(&app, Tab::Actions.widget_id());
        authoring_click(&mut app, point);
        let point = authoring_control(&app, "authoring-tool-properties-table");
        authoring_click(&mut app, point);
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockPropertiesTable)
        );
        let _ = app.update(Message::BlockPropertiesTable(A::Property(0)));
        let point = find_in_view(&app, "Add Property");
        authoring_click(&mut app, point);
        let point = find_in_view(&app, "Add Row");
        authoring_click(&mut app, point);
        assert_eq!(
            app.block_properties_table.as_ref().unwrap().rows.len(),
            1
        );
        let _ = app.update(Message::BlockPropertiesTable(A::Cell(0, 0, "2.5".into())));
        let point = find_in_view(&app, "OK");
        authoring_click(&mut app, point);
        assert!(app.block_properties_table.is_none());
        assert!(app.active_modal.is_none());
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let view =
            crate::scene::block_properties_table::table(&app.tabs[i].scene.document, definition)
                .unwrap()
                .expect("the table was written");
        assert_eq!(view.columns.len(), 1);
        assert_eq!(view.rows, vec![vec![2.5]]);
    });
}

#[test]
fn block_properties_table_apply_writes_undo_once_and_stale_edits_stay_open() {
    with_stack(|| {
        use crate::ui::window::block_properties_table::Action as A;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,0 0,0.3");
        let i = app.active_tab;
        let history = app.tabs[i].history.undo_stack.len();
        let _ = app.dispatch_command("BPROPERTIESTABLE");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockPropertiesTable)
        );
        let _ = app.update(Message::BlockPropertiesTable(A::Property(0)));
        let _ = app.update(Message::BlockPropertiesTable(A::AddProperty));
        let _ = app.update(Message::BlockPropertiesTable(A::AddRow));
        let _ = app.update(Message::BlockPropertiesTable(A::Apply));
        assert!(app.block_properties_table.is_none());
        assert_eq!(app.tabs[i].history.undo_stack.len(), history + 1);
        let _ = app.dispatch_command("BPROPERTIESTABLE");
        assert!(app.block_properties_table.is_some());
        let _ = app.update(Message::BlockPropertiesTable(A::Apply));
        assert!(app.block_properties_table.is_none());
        assert_eq!(app.tabs[i].history.undo_stack.len(), history + 1);
        let _ = app.dispatch_command("BPROPERTIESTABLE");
        app.tabs[i].scene.bump_geometry();
        let _ = app.update(Message::BlockPropertiesTable(A::AddRow));
        let _ = app.update(Message::BlockPropertiesTable(A::Apply));
        assert!(app.block_properties_table.is_some());
        assert!(app.block_properties_table.as_ref().unwrap().error.is_some());
        let _ = app.update(Message::CloseModal);
        assert!(app.block_properties_table.is_none());
        assert!(app.active_modal.is_none());
    });
}

#[test]
fn authoring_palettes_ribbon_toggle_and_dock_close_leave_document_and_history_unchanged() {
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = authoring_app();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let history = app.tabs[i].history.undo_stack.len();
    let click = || Message::RibbonToolClick {
        tool_id: "BAUTHORPALETTE".into(),
        event: crate::modules::ModuleEvent::Command("BAUTHORPALETTE".into()),
    };
    let _ = app.update(click());
    assert!(app.dock_panel_visible(PanelId::AuthoringPalettes));
    assert!(app.ribbon.show_authoring_palettes);
    assert!(app.dock.is_placed(PanelId::AuthoringPalettes));
    let _ = app.update(click());
    assert!(!app.dock_panel_visible(PanelId::AuthoringPalettes));
    assert!(!app.ribbon.show_authoring_palettes);
    let _ = app.dispatch_command("BAUTHORPALETTE");
    let _ = app.on_dock(DockMsg::Close(PanelId::AuthoringPalettes));
    assert!(!app.show_authoring_palettes);
    assert!(!app.ribbon.show_authoring_palettes);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), history);
    assert!(!app.block_edit_dirty(i));
}

#[test]
fn authoring_palettes_follow_editor_context_and_keep_the_docked_placement() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = authoring_app();
    let _ = app.dispatch_command("BAUTHORPALETTE");
    let _ = app.on_dock(DockMsg::DockTo(PanelId::AuthoringPalettes, DockSide::Left));
    let placement = app.dock.location(PanelId::AuthoringPalettes);
    let _ = app.dispatch_command("BTESTBLOCK");
    assert!(!app.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert!(app.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = app.dispatch_command("BTESTBLOCK");
    assert!(!app.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = app.dispatch_command("BAUTHORPALETTECLOSE");
    let _ = app.dispatch_command("BAUTHORPALETTE");
    assert!(!app.show_authoring_palettes);
    assert!(!app.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    let _ = app.dispatch_command("BAUTHORPALETTE");
    assert!(app.dock_panel_visible(PanelId::AuthoringPalettes));
    assert_eq!(app.dock.location(PanelId::AuthoringPalettes), placement);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(!app.dock_panel_visible(PanelId::AuthoringPalettes));
    let _ = app.dispatch_command("BAUTHORPALETTECLOSE");
    let _ = app.dispatch_command("BAUTHORPALETTE");
    assert!(!app.show_authoring_palettes);
}

#[test]
fn authoring_palettes_toggle_keeps_the_running_drawing_command() {
    let mut app = authoring_app();
    let _ = app.dispatch_command("LINE");
    assert!(app.tabs[app.active_tab].active_cmd.is_some());
    let _ = app.update(Message::RibbonToolClick {
        tool_id: "BAUTHORPALETTE".into(),
        event: crate::modules::ModuleEvent::Command("BAUTHORPALETTE".into()),
    });
    assert!(app.tabs[app.active_tab].active_cmd.is_some());
    let _ = app.dispatch_command("BAUTHORPALETTECLOSE");
    assert!(app.tabs[app.active_tab].active_cmd.is_some());
}

#[test]
fn authoring_palettes_unwired_move_action_does_not_start_a_command() {
    with_stack(|| {
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        app.authoring_palette_tab = crate::ui::window::authoring_palettes::Tab::Actions;
        let point = find_in_view(&app, "Move");
        let before = app.tabs[app.active_tab].scene.document.clone();
        drive_view(
            &mut app,
            point,
            &[
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )),
            ],
        );
        assert_eq!(app.tabs[app.active_tab].scene.document, before);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    });
}

/// Locate rotated category controls by their stable widget IDs.
fn authoring_control(app: &OpenCADStudio, id: &'static str) -> iced::Point {
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless_renderer();
    let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, Cache::default(), &mut renderer);
    let mut find = widget::Id::new(id).find();
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    match widget::Operation::finish(&find) {
        widget::operation::Outcome::Some(Some(target)) => {
            target.visible_bounds().expect("visible").center()
        }
        _ => panic!("{id} not found"),
    }
}

/// An empty name has to be answered by clicking `OK`: while the button is
/// disabled for an empty field the dialog's own refusal is unreachable.
#[test]
fn block_edit_entry_dialog_answers_an_empty_name_when_ok_is_clicked() {
    use crate::app::ModalKind;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    let _ = app.update(Message::BlockEditPickerName(String::new()));
    assert!(app.block_edit_picker.as_ref().unwrap().error.is_none());
    assert!(
        try_find_in_view(
            &app,
            "The name must be at least one character long."
        )
        .is_none(),
        "nothing is said before the button is pressed"
    );
    let point = find_in_view(&app, "OK");
    drive_view(
        &mut app,
        point,
        &[
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
        ],
    );
    assert_eq!(
        app.block_edit_picker.as_ref().unwrap().error.as_deref(),
        Some("The name must be at least one character long."),
        "the click reached the dialog"
    );
    assert!(
        try_find_in_view(
            &app,
            "The name must be at least one character long."
        )
        .is_some(),
        "the reason is on screen"
    );
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    assert!(app.tabs[tab].active_block_edit.is_none());
}

fn authoring_click(app: &mut OpenCADStudio, point: iced::Point) {
    drive_view(
        app,
        point,
        &[
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
        ],
    );
}

#[test]
fn authoring_palettes_real_tabs_start_supported_rows_without_placing_until_picked() {
    with_stack(|| {
        use crate::ui::window::authoring_palettes::Tab;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let history = app.tabs[i].history.undo_stack.len();
        let _ = app.dispatch_command("LINE");
        for (tab, row) in [
            (Tab::Actions, "Block Properties Table"),
            (Tab::ParameterSets, "Point Move"),
            (Tab::Parameters, "Basepoint"),
        ] {
            let point = authoring_control(&app, tab.widget_id());
            authoring_click(&mut app, point);
            assert_eq!(app.authoring_palette_tab, tab);
            authoring_capture(&app, tab.widget_id());
            let point = find_in_view(&app, row);
            authoring_click(&mut app, point);
            if tab == Tab::ParameterSets {
                let mut simulator = iced_test::Simulator::with_size(
                    iced::Settings::default(),
                    VIEW_SIZE,
                    app.view_main(),
                );
                simulator.point_at(point);
                simulator.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                    delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -100.0 },
                })]);
                simulator
                    .click("Lookup Set")
                    .expect("last set is reachable by scrolling");
                authoring_snapshot(&mut simulator, &app.active_theme, "parameter-sets-bottom");
                let messages = simulator.into_messages().collect::<Vec<_>>();
                for message in messages {
                    let _ = app.update(message);
                }
            }
            assert_eq!(
                app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.name()),
                Some(if matches!(tab, Tab::Parameters | Tab::ParameterSets) {
                    "BPARAMETER"
                } else {
                    "LINE"
                })
            );
        }
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), history);
        assert!(!app.block_edit_dirty(i));
    });
}

#[test]
fn authoring_palettes_constraints_start_the_existing_commands_from_real_rows() {
    with_stack(|| {
        use crate::ui::window::authoring_palettes::Tab;
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let point = authoring_control(&app, Tab::Constraints.widget_id());
        authoring_click(&mut app, point);
        assert_eq!(app.authoring_palette_tab, Tab::Constraints);
        authoring_capture(&app, Tab::Constraints.widget_id());
        // The palette row is distinct from the ribbon's Coincident button.
        let point = authoring_control(&app, "authoring-tool-CCONSTRAINT");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .map(|command| command.name()),
            Some("GCCOINCIDENT")
        );
        let messages = {
            let mut simulator = iced_test::Simulator::with_size(
                iced::Settings::default(),
                VIEW_SIZE,
                app.view_main(),
            );
            simulator.point_at(point);
            simulator.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -100.0 },
            })]);
            simulator
                .click(iced_test::core::widget::Id::new(
                    "authoring-tool-DCDIAMETER",
                ))
                .expect("last dimensional constraint is reachable by scrolling");
            authoring_snapshot(&mut simulator, &app.active_theme, "constraints-bottom");
            simulator.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(
            app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .map(|command| command.name()),
            Some("DCDIAMETER")
        );
        assert!(!app.block_edit_dirty(app.active_tab));
    });
}

#[test]
fn authoring_palettes_selected_page_survives_test_block_and_close_and_ignores_hidden_input() {
    use crate::ui::window::authoring_palettes::Tab;
    let mut app = authoring_app();
    let _ = app.dispatch_command("BAUTHORPALETTE");
    let _ = app.update(Message::AuthoringPaletteTabSelected(Tab::ParameterSets));
    let _ = app.dispatch_command("BTESTBLOCK");
    let _ = app.update(Message::AuthoringPaletteTabSelected(Tab::Constraints));
    assert_eq!(app.authoring_palette_tab, Tab::ParameterSets);
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.authoring_palette_tab, Tab::ParameterSets);
    let _ = app.dispatch_command("BAUTHORPALETTECLOSE");
    let _ = app.update(Message::AuthoringPaletteTabSelected(Tab::Actions));
    assert_eq!(app.authoring_palette_tab, Tab::ParameterSets);
    let _ = app.dispatch_command("BAUTHORPALETTE");
    assert_eq!(app.authoring_palette_tab, Tab::ParameterSets);
}

/// Optional rendered evidence when reviewing palette layout changes.
fn authoring_capture(app: &OpenCADStudio, name: &str) {
    if std::env::var_os("OCS_AUTHORING_PALETTE_CAPTURES").is_none() {
        return;
    }
    let mut simulator =
        iced_test::Simulator::with_size(iced::Settings::default(), VIEW_SIZE, app.view_main());
    authoring_snapshot(&mut simulator, &app.active_theme, name);
}

fn authoring_snapshot(
    simulator: &mut iced_test::Simulator<'_, Message>,
    theme: &iced::Theme,
    name: &str,
) {
    let Some(directory) = std::env::var_os("OCS_AUTHORING_PALETTE_CAPTURES") else {
        return;
    };
    let path = std::path::PathBuf::from(directory).join(format!("{name}.png"));
    assert!(simulator
        .snapshot(theme)
        .expect("render palette")
        .matches_image(path)
        .expect("write palette evidence"));
}

#[test]
fn visibility_parameter_placement_starts_from_the_actual_palette_row() {
    with_stack(|| {
        let mut app = authoring_app();
        let _ = app.dispatch_command("BAUTHORPALETTE");
        let before = app.tabs[app.active_tab].scene.document.clone();
        let point = authoring_control(&app, "authoring-tool-visibility");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
            "BPARAMETER"
        );
        assert_eq!(app.tabs[app.active_tab].scene.document, before);
        let _ = app.update(Message::CommandInput("0.25,0.25,0".into()));
        let _ = app.update(Message::CommandSubmit);
        assert!(app
            .visibility_placement_error()
            .unwrap()
            .contains("already"));
        let before = app.tabs[app.active_tab].scene.document.clone();
        let _ = app.dispatch_command("LINE");
        let point = authoring_control(&app, "authoring-tool-visibility");
        authoring_click(&mut app, point);
        assert_eq!(
            app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
            "LINE"
        );
        assert_eq!(app.tabs[app.active_tab].scene.document, before);
        authoring_capture(&app, "visibility-parameter-placed");
    });
}
