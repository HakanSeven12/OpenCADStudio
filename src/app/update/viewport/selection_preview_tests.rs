use super::*;

use super::*;
use crate::app::{HoverDwell, OpenCADStudio, HOVER_DWELL_MS};

fn mtext_size_fixture(rotation: f64) -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let mut text = document::entities::MText::with_value(
        "asd asd asd asd",
        document::types::Vector3::new(100.0, 50.0, 0.0),
    );
    text.height = 2.5;
    text.rectangle_width = 60.0;
    text.rectangle_height = Some(10.0);
    text.rotation = rotation;
    text.attachment_point = document::entities::AttachmentPoint::TopLeft;
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::MText(Box::new(text)));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    (app, handle)
}

fn mtext_size_value(app: &OpenCADStudio, handle: Handle) -> &document::entities::MText {
    let Some(document::EntityType::MText(text)) =
        app.tabs[app.active_tab].scene.document.get_entity(handle)
    else {
        panic!("expected MTEXT")
    };
    text
}

fn drag_mtext_size(app: &mut OpenCADStudio, id: usize, points: &[glam::DVec3]) {
    let i = app.active_tab;
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| g.id == id)
        .unwrap()
        .clone();
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(grip.world, bounds)
        .unwrap();
    app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
        Some(Point::new(cursor.x, cursor.y));
    let _ = app.on_viewport_left_press();
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|g| g.grip_id),
        Some(id)
    );
    for point in points {
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(*point, bounds)
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
    }
    let _ = app.on_viewport_left_release();
    assert!(app.tabs[i].active_grip.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
}

#[test]
fn mtext_size_width_drag_rewraps_and_undo_restores() {
    for rotation in [0.0, std::f64::consts::FRAC_PI_2] {
        let (mut app, handle) = mtext_size_fixture(rotation);
        let before = mtext_size_value(&app, handle).clone();
        let line_count = crate::entities::mtext::layout_of(
            &before,
            &app.tabs[app.active_tab].scene.document,
            false,
        )
        .line_count;
        let x = glam::DVec3::new(rotation.cos(), rotation.sin(), 0.0);
        let down = glam::DVec3::new(rotation.sin(), -rotation.cos(), 0.0);
        let origin = glam::DVec3::new(100.0, 50.0, 0.0);
        drag_mtext_size(
            &mut app,
            1,
            &[
                origin + x * 20.0 + down * 3.0,
                origin + x * 8.0 + down * 5.0,
            ],
        );
        let text = mtext_size_value(&app, handle);
        assert!(
            (text.rectangle_width - 8.0).abs() < 1e-3,
            "width = {}",
            text.rectangle_width
        );
        assert_eq!(text.insertion_point, before.insertion_point);
        assert!(
            crate::entities::mtext::layout_of(
                text,
                &app.tabs[app.active_tab].scene.document,
                false,
            )
            .line_count
                > line_count
        );
        app.undo_steps(1);
        assert_eq!(mtext_size_value(&app, handle), &before);
        let grip = app.tabs[app.active_tab]
            .selected_grips
            .iter()
            .find(|g| g.id == 1)
            .unwrap();
        assert_eq!(grip.shape, crate::scene::model::object::GripShape::Triangle);
        assert!((grip.dir.expect("width arrow direction") - x).length() < 1e-9);
    }
}

#[test]
fn mtext_size_height_drag_and_undo_restores() {
    for rotation in [0.0, std::f64::consts::FRAC_PI_2] {
        let (mut app, handle) = mtext_size_fixture(rotation);
        let before = mtext_size_value(&app, handle).clone();
        let x = glam::DVec3::new(rotation.cos(), rotation.sin(), 0.0);
        let down = glam::DVec3::new(rotation.sin(), -rotation.cos(), 0.0);
        let origin = glam::DVec3::new(100.0, 50.0, 0.0);
        drag_mtext_size(
            &mut app,
            2,
            &[
                origin + down * 20.0 + x * 3.0,
                origin + down * 30.0 + x * 5.0,
            ],
        );
        let text = mtext_size_value(&app, handle);
        assert!(
            (text.rectangle_height.unwrap() - 30.0).abs() < 1e-3,
            "height = {:?}",
            text.rectangle_height
        );
        assert_eq!(text.insertion_point, before.insertion_point);
        assert_eq!(text.rectangle_width, before.rectangle_width);
        app.undo_steps(1);
        assert_eq!(mtext_size_value(&app, handle), &before);
        let grip = app.tabs[app.active_tab]
            .selected_grips
            .iter()
            .find(|g| g.id == 2)
            .unwrap();
        assert!((grip.dir.expect("height arrow direction") - down).length() < 1e-9);
    }
}

#[test]
fn mtext_size_width_drag_clamps_before_insertion() {
    let (mut app, handle) = mtext_size_fixture(0.0);
    let before = mtext_size_value(&app, handle).clone();
    drag_mtext_size(&mut app, 1, &[glam::DVec3::new(80.0, 55.0, 0.0)]);
    assert_eq!(mtext_size_value(&app, handle).rectangle_width, 0.01);
    app.undo_steps(1);
    assert_eq!(mtext_size_value(&app, handle), &before);
    drag_mtext_size(&mut app, 2, &[glam::DVec3::new(105.0, 70.0, 0.0)]);
    assert_eq!(mtext_size_value(&app, handle).rectangle_height, Some(0.01));
    app.undo_steps(1);
    assert_eq!(mtext_size_value(&app, handle), &before);
}

#[test]
fn lower_endpoint_drag_previews_connected_tangent_profile_and_keeps_top_fixed() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    use document::{
        entities::{Arc, Line},
        types::Vector3,
        EntityType,
    };

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.constraint_solve_mode = true;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let scene = &mut app.tabs[i].scene;
    let [outer, inner, upper, lower] = [
        ([0.0, 0.0], [0.0, 5.0]),
        ([-1.0, -1.0], [-1.0, 5.0]),
        ([4.0, 0.0], [0.0, 0.0]),
        ([4.0, -1.0], [-1.0, -1.0]),
    ]
    .map(|(a, b)| {
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        )))
    });
    let top = scene.add_entity(EntityType::Arc(Arc::from_coords(
        -0.5,
        5.0,
        0.0,
        0.5,
        0.0,
        std::f64::consts::PI,
    )));
    let right = scene.add_entity(EntityType::Arc(Arc::from_coords(
        4.0,
        -0.5,
        0.0,
        0.5,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    )));
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    for (a, ai, b, bi) in [
        (outer, 1, top, 0),
        (top, 1, inner, 1),
        (outer, 0, upper, 1),
        (upper, 0, right, 1),
        (inner, 0, lower, 1),
        (right, 0, lower, 0),
    ] {
        set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(a, ai), ParametricRef::point(b, bi)],
            None,
        );
    }
    for (kind, a, b) in [
        (ConstraintKind::Parallel, outer, inner),
        (ConstraintKind::Parallel, upper, lower),
        (ConstraintKind::Tangent, outer, top),
        (ConstraintKind::Tangent, inner, top),
        (ConstraintKind::Tangent, upper, right),
        (ConstraintKind::Tangent, lower, right),
        (ConstraintKind::Equal, top, right),
    ] {
        set.add(
            kind,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
    }
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(lower)],
        None,
    );
    let handles = if let Ok(path) = std::env::var("OCS_GRIP_TEST_DRAWING") {
        let result = app.automation_op(&serde_json::json!({"op":"open","path":path}).to_string());
        assert_eq!(result["ok"], true, "{result}");
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        [0x57F, 0x57D, 0x57C, 0x57A, 0x57E, 0x57B].map(Handle::new)
    } else {
        [outer, inner, upper, lower, top, right]
    };
    let inner = handles[1];
    let geometry = |app: &OpenCADStudio| {
        handles.map(|handle| {
            app.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .unwrap()
                .clone()
        })
    };
    let before = geometry(&app);
    let EntityType::Line(original_line) = &before[1] else {
        panic!("line")
    };
    let lower_endpoint = original_line.start;
    let upper_endpoint = original_line.end;
    let EntityType::Arc(original_arc) = &before[4] else {
        panic!("arc")
    };
    let radius = original_arc.radius;
    app.tabs[i].scene.selected.insert(inner);
    app.refresh_selected_grips();
    app.tabs[i].active_grip = Some(GripEdit::single(
        inner,
        0,
        false,
        glam::DVec3::new(lower_endpoint.x, lower_endpoint.y, 0.0),
    ));
    let check_profile = |app: &OpenCADStudio| {
        let entities = geometry(app);
        let EntityType::Line(line) = &entities[1] else {
            panic!("line")
        };
        assert_eq!(line.end, upper_endpoint, "top endpoint drifted");
        let EntityType::Arc(arc) = &entities[4] else {
            panic!("arc")
        };
        assert!(
            (arc.end_point() - line.end).length() < 1e-7,
            "top arc detached"
        );
        for index in [4, 5] {
            let EntityType::Arc(arc) = &entities[index] else {
                panic!("arc")
            };
            assert!((arc.radius - radius).abs() < 1e-7, "parallel gap changed");
        }
        for (line_index, arc_index, marker) in [(0, 4, 0), (1, 4, 1), (2, 5, 1), (3, 5, 0)] {
            let EntityType::Line(line) = &entities[line_index] else {
                panic!("line")
            };
            let EntityType::Arc(arc) = &entities[arc_index] else {
                panic!("arc")
            };
            let contact = if marker == 0 {
                arc.start_point()
            } else {
                arc.end_point()
            };
            let endpoint = if line_index < 2 { line.end } else { line.start };
            assert!(
                (contact - endpoint).length() < 1e-7,
                "tangent contact detached"
            );
            let direction = line.end - line.start;
            assert!(
                direction.dot(&(contact - arc.center)).abs() < 1e-7,
                "tangency lost: line={line:?}, arc={arc:?}, error={}",
                direction.dot(&(contact - arc.center))
            );
        }
    };
    for offset in [[-0.4, -0.5], [-1.0, -1.0], [-1.0, -1.0], [0.3, 0.4]] {
        let target = [lower_endpoint.x + offset[0], lower_endpoint.y + offset[1]];
        let cursor = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                glam::DVec3::new(target[0], target[1], 0.0),
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .unwrap();
        let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
        check_profile(&app);
        let EntityType::Line(line) = app.tabs[i].scene.document.get_entity(inner).unwrap() else {
            panic!("line")
        };
        assert!((line.start - Vector3::new(target[0], target[1], 0.0)).length() < 1e-5);
        for handle in handles {
            assert!(
                app.tabs[i]
                    .scene
                    .preview_wires
                    .iter()
                    .any(|wire| wire.name == handle.value().to_string() && wire.color[3] > 0.9),
                "connected entity {handle:?} missing from preview"
            );
        }
    }
    let preview = geometry(&app);
    let _ = app.on_viewport_left_release();
    check_profile(&app);
    assert_eq!(
        geometry(&app),
        preview,
        "release changed the preview solution"
    );
    app.undo_steps(1);
    assert_eq!(geometry(&app), before);
    app.redo_steps(1);
    assert_eq!(geometry(&app), preview);
}

#[test]
fn grip_moves_keep_perpendicular_constraints_live_and_undoable() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
    use document::{entities::Line, types::Vector3, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let a = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(-10.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
    let b = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(15.0, -10.0, 0.0),
            Vector3::new(15.0, 10.0, 0.0),
        )));
    let _ = app.apply_cmd_result(crate::command::CmdResult::AddParametricConstraint {
        kind: ConstraintKind::Perpendicular,
        refs: vec![ParametricRef::whole(a), ParametricRef::whole(b)],
        driving_param: None,
        label: "Perpendicular constraint",
    });
    let geometry = |app: &OpenCADStudio| {
        [a, b].map(
            |handle| match app.tabs[i].scene.document.get_entity(handle).unwrap() {
                EntityType::Line(line) => [line.start, line.end],
                _ => panic!("expected line"),
            },
        )
    };
    let before = geometry(&app);
    app.tabs[i].active_grip = Some(GripEdit::single(
        a,
        1,
        false,
        glam::DVec3::new(10.0, 0.0, 0.0),
    ));
    for cursor in [Point::new(520.0, 220.0), Point::new(490.0, 180.0)] {
        let _ = app.on_viewport_move(cursor);
        let current = geometry(&app);
        for point in current.iter().flatten() {
            assert_eq!(point.z, 0.0, "grip left the drawing plane: {point:?}");
        }
        let direction = |ends: [Vector3; 2]| {
            glam::DVec3::new(
                ends[1].x - ends[0].x,
                ends[1].y - ends[0].y,
                ends[1].z - ends[0].z,
            )
            .normalize()
        };
        assert!(direction(current[0]).dot(direction(current[1])).abs() < 1e-8);
        assert_ne!(
            current[1], before[1],
            "the constrained neighbor must follow the edited line"
        );
        assert_eq!(
            current[0][0], before[0][0],
            "the endpoint opposite the dragged grip must stay fixed"
        );
    }
    let _ = app.on_viewport_left_release();
    assert!(app.tabs[i].active_grip.is_none());
    let after = geometry(&app);
    app.undo_steps(1);
    assert_eq!(geometry(&app), before);
    app.redo_steps(1);
    assert_eq!(geometry(&app), after);
}

#[test]
fn constrained_rectangle_grips_distinguish_perpendicular_corner_and_free_ends() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    use document::{entities::LwPolyline, types::Vector2, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.constraint_solve_mode = true;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let mut rectangle = LwPolyline::from_points(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(4.0, 0.0),
        Vector2::new(4.0, 2.0),
        Vector2::new(0.0, 2.0),
    ]);
    rectangle.is_closed = true;
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::LwPolyline(rectangle));
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Parallel,
        vec![
            ParametricRef::segment(handle, 0),
            ParametricRef::segment(handle, 2),
        ],
        None,
    );
    set.add(
        ConstraintKind::Parallel,
        vec![
            ParametricRef::segment(handle, 1),
            ParametricRef::segment(handle, 3),
        ],
        None,
    );
    set.add(
        ConstraintKind::Perpendicular,
        vec![
            ParametricRef::segment(handle, 3),
            ParametricRef::segment(handle, 2),
        ],
        None,
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::segment(handle, 2)],
        None,
    );
    let handle = if let Ok(path) = std::env::var("OCS_GRIP_TEST_DRAWING") {
        let result = app.automation_op(&serde_json::json!({"op":"open","path":path}).to_string());
        assert_eq!(result["ok"], true, "{result}");
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        Handle::new(0x556)
    } else {
        handle
    };
    let vertices = |app: &OpenCADStudio| {
        let EntityType::LwPolyline(polyline) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("expected polyline")
        };
        polyline
            .vertices
            .iter()
            .map(|vertex| vertex.location)
            .collect::<Vec<_>>()
    };
    let before = vertices(&app);
    let height = before[3].y - before[0].y;
    let width = before[1].x - before[0].x;
    for grip_id in 0..8 {
        let vertex = if grip_id < 4 {
            before[grip_id]
        } else {
            (before[grip_id - 4] + before[(grip_id - 3) % 4]) * 0.5
        };
        let origin = glam::DVec3::new(vertex.x, vertex.y, 0.0);
        app.tabs[i].active_grip = Some(GripEdit::single(handle, grip_id, grip_id >= 4, origin));
        for offset in [
            [-1.0, height],
            [-1.0, 3.0],
            [-2.0, 6.0],
            [-2.0, 6.0],
            [1.0, -4.0],
        ] {
            let target = origin + glam::DVec3::new(offset[0], offset[1], 0.0);
            let cursor = app.tabs[i]
                .scene
                .camera
                .borrow()
                .project(
                    target,
                    iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
                )
                .unwrap();
            let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
            let after = vertices(&app);
            let (left, right, bottom, top) = match grip_id {
                0 => (target.x, target.x + width, target.y, before[3].y),
                1 => (before[0].x, target.x, target.y, before[3].y),
                2 => (before[0].x, target.x, target.y - height, target.y),
                3 => (target.x, target.x + width, target.y - height, target.y),
                4 => (
                    before[0].x + offset[0],
                    before[1].x + offset[0],
                    target.y,
                    before[3].y,
                ),
                5 => (
                    before[0].x,
                    target.x,
                    before[0].y + offset[1],
                    before[3].y + offset[1],
                ),
                6 => (
                    before[0].x + offset[0],
                    before[1].x + offset[0],
                    before[0].y,
                    target.y,
                ),
                7 => (
                    target.x,
                    before[1].x,
                    before[0].y + offset[1],
                    before[3].y + offset[1],
                ),
                _ => unreachable!(),
            };
            let expected = [
                Vector2::new(left, bottom),
                Vector2::new(right, bottom),
                Vector2::new(right, top),
                Vector2::new(left, top),
            ];
            for (actual, expected) in after.iter().zip(expected) {
                assert!((*actual - expected).length_squared() < 1e-10,
                    "grip {grip_id}: rectangle collapsed or drifted: {after:?}, expected {expected:?}");
            }
        }
        let preview = vertices(&app);
        let _ = app.on_viewport_left_release();
        assert_eq!(vertices(&app), preview);
        app.undo_steps(1);
        assert_eq!(vertices(&app), before);
        app.redo_steps(1);
        assert_eq!(vertices(&app), preview);
        app.undo_steps(1);
    }
}

#[test]
fn axis_constrained_line_grip_changes_length_and_translates_normal_to_axis() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    use document::{entities::Line, types::Vector3, EntityType};

    for vertical in [false, true] {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.constraint_solve_mode = true;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let end = if vertical {
            Vector3::new(0.0, 4.0, 0.0)
        } else {
            Vector3::new(4.0, 0.0, 0.0)
        };
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, end)));
        app.tabs[i]
            .scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                if vertical {
                    ConstraintKind::Vertical
                } else {
                    ConstraintKind::Horizontal
                },
                vec![ParametricRef::whole(handle)],
                None,
            );
        app.tabs[i].active_grip = Some(GripEdit::single(handle, 0, false, glam::DVec3::ZERO));
        for target in [
            glam::DVec3::new(-1.0, 3.0, 0.0),
            glam::DVec3::new(2.0, -2.0, 0.0),
        ] {
            let cursor = app.tabs[i]
                .scene
                .camera
                .borrow()
                .project(
                    target,
                    iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
                )
                .unwrap();
            let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
            let EntityType::Line(line) = app.tabs[i].scene.document.get_entity(handle).unwrap()
            else {
                panic!("line")
            };
            let opposite = if vertical {
                Vector3::new(target.x, end.y, 0.0)
            } else {
                Vector3::new(end.x, target.y, 0.0)
            };
            assert!((line.start - Vector3::new(target.x, target.y, 0.0)).length() < 1e-5);
            assert!((line.end - opposite).length() < 1e-5, "{line:?}");
        }
    }
}

#[test]
fn a_point_step_takes_the_point_where_the_button_went_down() {
    use document::EntityType;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let cursor = |app: &OpenCADStudio, x: f64, y: f64| {
        let at = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(x, y, 0.0), bounds);
        let at = at.unwrap();
        iced::Point::new(at.x, at.y)
    };
    let _ = app.run_command_line("LINE");
    // Away from the origin, where the UCS icon takes presses.
    for (down, up) in [((40.0, 30.0), (45.0, 34.0)), ((80.0, 30.0), (86.0, 25.0))] {
        let _ = app.on_viewport_move(cursor(&app, down.0, down.1));
        let _ = app.on_viewport_left_press();
        // The hand moves before the button comes up.
        let _ = app.on_viewport_move(cursor(&app, up.0, up.1));
        let _ = app.on_viewport_left_release();
    }
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let lines: Vec<_> = app.tabs[i]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Line(line) => Some(line.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "one point per click, not one per press and release"
    );
    let line = &lines[0];
    assert!(
        (line.start.x - 40.0).abs() < 0.5 && (line.start.y - 30.0).abs() < 0.5,
        "{line:?}"
    );
    assert!(
        (line.end.x - 80.0).abs() < 0.5 && (line.end.y - 30.0).abs() < 0.5,
        "{line:?}"
    );
}

#[test]
fn xline_direction_grip_drag_changes_direction() {
    use document::{types::Vector3, EntityType};
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    // Base away from the model origin so the UCS icon cannot swallow
    // the press (it sits at the origin in a fresh drawing).
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::XLine(document::entities::XLine::new(
            Vector3::new(100.0, 50.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    assert_eq!(app.tabs[i].selected_grips.len(), 2);
    // Press on grip 1 (direction handle) and drag it to point along +X.
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let g1world = app.tabs[i].selected_grips[1].world;
    let g1cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(g1world, bounds)
        .unwrap();
    app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
        Some(iced::Point::new(g1cursor.x, g1cursor.y));
    let _ = app.on_viewport_left_press();
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|g| g.grip_id),
        Some(1),
        "press on the direction grip must engage grip 1"
    );
    let dir_target = glam::DVec3::new(200.0, 50.0, 0.0);
    let dir_cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(dir_target, bounds)
        .unwrap();
    let _ = app.on_viewport_move(iced::Point::new(dir_cursor.x, dir_cursor.y));
    let EntityType::XLine(moved) = app.tabs[i].scene.document.get_entity(handle).unwrap() else {
        panic!("expected xline")
    };
    assert!(
        (moved.direction.x - 1.0).abs() < 1e-6 && moved.direction.y.abs() < 1e-6,
        "direction grip drag must point the xline along +X, got {:?}",
        moved.direction
    );
}

#[test]
fn constraint_glyph_tooltip_appears_after_hover_dwell() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    use document::{entities::Line, types::Vector3, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
    app.constraint_bar_display = 3;
    let _ = app.apply_cmd_result(crate::command::CmdResult::AddParametricConstraint {
        kind: ConstraintKind::Horizontal,
        refs: vec![ParametricRef::whole(handle)],
        driving_param: None,
        label: "Horizontal constraint",
    });
    let anchor = app.tabs[i].scene.constraint_glyph_placements_screen(
        ParametricScope::ModelSpace,
        (800.0, 600.0),
        true,
        3,
        4095,
    )[0]
    .1;
    let point = (-30..=30)
        .find_map(|y| {
            (-30..=30).find_map(|x| {
                let point = Point::new(anchor.x + x as f32, anchor.y + y as f32);
                app.constraint_glyph_under(i, point)
                    .is_some()
                    .then_some(point)
            })
        })
        .expect("projected constraint glyph should be hit-testable");

    let _ = app.on_viewport_move(point);
    assert_eq!(app.constraint_glyph_tooltip, None);
    app.hover_dwell.as_mut().unwrap().last_move_at =
        Instant::now() - std::time::Duration::from_millis(crate::app::HOVER_DWELL_MS as u64 + 1);
    let _ = app.on_hover_dwell_tick();

    assert_eq!(
        app.constraint_glyph_tooltip,
        Some(ConstraintKind::Horizontal)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn hyperlink_click_respects_existing_pointer_owners() {
    use document::entities::Line;
    use document::types::Vector3;
    use document::xdata::{ExtendedDataRecord, XDataValue};
    use document::EntityType;
    use glam::DVec3;

    for mode in ["link", "plain", "pan", "orbit", "zoom", "grip", "command"] {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.show_ucs_icon = false;
        // Keep the browser-opening window task deferred during this test.
        app.main_window = Some(iced::window::Id::unique());
        let mut line =
            Line::from_points(Vector3::new(-10.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        let mut record = ExtendedDataRecord::new("PE_URL");
        record.add_value(XDataValue::String("https://example.com/linked".into()));
        line.common.extended_data.add_record(record);
        let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let _ = app.run_command_line("ZOOM EXTENTS");
        app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
            Some(Point::new(400.0, 300.0));
        app.ctrl_down = mode != "plain";
        match mode {
            "pan" => app.tabs[i].pan_mode = true,
            "orbit" => app.tabs[i].orbit_mode = true,
            "zoom" => app.tabs[i].zoom_dynamic_mode = true,
            "grip" => {
                app.tabs[i].active_grip = Some(GripEdit::single(handle, 0, false, DVec3::ZERO))
            }
            "command" => {
                let _ = app.run_command_line("LINE");
            }
            _ => {}
        }
        let task = app.on_viewport_left_press();
        let opened = app
            .command_line
            .history
            .iter()
            .any(|entry| entry.text.contains("https://example.com/linked"));
        assert_eq!(opened, mode == "link", "{mode}");
        if mode == "link" {
            assert!(task.units() > 0);
            let selection = app.tabs[i].scene.selection.borrow();
            assert!(!selection.input.left_down && selection.gesture.box_anchor.is_none());
        } else if matches!(mode, "pan" | "orbit" | "zoom") {
            assert!(
                app.tabs[i].scene.selection.borrow().input.middle_down,
                "{mode}"
            );
        }
    }
}

/// Drive one settled rollover pick over a line and report what the scene
/// ended up highlighting.
fn rollover_hits(preview: u8) -> bool {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.model_space.selection_preview = preview;
    let _ = app.run_command_line("LINE 0,0 10,10");
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let _ = app.run_command_line("ZOOM EXTENTS");

    // The pick only runs once the cursor has been still for the dwell
    // window, so the arming timestamp is backdated past it.
    app.hover_dwell = Some(HoverDwell {
        last_move_at: Instant::now() - std::time::Duration::from_millis(HOVER_DWELL_MS as u64 * 2),
        point: iced::Point::new(400.0, 300.0),
        tile_size: (800.0, 600.0),
        tab: i,
        camera_generation: app.tabs[i].scene.camera_generation,
    });
    let _ = app.on_hover_dwell_tick();
    app.tabs[i].scene.hover_highlight.is_some()
}

/// Bit 1 of `SELECTIONPREVIEW` is the rollover that runs with no command
/// active, and the Options card gives it a checkbox. The config comment
/// used to describe the bits the wrong way round, so which bit does what
/// is worth asserting rather than reading.
#[test]
fn the_idle_rollover_follows_bit_one() {
    assert!(
        rollover_hits(1),
        "bit 1 set: the line under the cursor must be highlighted",
    );
    assert!(rollover_hits(3), "both bits set: still highlighted",);
    assert!(!rollover_hits(0), "preview off: nothing may be highlighted",);
    assert!(
        !rollover_hits(2),
        "only the in-command bit: the idle rollover stays off",
    );
}

/// The quad's own dwell poll (docs/quad-menu.md §3.4): with the quad's
/// delay beyond the rollover cadence, the re-armed `HoverDwellTick`s
/// must not re-run the full O(N) settled pick while cursor and scene
/// rest — and must re-run it once the cursor moved again, even away and
/// back to the same point.
#[test]
fn quad_hover_poll_skips_the_settled_pick_while_resting() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let _ = app.run_command_line("LINE 0,0 10,10");
    let _ = app.run_command_line("ZOOM EXTENTS");
    app.quad = true;
    app.quad_hover_delay_ms = 700; // past HOVER_DWELL_MS: the poll continues
    app.hover_dwell = Some(HoverDwell {
        last_move_at: Instant::now() - std::time::Duration::from_millis(HOVER_DWELL_MS as u64 + 1),
        point: iced::Point::new(400.0, 300.0),
        tile_size: (800.0, 600.0),
        tab: i,
        camera_generation: app.tabs[i].scene.camera_generation,
    });
    // The first tick picks (the line under the cursor is highlighted)
    // and arms the quad dwell — rest 501 < 700, so the quad is waiting.
    let _ = app.on_hover_dwell_tick();
    assert!(
        app.tabs[i].scene.hover_highlight.is_some(),
        "the first settled pick ran and highlighted the line"
    );
    assert!(
        app.quad_hover_counting(),
        "the quad dwell is still counting"
    );
    assert!(
        app.quad_dwell_pick.is_some(),
        "the settled pick is recorded"
    );

    // A repeat with nothing changed must not re-pick: a highlight
    // dropped by hand stays dropped (a re-pick would restore it), and
    // the disqualified counting dwell tears down like the settled tail.
    app.tabs[i].scene.set_hover_highlight(None);
    app.hover_dwell.as_mut().unwrap().last_move_at =
        Instant::now() - std::time::Duration::from_millis(HOVER_DWELL_MS as u64 + 1);
    let _ = app.on_hover_dwell_tick();
    assert!(
        app.tabs[i].scene.hover_highlight.is_none(),
        "the resting poll skipped the settled pick"
    );
    assert!(
        app.quad_hover_since.is_none(),
        "the skipped tick disqualified the counting dwell with the highlight"
    );

    // A cursor move back onto the same point (the move count changed)
    // re-runs the pick: the line is highlighted again.
    app.quad_hover_since = Some(std::time::Instant::now());
    app.hover_move_seq += 1;
    app.hover_dwell = Some(HoverDwell {
        last_move_at: Instant::now() - std::time::Duration::from_millis(HOVER_DWELL_MS as u64 + 1),
        point: iced::Point::new(400.0, 300.0),
        tile_size: (800.0, 600.0),
        tab: i,
        camera_generation: app.tabs[i].scene.camera_generation,
    });
    let _ = app.on_hover_dwell_tick();
    assert!(
        app.tabs[i].scene.hover_highlight.is_some(),
        "a moved cursor re-runs the settled pick"
    );
}

/// Snapping the first LINE point to a 3D vertex must keep its elevation:
/// the marker, the preview point and (via the click path) the committed
/// point all carry the snapped Z instead of dropping to the XY plane.
#[test]
fn line_snap_to_3d_endpoint_keeps_elevation() {
    use crate::snap::SnapType;
    use document::{entities::Line, types::Vector3, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = true;
    app.snapper.enabled.insert(SnapType::Endpoint);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 25.0),
        )));
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    assert!(app.tabs[i].active_cmd.is_some(), "LINE did not start");

    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            glam::DVec3::new(10.0, 0.0, 25.0),
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    let hit = app.tabs[i]
        .snap_result
        .expect("endpoint snap missed the 3D vertex");
    assert_eq!(hit.snap_type, SnapType::Endpoint);
    assert!(
        (hit.world.z - 25.0).abs() < 1e-6,
        "snap landed off-elevation: {:?}",
        hit.world
    );
    let preview = app.tabs[i].last_cursor_world;
    assert!(
        (preview.z - 25.0).abs() < 1e-6,
        "command point lost its elevation: {preview:?}"
    );
}

/// The user's repro: BOX, then LINE snapping its first point to a box
/// corner. Solid B-rep corners snap as 3D Vertex points, gated by the
/// separate 3D master toggle — the 2D Node mode must not catch them.
#[test]
fn line_snaps_to_box_corner() {
    use crate::snap::SnapType;
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    // 2D osnap fully off: only the 3D system may fire.
    app.snapper.snap_enabled = false;
    app.snapper.snap3d_enabled = true;
    app.snapper.enabled3d.insert(SnapType::Vertex);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    // In-app BOX equivalent: kernel box committed as a SAT solid.
    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

    // A real corner straight from the solid's edge data: the topmost
    // one, since coincident corners resolve to the nearer eye.
    let target = {
        let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
        let (edges, lows) = set.geometry_edges();
        assert!(edges.len() >= 2, "box mesh has no B-rep edges");
        let at = |index: usize| {
            let hi = edges[index];
            let lo = lows.get(index).copied().unwrap_or([0.0; 3]);
            glam::DVec3::new(
                hi[0] as f64 + lo[0] as f64,
                hi[1] as f64 + lo[1] as f64,
                hi[2] as f64 + lo[2] as f64,
            )
        };
        (0..edges.len())
            .map(at)
            .max_by(|a, b| a.z.total_cmp(&b.z))
            .expect("box mesh has no endpoints")
    };

    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    assert!(app.tabs[i].active_cmd.is_some(), "LINE did not start");
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            target,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    let hit = app.tabs[i]
        .snap_result
        .expect("no snap point on the box corner");
    assert_eq!(
        hit.snap_type,
        SnapType::Vertex,
        "expected a box-corner vertex, got {:?} at {:?}",
        hit.snap_type,
        hit.world
    );
    assert!(
        (hit.world - target).length() < 1e-6,
        "snap missed the corner: {:?} vs {target:?}",
        hit.world
    );
    let preview = app.tabs[i].last_cursor_world;
    assert!(
        (preview - target).length() < 1e-6,
        "command point left the corner: {preview:?} vs {target:?}"
    );
}

/// With the 3D master off, a solid's corner is no 3D Vertex snap, but
/// the 2D Endpoint mode still catches it, as on any edge (#2153).
#[test]
fn box_corner_answers_2d_endpoint_with_3d_master_off() {
    use crate::snap::SnapType;
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = true;
    app.snapper.enabled.insert(SnapType::Endpoint);
    app.snapper.snap3d_enabled = false;
    app.snapper.enabled3d.insert(SnapType::Vertex);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

    let target = {
        let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
        let (edges, lows) = set.geometry_edges();
        let hi = edges[0];
        let lo = lows.first().copied().unwrap_or([0.0; 3]);
        glam::DVec3::new(
            hi[0] as f64 + lo[0] as f64,
            hi[1] as f64 + lo[1] as f64,
            hi[2] as f64 + lo[2] as f64,
        )
    };

    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            target,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    assert_eq!(
        app.tabs[i].snap_result.map(|s| s.snap_type),
        Some(SnapType::Endpoint),
        "3D master off leaves the corner to the 2D Endpoint mode"
    );
}

/// Face centres snap as their own 3D type at the loop average (exact
/// for the box's planar faces).
#[test]
fn line_snaps_to_box_face_center() {
    use crate::snap::SnapType;
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.snap3d_enabled = true;
    app.snapper.enabled3d.insert(SnapType::FaceCenter);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

    // Top-face centre of the 10×10×5 box.
    let target = glam::DVec3::new(0.0, 0.0, 5.0);
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            target,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    let hit = app.tabs[i]
        .snap_result
        .expect("no snap point on the box face");
    assert_eq!(
        hit.snap_type,
        SnapType::FaceCenter,
        "expected a face centre, got {:?} at {:?}",
        hit.snap_type,
        hit.world
    );
    assert!(
        (hit.world - target).length() < 1e-6,
        "snap missed the face centre: {:?} vs {target:?}",
        hit.world
    );
}

/// Nearest-to-face catches an interior face point with no base point.
#[test]
fn line_snaps_to_box_face_nearest() {
    use crate::snap::SnapType;
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.snap3d_enabled = true;
    app.snapper.enabled3d.insert(SnapType::NearestFace);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

    // Interior point of the top face (off the diagonal so exactly one
    // mesh triangle owns it).
    let target = glam::DVec3::new(2.0, 3.0, 5.0);
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            target,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    let hit = app.tabs[i]
        .snap_result
        .expect("no snap point on the box face");
    assert_eq!(
        hit.snap_type,
        SnapType::NearestFace,
        "expected nearest-to-face, got {:?} at {:?}",
        hit.snap_type,
        hit.world
    );
    assert!(
        (hit.world - target).length() < 1e-3,
        "snap missed the face point: {:?} vs {target:?}",
        hit.world
    );
}

/// Perpendicular-to-face drops from an explicit base point: exercised
/// grey-box (real mesh, real helper) since it needs no event plumbing.
#[test]
fn face_perpendicular_needs_base_and_interior_foot() {
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));
    let _ = app.run_command_line("ZOOM EXTENTS");

    let scene = &app.tabs[i].scene;
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let cam = scene.camera.borrow();
    let view_rot = cam.view_proj_rte(bounds);
    let eye = cam.eye();
    let foot = glam::DVec3::new(2.0, 3.0, 5.0);
    let cursor = cam.project(foot, bounds).unwrap();
    drop(cam);
    let cursor = Point::new(cursor.x, cursor.y);

    // Base below the face interior → foot straight above it.
    let hit = app.tabs[i].scene.solid_face_snaps(
        cursor,
        view_rot,
        eye,
        bounds,
        15.0,
        Some(glam::DVec3::new(2.0, 3.0, 0.0)),
        true,
        true,
    );
    let hit = hit.expect("perpendicular foot missed");
    assert_eq!(hit.snap_type, crate::snap::SnapType::FacePerpendicular);
    assert!(
        (hit.world - foot).length() < 1e-3,
        "foot off target: {:?} vs {foot:?}",
        hit.world
    );

    // No base → perpendicular cannot fire (nearest still can).
    let hit = app.tabs[i]
        .scene
        .solid_face_snaps(cursor, view_rot, eye, bounds, 15.0, None, true, false);
    assert!(hit.is_none(), "perp without a base must stay silent");

    // Neither mode wanted → silence.
    let hit = app.tabs[i].scene.solid_face_snaps(
        cursor,
        view_rot,
        eye,
        bounds,
        15.0,
        Some(glam::DVec3::new(2.0, 3.0, 0.0)),
        false,
        false,
    );
    assert!(hit.is_none(), "unwanted modes must stay silent");
}

/// Edge midpoints snap as their own 3D type at the segment centre.
#[test]
fn line_snaps_to_box_edge_midpoint() {
    use crate::snap::SnapType;
    use document::{entities::Solid3D, EntityType};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    app.snapper.snap3d_enabled = true;
    app.snapper.enabled3d.insert(SnapType::EdgeMidpoint);
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, 2.5], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    let h = app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));

    // Midpoint of a top edge (both ends at max z): bottom edges project
    // onto the same pixels, and coincident points resolve to the eye.
    let target = {
        let set = app.tabs[i].scene.meshes.get(&h).expect("box mesh");
        let (edges, lows) = set.geometry_edges();
        assert!(edges.len() >= 2, "box mesh has no B-rep edges");
        let at = |index: usize| {
            let hi = edges[index];
            let lo = lows.get(index).copied().unwrap_or([0.0; 3]);
            glam::DVec3::new(
                hi[0] as f64 + lo[0] as f64,
                hi[1] as f64 + lo[1] as f64,
                hi[2] as f64 + lo[2] as f64,
            )
        };
        let top = (0..edges.len())
            .map(at)
            .map(|p| p.z)
            .fold(f64::NEG_INFINITY, f64::max);
        (0..edges.len() / 2)
            .map(|k| (at(2 * k), at(2 * k + 1)))
            .find(|(a, b)| (a.z - top).abs() < 1e-9 && (b.z - top).abs() < 1e-9)
            .map(|(a, b)| (a + b) * 0.5)
            .expect("box mesh has no top edge")
    };

    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            target,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));

    let hit = app.tabs[i]
        .snap_result
        .expect("no snap point on the box edge");
    assert_eq!(
        hit.snap_type,
        SnapType::EdgeMidpoint,
        "expected an edge midpoint, got {:?} at {:?}",
        hit.snap_type,
        hit.world
    );
    assert!(
        (hit.world - target).length() < 1e-6,
        "snap missed the edge midpoint: {:?} vs {target:?}",
        hit.world
    );
}
