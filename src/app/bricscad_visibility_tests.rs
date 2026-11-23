//! A reference to a BricsCAD parametric block names the block and shows its
//! visibility parameters in Properties, also when it shows an anonymous copy
//! of the block in another state.

use super::*;
use crate::scene::model::object::PropValue;
use document::Handle;

/// The BricsCAD drawing in `tests/fixtures/bricscad` (see its README), opened
/// the way the application opens a file.
fn opened() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bricscad/visibility-states.dwg"
    );
    app.automation_op(&serde_json::json!({"op": "open", "path": path}).to_string());
    let i = app.active_tab;
    assert!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("ocs-bricscad-visibility-block")
            .is_some(),
        "the fixture did not open"
    );
    app
}

fn select(app: &mut OpenCADStudio, references: &[u64]) {
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    for (index, &reference) in references.iter().enumerate() {
        app.tabs[i]
            .scene
            .select_entity(Handle::from(reference), index == 0);
    }
    app.tabs[i].selected_handle = (references.len() == 1).then(|| Handle::from(references[0]));
    app.refresh_properties();
}

/// The visibility rows as label, value and tooltip.
fn visibility_rows(app: &OpenCADStudio) -> Vec<(String, String, String)> {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .filter(|property| property.field == "bricscad_visibility")
        .map(|property| match &property.value {
            PropValue::ReadOnlyWithTooltip { value, tooltip } => {
                (property.label.clone(), value.clone(), tooltip.clone())
            }
            other => panic!("{} is {other:?}", property.label),
        })
        .collect()
}

fn block_name(app: &OpenCADStudio) -> String {
    let row = app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == "block")
        .expect("no block row");
    match &row.value {
        PropValue::EditChoice { value, .. } | PropValue::ReadOnly(value) => value.clone(),
        other => panic!("block row is {other:?}"),
    }
}

fn rows(lamp: &str, label: &str) -> Vec<(String, String, String)> {
    vec![
        ("LAMP".into(), lamp.into(), "On, Off".into()),
        ("LABEL".into(), label.into(), "Shown, Hidden".into()),
    ]
}

#[test]
fn a_reference_shows_the_states_it_was_set_to_and_names_its_block() {
    let mut app = opened();
    for (reference, lamp, label) in [
        (0xEE, "On", "Shown"),
        (0x146, "Off", "Shown"),
        (0x19E, "Off", "Hidden"),
    ] {
        select(&mut app, &[reference]);
        assert_eq!(visibility_rows(&app), rows(lamp, label), "{reference:X}");
        assert_eq!(
            block_name(&app),
            "ocs-bricscad-visibility-block",
            "{reference:X} shows its block's name, not its copy's"
        );
    }
}
