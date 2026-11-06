use super::*;
use crate::scene::model::object::{
    BlockParameterComponent as Component, BlockParameterKey, PropValue,
};
use crate::scene::{
    block_actions as actions, block_authoring::metadata,
    block_orientation_parameter as orientation, block_parameter_properties as native,
};
use crate::ui::properties::FieldKey;

const BASE: &str = "  Ouvert — مفتوح — 1 + 2  ";
const FLIPPED: &str = "Fermé — 關閉 \\ literal";

fn setup() -> (OpenCADStudio, Handle, Handle, Handle) {
    let (mut app, insert) = fixture();
    begin(&mut app, insert);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let member = members(&app)[0];
    let doc = &mut app.tabs[i].scene.document;
    let parameter = orientation::create(
        doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::new(0., 10., 0.),
        Vector3::UNIT_X,
    )
    .unwrap();
    actions::create(
        doc,
        def,
        actions::Kind::Flip,
        parameter,
        &[member],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    app.tabs[i].scene.selected.clear();
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    (app, insert, def, parameter)
}

fn edit(app: &mut OpenCADStudio, field: &'static str, value: &str) {
    let _ = app.update(Message::PropGeomInput {
        field,
        value: value.into(),
    });
    let _ = app.update(Message::PropGeomCommit(field));
}

fn labels(doc: &document::Document, def: Handle, parameter: Handle) -> Vec<String> {
    metadata::read(doc, def, parameter)
        .unwrap()
        .text
        .into_iter()
        .filter(|text| {
            matches!(
                text.field,
                metadata::Field::BaseStateLabel | metadata::Field::FlippedStateLabel
            )
        })
        .map(|text| text.value.to_owned())
        .collect()
}

fn choice(app: &OpenCADStudio) -> (BlockParameterKey, usize, Vec<String>) {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .find_map(|p| match &p.value {
            PropValue::BlockChoice {
                key,
                selected,
                options,
                ..
            } if key.component == Component::Flip => Some((*key, *selected, options.clone())),
            _ => None,
        })
        .unwrap()
}

#[test]
fn batch_reference_properties_edit_common_numeric_and_state_rows() {
    let (mut app, first, def, flip) = setup();
    let i = app.active_tab;
    let linear = crate::scene::block_distance_parameter::create(
        &mut app.tabs[i].scene.document,
        def,
        crate::scene::block_distance_parameter::Kind::Linear,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let doc = &mut app.tabs[i].scene.document;
    let second = doc
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::UNIT_X * 20.,
        )))
        .unwrap();
    let other = doc
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::UNIT_X * 40.,
        )))
        .unwrap();
    *doc = actions::instance::evaluate_value(doc, first, flip, actions::Value::Flip(true))
        .unwrap()
        .unwrap();
    *doc = actions::instance::evaluate_value(
        doc,
        first,
        linear,
        actions::Value::Point(glam::DVec3::X * 12.),
    )
    .unwrap()
    .unwrap();
    let parameters = actions::all_parameters(doc, def);
    let distance = parameters.iter().find(|p| p.handle == linear).unwrap();
    let flip = parameters.iter().find(|p| p.handle == flip).unwrap();
    let select = |app: &mut OpenCADStudio, second, refresh| {
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(first, true);
        app.tabs[i].scene.select_entity(second, false);
        if refresh {
            app.refresh_properties();
        }
    };
    select(&mut app, second, true);
    let number = |app: &OpenCADStudio| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .find_map(|row| match &row.value {
                PropValue::BlockNumber { key, value, .. }
                    if key.component == Component::Distance =>
                {
                    Some((*key, value.clone()))
                }
                _ => None,
            })
            .unwrap()
    };
    let (key, mixed) = number(&app);
    assert!(key.batch_generation.is_some());
    assert_eq!(mixed, crate::tr!("common", "varies"));
    let (flip_key, selected, _) = choice(&app);
    assert_eq!(selected, crate::ui::properties::MIXED_BLOCK_CHOICE);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let submit = |app: &mut OpenCADStudio, key, value: &str| {
        let _ = app.update(Message::PropBlockInput {
            key,
            value: value.into(),
        });
        let _ = app.update(Message::PropBlockCommit(key));
    };
    submit(&mut app, key, "-3");
    assert!(app.tabs[i].scene.document == before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);

    // The second evaluation must be able to fail after staging the first.
    let last = *app.tabs[i].properties.source_handles.last().unwrap();
    let doc = &mut app.tabs[i].scene.document;
    let dictionary = doc.ensure_extension_dictionary(last);
    let Some(document::objects::ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&dictionary)
    else {
        panic!()
    };
    dictionary.owner = Handle::NULL;
    let malformed = doc.clone();
    assert!(app.block_property_target_is_current(key));
    submit(&mut app, key, "15");
    assert!(app.tabs[i].scene.document == malformed);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].scene.document = before.clone();

    submit(&mut app, key, "8 + 7");
    let changed = app.tabs[i].scene.document.clone();
    for (reference, expected) in [(first, 15.), (second, 15.), (other, 10.)] {
        let frame = actions::instance::frame(&changed, reference, distance).unwrap();
        assert_eq!(frame.base.distance(frame.end), expected);
    }
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    let restored = &app.tabs[i].scene.document;
    for (reference, expected) in [(first, 12.), (second, 10.), (other, 10.)] {
        let frame = actions::instance::frame(restored, reference, distance).unwrap();
        assert_eq!(frame.base.distance(frame.end), expected);
        assert_eq!(restored.get_entity(reference), before.get_entity(reference));
    }
    app.redo_steps(1);
    assert!(app.tabs[i].scene.document == changed);
    assert_eq!(number(&app).0, key);

    select(&mut app, other, false);
    let _ = app.update(Message::PropBlockChoice {
        key: flip_key,
        index: 1,
    });
    assert!(app.tabs[i].scene.document == changed);
    select(&mut app, other, true);
    assert_ne!(choice(&app).0, flip_key);
    select(&mut app, second, true);
    let (current, _, _) = choice(&app);
    let _ = app.update(Message::PropBlockChoice {
        key: flip_key,
        index: 1,
    });
    assert!(app.tabs[i].scene.document == changed);
    let _ = app.update(Message::PropBlockChoice {
        key: current,
        index: 1,
    });
    for (reference, flipped) in [(first, true), (second, true), (other, false)] {
        assert_eq!(
            actions::instance::value(&app.tabs[i].scene.document, reference, flip).unwrap(),
            actions::Value::Flip(flipped)
        );
    }
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    let _ = app.update(Message::PropBlockChoice { key: current, index: 1 });
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
}

#[test]
fn block_flip_grip_toggles_directly_and_reverses_the_arrow() {
    let (mut app, reference, def, _) = setup();
    edit(&mut app, "authoring_base_state_label", BASE);
    edit(&mut app, "authoring_flipped_state_label", FLIPPED);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-4., -3., 0.),
        glam::DVec3::new(13., 13., 0.),
        1600. / 900.,
    );
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let flip = app.tabs[i]
        .selected_grips
        .iter()
        .find_map(|grip| {
            let (node, _) = crate::scene::block_grips::runtime::decode(grip.id)?;
            let p = actions::all_parameters(&app.tabs[i].scene.document, def)
                .into_iter()
                .find(|p| p.id == node)?;
            p.flip.then_some(grip.clone())
        })
        .expect("the reference draws the Flip arrow");
    assert_eq!(flip.shape, crate::scene::model::object::GripShape::Arrow);
    assert_eq!(choice(&app).2, [BASE, FLIPPED]);
    let click = |app: &mut OpenCADStudio, world: glam::DVec3| {
        let scene = &app.tabs[app.active_tab].scene;
        let (w, h) = scene.selection.borrow().view.vp_size;
        let tile = scene.active_model_tile_bounds(w, h);
        let at = scene
            .camera
            .borrow()
            .project(
                world,
                iced::Rectangle {
                    x: 0.,
                    y: 0.,
                    width: tile.width,
                    height: tile.height,
                },
            )
            .unwrap();
        let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(
            at.x + tile.x,
            at.y + tile.y,
        )));
        let _ = app.update(Message::ViewportLeftRelease);
    };
    let member_end = |app: &OpenCADStudio| {
        let doc = &app.tabs[app.active_tab].scene.document;
        let EntityType::Insert(ins) = doc.get_entity(reference).unwrap() else {
            panic!()
        };
        let member = doc.block_records.get(&ins.block_name).unwrap().entity_handles[0];
        let EntityType::Line(line) = doc.get_entity(member).unwrap() else {
            panic!()
        };
        line.end
    };
    let original_end = member_end(&app);
    let depth = app.tabs[i].history.undo_stack.len();
    click(&mut app, flip.world);
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(app.grip_popup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let flipped_end = member_end(&app);
    assert!((flipped_end.x + 1.).abs() < 1e-8, "{:?}", flipped_end);
    assert!(flipped_end.distance(&original_end) > 0.5);
    let reversed = app.tabs[i]
        .selected_grips
        .iter()
        .find(|grip| grip.id == flip.id)
        .expect("the selected reference retains its Flip arrow");
    assert!((reversed.dir.unwrap() + flip.dir.unwrap()).length() < 1e-9);
    assert!(reversed.world.distance(flip.world) < 1e-9);
    assert_eq!(choice(&app).1, 1);
    click(&mut app, flip.world);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    assert!(member_end(&app).distance(&original_end) < 1e-9);
    app.undo_steps(1);
    assert!(member_end(&app).distance(&flipped_end) < 1e-9);
}

#[test]
fn block_flip_state_label_authoring_is_literal_undoable_and_checkpointed() {
    let (mut app, insert, def, parameter) = setup();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for field in [
        "authoring_base_state_label",
        "authoring_flipped_state_label",
    ] {
        assert!(app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .any(|p| p.field == field && matches!(p.value, PropValue::PlainText(_))));
    }
    assert_eq!(labels(&before, def, parameter), ["", ""]);
    edit(&mut app, "authoring_base_state_label", BASE);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    edit(&mut app, "authoring_flipped_state_label", FLIPPED);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    let authored = app.tabs[i].scene.document.clone();
    assert_eq!(labels(&authored, def, parameter), [BASE, FLIPPED]);
    edit(&mut app, "authoring_flipped_state_label", FLIPPED);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    edit(&mut app, "authoring_flipped_state_label", "bad\0label");
    assert_eq!(app.tabs[i].scene.document, authored);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    assert!(app.tabs[i]
        .properties
        .edit_buf
        .contains_key(&FieldKey::Geom("authoring_flipped_state_label")));
    app.undo_steps(1);
    assert_eq!(
        labels(&app.tabs[i].scene.document, def, parameter),
        [BASE, ""]
    );
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, authored);
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&authored, extension, authored.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("flip-labels.{extension}"), bytes).unwrap();
        assert_eq!(labels(&loaded, def, parameter), [BASE, FLIPPED]);
    }
    let _ = app.dispatch_command("BEDIT_DISCARD");
    begin(&mut app, insert);
    assert_eq!(
        labels(&app.tabs[i].scene.document, def, parameter),
        ["", ""]
    );
}

#[test]
fn block_flip_authored_labels_drive_reference_choices_and_grip_summaries_in_both_contexts() {
    for testing in [false, true] {
        let (mut app, reference, def, parameter) = setup();
        let source = app.active_tab;
        edit(&mut app, "authoring_base_state_label", BASE);
        edit(&mut app, "authoring_flipped_state_label", FLIPPED);
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
        let source_before = app.tabs[source].scene.document.clone();
        let (key, selected, options) = choice(&app);
        assert_eq!(selected, 0);
        assert_eq!(options, [BASE, FLIPPED]);
        let id = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| crate::scene::block_grips::runtime::decode(g.id).is_some())
            .unwrap()
            .id;
        assert!(app.dynamic_grip_tip(insert, id).unwrap().contains(BASE));
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.update(Message::PropBlockChoice { key, index: 1 });
        assert_eq!(choice(&app).1, 1);
        assert!(app.dynamic_grip_tip(insert, id).unwrap().contains(FLIPPED));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        let EntityType::Line(line) = doc.entities_in_block(&reference.block_name).next().unwrap()
        else {
            panic!()
        };
        assert!((line.end.x + 1.).abs() < 1e-8);
        app.undo_steps(1);
        assert_eq!(choice(&app).1, 0);
        app.redo_steps(1);
        assert_eq!(choice(&app).1, 1);
        assert_eq!(
            labels(&app.tabs[i].scene.document, def, parameter),
            [BASE, FLIPPED]
        );
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

#[test]
fn block_flip_blank_defaults_duplicate_labels_and_foreign_field_guards() {
    let (mut app, _, def, parameter) = setup();
    let i = app.active_tab;
    edit(&mut app, "authoring_base_state_label", "Same");
    edit(&mut app, "authoring_flipped_state_label", "Same");
    assert_eq!(
        labels(&app.tabs[i].scene.document, def, parameter),
        ["Same", "Same"]
    );
    edit(&mut app, "authoring_base_state_label", "");
    edit(&mut app, "authoring_flipped_state_label", "");
    let doc = &app.tabs[i].scene.document;
    assert_eq!(
        native::flip_state_label(doc, parameter, false).unwrap(),
        crate::tr!("block-grip", "not-flipped")
    );
    assert_eq!(
        native::flip_state_label(doc, parameter, true).unwrap(),
        crate::tr!("block-grip", "flipped")
    );
    let point = crate::scene::block_point_parameter::create(
        &mut app.tabs[i].scene.document,
        def,
        Vector3::ZERO,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let before = app.tabs[i].scene.document.clone();
    assert!(metadata::set_text(
        &mut app.tabs[i].scene.document,
        def,
        point,
        metadata::Field::BaseStateLabel,
        "unsupported"
    )
    .is_err());
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(metadata::set_text(
        &mut app.tabs[i].scene.document,
        Handle::NULL,
        parameter,
        metadata::Field::FlippedStateLabel,
        "foreign"
    )
    .is_err());
    assert_eq!(app.tabs[i].scene.document, before);
}
