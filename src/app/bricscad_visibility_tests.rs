//! A reference to a BricsCAD parametric block names the block and lists its
//! visibility parameters in Properties, also when it shows an anonymous copy
//! of the block in another state, and a state picked there is shown the way
//! BricsCAD shows it.

use super::*;
use crate::scene::model::object::{BlockParameterComponent, BlockParameterKey, PropValue};
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

/// The visibility rows as label, shown state and states, with their keys.
fn visibility_rows(app: &OpenCADStudio) -> Vec<(String, String, Vec<String>, BlockParameterKey)> {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .filter_map(|property| match &property.value {
            PropValue::BlockChoice {
                key,
                selected,
                options,
                ..
            } if matches!(key.component, BlockParameterComponent::Bricscad(_)) => Some((
                property.label.clone(),
                options
                    .get(*selected)
                    .cloned()
                    .unwrap_or_else(|| "*VARIES*".into()),
                options.clone(),
                *key,
            )),
            _ => None,
        })
        .collect()
}

fn shown(app: &OpenCADStudio) -> Vec<(String, String)> {
    visibility_rows(app)
        .into_iter()
        .map(|(label, state, ..)| (label, state))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
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

/// Pick `state` in the list of `parameter`, as a click in Properties does.
fn pick(app: &mut OpenCADStudio, parameter: &str, state: &str) {
    let (_, _, options, key) = visibility_rows(app)
        .into_iter()
        .find(|(label, ..)| label == parameter)
        .unwrap_or_else(|| panic!("no {parameter} row"));
    let index = options.iter().position(|option| option == state).unwrap();
    let _ = app.update(Message::PropBlockChoice { key, index });
}

#[test]
fn a_reference_lists_the_states_it_was_set_to_and_names_its_block() {
    let mut app = opened();
    for (reference, lamp, label) in [
        (0xEE, "On", "Shown"),
        (0x146, "Off", "Shown"),
        (0x19E, "Off", "Hidden"),
    ] {
        select(&mut app, &[reference]);
        assert_eq!(
            shown(&app),
            pairs(&[("LAMP", lamp), ("LABEL", label)]),
            "{reference:X}"
        );
        let rows = visibility_rows(&app);
        assert_eq!(rows[0].2, ["On", "Off"]);
        assert_eq!(rows[1].2, ["Shown", "Hidden"]);
        assert_eq!(
            block_name(&app),
            "ocs-bricscad-visibility-block",
            "{reference:X} shows its block's name, not its copy's"
        );
    }
}

#[test]
fn a_state_picked_in_properties_is_shown_and_undone_in_one_step() {
    let mut app = opened();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    select(&mut app, &[0xEE]);
    pick(&mut app, "LABEL", "Hidden");
    assert_eq!(shown(&app), pairs(&[("LAMP", "On"), ("LABEL", "Hidden")]));
    assert!(app.tabs[i].dirty);
    let _ = app.update(Message::Undo);
    select(&mut app, &[0xEE]);
    assert_eq!(shown(&app), pairs(&[("LAMP", "On"), ("LABEL", "Shown")]));
    assert!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("*U4")
            .is_none(),
        "undo takes the new copy away"
    );
    assert_eq!(
        app.tabs[i].scene.document.block_records.iter().count(),
        before.block_records.iter().count()
    );
}

#[test]
fn references_selected_together_switch_together() {
    let mut app = opened();
    select(&mut app, &[0xEE, 0x146]);
    assert_eq!(
        shown(&app),
        pairs(&[("LAMP", "*VARIES*"), ("LABEL", "Shown")])
    );
    pick(&mut app, "LABEL", "Hidden");
    for (reference, lamp) in [(0xEE, "On"), (0x146, "Off")] {
        select(&mut app, &[reference]);
        assert_eq!(
            shown(&app),
            pairs(&[("LAMP", lamp), ("LABEL", "Hidden")]),
            "{reference:X}"
        );
    }
}
