//! Dock behaviour through the app: drags, hover, menus, and the real view.

use crate::app::{Message, OpenCADStudio};

fn fresh() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
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
    app.dock_drag.and_then(|d| d.movement()).and_then(|(_, _, target)| target)
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
    app.dock.set_width(PanelId::Browser, 400.0);
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
    let f = app.dock.float_rect(PanelId::BlockPalette).expect("floating").clone();
    assert!(f.x > 600.0 && f.x < 800.0 && f.y > 350.0 && f.y < 400.0);
    // Dragging a floating panel keeps the grab point under the pointer.
    let target = drag_panel(
        &mut app,
        PanelId::BlockPalette,
        iced::Point::new(f.x + 20.0, f.y + 10.0),
        iced::Point::new(f.x + 120.0, f.y + 60.0),
    );
    assert_eq!(target, Some(DropTarget::Float { x: f.x + 100.0, y: f.y + 50.0 }));
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
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(20.0, layout[1].icons_top + 10.0)));
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
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, layout[0].top + 2.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, plus_top + 10.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock.left[0].panels, vec![PanelId::BlockPalette]);
    assert_eq!(
        app.dock.left[1].panels,
        vec![PanelId::Properties, PanelId::Browser]
    );
    // A group dropped over the viewport floats as one window.
    let _ = app.on_dock(DockMsg::GroupGrab(DockSide::Left, 1));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, 300.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(800.0, 400.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert_eq!(app.dock.left.len(), 1);
    let f = app.dock.float_rect(PanelId::Browser).expect("floats");
    assert_eq!(f.group.panels, vec![PanelId::Properties, PanelId::Browser]);
    // Dragging the window's title bar onto the strip docks it whole again.
    let _ = app.on_dock(DockMsg::FloatGrab(PanelId::Properties));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(800.0, 400.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, plus_top + 10.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    assert!(app.dock.floating.is_empty());
    assert_eq!(
        app.dock.left[1].panels,
        vec![PanelId::Properties, PanelId::Browser]
    );
}

#[test]
fn floating_menu_adds_pallets_and_docks_the_window() {
    use crate::app::config::DockSide;
    use crate::ui::dock::{DockMsg, PanelId};
    let mut app = dock_app();
    let _ = app.on_dock(DockMsg::FloatOut(PanelId::BlockPalette));
    let _ = app.on_dock(DockMsg::FloatMenu(Some(PanelId::BlockPalette)));
    let _ = app.on_dock(DockMsg::FloatMenuPallets(true));
    // Picking a pallet adds it to the window, opens it and closes the menu.
    let _ = app.on_dock(DockMsg::FloatMenuToggle(PanelId::BlockPalette, PanelId::Browser));
    assert!(app.show_browser);
    assert_eq!(app.dock_float_menu, None);
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert_eq!(f.group.panels, vec![PanelId::BlockPalette, PanelId::Browser]);
    // Picking a checked one hides it but keeps its place.
    let _ = app.on_dock(DockMsg::FloatMenuToggle(PanelId::BlockPalette, PanelId::Browser));
    assert!(!app.show_browser);
    assert!(app.dock.float_rect(PanelId::Browser).is_some());
    // Dock left takes the whole window to the left edge.
    let _ = app.on_dock(DockMsg::DockTo(PanelId::BlockPalette, DockSide::Left));
    assert!(app.dock.floating.is_empty());
    let (side, gi) = app.dock.location(PanelId::Browser).unwrap();
    assert_eq!(side, DockSide::Left);
    assert_eq!(
        app.dock.groups(side)[gi].panels,
        vec![PanelId::BlockPalette, PanelId::Browser]
    );
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
    app.dock.left = vec![DockGroup::stack(vec![PanelId::BlockPalette, PanelId::Properties])];
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
fn dock_float_resize_grows_the_panel() {
    use crate::ui::dock::{DockMsg, FloatPanel, PanelId};
    let mut app = dock_app();
    app.dock.float(FloatPanel::new(PanelId::BlockPalette, 100.0, 100.0, 260.0, 300.0));
    let _ = app.on_dock(DockMsg::FloatResizeGrab(PanelId::BlockPalette, false));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(360.0, 400.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(400.0, 450.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    let f = app.dock.float_rect(PanelId::BlockPalette).unwrap().clone();
    assert_eq!((f.w, f.h), (300.0, 350.0));
    // The bottom-left grip grows the panel leftward, keeping its right edge.
    let _ = app.on_dock(DockMsg::FloatResizeGrab(PanelId::BlockPalette, true));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 450.0)));
    let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(60.0, 450.0)));
    let _ = app.on_dock(DockMsg::DragRelease);
    let g = app.dock.float_rect(PanelId::BlockPalette).unwrap();
    assert_eq!((g.x, g.w, g.h), (f.x - 40.0, 340.0, 350.0));
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
    assert_eq!(app.dock.location(PanelId::Browser), Some((DockSide::Left, 1)));
    // Opening it again from the same edge reuses that place.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
    assert_eq!(app.dock.left.len(), 2);
    // A pallet open on the other edge is not checked here: picking it
    // moves it over instead of hiding it.
    let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::BlockPalette));
    assert!(app.show_block_palette);
    assert!(app.dock.right.is_empty());
    assert_eq!(app.dock.location(PanelId::BlockPalette), Some((DockSide::Left, 2)));
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
    let f = app.dock.float_rect(PanelId::Layers).expect("floats on first use");
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
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless_renderer();
    let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, Cache::default(), &mut renderer);
    let mut find = Selector::find(label);
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    match widget::Operation::finish(&find) {
        widget::operation::Outcome::Some(Some(target)) => target
            .visible_bounds()
            .expect("visible")
            .center(),
        _ => panic!("{label} not found"),
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

#[test]
fn docked_title_bar_double_click_floats_the_pallet_in_the_real_view() {
    use crate::ui::dock::PanelId;
    let mut app = dock_app();
    let press = iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
    let release =
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left));
    // The block palette's title text sits in its docked title bar.
    let title = find_in_view(&app, "Blocks");
    drive_view(&mut app, title, &[press.clone(), release.clone(), press, release]);
    assert!(
        app.dock.float_rect(PanelId::BlockPalette).is_some(),
        "a double-click on a docked title bar floats the pallet"
    );
}

#[test]
fn hovering_a_strip_icon_reveals_an_auto_hiding_edge_in_the_real_view() {
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
        &[iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: icon })],
    );
    let _ = app.on_dock(DockMsg::HoverSettled(app.dock_hover_gen));
    assert!(
        app.dock_edge_expanded(DockSide::Right),
        "hovering the icon reveals the auto-hiding edge"
    );
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
    assert_eq!(app.dock.location(PanelId::Properties), Some((DockSide::Right, 1)));
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
