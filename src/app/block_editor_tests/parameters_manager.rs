use super::*;
use crate::scene::block_parameters_manager as manager;

fn edited(commands: &[&str], attributes: &[&str]) -> Vec<manager::Row> {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    for command in commands {
        let _ = app.run_command_line(command);
    }
    for tag in attributes {
        author_attribute(&mut app, tag);
    }
    let tab = app.active_tab;
    let definition = app.tabs[tab].active_block_edit_session().unwrap().br_handle;
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    rows
}

fn row<'a>(rows: &'a [manager::Row], name: &str) -> &'a manager::Row {
    rows.iter().find(|row| row.name == name).unwrap_or_else(|| {
        panic!(
            "no {name} row in {:?}",
            rows.iter().map(|r| &r.name).collect::<Vec<_>>()
        )
    })
}

#[test]
fn polar_parameter_lists_one_row_per_component() {
    let rows = edited(&["BPARAMETER POLAR 0,0 10.136545986,0 5,2"], &[]);
    assert_eq!(
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["Angle1", "Distance1"],
        "one polar parameter is an angle row and a distance row"
    );
    let distance = row(&rows, "Distance1");
    assert_eq!(distance.group, manager::Group::ActionParameters);
    assert_eq!(distance.type_name, "Polar");
    assert_eq!(
        distance.expression, "10.136545986",
        "the expression cell keeps the stored value"
    );
    assert_eq!(
        distance.value, "10.1365",
        "the value cell prints at the drawing's precision"
    );
    assert!(distance.show, "a fresh parameter shows in the manager");
    assert_eq!(distance.order, "", "the order column starts empty");
    let angle = row(&rows, "Angle1");
    assert_eq!(angle.type_name, "Polar");
    assert_eq!(angle.expression, "0");
    assert_eq!(angle.value, "0");
}

#[test]
fn every_parameter_and_attribute_of_the_definition_is_listed() {
    let rows = edited(
        &[
            "BPARAMETER POLAR 0,0 10,0 5,2",
            "BPARAMETER LINEAR 0,0 3,4 6,6",
        ],
        &["TAG1"],
    );
    assert_eq!(
        rows.iter()
            .map(|r| (r.name.as_str(), r.group))
            .collect::<Vec<_>>(),
        [
            ("Angle1", manager::Group::ActionParameters),
            ("Distance1", manager::Group::ActionParameters),
            ("Distance2", manager::Group::ActionParameters),
            ("TAG1", manager::Group::Attributes),
        ]
    );
    let attribute = row(&rows, "TAG1");
    assert_eq!(attribute.type_name, "Variable");
    assert_eq!(attribute.expression, "", "an attribute has no expression");
    assert_eq!(attribute.value, "", "an attribute has no value");
    assert_eq!(row(&rows, "Distance2").type_name, "Linear");
}

#[test]
fn the_filter_and_search_narrow_the_rows_and_the_footer_counts_them() {
    let rows = edited(
        &[
            "BPARAMETER POLAR 0,0 10,0 5,2",
            "BPARAMETER LINEAR 0,0 3,4 6,6",
        ],
        &["TAG1"],
    );
    assert_eq!(
        manager::displayed(&rows, "*", "").len(),
        4,
        "the default filter lists every row"
    );
    assert_eq!(
        manager::displayed(&rows, "*1", "").len(),
        3,
        "Angle1, Distance1 and TAG1 end in 1"
    );
    assert_eq!(manager::displayed(&rows, "Distance*", "").len(), 2);
    assert_eq!(manager::displayed(&rows, "*", "angle").len(), 1);
    assert_eq!(manager::displayed(&rows, "Angle*", "distance").len(), 0);
    assert_eq!(
        manager::footer(rows.len(), manager::displayed(&rows, "Angle*", "").len()),
        "All: 1 of 4 parameters displayed"
    );
}

#[test]
fn bparameters_opens_the_manager_panel_inside_the_block_editor() {
    use crate::ui::dock::PanelId;
    let (mut app, reference) = fixture();
    let _ = app.dispatch_command("BPARAMETERS");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Open the Block Editor to use the Parameters Manager."),
        "outside the editor the command names the panel it refused"
    );
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "outside the editor the command opens nothing"
    );
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETERS");
    assert!(
        app.dock_panel_visible(PanelId::ParametersManager),
        "the command docks the Parameters Manager"
    );
    let _ = app.dispatch_command("BPARAMETERSCLOSE");
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "the paired command closes it again"
    );
}

#[test]
fn a_lookup_parameter_lists_one_row_typed_lookup() {
    let rows = edited(
        &["BPARAMETER POLAR 0,0 10,0 5,2", "BPARAMETER LOOKUP 3,3"],
        &[],
    );
    let lookups: Vec<&manager::Row> = rows
        .iter()
        .filter(|row| row.type_name == "Lookup")
        .collect();
    assert_eq!(
        lookups.len(),
        1,
        "one lookup parameter is one row, alongside {:?}",
        rows.iter()
            .map(|row| (row.name.as_str(), row.type_name))
            .collect::<Vec<_>>()
    );
    let lookup = lookups[0];
    assert_eq!(lookup.group, manager::Group::ActionParameters);
    assert!(
        !lookup.name.is_empty(),
        "the row carries the lookup's own name"
    );
    assert!(
        lookup.expression.is_empty() && lookup.value.is_empty(),
        "a lookup parameter has no number of its own to print"
    );
    assert_eq!(
        rows.iter().filter(|row| row.name == lookup.name).count(),
        1,
        "the lookup is listed once, not again through its own components"
    );
    assert_eq!(
        rows.iter().filter(|row| row.type_name == "Polar").count(),
        2,
        "the polar parameter keeps its angle and distance rows"
    );
}
