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

fn labels(doc: &codec::Document, def: Handle, parameter: Handle) -> Vec<String> {
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
fn block_flip_grip_outside_the_editor_lists_states_and_applies_the_choice() {
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
            p.flip.then_some((grip.id, grip.world))
        })
        .expect("the reference draws the flip chevron");
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
    let before = app.tabs[i].scene.document.clone();
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
    click(&mut app, flip.1);
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the flip chevron lists states instead of starting a command"
    );
    let popup = app.flip_popup.as_ref().expect("flip state list");
    assert_eq!(popup.current, 0);
    assert_eq!(popup.labels, [BASE.to_string(), FLIPPED.to_string()]);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.update(Message::CommandEscape);
    assert!(app.flip_popup.is_none());
    assert_eq!(app.tabs[i].selected_handle, Some(reference));
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    click(&mut app, flip.1);
    let _ = app.update(Message::FlipPick(1));
    assert!(app.flip_popup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let flipped_end = member_end(&app);
    assert!((flipped_end.x + 1.).abs() < 1e-8, "{:?}", flipped_end);
    assert!(flipped_end.distance(&original_end) > 0.5);
    app.undo_steps(1);
    assert!(member_end(&app).distance(&original_end) < 1e-9);
    let _ = flip.0;
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
