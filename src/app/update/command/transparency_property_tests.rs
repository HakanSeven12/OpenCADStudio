use super::*;

use crate::app::{Message, OpenCADStudio};
use crate::scene::model::object::PropValue;
use crate::ui::properties::FieldKey;

fn type_value(app: &mut OpenCADStudio, value: &str) {
    let _ = app.update(Message::PropGeomInput {
        field: "transparency",
        value: value.into(),
    });
    let _ = app.update(Message::PropGeomCommit("transparency"));
}

#[test]
fn transparency_typed_values_and_presets_update_a_solid_hatch() {
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;
    let handle = app
        .commit_entity_handle(document::EntityType::Hatch(Box::new(
            document::entities::Hatch::new(),
        )))
        .expect("solid hatch");
    app.tabs[i].scene.select_entity(handle, true);
    app.refresh_properties();

    for value in ["0", "37", "90"] {
        type_value(&mut app, value);
        let common = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common();
        assert_eq!(
            crate::scene::creation_style::current_transparency_label(common.transparency),
            value
        );
    }
    // A preset must replace an uncommitted custom value in both the
    // document and the displayed field, and close the menu.
    let _ = app.update(Message::PropGeomInput {
        field: "transparency",
        value: "37".into(),
    });
    app.tabs[i].properties.edit_choice_open = Some("transparency");
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "transparency",
        value: "50".into(),
    });
    assert!(app.tabs[i].properties.edit_choice_open.is_none());
    assert!(!app.tabs[i]
        .properties
        .edit_buf
        .contains_key(&FieldKey::Geom("transparency")));
    let property = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .find(|p| p.field == "transparency")
        .unwrap();
    let PropValue::EditChoice { value, options } = &property.value else {
        panic!("editable transparency");
    };
    assert_eq!(value, "50");
    assert_eq!(
        options,
        &crate::scene::creation_style::transparency_options()
    );

    let before = app.tabs[i]
        .scene
        .document
        .get_entity(handle)
        .unwrap()
        .common()
        .transparency;
    app.undo_active_tab();
    assert_ne!(
        app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .transparency,
        before
    );
}

#[test]
fn transparency_typing_without_selection_updates_the_creation_default() {
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;
    app.refresh_properties();
    type_value(&mut app, "37");
    assert_eq!(
        crate::scene::creation_style::current_transparency_label(
            app.tabs[i].scene.document.current_entity_transparency()
        ),
        "37"
    );
    for invalid in ["91", "-3", "25.5", "abc"] {
        type_value(&mut app, invalid);
        assert_eq!(
            crate::scene::creation_style::current_transparency_label(
                app.tabs[i].scene.document.current_entity_transparency()
            ),
            "37"
        );
    }
}
