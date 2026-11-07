//! Grip interaction on small dynamic definitions built entirely in Rust.
//!
//! Each definition is authored here with exact parameters, actions and
//! geometry, so the checks need no drawing outside the test process.

use super::*;
use crate::scene::block_actions::{self as actions, Kind, XyKeyPoint};
use crate::scene::block_distance_parameter as distance;
use crate::scene::block_grips::{config, runtime};
use document::entities::Arc;
use glam::DVec3;

const EPS: f64 = 1e-6;

fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(a.0, a.1, 0.),
        Vector3::new(b.0, b.1, 0.),
    ))
}

/// Adds `name` with `entities`, one plain reference at the origin and one
/// placed by `second`, and returns the app with both reference handles.
fn drawing(
    name: &str,
    entities: Vec<EntityType>,
    second: impl FnOnce(&mut Insert),
) -> (OpenCADStudio, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .define_block_raw(name, Vector3::ZERO, entities);
    let doc = &mut app.tabs[i].scene.document;
    let plain = doc
        .add_entity(EntityType::Insert(Insert::new(name, Vector3::ZERO)))
        .unwrap();
    let mut insert = Insert::new(name, Vector3::ZERO);
    second(&mut insert);
    let placed = doc.add_entity(EntityType::Insert(insert)).unwrap();
    app.tabs[i].dirty = false;
    (app, plain, placed)
}

fn definition_members(app: &OpenCADStudio, name: &str) -> Vec<Handle> {
    app.tabs[app.active_tab]
        .scene
        .document
        .block_records
        .get(name)
        .unwrap()
        .entity_handles
        .clone()
}

fn save_and_close(app: &mut OpenCADStudio) {
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
}

/// A swing door: a 70 x 4 leaf hinged at the origin, its 90 degree swing arc
/// and a short jamb. A one-grip Linear "Thickness" parameter runs from the
/// hinge across the leaf and stretches the leaf's far edge.
fn swing_door() -> (OpenCADStudio, Handle, Handle, i32) {
    let entities = vec![
        line((0., 0.), (70., 0.)),
        line((70., 0.), (70., 4.)),
        line((70., 4.), (0., 4.)),
        line((0., 4.), (0., 0.)),
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::ZERO,
            70.,
            0.,
            std::f64::consts::FRAC_PI_2,
        )),
        line((0., 0.), (0., -6.)),
    ];
    let (mut app, plain, rotated) = drawing("SWING_DOOR", entities, |insert| {
        insert.insert_point = Vector3::new(200., 0., 0.);
        insert.rotation = std::f64::consts::FRAC_PI_2;
    });
    begin(&mut app, plain);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    // The bottom edge lies outside the frame and must stay put, so it is not a
    // target: Stretch moves selected objects that the frame does not cross.
    let leaf = definition_members(&app, "SWING_DOOR")[1..4].to_vec();
    let doc = &mut app.tabs[i].scene.document;
    let thickness = distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(0., 4., 0.),
        Vector3::new(-6., 2., 0.),
    )
    .unwrap();
    config::set(doc, def, thickness, Some(1)).unwrap();
    actions::create_with_options(
        doc,
        def,
        Kind::Stretch,
        thickness,
        Some(XyKeyPoint::End),
        &leaf,
        DVec3::ZERO,
        Some([DVec3::new(-2., 2., 0.), DVec3::new(72., 8., 0.)]),
        None,
    )
    .unwrap();
    let id = actions::all_parameters(doc, def)
        .into_iter()
        .find(|p| p.handle == thickness)
        .unwrap()
        .id;
    save_and_close(&mut app);
    (app, plain, rotated, id)
}

/// A window: jambs at x = 0 and x = 100, two sill lines and a glazing line.
/// A two-grip Linear "Width" parameter spans the opening; one Stretch follows
/// the end grip over the right jamb and another follows the base grip over
/// the left jamb.
fn window() -> (OpenCADStudio, Handle, Handle, i32) {
    let entities = vec![
        line((0., 0.), (0., 10.)),
        line((100., 0.), (100., 10.)),
        line((0., 0.), (100., 0.)),
        line((0., 10.), (100., 10.)),
        line((0., 5.), (100., 5.)),
        EntityType::Arc(Arc::from_center_radius_angles(Vector3::new(40., 30., 0.), 20., 0., std::f64::consts::FRAC_PI_2)),
    ];
    let (mut app, plain, mirrored) = drawing("WINDOW", entities, |insert| {
        insert.insert_point = Vector3::new(300., 50., 0.);
        insert.set_x_scale(-1.);
    });
    begin(&mut app, plain);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let members = definition_members(&app, "WINDOW");
    // Each Stretch selects only what its frame crosses or contains: its own
    // jamb and the three horizontal lines.
    let end_targets = [members[1], members[2], members[3], members[4]];
    let base_targets = [members[0], members[2], members[3], members[4]];
    let doc = &mut app.tabs[i].scene.document;
    let width = distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(100., 0., 0.),
        Vector3::new(50., -8., 0.),
    )
    .unwrap();
    config::set(doc, def, width, Some(2)).unwrap();
    for (key, targets, frame) in [
        (XyKeyPoint::End, end_targets, [DVec3::new(90., -5., 0.), DVec3::new(110., 15., 0.)]),
        (XyKeyPoint::Base, base_targets, [DVec3::new(-10., -5., 0.), DVec3::new(10., 15., 0.)]),
    ] {
        actions::create_with_options(
            doc,
            def,
            Kind::Stretch,
            width,
            Some(key),
            &targets,
            DVec3::ZERO,
            Some(frame),
            None,
        )
        .unwrap();
    }
    actions::create(doc, def, Kind::Scale, width, &[members[5]], DVec3::ZERO).unwrap();
    let id = actions::all_parameters(doc, def)
        .into_iter()
        .find(|p| p.handle == width)
        .unwrap()
        .id;
    save_and_close(&mut app);
    (app, plain, mirrored, id)
}

fn select(app: &mut OpenCADStudio, insert: Handle) {
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(insert, true);
    app.tabs[i].selected_handle = Some(insert);
    app.refresh_properties();
}

fn grip(app: &mut OpenCADStudio, insert: Handle, id: i32, role: usize) -> crate::scene::GripDef {
    select(app, insert);
    let wanted = runtime::id(id, role).unwrap();
    app.tabs[app.active_tab]
        .selected_grips
        .iter()
        .find(|g| g.id == wanted)
        .unwrap_or_else(|| panic!("grip {id}/{role} is offered"))
        .clone()
}

/// Drags the grip `distance` units along its own direction and returns the
/// requested world target.
fn drag(app: &mut OpenCADStudio, insert: Handle, id: i32, role: usize, distance: f64) -> DVec3 {
    let start = grip(app, insert, id, role);
    let target = start.world + start.dir.expect("linear grips have a direction") * distance;
    assert!(app.start_test_action_grip(insert, start.id));
    let _ = app.feed_command(crate::command::StepInput::Point(target));
    assert_eq!(app.command_line.last_error, None);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    target
}

/// The evaluated geometry of `insert`, in block coordinates, as line end
/// points and arcs (center, radius, start and end angle).
fn geometry(
    doc: &document::Document,
    insert: Handle,
) -> (Vec<(DVec3, DVec3)>, Vec<(DVec3, f64, f64, f64)>) {
    let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    let mut lines = vec![];
    let mut arcs = vec![];
    for entity in doc.entities_in_block(&reference.block_name) {
        match entity {
            EntityType::Line(l) => lines.push((actions::v(l.start), actions::v(l.end))),
            EntityType::Arc(a) => {
                arcs.push((actions::v(a.center), a.radius, a.start_angle, a.end_angle))
            }
            _ => {}
        }
    }
    (lines, arcs)
}

fn assert_lines(actual: &[(DVec3, DVec3)], expected: &[((f64, f64), (f64, f64))], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}: line count");
    for (index, ((a, b), (ea, eb))) in actual.iter().zip(expected).enumerate() {
        let ea = DVec3::new(ea.0, ea.1, 0.);
        let eb = DVec3::new(eb.0, eb.1, 0.);
        assert!(
            a.distance(ea) < EPS && b.distance(eb) < EPS,
            "{context}: line {index} is {a:?}-{b:?}, expected {ea:?}-{eb:?}"
        );
    }
}

fn block_name(app: &OpenCADStudio, insert: Handle) -> String {
    match app.tabs[app.active_tab].scene.document.get_entity(insert) {
        Some(EntityType::Insert(reference)) => reference.block_name.clone(),
        _ => panic!(),
    }
}

const DOOR_AT_REST: [((f64, f64), (f64, f64)); 5] = [
    ((0., 0.), (70., 0.)),
    ((70., 0.), (70., 4.)),
    ((70., 4.), (0., 4.)),
    ((0., 4.), (0., 0.)),
    ((0., 0.), (0., -6.)),
];

#[test]
fn synthetic_swing_door_thickness_grip_tracks_requested_position() {
    for rotated in [false, true] {
        let (mut app, plain, turned, id) = swing_door();
        let (insert, other) = if rotated { (turned, plain) } else { (plain, turned) };
        let other_before = app.tabs[app.active_tab].scene.document.get_entity(other).cloned();
        let target = drag(&mut app, insert, id, 1, 2.);
        let after = grip(&mut app, insert, id, 1);
        assert!(
            after.world.distance(target) < EPS,
            "rotated={rotated}: grip {:?} != target {target:?}",
            after.world
        );
        let (lines, arcs) = geometry(&app.tabs[app.active_tab].scene.document, insert);
        assert_lines(
            &lines,
            &[
                ((0., 0.), (70., 0.)),
                ((70., 0.), (70., 6.)),
                ((70., 6.), (0., 6.)),
                ((0., 6.), (0., 0.)),
                ((0., 0.), (0., -6.)),
            ],
            &format!("rotated={rotated}"),
        );
        assert_eq!(arcs.len(), 1);
        let (center, radius, start, end) = arcs[0];
        assert!(center.length() < EPS && (radius - 70.).abs() < EPS);
        assert!(start.abs() < EPS && (end - std::f64::consts::FRAC_PI_2).abs() < EPS);
        assert_eq!(
            app.tabs[app.active_tab].scene.document.get_entity(other).cloned(),
            other_before,
            "the other reference is untouched"
        );
        let (lines, _) = geometry(&app.tabs[app.active_tab].scene.document, other);
        assert_lines(&lines, &DOOR_AT_REST, "other reference");
    }
}

/// The window after one 20 unit drag of `role` (0 base, 1 end), in block
/// coordinates: jambs, outer and inner sill, glazing.
fn window_after(role: usize) -> [((f64, f64), (f64, f64)); 5] {
    let (left, right) = if role == 0 { (-20., 100.) } else { (0., 120.) };
    [
        ((left, 0.), (left, 10.)),
        ((right, 0.), (right, 10.)),
        ((left, 0.), (right, 0.)),
        ((left, 10.), (right, 10.)),
        ((left, 5.), (right, 5.)),
    ]
}

const WINDOW_AT_REST: [((f64, f64), (f64, f64)); 5] = [
    ((0., 0.), (0., 10.)),
    ((100., 0.), (100., 10.)),
    ((0., 0.), (100., 0.)),
    ((0., 10.), (100., 10.)),
    ((0., 5.), (100., 5.)),
];

#[test]
fn synthetic_window_endpoint_grips_track_requested_positions() {
    for mirrored in [false, true] {
        for role in [0, 1] {
            let context = format!("mirrored={mirrored} role={role}");
            let (mut app, plain, flipped, id) = window();
            let (insert, other) = if mirrored { (flipped, plain) } else { (plain, flipped) };
            let start = grip(&mut app, insert, id, role).world;
            let target = drag(&mut app, insert, id, role, 20.);
            assert!((target - start).length() > 19.9, "{context}");
            let after = grip(&mut app, insert, id, role);
            assert!(
                after.world.distance(target) < EPS,
                "{context}: grip {:?} != target {target:?}",
                after.world
            );
            let doc = &app.tabs[app.active_tab].scene.document;
            assert_lines(&geometry(doc, insert).0, &window_after(role), &context);
            assert_lines(&geometry(doc, other).0, &WINDOW_AT_REST, &context);
            assert_eq!(block_name(&app, other), "WINDOW", "{context}: isolation");
        }
    }
}

#[test]
fn synthetic_window_grip_edits_undo_and_redo_the_whole_drawing() {
    for mirrored in [false, true] {
        for role in [0, 1] {
            let context = format!("mirrored={mirrored} role={role}");
            let (mut app, plain, flipped, id) = window();
            let insert = if mirrored { flipped } else { plain };
            let before = app.tabs[app.active_tab].scene.document.clone();
            let depth = app.tabs[app.active_tab].history.undo_stack.len();
            drag(&mut app, insert, id, role, 20.);
            assert_eq!(
                app.tabs[app.active_tab].history.undo_stack.len(),
                depth + 1,
                "{context}: one undo step"
            );
            let orig_name = "WINDOW";
            let changed_name = block_name(&app, insert);
            let _ = app.update(Message::Undo);
            assert_eq!(block_name(&app, insert), orig_name, "undo {context}: name");
            assert_lines(
                &geometry(&app.tabs[app.active_tab].scene.document, insert).0,
                &WINDOW_AT_REST,
                &context,
            );
            let _ = app.update(Message::Redo);
            assert_eq!(block_name(&app, insert), changed_name, "redo {context}: name");
            assert_lines(
                &geometry(&app.tabs[app.active_tab].scene.document, insert).0,
                &window_after(role),
                &context,
            );
        }
    }
}

#[test]
fn synthetic_window_grip_edits_persist_through_dwg_and_dxf() {
    for mirrored in [false, true] {
        for role in [0, 1] {
            let (mut app, plain, flipped, id) = window();
            let insert = if mirrored { flipped } else { plain };
            let target = drag(&mut app, insert, id, role, 20.);
            let changed = app.tabs[app.active_tab].scene.document.clone();
            for extension in ["dwg", "dxf"] {
                let context = format!("{extension} mirrored={mirrored} role={role}");
                let bytes = crate::io::save_to_bytes(&changed, extension, changed.version).unwrap();
                let saved =
                    crate::io::load_bytes(&format!("window.{extension}"), bytes).unwrap();
                let mut reopened = OpenCADStudio::new_for_test();
                let j = reopened.active_tab;
                reopened.tabs[j].scene.document = saved;
                let restored = grip(&mut reopened, insert, id, role);
                assert!(
                    restored.world.distance(target) < EPS,
                    "{context}: grip {:?} != target {target:?}",
                    restored.world
                );
                assert_lines(
                    &geometry(&reopened.tabs[j].scene.document, insert).0,
                    &window_after(role),
                    &context,
                );
                // A second edit continues from the persisted value.
                drag(&mut reopened, insert, id, role, 5.);
                let (left, right) = if role == 0 { (-25., 100.) } else { (0., 125.) };
                assert_lines(
                    &geometry(&reopened.tabs[j].scene.document, insert).0,
                    &[
                        ((left, 0.), (left, 10.)),
                        ((right, 0.), (right, 10.)),
                        ((left, 0.), (right, 0.)),
                        ((left, 10.), (right, 10.)),
                        ((left, 5.), (right, 5.)),
                    ],
                    &format!("{context} second edit"),
                );
            }
        }
    }
}


/// A synthetic coupled-flip block with two orthogonal Flip parameters and a Linear parameter
/// driving a Stretch action.
fn coupled_flip_synthetic() -> (OpenCADStudio, Handle, i32, i32, i32) {
    let entities = vec![
        line((0., 0.), (100., 0.)),
        line((100., 0.), (100., 20.)),
        line((100., 20.), (0., 20.)),
        line((0., 20.), (0., 0.)),
    ];
    let (mut app, plain, _) = drawing("COUPLED_FLIP", entities, |_| {});
    begin(&mut app, plain);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let members = definition_members(&app, "COUPLED_FLIP");
    let doc = &mut app.tabs[i].scene.document;

    // 1. Linear parameter along X
    let linear = distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(100., 0., 0.),
        Vector3::new(50., -10., 0.),
    )
    .unwrap();
    config::set(doc, def, linear, Some(1)).unwrap();

    // 2. Flip parameter 1 (vertical flip axis along X = 50)
    let flip_v = crate::scene::block_orientation_parameter::create(
        doc,
        def,
        crate::scene::block_orientation_parameter::Kind::Flip,
        Vector3::new(50., 0., 0.),
        Vector3::new(50., 50., 0.),
        Vector3::new(50., 60., 0.),
    )
    .unwrap();

    // 3. Flip parameter 2 (horizontal flip axis along Y = 10)
    let flip_h = crate::scene::block_orientation_parameter::create(
        doc,
        def,
        crate::scene::block_orientation_parameter::Kind::Flip,
        Vector3::new(0., 10., 0.),
        Vector3::new(100., 10., 0.),
        Vector3::new(110., 10., 0.),
    )
    .unwrap();

    // Stretch action associated with Linear
    actions::create_with_options(
        doc,
        def,
        Kind::Stretch,
        linear,
        Some(XyKeyPoint::End),
        &[members[0], members[1], members[2]],
        DVec3::ZERO,
        Some([DVec3::new(90., -5., 0.), DVec3::new(110., 25., 0.)]),
        None,
    )
    .unwrap();

    // Flip actions affecting all geometry and the linear parameter
    let mut targets = members.clone();
    targets.push(linear);
    let first = actions::create(doc, def, Kind::Flip, flip_v, &targets, DVec3::ZERO).unwrap();
    let second = actions::create(doc, def, Kind::Flip, flip_h, &targets, DVec3::ZERO).unwrap();
    let basepoint = crate::scene::block_basepoint::create(doc, def, Vector3::ZERO).unwrap();
    let base_id = crate::scene::block_basepoint::parameter(doc, def).unwrap().1.parameter.parameter.element.eval.node_id;
    for handle in [first, second] {
        let document::objects::ObjectType::DynamicBlock(object) = doc.objects.get_mut(&handle).unwrap() else { panic!() };
        let document::objects::DynamicBlockData::FlipAction(action) = &mut object.data else { panic!() };
        action.action.dependencies.push(basepoint);
        action.action.action_ids.push(base_id);
    }

    let all_p = actions::all_parameters(doc, def);
    let lin_id = all_p.iter().find(|p| p.handle == linear).unwrap().id;
    let flip_v_id = all_p.iter().find(|p| p.handle == flip_v).unwrap().id;
    let flip_h_id = all_p.iter().find(|p| p.handle == flip_h).unwrap().id;

    save_and_close(&mut app);
    (app, plain, lin_id, flip_v_id, flip_h_id)
}

fn assert_coupled_geometry(app: &OpenCADStudio, insert: Handle, width: f64, vertical: bool, horizontal: bool) {
    let transform = |x: f64, y: f64| (if vertical { 100. - x } else { x }, if horizontal { 20. - y } else { y });
    let expected = [
        (transform(0., 0.), transform(width, 0.)),
        (transform(width, 0.), transform(width, 20.)),
        (transform(width, 20.), transform(0., 20.)),
        (transform(0., 20.), transform(0., 0.)),
    ];
    assert_lines(&geometry(&app.tabs[app.active_tab].scene.document, insert).0, &expected, "coupled geometry");
}

fn toggle(app: &mut OpenCADStudio, insert: Handle, id: i32) {
    let before = grip(app, insert, id, 0);
    assert_eq!(before.shape, crate::scene::model::object::GripShape::Arrow);
    assert!(app.start_test_action_grip(insert, before.id));
    assert_eq!(app.command_line.last_error, None);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!((grip(app, insert, id, 0).dir.unwrap() + before.dir.unwrap()).length() < EPS);
}

#[test]
fn synthetic_coupled_flip_workflow_and_linear_stretch_roundtrip() {
    for reverse in [false, true] {
        let (mut app, insert, lin_id, flip_v_id, flip_h_id) = coupled_flip_synthetic();
        let i = app.active_tab;
        let order = if reverse { [flip_h_id, flip_v_id] } else { [flip_v_id, flip_h_id] };
        let baseline = app.tabs[i].scene.document.clone();
        toggle(&mut app, insert, order[0]);
        assert_coupled_geometry(&app, insert, 100., !reverse, reverse);
        toggle(&mut app, insert, order[1]);
        assert_coupled_geometry(&app, insert, 100., true, true);
        actions::instance::clear_canonical_inputs(&mut app.tabs[i].scene.document, insert);
        let before_stretch = geometry(&app.tabs[i].scene.document, insert);
        let target = drag(&mut app, insert, lin_id, 1, 30.);
        assert_coupled_geometry(&app, insert, 130., true, true);
        let changed = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::Undo);
        assert_eq!(geometry(&app.tabs[i].scene.document, insert), before_stretch);
        let _ = app.update(Message::Redo);
        assert_coupled_geometry(&app, insert, 130., true, true);
        let def = changed.dynamic_definition_for_insert(insert).unwrap();
        app.tabs[i].scene.document = actions::instance::refresh_after_edit(&changed, &baseline, def).unwrap();
        assert_coupled_geometry(&app, insert, 130., true, true);
        for ext in ["dwg", "dxf"] {
            let doc = &app.tabs[i].scene.document;
            let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
            let loaded = crate::io::load_bytes_finalized(std::path::Path::new(&format!("coupled.{ext}")), bytes).unwrap();
            let mut reopened = OpenCADStudio::new_for_test();
            let j = reopened.active_tab;
            reopened.tabs[j].scene.document = loaded;
            assert!(grip(&mut reopened, insert, lin_id, 1).world.distance(target) < EPS);
            assert_coupled_geometry(&reopened, insert, 130., true, true);
            drag(&mut reopened, insert, lin_id, 1, 10.);
            assert_coupled_geometry(&reopened, insert, 140., true, true);
            toggle(&mut reopened, insert, order[1]);
            assert_coupled_geometry(&reopened, insert, 140., !reverse, reverse);
            toggle(&mut reopened, insert, order[0]);
            assert_coupled_geometry(&reopened, insert, 140., false, false);
            select(&mut reopened, insert);
            let _ = reopened.reset_selected_block_references(false);
            assert_eq!(reopened.command_line.last_error, None);
            assert_coupled_geometry(&reopened, insert, 100., false, false);
            drag(&mut reopened, insert, lin_id, 1, 5.);
            assert_coupled_geometry(&reopened, insert, 105., false, false);
        }
    }
}

fn assert_window_arc(app: &OpenCADStudio, insert: Handle, center: DVec3, radius: f64) {
    let (_, arcs) = geometry(&app.tabs[app.active_tab].scene.document, insert);
    assert_eq!(arcs.len(), 1);
    assert!(arcs[0].0.distance(center) < EPS, "actual={:?} expected={center:?}", arcs[0]);
    assert!((arcs[0].1 - radius).abs() < EPS);
    assert!(arcs[0].2.abs() < EPS);
    assert!((arcs[0].3 - std::f64::consts::FRAC_PI_2).abs() < EPS);
}

#[test]
fn synthetic_window_scale_grips_refresh_reload_reset_and_continue() {
    for mirrored in [false, true] {
        let (mut app, plain, mirror, id) = window();
        let (insert, other) = if mirrored { (mirror, plain) } else { (plain, mirror) };
        let i = app.active_tab;
        let original = app.tabs[i].scene.document.clone();
        drag(&mut app, insert, id, 0, 20.);
        assert_window_arc(&app, insert, DVec3::new(52., 36., 0.), 24.);
        let edited = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::Undo);
        assert_eq!(geometry(&app.tabs[i].scene.document, insert), geometry(&original, insert));
        let def = original.dynamic_definition_for_insert(insert).unwrap();
        assert_eq!(actions::instance::load_action_bases(&app.tabs[i].scene.document, insert, def).unwrap(), actions::instance::load_action_bases(&original, insert, def).unwrap());
        let _ = app.update(Message::Redo);
        assert_eq!(geometry(&app.tabs[i].scene.document, insert), geometry(&edited, insert));
        assert_eq!(actions::instance::load_action_bases(&app.tabs[i].scene.document, insert, def).unwrap(), actions::instance::load_action_bases(&edited, insert, def).unwrap());
        let def = edited.dynamic_definition_for_insert(insert).unwrap();
        app.tabs[i].scene.document = actions::instance::refresh_after_edit(&edited, &original, def).unwrap();
        assert_window_arc(&app, insert, DVec3::new(52., 36., 0.), 24.);
        assert_window_arc(&app, other, DVec3::new(40., 30., 0.), 20.);
        for ext in ["dwg", "dxf"] {
            let doc = &app.tabs[i].scene.document;
            let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
            let loaded = crate::io::load_bytes(&format!("synthetic.{ext}"), bytes).unwrap();
            let mut reopened = OpenCADStudio::new_for_test();
            let j = reopened.active_tab;
            reopened.tabs[j].scene.document = loaded;
            drag(&mut reopened, insert, id, 1, 20.);
            // Incremental scale about base -20: -20 + (52 + 20) * 140/120.
            assert_window_arc(&reopened, insert, DVec3::new(64., 42., 0.), 28.);
            assert_window_arc(&reopened, other, DVec3::new(40., 30., 0.), 20.);
            let reset = actions::instance::reset(&reopened.tabs[j].scene.document, insert).unwrap().unwrap();
            reopened.tabs[j].scene.document = reset;
            assert_window_arc(&reopened, insert, DVec3::new(40., 30., 0.), 20.);
            drag(&mut reopened, insert, id, 1, 20.);
            assert_window_arc(&reopened, insert, DVec3::new(48., 36., 0.), 24.);
        }
    }
}

#[test]
fn synthetic_basepoint_stretch_grips_survive_finalized_open_and_continue() {
    for mirrored in [false, true] {
        let (mut app, plain, mirror, id) = window();
        begin(&mut app, plain);
        let i = app.active_tab;
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let doc = &mut app.tabs[i].scene.document;
        let action = actions::actions(doc, def).into_iter().find_map(|(h, _, kind, _)| {
            if kind != Kind::Stretch { return None }
            let document::objects::ObjectType::DynamicBlock(object) = &doc.objects[&h] else { return None };
            let document::objects::DynamicBlockData::StretchAction(action) = &object.data else { return None };
            action.connections.iter().any(|c| c.name == "BaseXDelta").then_some(h)
        }).unwrap();
        let basepoint = crate::scene::block_basepoint::create(doc, def, Vector3::ZERO).unwrap();
        let node = crate::scene::block_basepoint::parameter(doc, def).unwrap().1.parameter.parameter.element.eval.node_id;
        let document::objects::ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else { panic!() };
        let document::objects::DynamicBlockData::StretchAction(action) = &mut object.data else { panic!() };
        action.action.dependencies.push(basepoint);
        action.action.action_ids.push(node);
        action.codes.push(document::objects::BlockStretchCode { code: node, indexes: vec![0] });
        save_and_close(&mut app);
        let insert = if mirrored { mirror } else { plain };
        let target = drag(&mut app, insert, id, 0, 20.);
        let changed = app.tabs[i].scene.document.clone();
        for ext in ["dwg", "dxf"] {
            let bytes = crate::io::save_to_bytes(&changed, ext, changed.version).unwrap();
            let loaded = crate::io::load_bytes_finalized(std::path::Path::new(&format!("basepoint.{ext}")), bytes).unwrap();
            let mut reopened = OpenCADStudio::new_for_test();
            let j = reopened.active_tab;
            reopened.tabs[j].scene.document = loaded;
            let actual = grip(&mut reopened, insert, id, 0).world;
            assert!(actual.distance(target) < EPS, "{ext} mirrored={mirrored} actual={actual:?} expected={target:?}");
            assert_window_arc(&reopened, insert, DVec3::new(52., 36., 0.), 24.);
            drag(&mut reopened, insert, id, 1, 10.);
            assert_window_arc(&reopened, insert, DVec3::new(58., 39., 0.), 26.);
        }
    }
}
