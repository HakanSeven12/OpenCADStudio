use super::*;
use crate::scene::model::object::PropValue;
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{EntityType, Handle};

/// The fixture reference is given its own evaluated `*U` copy, the way a
/// visibility state change isolates an instance, and the definition carries a
/// non-unitless record.
fn anonymous_reference(app: &mut OpenCADStudio) -> Handle {
    let i = app.active_tab;
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let parameter = {
        let doc = &mut app.tabs[i].scene.document;
        if let Some(record) = doc.block_records.get_mut("EDIT_ME") {
            record.units = 1;
        }
        let parameter = crate::scene::block_visibility::create(doc, definition, Vector3::ZERO)
            .expect("visibility parameter");
        crate::scene::block_visibility::add_state(doc, parameter, "Open", 0).unwrap();
        crate::scene::block_visibility::finish_editing(doc, definition);
        parameter
    };
    let _ = parameter;
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(0., 120., 0.),
        )))
        .unwrap();
    let copy = app.tabs[i]
        .scene
        .visibility_representation(reference)
        .expect("evaluated copy");
    assert!(copy.starts_with("*U"), "{copy}");
    reference
}

fn row(app: &OpenCADStudio, field: &str) -> String {
    let row = app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == field)
        .unwrap_or_else(|| panic!("no {field} row"));
    match &row.value {
        PropValue::ReadOnly(text) | PropValue::PlainText(text) | PropValue::EditText(text) => {
            text.clone()
        }
        other => panic!("{field} is {other:?}"),
    }
}

#[test]
fn block_reference_properties_hide_internal_records_and_name_the_definition() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let sections = &app.tabs[i].properties.sections;
    assert!(
        !sections.iter().any(|section| section.title == "Dynamic Block"
            || section.title == "Dynamic Parameters / Actions"),
        "{:?}",
        sections.iter().map(|s| &s.title).collect::<Vec<_>>()
    );
    assert!(
        !sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| property.label.starts_with("Definition ")
                || property.label == "Native records"),
        "internal definition rows leak into the panel"
    );
    assert_eq!(row(&app, "block"), "EDIT_ME");
    assert_eq!(row(&app, "block_unit"), "Inches");
}
