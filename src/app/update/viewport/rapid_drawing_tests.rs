use super::*;

use super::*;

#[test]
fn discrete_snap_preview_is_rate_limited_to_60_hz() {
    let last = Instant::now();
    let dwell = crate::app::SnapDwell {
        last_move_at: last,
        last_preview_snap_at: last,
        point: Point::new(0.0, 0.0),
        tab: 0,
        preview_dirty: false,
        snap_resolved: false,
        full_snap_due: last,
    };
    assert!(!dwell.discrete_preview_due_at(last + std::time::Duration::from_millis(15)));
    assert!(dwell.discrete_preview_due_at(last + std::time::Duration::from_millis(16)));
}

fn drawing_app(command: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.tabs[app.active_tab]
        .scene
        .selection
        .borrow_mut()
        .view
        .vp_size = (800.0, 600.0);
    app.snapper.snap_enabled = false;
    app.snapper.snap3d_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.dynamic_ucs = false;
    let _ = app.run_command_line(command);
    assert!(app.tabs[app.active_tab].active_cmd.is_some(), "{command}");
    app
}

/// Sends one pointer move. A pass that took `MOVE_COALESCE_FROM_MS` or
/// more (common on a loaded CI runner) arms the coalescing gate, which
/// would withhold the next move; these tests are about what each move
/// does, not about the gate, whose own tests arm it explicitly.
fn move_to(app: &mut OpenCADStudio, at: Point) {
    app.move_pass_due = None;
    let _ = app.update(Message::ViewportMove(at));
}

fn screen(app: &OpenCADStudio, point: glam::DVec3) -> Point {
    let p = app.tabs[app.active_tab]
        .scene
        .camera
        .borrow()
        .project(
            point,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    Point::new(p.x, p.y)
}

#[test]
fn point_modifier_mouse_moves_keep_the_parent_live_on_full_and_fast_paths() {
    use crate::command::StepInput;
    for from in [false, true] {
        let mut app = drawing_app("LINE");
        let i = app.active_tab;
        let _ = app.feed_command(StepInput::Point(glam::DVec3::ZERO));
        if from {
            app.start_from_modifier(i);
        } else {
            app.start_mtp_modifier(i);
        }
        for second_step in [false, true] {
            if second_step {
                let _ = app.feed_command(StepInput::Point(glam::DVec3::new(3.0, 7.0, 0.0)));
            }
            // No pending dwell takes the full path; the next move takes the fast path.
            app.snap_dwell = None;
            for target in [
                glam::DVec3::new(10.0, 20.0, 0.0),
                glam::DVec3::new(25.0, 30.0, 0.0),
            ] {
                let at = screen(&app, target);
                move_to(&mut app, at);
                let cursor = app.tabs[i].last_cursor_world;
                let expected = app.tabs[i]
                    .suspended_cmd
                    .as_mut()
                    .unwrap()
                    .on_preview_wires(cursor);
                let actual = &app.tabs[i].scene.preview_wires;
                assert!(!expected.is_empty());
                assert_eq!(actual.first().unwrap().points, expected[0].points);
                assert_eq!(
                    actual.len(),
                    1 + if second_step {
                        if from {
                            1
                        } else {
                            2
                        }
                    } else {
                        0
                    }
                );
            }
        }
    }
}

#[test]
fn otrack_rapid_moves_defer_and_stationary_ticks_acquire_once() {
    use crate::snap::SnapType;
    use document::{entities::Line, types::Vector3, EntityType};
    let mut app = drawing_app("LINE");
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
    app.snapper.snap_enabled = true;
    app.snapper.enabled.clear();
    app.snapper.enabled.insert(SnapType::Endpoint);
    app.snapper.otrack_enabled = true;
    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
    assert!((0.0..800.0).contains(&at.x) && (0.0..600.0).contains(&at.y));
    move_to(&mut app, at);
    move_to(&mut app, at);
    assert!(
        !app.snap_dwell.as_ref().unwrap().snap_resolved,
        "OTRACK must not bypass the movement throttle"
    );
    assert!(app.snapper.tracking_points.is_empty());

    // Model frame ticks and elapsed dwell without sleeping.
    for _ in 0..6 {
        app.snap_dwell.as_mut().unwrap().last_move_at =
            Instant::now() - std::time::Duration::from_millis(100);
        if app.snapper.dwell_since.is_some() {
            app.snapper.dwell_since = Some(Instant::now() - std::time::Duration::from_millis(300));
        }
        let _ = app.update(Message::SnapDwellTick);
        assert!(
            app.tabs[i].snap_result.is_some(),
            "settled endpoint must snap"
        );
    }
    assert_eq!(app.snapper.tracking_points.len(), 1);
    assert!((app.snapper.tracking_points[0] - glam::DVec3::new(40.0, 30.0, 0.0)).length() < 0.001);
    assert!(
        app.snap_dwell.as_ref().unwrap().snap_resolved,
        "acquired dwell must stop scheduling full snap passes"
    );
    let _ = app.update(Message::SnapDwellTick);
    assert_eq!(
        app.snapper.tracking_points.len(),
        1,
        "must not toggle twice"
    );
    let _ = app.update(Message::ViewportLeftPressAt(at));
    assert!((app.last_point.unwrap() - glam::DVec3::new(40.0, 30.0, 0.0)).length() < 0.001);
}

#[test]
fn dynamic_ucs_point_command_uses_the_exact_button_position() {
    let mut app = drawing_app("LINE");
    app.dynamic_ucs = true;
    let target = glam::DVec3::new(40.0, 30.0, 0.0);
    let at = screen(&app, target);

    let _ = app.update(Message::ViewportLeftPressAt(at));

    assert!((app.last_point.unwrap() - target).length() < 0.001);
    assert!(app.dynamic_ucs);
    assert!(app.tabs[app.active_tab].active_cmd.is_some());
}

#[test]
fn dynamic_ucs_aligns_drawing_commands_but_not_move_or_copy() {
    for (command, aligns) in [("LINE", true), ("MOVE", false), ("COPY", false)] {
        let mut app = drawing_app("LINE");
        let _ = app.run_command_line("CANCEL");
        app.tabs[app.active_tab].active_cmd = None;
        assert_eq!(
            app.automation_op(r#"{"op":"run","cmd":"BOX -50,-50,0 50,50 10"}"#)["ok"],
            true
        );
        app.tabs[app.active_tab].scene.select_all_visible();
        let _ = app.run_command_line(command);
        assert_eq!(
            app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .map(|c| c.name()),
            Some(command)
        );
        app.dynamic_ucs = true;
        let at = screen(&app, glam::DVec3::new(0.0, 0.0, 10.0));
        app.tabs[app.active_tab]
            .scene
            .selection
            .borrow_mut()
            .input
            .last_move_pos = Some(at);

        let picked = app.apply_dynamic_ucs(None);

        assert_eq!(picked.is_some(), aligns, "{command}");
        assert_eq!(
            app.tabs[app.active_tab].active_ucs.is_some(),
            aligns,
            "{command}"
        );
    }
}

#[test]
fn ortho_in_a_3d_view_runs_up_the_z_axis_and_keeps_that_level() {
    let mut app = drawing_app("LINE");
    let _ = app.apply_view_preset("SWISO");
    app.ortho_mode = true;
    let click = |app: &mut OpenCADStudio, aim: glam::DVec3| {
        let at = screen(app, aim);
        move_to(app, at);
        let _ = app.update(Message::ViewportLeftPressAt(at));
        let _ = app.update(Message::ViewportLeftRelease);
        app.last_point.unwrap()
    };
    let start = click(&mut app, glam::DVec3::ZERO);
    // Straight up: exactly on the axis, as high as the cursor shows.
    let up = click(&mut app, glam::DVec3::new(0.5, 0.0, 50.0));
    assert!(
        (up - start).truncate().length() < 1e-9,
        "{start:?} -> {up:?}"
    );
    assert!((up.z - 50.0).abs() < 0.5, "{up:?}");
    // Then along X at that level, not back down to the ground plane.
    let across = click(&mut app, glam::DVec3::new(30.0, 0.5, up.z));
    assert!((across.y - up.y).abs() < 1e-9, "{across:?}");
    assert!((across.z - up.z).abs() < 1e-6, "{across:?}");
    assert!((across.x - 30.0).abs() < 0.5, "{across:?}");
}

#[test]
fn rapid_drawing_zigzag_uses_button_positions_without_hover_or_duplicate_release() {
    for command in ["LINE", "PLINE"] {
        for pane in [false, true] {
            let mut app = drawing_app(command);
            for (x, y) in [(40.0, 30.0), (80.0, 60.0), (120.0, 30.0), (160.0, 60.0)] {
                let target = glam::DVec3::new(x, y, 0.0);
                let old = screen(&app, target + glam::DVec3::new(-15.0, 10.0, 0.0));
                move_to(&mut app, old);
                let at = screen(&app, target);
                let _ = app.update(if pane {
                    Message::PanePressAt(0, at)
                } else {
                    Message::ViewportLeftPressAt(at)
                });
                assert!(
                    (app.last_point.unwrap() - target).length() < 0.001,
                    "{command} pane={pane}: {:?} != {target:?}",
                    app.last_point
                );
                assert!(
                    app.snap_dwell
                        .as_ref()
                        .is_some_and(|dwell| dwell.snap_resolved),
                    "{command} pane={pane}: accepted click should leave the snap gate resolved"
                );
                let after_press = app.last_point;
                move_to(&mut app, old);
                let _ = app.update(Message::ViewportLeftRelease);
                assert_eq!(app.last_point, after_press);
            }
            let _ = app.feed_command(crate::command::StepInput::Enter);
            let entities: Vec<_> = app.tabs[app.active_tab].scene.document.entities().collect();
            assert_eq!(entities.len(), if command == "LINE" { 3 } else { 1 });
        }
    }
}

#[test]
fn rapid_drawing_preview_coalesces_moves_and_flushes_latest_position() {
    for command in ["LINE", "PLINE", "RECTANG", "CIRCLE"] {
        let mut app = drawing_app(command);
        let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
        let _ = app.update(Message::ViewportLeftPressAt(at));
        let at = screen(&app, glam::DVec3::new(50.0, 40.0, 0.0));
        move_to(&mut app, at);
        let i = app.active_tab;
        app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
        let _ = app.on_snap_dwell_tick();
        assert!(!app.tabs[i].scene.preview_wires.is_empty(), "{command}");
        let before = app.tabs[i].scene.preview_wires[0].points.clone();
        for x in 60..90 {
            let at = screen(&app, glam::DVec3::new(f64::from(x), 55.0, 0.0));
            move_to(&mut app, at);
        }
        assert_eq!(
            app.tabs[i].scene.preview_wires[0].points, before,
            "{command}"
        );
        assert!(app.snap_dwell.as_ref().unwrap().preview_dirty);
        let latest = app.tabs[i].last_cursor_world;
        app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
        let _ = app.on_snap_dwell_tick();
        assert!(!app.snap_dwell.as_ref().unwrap().preview_dirty);
        assert_ne!(
            app.tabs[i].scene.preview_wires[0].points, before,
            "{command}"
        );
        assert_eq!(app.tabs[i].last_cursor_world, latest);
        // Expire without sleeping: the full pass must finish in this
        // update, not emit an old ViewportMove for the daemon to replay.
        app.snap_dwell.as_mut().unwrap().last_move_at =
            Instant::now() - std::time::Duration::from_millis(100);
        let _ = app.on_snap_dwell_tick();
        assert!(app.snap_dwell.as_ref().unwrap().snap_resolved);
        assert!((app.tabs[i].last_cursor_world - latest).length() < 0.001);
        let resolved_point = app.tabs[i].last_cursor_world;
        let _ = app.on_snap_dwell_tick();
        assert!(app.snap_dwell.as_ref().unwrap().snap_resolved);
        assert_eq!(app.tabs[i].last_cursor_world, resolved_point);
    }
}

/// A drawing command's live preview keeps the colour of the object it
/// makes on both passes that redraw it while the pointer moves: the
/// per-frame refresh between moves must not put back the preview cyan
/// that the full pointer-move pass replaced, or the preview flickers.
#[test]
fn drawing_preview_keeps_the_new_object_colour_on_full_and_fast_passes() {
    use crate::scene::WireModel;
    for command in ["LINE", "PLINE", "ARC", "CIRCLE", "XLINE", "SPLINE"] {
        let mut app = drawing_app(command);
        let i = app.active_tab;
        let want = app
            .creation_preview_color(i)
            .unwrap_or_else(|| panic!("{command} draws a new object"));
        assert_ne!(want, WireModel::CYAN, "{command}");
        let colours = |app: &OpenCADStudio| -> Vec<[f32; 4]> {
            app.tabs[i]
                .scene
                .preview_wires
                .iter()
                .map(|wire| wire.color)
                .collect()
        };
        let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
        let _ = app.update(Message::ViewportLeftPressAt(at));
        for (step, x) in [70.0, 75.0, 80.0, 85.0].into_iter().enumerate() {
            let at = screen(&app, glam::DVec3::new(x, 50.0, 0.0));
            move_to(&mut app, at);
            // The pointer is still moving: the frame tick rebuilds only
            // the command preview.
            let dwell = app.snap_dwell.as_mut().unwrap();
            assert!(dwell.preview_dirty, "{command} step {step}");
            dwell.last_move_at = Instant::now();
            dwell.full_snap_due = Instant::now() + std::time::Duration::from_secs(60);
            let _ = app.on_snap_dwell_tick();
            let fast = colours(&app);
            assert!(!fast.is_empty(), "{command} step {step}: fast pass");
            assert!(
                fast.contains(&want) && !fast.contains(&WireModel::CYAN),
                "{command} step {step}: fast pass drew {fast:?}, want {want:?}"
            );
            // The pointer settles: the full pointer-move pass runs.
            app.snap_dwell.as_mut().unwrap().last_move_at =
                Instant::now() - std::time::Duration::from_millis(100);
            let _ = app.on_snap_dwell_tick();
            let full = colours(&app);
            assert!(!full.is_empty(), "{command} step {step}: full pass");
            assert!(
                full.contains(&want) && !full.contains(&WireModel::CYAN),
                "{command} step {step}: full pass drew {full:?}, want {want:?}"
            );
        }
    }
}

/// The interaction index is built from the hover dwell, which does not
/// run while a command is active. A command's snaps and picks query that
/// index too: once an edit (a LAYOFF pick) replaced the wire set, every
/// move and click of the command scanned the whole drawing until the
/// command ended. A move during a command asks for the index itself.
#[test]
fn a_move_during_a_command_asks_for_the_interaction_index() {
    use crate::modules::draw::layers::layoff::LayerOffCommand;
    use document::{entities::Line, types::Vector3, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    // More wires than the index threshold, so the drawing wants one.
    for k in 0..5_000 {
        let y = k as f64 * 0.01;
        app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(10.0, y, 0.0),
            )));
    }
    app.tabs[i].active_cmd = Some(Box::new(LayerOffCommand::default()));
    assert!(app.active_interaction_index.is_none());

    let _ = app.on_viewport_move(iced::Point::new(400.0, 300.0));
    assert!(
        app.active_interaction_index.is_some(),
        "a move during a command should start the interaction index build"
    );
}

#[test]
fn stretch_cursor_move_keeps_live_lines_visible_even_during_fast_pointer_updates() {
    use crate::command::{CadCommand, CmdResult};
    use crate::modules::draw::modify::stretch::StretchCommand;
    use document::{entities::Line, types::Vector3, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;

    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
    let wires = app.tabs[i].scene.wire_models_for(&[handle]);
    let window = (
        glam::DVec3::new(39.0, 29.0, -1.0),
        glam::DVec3::new(81.0, 31.0, 1.0),
    );
    let base = glam::DVec3::new(40.0, 30.0, 0.0);
    let mut cmd = StretchCommand::preselected(vec![handle], wires, window);
    assert!(matches!(cmd.on_point(base), CmdResult::NeedPoint));
    app.tabs[i].active_cmd = Some(Box::new(cmd));

    let preview_has_endpoint = |app: &OpenCADStudio, x: f32, y: f32| {
        app.tabs[i].scene.preview_wires.iter().any(|wire| {
            wire.points
                .iter()
                .any(|pt| (pt[0] - x).abs() < 0.1 && (pt[1] - y).abs() < 0.1)
        })
    };
    let first = glam::DVec3::new(45.0, 40.0, 0.0);
    let _ = app.on_viewport_move(screen(&app, first));
    assert!(
        preview_has_endpoint(&app, 85.0, 40.0),
        "first cursor move must show the translated STRETCH line, not just its guide"
    );

    // A subsequent move normally takes the low-cost SnapDwell path.
    // Make the 60 Hz overlay budget due without sleeping.
    let dwell = app.snap_dwell.as_mut().expect("STRETCH dwell");
    dwell.last_preview_snap_at = Instant::now() - std::time::Duration::from_millis(100);
    let second = glam::DVec3::new(48.0, 42.0, 0.0);
    let _ = app.on_viewport_move(screen(&app, second));
    assert!(
        preview_has_endpoint(&app, 88.0, 42.0),
        "fast cursor move must update the STRETCH geometry, not hide it"
    );
}

/// Shift held through several PLINE points: each segment takes the axis
/// it is drawn along, without releasing Shift in between (#1422).
#[test]
fn shift_axis_lock_follows_each_new_segment() {
    let mut app = drawing_app("PLINE");
    let start = screen(&app, glam::DVec3::ZERO);
    let _ = app.update(Message::ViewportLeftPressAt(start));
    let _ = app.update(Message::ViewportLeftRelease);
    app.shift_down = true;
    app.snap_dwell = None;
    let at = screen(&app, glam::DVec3::new(40.0, 3.0, 0.0));
    move_to(&mut app, at);
    let first = app.axis_lock_dir.expect("locked to the first axis");
    assert!(first.x.abs() > 0.99, "{first:?}");
    let _ = app.update(Message::ViewportLeftPressAt(at));
    let _ = app.update(Message::ViewportLeftRelease);
    // Shift is still down: the next segment goes up.
    // The first move after the click drifts on along the old axis.
    for to in [
        glam::DVec3::new(40.4, 0.05, 0.0),
        glam::DVec3::new(43.0, 40.0, 0.0),
    ] {
        app.snap_dwell = None;
        let at = screen(&app, to);
        move_to(&mut app, at);
    }
    let second = app.axis_lock_dir.expect("locked to the second axis");
    assert!(second.y.abs() > 0.99, "{second:?}");
}

#[test]
fn rapid_drafting_polar_matches_authoritative_point() {
    for (target, engaged) in [
        (glam::DVec3::new(40.0, 0.25, 0.0), true),
        (glam::DVec3::new(40.0, 20.0, 0.0), false),
    ] {
        let exercise = |force_full: bool| {
            let mut app = drawing_app("LINE");
            let i = app.active_tab;
            let start = screen(&app, glam::DVec3::ZERO);
            let _ = app.update(Message::ViewportLeftPressAt(start));
            app.polar_mode = true;
            app.polar_increment_deg = 90.0;
            let first = screen(&app, glam::DVec3::new(20.0, 0.0, 0.0));
            move_to(&mut app, first);
            assert!(app.snap_dwell.is_some());
            if force_full {
                app.snap_dwell = None;
            }
            let at = screen(&app, target);
            move_to(&mut app, at);
            assert_eq!(app.last_point, Some(glam::DVec3::ZERO));
            assert!(app.tabs[i].active_cmd.is_some());
            if !force_full {
                assert!(!app.snap_dwell.as_ref().unwrap().snap_resolved);
            }
            app.tabs[i].last_cursor_world
        };
        let full = exercise(true);
        let rapid = exercise(false);
        assert!(
            rapid.distance(full) < 0.001,
            "engaged={engaged}: {rapid:?} != {full:?}"
        );
        if engaged {
            assert!(rapid.y.abs() < 0.001);
        } else {
            assert!(rapid.distance(target) < 0.001);
        }
    }
}

#[test]
fn rapid_drafting_3d_matches_authoritative_z_axis() {
    for polar in [false, true] {
        let exercise = |force_full: bool| {
            let mut app = drawing_app("LINE");
            let i = app.active_tab;
            let _ = app.apply_view_preset("SWISO");
            let start = screen(&app, glam::DVec3::ZERO);
            let _ = app.update(Message::ViewportLeftPressAt(start));
            let anchor = app.last_point.expect("initial LINE point");
            app.ortho_mode = !polar;
            app.polar_mode = polar;
            app.polar_increment_deg = 90.0;
            let first = screen(&app, glam::DVec3::new(0.0, 0.0, 10.0));
            move_to(&mut app, first);
            assert!(app.snap_dwell.is_some());
            if force_full {
                app.snap_dwell = None;
            }
            let at = screen(&app, glam::DVec3::new(0.0, 0.0, 20.0));
            move_to(&mut app, at);
            assert_eq!(app.last_point, Some(anchor));
            (app.tabs[i].last_cursor_world, app.otrack_kind)
        };
        let full = exercise(true);
        let rapid = exercise(false);
        assert!(
            rapid.0.distance(full.0) < 0.001,
            "polar={polar}: {rapid:?} != {full:?}"
        );
        assert_eq!(rapid.1, full.1);
        assert!(rapid.0.distance(glam::DVec3::new(0.0, 0.0, 20.0)) < 0.001);
        assert_eq!(rapid.1, Some(crate::snap::TrackingKind::PlusZ));
    }
}

#[test]
fn throttle_axis_lock_matches_authoritative_move() {
    for explicit_angle in [false, true] {
        let exercise = |force_full: bool| {
            let mut app = drawing_app("LINE");
            // Shift is the axis lock with ortho off; with it on, Shift frees.
            app.ortho_mode = explicit_angle;
            let i = app.active_tab;
            let start = screen(&app, glam::DVec3::ZERO);
            let _ = app.update(Message::ViewportLeftPressAt(start));
            app.shift_down = !explicit_angle;
            if explicit_angle {
                app.angle_override = Some(glam::DVec3::X);
            }
            app.snap_dwell = None;
            let at = screen(&app, glam::DVec3::new(40.0, 10.0, 0.0));
            move_to(&mut app, at);
            assert!(app.axis_lock_dir.is_some());
            if force_full {
                app.snap_dwell = None;
            }
            let at = screen(&app, glam::DVec3::new(10.0, 40.0, 0.0));
            move_to(&mut app, at);
            (app.tabs[i].last_cursor_world, app.axis_lock_dir)
        };
        let full = exercise(true);
        let moving = exercise(false);
        assert!(
            moving.0.distance(full.0) < 0.001,
            "explicit angle={explicit_angle}: {moving:?} != {full:?}"
        );
        assert_eq!(moving.1, full.1);
    }
}

#[test]
fn throttle_axis_command_matches_authoritative_move() {
    struct AxisCommand;
    impl crate::command::CadCommand for AxisCommand {
        fn name(&self) -> &'static str {
            "AXIS_TEST"
        }
        fn prompt(&self) -> String {
            "Point on axis:".into()
        }
        fn on_enter(&mut self) -> crate::command::CmdResult {
            crate::command::CmdResult::NeedPoint
        }
        fn on_point(&mut self, _: glam::DVec3) -> crate::command::CmdResult {
            crate::command::CmdResult::NeedPoint
        }
        fn cursor_axis(&self) -> Option<(glam::DVec3, glam::DVec3)> {
            Some((glam::DVec3::ZERO, glam::DVec3::X))
        }
    }
    let exercise = |force_full: bool| {
        let mut app = drawing_app("LINE");
        app.ortho_mode = true;
        let i = app.active_tab;
        app.tabs[i].active_cmd = Some(Box::new(AxisCommand));
        let at = screen(&app, glam::DVec3::new(40.0, 10.0, 0.0));
        move_to(&mut app, at);
        if force_full {
            app.snap_dwell = None;
        }
        let at = screen(&app, glam::DVec3::new(10.0, 40.0, 0.0));
        move_to(&mut app, at);
        app.tabs[i].last_cursor_world
    };
    assert!(exercise(false).distance(exercise(true)) < 0.001);
}

#[test]
fn dimension_and_constrained_moves_coalesce_and_flush_preview() {
    let mut app = drawing_app("DIMLINEAR");
    app.ortho_mode = true;
    let i = app.active_tab;
    let first = screen(&app, glam::DVec3::new(0.0, 0.0, 0.0));
    let _ = app.update(Message::ViewportLeftPressAt(first));
    let second = screen(&app, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.update(Message::ViewportLeftPressAt(second));
    let start = screen(&app, glam::DVec3::new(40.0, 15.0, 0.0));
    move_to(&mut app, start);
    app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
    let _ = app.on_snap_dwell_tick();
    assert!(!app.tabs[i].scene.preview_wires.is_empty());
    let bits = |wires: &[crate::scene::WireModel]| -> Vec<[u32; 3]> {
        wires[0]
            .points
            .iter()
            .map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
            .collect()
    };
    let initial = bits(&app.tabs[i].scene.preview_wires);
    for y in 20..35 {
        let at = screen(&app, glam::DVec3::new(40.0, f64::from(y), 0.0));
        move_to(&mut app, at);
    }
    assert!(
        app.snap_dwell.as_ref().unwrap().preview_dirty,
        "rapid constrained dimension moves must defer full snap/preview rebuild"
    );
    assert!(!app.snap_dwell.as_ref().unwrap().snap_resolved);
    assert_eq!(bits(&app.tabs[i].scene.preview_wires), initial);
    app.snap_dwell.as_mut().unwrap().last_move_at = Instant::now();
    let _ = app.on_snap_dwell_tick();
    assert!(!app.snap_dwell.as_ref().unwrap().preview_dirty);
    assert_ne!(bits(&app.tabs[i].scene.preview_wires), initial);
}

#[test]
fn otrack_departure_resets_acquired_latch_after_removing_final_reference() {
    use crate::snap::SnapType;
    use document::{entities::Line, types::Vector3, EntityType};
    let mut app = drawing_app("LINE");
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
    app.snapper.snap_enabled = true;
    app.snapper.enabled.clear();
    app.snapper.enabled.insert(SnapType::Endpoint);
    app.snapper.otrack_enabled = true;
    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");

    let at = screen(&app, glam::DVec3::new(40.0, 30.0, 0.0));
    let away = screen(&app, glam::DVec3::new(40.0, 100.0, 0.0));
    let at_jitter = Point::new(at.x + 1.0, at.y + 1.0);

    // 1. Move to A and stationary dwell to acquire A.
    move_to(&mut app, at);
    for _ in 0..6 {
        app.snap_dwell.as_mut().unwrap().last_move_at =
            Instant::now() - std::time::Duration::from_millis(100);
        if app.snapper.dwell_since.is_some() {
            app.snapper.dwell_since = Some(Instant::now() - std::time::Duration::from_millis(300));
        }
        let _ = app.update(Message::SnapDwellTick);
    }
    assert_eq!(
        app.snapper.tracking_points.len(),
        1,
        "point A must be acquired initially"
    );

    // Small jitter within aperture must not toggle or reset.
    move_to(&mut app, at_jitter);
    let _ = app.update(Message::SnapDwellTick);
    assert_eq!(
        app.snapper.tracking_points.len(),
        1,
        "jitter within aperture must not toggle"
    );

    // 2. Leave and return.
    move_to(&mut app, away);
    move_to(&mut app, at);

    // 3. Stationary dwell removes final point A.
    for _ in 0..6 {
        app.snap_dwell.as_mut().unwrap().last_move_at =
            Instant::now() - std::time::Duration::from_millis(100);
        if app.snapper.dwell_since.is_some() {
            app.snapper.dwell_since = Some(Instant::now() - std::time::Duration::from_millis(300));
        }
        let _ = app.update(Message::SnapDwellTick);
    }
    assert_eq!(
        app.snapper.tracking_points.len(),
        0,
        "dwelling on A again removes final reference"
    );

    // 4. Rapid departure and return.
    move_to(&mut app, away);
    move_to(&mut app, at);

    // 5. Stationary dwell must reacquire A.
    for _ in 0..6 {
        if let Some(dwell) = app.snap_dwell.as_mut() {
            dwell.last_move_at = Instant::now() - std::time::Duration::from_millis(100);
        }
        if app.snapper.dwell_since.is_some() {
            app.snapper.dwell_since = Some(Instant::now() - std::time::Duration::from_millis(300));
        }
        let _ = app.update(Message::SnapDwellTick);
    }
    assert_eq!(
        app.snapper.tracking_points.len(),
        1,
        "point A must reacquire after departure and stationary dwell"
    );
}

#[test]
fn parallel_snap_acquires_reference_on_line_interior() {
    use crate::snap::SnapType;
    use document::{entities::Line, types::Vector3, EntityType};
    let mut app = drawing_app("LINE");
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(40.0, 30.0, 0.0),
            Vector3::new(80.0, 30.0, 0.0),
        )));
    app.snapper.snap_enabled = true;
    app.snapper.enabled.clear();
    app.snapper.enabled.insert(SnapType::Parallel);
    app.snapper.otrack_enabled = true;
    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");

    let interior = screen(&app, glam::DVec3::new(60.0, 30.0, 0.0));
    assert!((0.0..800.0).contains(&interior.x) && (0.0..600.0).contains(&interior.y));

    move_to(&mut app, interior);
    std::thread::sleep(std::time::Duration::from_millis(160));
    move_to(&mut app, interior);

    assert!(
        app.snapper.parallel_ref.is_some(),
        "Parallel snap must acquire reference on line interior when Parallel is enabled"
    );
    let (dir, pt) = app.snapper.parallel_ref.unwrap();
    assert!((dir.x.abs() - 1.0).abs() < 0.001);
    assert!((pt.y - 30.0).abs() < 0.001);
}

/// While a draw command waits for its next point, wheel-zooming moves the
/// dynamic-input guide anchor with the view instead of leaving it at its
/// old pixel until the next cursor move.
#[test]
fn dynamic_input_guide_anchor_follows_a_wheel_zoom() {
    use crate::command::StepInput;
    let mut app = drawing_app("CIRCLE");
    let i = app.active_tab;
    let centre = glam::DVec3::new(10.0, 10.0, 0.0);
    let _ = app.feed_command(StepInput::Point(centre));
    let at = screen(&app, glam::DVec3::new(30.0, 25.0, 0.0));
    move_to(&mut app, at);
    let before = app.tabs[i].last_point_screen.expect("guide anchor");

    app.touchpad_pan = false;
    app.ctrl_down = false;
    app.zoom_wheel_reversed = false;
    let _ = app.update(Message::ViewportScroll(iced::mouse::ScrollDelta::Lines {
        x: 0.0,
        y: 3.0,
    }));

    let after = app.tabs[i].last_point_screen.expect("guide anchor");
    let expected = screen(&app, centre);
    assert!(
        (after.x - expected.x).abs() < 0.01 && (after.y - expected.y).abs() < 0.01,
        "anchor {after:?} must sit on the projected centre {expected:?}"
    );
    assert!(
        (after.x - before.x).abs() > 1.0 || (after.y - before.y).abs() > 1.0,
        "the zoom must have moved the anchor"
    );
}

fn rubber_band_drawn(app: &OpenCADStudio) -> bool {
    let (segment, wires) = app.tabs[app.active_tab].scene.shown_rubber_band();
    segment.is_some() || !wires.is_empty()
}

/// While the middle button drags the view, a command draws neither its
/// rubber band nor its guide; the release brings both back.
#[test]
fn a_middle_button_pan_hides_the_drawing_feedback_until_release() {
    use crate::command::StepInput;
    let mut app = drawing_app("CIRCLE");
    let i = app.active_tab;
    let _ = app.feed_command(StepInput::Point(glam::DVec3::new(10.0, 10.0, 0.0)));
    let from = screen(&app, glam::DVec3::new(30.0, 25.0, 0.0));
    move_to(&mut app, from);
    assert!(app.tabs[i].scene.drawing_feedback_shown());
    assert!(rubber_band_drawn(&app), "the circle follows the cursor");

    let _ = app.update(Message::ViewportMiddlePress);
    move_to(&mut app, Point::new(from.x + 120.0, from.y - 60.0));
    assert!(
        !app.tabs[i].scene.drawing_feedback_shown(),
        "a view being dragged shows no guide"
    );
    assert!(
        !rubber_band_drawn(&app),
        "a view being dragged shows no rubber band"
    );

    let _ = app.update(Message::ViewportMiddleRelease);
    assert!(app.tabs[i].scene.drawing_feedback_shown());
    assert!(rubber_band_drawn(&app));
}

/// The release of a pan puts the guide and the rubber band back under
/// the cursor, without waiting for the mouse to move again.
#[test]
fn a_pan_release_puts_the_drawing_feedback_back_under_the_cursor() {
    use crate::command::StepInput;
    let mut app = drawing_app("CIRCLE");
    let i = app.active_tab;
    let centre = glam::DVec3::new(10.0, 10.0, 0.0);
    let _ = app.feed_command(StepInput::Point(centre));
    let from = screen(&app, glam::DVec3::new(30.0, 25.0, 0.0));
    move_to(&mut app, from);
    assert_eq!(app.tabs[i].last_cursor_screen, from);
    let before = app.tabs[i].last_point_screen.expect("guide anchor");

    let _ = app.update(Message::ViewportMiddlePress);
    let to = Point::new(from.x + 120.0, from.y - 60.0);
    move_to(&mut app, to);
    let _ = app.update(Message::ViewportMiddleRelease);
    let near = |a: Point, b: Point| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01;
    let cursor = app.tabs[i].last_cursor_screen;
    assert!(
        near(cursor, to),
        "the guide ends at the cursor {to:?}, not at {cursor:?}"
    );
    let anchor = app.tabs[i].last_point_screen.expect("guide anchor");
    let expected = screen(&app, centre);
    assert!(
        near(anchor, expected),
        "anchor {anchor:?} must sit on the projected centre {expected:?}"
    );
    assert!(
        (anchor.x - before.x).abs() > 1.0 || (anchor.y - before.y).abs() > 1.0,
        "the pan must have moved the view"
    );
    let end = screen(&app, app.tabs[i].last_cursor_world);
    assert!(
        near(end, to),
        "the rubber band ends under the cursor {to:?}, not at {end:?}"
    );
}
