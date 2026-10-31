use super::*;
use crate::scene::{
    block_actions as actions,
    block_authoring::metadata,
    block_distance_parameter as distance,
    block_grips::{config, runtime},
    block_orientation_parameter as orientation,
};

// Drawing-authored test data, not UI labels or Fluent translations.
const AUTHORED_DESCRIPTION: &str = "  Façade — مسافة — 距離 ";
const AUTHORED_SECONDARY_DESCRIPTION: &str = "Angle help \\ literal";

fn numeric_fixture(kind: &str, testing: bool) -> (OpenCADStudio, usize, Handle, Handle, Handle) {
    let (mut app, reference) = fixture();
    let source = app.active_tab;
    begin(&mut app, reference);
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let member = members(&app)[0];
    let doc = &mut app.tabs[source].scene.document;
    let parameter = match kind {
        "Point" => crate::scene::block_point_parameter::create(
            doc,
            definition,
            Vector3::ZERO,
            Vector3::UNIT_Y,
        )
        .unwrap(),
        "Linear" | "Polar" | "XY" => distance::create(
            doc,
            definition,
            match kind {
                "Linear" => distance::Kind::Linear,
                "Polar" => distance::Kind::Polar,
                _ => distance::Kind::XY,
            },
            Vector3::ZERO,
            Vector3::new(10., 5., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap(),
        _ => orientation::create(
            doc,
            definition,
            if kind == "Flip" {
                orientation::Kind::Flip
            } else {
                orientation::Kind::Rotation
            },
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::new(0., 10., 0.),
        )
        .unwrap(),
    };
    actions::create(
        doc,
        definition,
        match kind {
            "Flip" => actions::Kind::Flip,
            "Rotation" => actions::Kind::Rotate,
            _ => actions::Kind::Move,
        },
        parameter,
        &[member],
        glam::DVec3::ZERO,
    )
    .unwrap();
    metadata::set_text(
        doc,
        definition,
        parameter,
        metadata::Field::Description,
        AUTHORED_DESCRIPTION,
    )
    .unwrap();
    if matches!(kind, "XY" | "Polar") {
        metadata::set_text(
            doc,
            definition,
            parameter,
            metadata::Field::SecondaryDescription,
            AUTHORED_SECONDARY_DESCRIPTION,
        )
        .unwrap();
    }
    if kind == "XY" {
        config::set(doc, definition, parameter, Some(4)).unwrap();
    }
    if testing {
        let _ = app.open_block_test(source);
    } else {
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
    }
    let i = app.active_tab;
    let insert = app.tabs[i]
        .block_test
        .as_ref()
        .map_or(reference, |test| test.insert);
    app.tabs[i].scene.select_entity(insert, true);
    app.tabs[i].selected_handle = Some(insert);
    app.refresh_properties();
    (app, source, definition, parameter, insert)
}

#[test]
fn block_parameter_grip_descriptions_follow_components_in_drawing_and_test_block() {
    for testing in [false, true] {
        for kind in ["Point", "Linear", "Polar", "XY", "Rotation", "Flip"] {
            let (app, source, _, parameter, insert) = numeric_fixture(kind, testing);
            let i = app.active_tab;
            let before = app.tabs[i].scene.document.clone();
            let source_before = app.tabs[source].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            let grips: Vec<_> = app.tabs[i]
                .selected_grips
                .iter()
                .filter(|grip| runtime::decode(grip.id).is_some())
                .collect();
            assert!(!grips.is_empty(), "{kind}");
            for grip in grips {
                let (_, role) = runtime::decode(grip.id).unwrap();
                let tip = app.dynamic_grip_tip(insert, grip.id).unwrap();
                assert!(
                    tip.contains(if kind == "XY" && role == 3 {
                        AUTHORED_SECONDARY_DESCRIPTION
                    } else {
                        AUTHORED_DESCRIPTION
                    }),
                    "{kind}, role {role}: {tip}"
                );
                if kind == "XY" && role == 2 {
                    assert!(!tip.contains(AUTHORED_SECONDARY_DESCRIPTION));
                }
                if kind == "XY" && role == 3 {
                    assert!(!tip.contains(AUTHORED_DESCRIPTION));
                }
                if kind == "Polar" || (kind == "XY" && role == 1) {
                    assert!(tip.contains(AUTHORED_SECONDARY_DESCRIPTION));
                }
            }
            assert!(before.objects.contains_key(&parameter));
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[source].scene.document, source_before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
    }
}

#[test]
fn block_grip_description_hover_dwell_guards_and_native_visibility_are_independent() {
    let (mut app, _, definition, parameter, insert) = numeric_fixture("Linear", false);
    let i = app.active_tab;
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|grip| runtime::decode(grip.id).is_some())
        .unwrap()
        .clone();
    let document = app.tabs[i].id;
    let mut hover = super::super::GripHover {
        document,
        handle: insert,
        grip_id: grip.id,
        screen: iced::Point::new(400., 300.),
        started: iced::time::Instant::now(),
    };
    app.grip_hover = Some(hover.clone());
    assert!(app.hovered_dynamic_grip_tip().is_none());
    hover.started -= std::time::Duration::from_millis(600);
    app.grip_hover = Some(hover.clone());
    assert!(app.hovered_dynamic_grip_tip().unwrap().1.contains(AUTHORED_DESCRIPTION));
    metadata::set_visible(
        &mut app.tabs[i].scene.document,
        definition,
        parameter,
        false,
    )
    .unwrap();
    assert!(
        app.hovered_dynamic_grip_tip().unwrap().1.contains(AUTHORED_DESCRIPTION),
        "Show Properties does not hide an enabled grip's help"
    );
    app.grip_hover.as_mut().unwrap().document += 1;
    assert!(app.hovered_dynamic_grip_tip().is_none());
    app.grip_hover = Some(hover.clone());
    let _ = app.dispatch_command("GRIPTIPS 0");
    assert!(app.hovered_dynamic_grip_tip().is_none());
    let _ = app.dispatch_command("GRIPTIPS 1");
    app.grip_hover = Some(hover.clone());
    let _ = app.dispatch_command("LINE");
    assert!(app.hovered_dynamic_grip_tip().is_none());
    let _ = app.update(Message::CommandEscape);
    app.grip_hover = Some(hover.clone());
    app.tabs[i].selected_grips.clear();
    assert!(app.hovered_dynamic_grip_tip().is_none());
    app.refresh_properties();
    app.grip_hover = Some(hover);
    metadata::set_text(
        &mut app.tabs[i].scene.document,
        definition,
        parameter,
        metadata::Field::Description,
        "   ",
    )
    .unwrap();
    let tip = app.dynamic_grip_tip(insert, grip.id).unwrap();
    assert!(!tip.contains(AUTHORED_DESCRIPTION));
    assert!(
        !tip.contains('\n'),
        "empty descriptions retain the existing value summary"
    );
    config::set(
        &mut app.tabs[i].scene.document,
        definition,
        parameter,
        Some(0),
    )
    .unwrap();
    assert!(
        app.dynamic_grip_tip(insert, grip.id).is_none(),
        "disabled native grips reject stale help"
    );
}

#[test]
fn block_editor_lookup_visibility_and_component_grips_use_owned_description_text() {
    use crate::scene::{block_authoring::edit, block_lookup, block_visibility};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let doc = &mut app.tabs[i].scene.document;
    let lookup = block_lookup::create(doc, definition, Vector3::ZERO).unwrap();
    let visibility = block_visibility::create(doc, definition, Vector3::UNIT_Y).unwrap();
    let xy = distance::create(
        doc,
        definition,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 5., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    for parameter in [lookup, visibility, xy] {
        metadata::set_text(
            doc,
            definition,
            parameter,
            metadata::Field::Description,
            AUTHORED_DESCRIPTION,
        )
        .unwrap();
    }
    metadata::set_text(
        doc,
        definition,
        xy,
        metadata::Field::SecondaryDescription,
        AUTHORED_SECONDARY_DESCRIPTION,
    )
    .unwrap();
    for parameter in [lookup, visibility, xy] {
        app.tabs[i].selected_authoring = Some(parameter);
        app.refresh_properties();
        let item = edit::items(&app.tabs[i].scene.document, definition)
            .into_iter()
            .find(|item| item.handle == parameter)
            .unwrap();
        for (index, (part, _)) in item.grips.iter().enumerate() {
            let grip = app.tabs[i]
                .selected_grips
                .iter()
                .find(|grip| {
                    super::super::block_authoring_edit::editor_grip_index(grip.id) == Some(index)
                })
                .unwrap();
            let tip = app.dynamic_grip_tip(parameter, grip.id).unwrap();
            assert!(tip.contains(AUTHORED_DESCRIPTION));
            assert!(
                app.dynamic_grip_tip(reference, grip.id).is_none(),
                "foreign owner is rejected"
            );
        }
    }
    let source_before = app.tabs[i].scene.document.clone();
    let _ = app.open_block_test(i);
    let test = app.active_tab;
    let insert = app.tabs[test].block_test.as_ref().unwrap().insert;
    app.tabs[test].scene.select_entity(insert, true);
    app.tabs[test].selected_handle = Some(insert);
    app.refresh_properties();
    assert!(app
        .dynamic_grip_tip(insert, super::super::visibility::VIS_GRIP_ID)
        .unwrap()
        .contains(AUTHORED_DESCRIPTION));
    assert_eq!(app.tabs[i].scene.document, source_before);
}
