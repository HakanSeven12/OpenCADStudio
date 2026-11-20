use super::*;

use super::*;

#[test]
fn boxes_outside_pane_and_zero_area_use_full_pick() {
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    assert!(box_preview_rect(Point::new(50.0, 50.0), Point::new(801.0, 100.0), bounds).is_none());
    assert!(box_preview_rect(Point::new(50.0, 50.0), Point::new(50.0, 100.0), bounds).is_none());
}

#[test]
fn keeps_latest_position_until_frame_and_respects_due_time() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().gesture.box_anchor = Some(Point::new(10.0, 10.0));
    app.queue_box_selection_preview(i, Point::new(20.0, 20.0));
    app.queue_box_selection_preview(i, Point::new(40.0, 50.0));
    assert_eq!(
        app.box_preview_pending,
        Some((Point::new(40.0, 50.0), i, app.tabs[i].id))
    );
    // A newer coalesced pointer move must run before this older preview.
    app.move_owed = Some((Point::new(60.0, 70.0), i));
    let _ = app.on_box_preview_tick();
    assert!(app.box_preview_pending.is_some());
    app.move_owed = None;
    app.box_preview_due = Some(Instant::now() + std::time::Duration::from_secs(60));
    let _ = app.on_box_preview_tick();
    assert_eq!(
        app.box_preview_pending,
        Some((Point::new(40.0, 50.0), i, app.tabs[i].id))
    );
    app.tabs[i].scene.selection.borrow_mut().gesture.box_anchor = None;
    let _ = app.on_box_preview_tick();
    assert!(app.box_preview_pending.is_none());
    assert!(app.box_preview_due.is_none());
}

#[test]
fn disabled_preview_never_queues_work() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.model_space.selection_preview = 0;
    app.tabs[i].scene.selection.borrow_mut().gesture.box_anchor = Some(Point::new(10.0, 10.0));
    app.queue_box_selection_preview(i, Point::new(30.0, 30.0));
    assert!(app.box_preview_pending.is_none());
}

#[test]
fn incremental_box_hits_match_full_pick_when_growing_shrinking_and_switching_mode() {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, Line, PolylineEdge};
    use document::types::{Vector2, Vector3};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    for n in 0..4096 {
        let x = (n % 64) as f64 * 0.6 - 19.2;
        let y = (n / 64) as f64 * 0.6 - 19.2;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 0.9, y + 0.4, 0.0);
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(line));
    }
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut boundary = BoundaryPath::new();
    boundary.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(boundary);
    app.tabs[i]
        .scene
        .add_entity(document::EntityType::Hatch(Box::new(hatch)));
    let scene = &app.tabs[i].scene;
    let wires = scene.hit_test_wires();
    let (epoch, source) = scene.interaction_index_build_key(&wires, 600.0).unwrap();
    scene.mark_interaction_index_pending(epoch, source);
    let index = Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
    assert!(scene.install_prepared_interaction_index(epoch, source, Arc::downgrade(&wires), index));

    let anchor = Point::new(650.0, 480.0);
    app.tabs[i].scene.selection.borrow_mut().gesture.box_anchor = Some(anchor);
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let points = [
        (Point::new(340.0, 260.0), false),
        (Point::new(330.0, 250.0), true),
        (Point::new(335.0, 255.0), true),
        (Point::new(660.0, 260.0), false),
        (Point::new(670.0, 250.0), true),
    ];
    for (point, expected_incremental) in points {
        app.refresh_box_selection_preview(i, point);
        let cache = app
            .box_preview_cache
            .as_ref()
            .expect("indexed flat preview");
        assert_eq!(cache.incremental, expected_incremental, "at {point:?}");
        let (full, _) =
            app.box_selection_handles(i, &None, bounds, anchor, point, point.x < anchor.x);
        let full: rustc_hash::FxHashSet<_> = full.into_iter().collect();
        assert_eq!(cache.handles, full, "incremental result at {point:?}");
    }

    // Editing the drawing invalidates the index and must force the normal
    // full pick instead of reusing hits from the previous geometry.
    let mut added = Line::new();
    added.start = Vector3::new(0.0, 0.0, 0.0);
    added.end = Vector3::new(1.0, 1.0, 0.0);
    app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(added));
    app.refresh_box_selection_preview(i, Point::new(672.0, 248.0));
    assert!(app.box_preview_cache.is_none());
}
