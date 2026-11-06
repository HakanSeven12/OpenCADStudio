use super::*;
use crate::scene::model::object::{BlockParameterComponent, PropValue};
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance, block_point_parameter as point,
    block_visibility,
};
use glam::DVec3;

#[test]
fn nested_dynamic_properties_follow_editor_and_drawing_history() {
    let (mut app, independent) = fixture();
    begin(&mut app, independent);
    let i = app.active_tab;
    let child_def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let child_member = members(&app)[0];
    let doc = &mut app.tabs[i].scene.document;
    let linear = distance::create(
        doc,
        child_def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    actions::create(
        doc,
        child_def,
        actions::Kind::Move,
        linear,
        &[child_member],
        DVec3::ZERO,
    )
    .unwrap();
    let visibility = block_visibility::create(doc, child_def, Vector3::ZERO).unwrap();
    block_visibility::add_state(doc, visibility, "Hidden", 0).unwrap();
    block_visibility::set_visible(doc, visibility, 1, &[child_member], false);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");

    let scene = &mut app.tabs[i].scene;
    scene.define_block_raw(
        "PARENT",
        Vector3::ZERO,
        vec![
            EntityType::Insert(Insert::new("EDIT_ME", Vector3::UNIT_Y)),
            EntityType::Insert(Insert::new("EDIT_ME", Vector3::UNIT_Y * 5.)),
            EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X)),
        ],
    );
    let doc = &mut scene.document;
    let record = doc.block_records.get("PARENT").unwrap();
    let parent_def = record.handle;
    let nested = record.entity_handles[0];
    let sibling = record.entity_handles[1];
    let line = record.entity_handles[2];
    let move_param = point::create(doc, parent_def, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
    actions::create(
        doc,
        parent_def,
        actions::Kind::Move,
        move_param,
        &[line],
        DVec3::ZERO,
    )
    .unwrap();
    let parent = doc
        .add_entity(EntityType::Insert(Insert::new("PARENT", Vector3::ZERO)))
        .unwrap();
    *doc =
        actions::instance::evaluate_value(doc, parent, move_param, actions::Value::Point(DVec3::X))
            .unwrap()
            .unwrap();
    let parameter = actions::all_parameters(doc, child_def)
        .into_iter()
        .find(|p| p.handle == linear)
        .unwrap();
    let length = |app: &OpenCADStudio, reference| {
        let frame =
            actions::instance::frame(&app.tabs[i].scene.document, reference, &parameter).unwrap();
        frame.base.distance(frame.end)
    };
    let evaluated_child = |app: &OpenCADStudio| {
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(reference) = doc.get_entity(parent).unwrap() else {
            panic!()
        };
        doc.entities_in_block(&reference.block_name)
            .find_map(|e| matches!(e, EntityType::Insert(_)).then(|| e.common().handle))
            .unwrap()
    };
    let select_nested = |app: &mut OpenCADStudio| {
        app.tabs[i].scene.select_entity(nested, true);
        app.tabs[i].selected_handle = Some(nested);
        app.refresh_properties();
    };
    let key = |app: &OpenCADStudio, visible: bool| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find_map(|row| match &row.value {
                PropValue::BlockNumber { key, .. }
                    if !visible && key.component == BlockParameterComponent::Distance =>
                {
                    Some(*key)
                }
                PropValue::BlockChoice { key, .. }
                    if visible
                        && matches!(key.component, BlockParameterComponent::Visibility(_)) =>
                {
                    Some(*key)
                }
                _ => None,
            })
            .unwrap()
    };
    let submit = |app: &mut OpenCADStudio, key, value: &str| {
        let _ = app.update(Message::PropBlockInput {
            key,
            value: value.into(),
        });
        let _ = app.update(Message::PropBlockCommit(key));
    };

    let drawing_depth = main_depth(&app);
    begin(&mut app, parent);
    select_nested(&mut app);
    let stale = key(&app, false);
    submit(&mut app, stale, "15");
    assert_eq!(length(&app, nested), 15.);
    assert_eq!(length(&app, sibling), 10.);
    assert_eq!(length(&app, independent), 10.);
    assert_eq!(main_depth(&app), drawing_depth);
    app.undo_steps(1);
    assert_eq!(length(&app, nested), 10.);
    app.redo_steps(1);
    assert_eq!(length(&app, nested), 15.);
    select_nested(&mut app);
    let _ = app.update(Message::PropBlockChoice {
        key: key(&app, true),
        index: 1,
    });
    assert!(app.save_block_edit(i), "{:?}", app.command_line.last_error);
    app.close_block_edit(i, false);
    assert_eq!(main_depth(&app), drawing_depth + 1);
    let evaluated = evaluated_child(&app);
    assert_eq!(length(&app, evaluated), 15.);
    assert!(matches!(
        OpenCADStudio::visibility_property(&app.tabs[i].scene.document, evaluated).unwrap().value,
        PropValue::ChoiceWithTooltip { selected, .. } if selected == "Hidden"
    ));
    app.undo_steps(1);
    assert_eq!(length(&app, nested), 10.);
    assert_eq!(length(&app, evaluated_child(&app)), 10.);
    app.redo_steps(1);
    assert_eq!(length(&app, nested), 15.);
    assert_eq!(length(&app, evaluated_child(&app)), 15.);
    for extension in ["dwg", "dxf"] {
        let doc = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
        let saved =
            crate::io::load_bytes(&format!("nested-properties.{extension}"), bytes).unwrap();
        for reference in [nested, evaluated] {
            assert_eq!(
                saved.dynamic_definition_for_insert(reference),
                Some(child_def)
            );
            let frame = actions::instance::frame(&saved, reference, &parameter).unwrap();
            assert_eq!(frame.base.distance(frame.end), 15.);
            assert!(matches!(
                OpenCADStudio::visibility_property(&saved, reference).unwrap().value,
                PropValue::ChoiceWithTooltip { selected, .. } if selected == "Hidden"
            ));
        }
    }

    begin(&mut app, parent);
    select_nested(&mut app);
    let current = key(&app, false);
    assert_ne!(current, stale);
    submit(&mut app, stale, "25");
    assert_eq!(length(&app, nested), 15.);
    submit(&mut app, current, "20");
    assert_eq!(length(&app, nested), 20.);
    app.close_block_edit(i, true);
    assert_eq!(length(&app, nested), 15.);
    assert_eq!(length(&app, evaluated_child(&app)), 15.);

    begin(&mut app, parent);
    select_nested(&mut app);
    let current = key(&app, false);
    submit(&mut app, current, "21");
    assert!(
        app.save_block_edit_as(i, "PARENT_COPY"),
        "{:?}",
        app.command_line.last_error
    );
    app.close_block_edit(i, false);
    let copied_nested = |app: &OpenCADStudio| {
        app.tabs[i]
            .scene
            .document
            .entities_in_block("PARENT_COPY")
            .find_map(|entity| {
                matches!(entity, EntityType::Insert(_)).then(|| entity.common().handle)
            })
            .unwrap()
    };
    assert_eq!(length(&app, nested), 15.);
    let copied = copied_nested(&app);
    assert_eq!(length(&app, copied), 21.);
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("PARENT_COPY")
        .is_none());
    assert!(app.tabs[i].scene.document.get_entity(copied).is_none());
    app.redo_steps(1);
    assert_eq!(length(&app, copied_nested(&app)), 21.);
}
