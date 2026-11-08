use super::*;
use crate::scene::block_parameters_manager as manager;
use crate::scene::model::object::BlockParameterComponent as Component;
use crate::ui::window::parameters_manager::Cell;

/// A definition the manager is open on, left in the editor so a test can answer
/// a cell the way the panel does.
fn open(commands: &[&str], attributes: &[&str]) -> (OpenCADStudio, usize, Handle) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for command in commands {
        let _ = app.run_command_line(command);
    }
    for tag in attributes {
        author_attribute(&mut app, tag);
    }
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.dispatch_command("BPARAMETERS");
    (app, i, definition)
}

/// Type into one row's cell and press Enter, as the panel's field does.
fn answer(app: &mut OpenCADStudio, name: &str, cell: Cell, typed: &str) {
    let _ = app.update(Message::ParametersManagerEdit(name.to_owned(), cell));
    let _ = app.update(Message::ParametersManagerInput(typed.to_owned()));
    let _ = app.update(Message::ParametersManagerCommit);
}

fn printed(app: &OpenCADStudio, tab: usize, definition: Handle, name: &str) -> manager::Row {
    row(
        &manager::rows(&app.tabs[tab].scene.document, definition),
        name,
    )
    .clone()
}

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

/// The parameter a row belongs to, read back off the definition.
fn endpoints(app: &OpenCADStudio, tab: usize, definition: Handle) -> (glam::DVec3, glam::DVec3) {
    let parameter =
        &crate::scene::block_actions::all_parameters(&app.tabs[tab].scene.document, definition)[0];
    (parameter.base, parameter.end)
}

/// A visibility parameter's row names the parameter and prints no number of its
/// own: the state set is not a quantity the manager writes back, so both numeric
/// cells stay empty and the row is not one it opens a field on. `Show` carries
/// the parameter's own flag, and `Order` and `Description` stay empty because a
/// visibility parameter carries neither.
#[test]
fn a_visibility_row_names_the_parameter_and_prints_no_number() {
    let (app, tab, definition) = open(&["BPARAMETER VISIBILITY 0,0"], &[]);
    let found = manager::rows(&app.tabs[tab].scene.document, definition)
        .into_iter()
        .find(|row| row.type_name == "Visibility")
        .expect("the placed visibility parameter gets a row");
    assert_eq!(found.name, "Visibility1", "the row names the parameter");
    assert!(
        !found.numeric,
        "neither numeric cell is a number the manager writes back"
    );
    assert!(
        found.expression.is_empty() && found.value.is_empty(),
        "the row prints no number of its own: {found:?}"
    );
    assert!(
        found.show,
        "a freshly authored parameter is shown, and the row carries that flag"
    );
    assert!(
        found.order.is_empty() && found.description.is_empty(),
        "and the row leaves the two cells no rule is measured for empty: {found:?}"
    );
    assert!(
        !found.description_writable,
        "a visibility row names no parameter component whose text it could write"
    );
}

#[test]
fn typing_a_distance_in_the_manager_moves_the_parameter() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Value, "20");
    let distance = printed(&app, tab, definition, "Distance1");
    assert_eq!(
        distance.expression, "20",
        "the typed number is what the definition stores"
    );
    assert_eq!(
        distance.value, "20",
        "the value cell prints the same number at the drawing's precision"
    );
    let (base, end) = endpoints(&app, tab, definition);
    assert!(
        (base - glam::DVec3::ZERO).length() < 1e-9,
        "the base stays put"
    );
    assert!(
        (end - glam::DVec3::new(20.0, 0.0, 0.0)).length() < 1e-9,
        "the endpoint moved along the parameter's own direction"
    );
    let _ = app.update(Message::Undo);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "10",
        "one undo step takes the edit back"
    );
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "the field closes with the commit"
    );
}

#[test]
fn the_expression_cell_edits_the_same_number_as_the_value_cell() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Expression, "10.136545986");
    let distance = printed(&app, tab, definition, "Distance1");
    assert_eq!(
        distance.expression, "10.136545986",
        "the expression cell keeps every digit typed"
    );
    assert_eq!(
        distance.value, "10.1365",
        "the value cell renders it at the drawing's precision"
    );
}

#[test]
fn an_angle_is_typed_in_degrees_and_leaves_the_distance_alone() {
    let (mut app, tab, definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    answer(&mut app, "Angle1", Cell::Value, "90");
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "10",
        "an angle edit does not change how far the endpoint sits"
    );
    let (base, end) = endpoints(&app, tab, definition);
    let direction = end - base;
    assert!(
        (direction.y - 10.0).abs() < 1e-9 && direction.x.abs() < 1e-9,
        "the endpoint swings to the typed bearing, got {direction}"
    );
    assert_eq!(
        printed(&app, tab, definition, "Angle1").value,
        "90",
        "the angle reads back at the drawing's angular precision"
    );
}

#[test]
fn the_description_cell_writes_the_text_it_shows() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "a freshly authored parameter has no description"
    );
    let before = endpoints(&app, tab, definition);
    answer(&mut app, "Distance1", Cell::Description, "Left edge");
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "Left edge",
        "the text typed in the cell is what the definition stores"
    );
    assert_eq!(
        endpoints(&app, tab, definition),
        before,
        "writing text moves no geometry"
    );
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Description,
    ));
    assert_eq!(
        app.tabs[tab].parameters_panel.edit_buf, "Left edge",
        "the field opens on the text it replaces, as an in-place edit does"
    );
    let _ = app.update(Message::ParametersManagerCommit);
    let _ = app.update(Message::Undo);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "a commit that changes no text adds no undo step"
    );
}

#[test]
fn every_parameter_kind_writes_the_description_of_the_row_it_prints() {
    use crate::scene::block_authoring::metadata;
    use crate::scene::block_distance_parameter as distance;
    use crate::scene::block_orientation_parameter as orientation;
    use crate::scene::block_point_parameter as point;
    let (mut app, tab, definition) = open(&[], &[]);
    let doc = &mut app.tabs[tab].scene.document;
    let xy = distance::create(
        doc,
        definition,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 5., 0.),
        Vector3::new(3., 3., 0.),
    )
    .unwrap();
    let rotation = orientation::create(
        doc,
        definition,
        orientation::Kind::Rotation,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(0., 10., 0.),
    )
    .unwrap();
    let flip = orientation::create(
        doc,
        definition,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 5., 0.),
    )
    .unwrap();
    let point = point::create(doc, definition, Vector3::ZERO, Vector3::new(5., 5., 0.)).unwrap();
    let ends: Vec<_> = crate::scene::block_actions::all_parameters(doc, definition)
        .into_iter()
        .map(|parameter| (parameter.handle, parameter.end))
        .collect();
    for (parameter, component, text) in [
        (xy, Component::X, "Pin"),
        (xy, Component::Y, "Height"),
        (rotation, Component::Angle, "Turn"),
        (flip, Component::Flip, "Mirror"),
        (point, Component::X, "Anchor"),
    ] {
        assert!(
            manager::write_description(doc, definition, parameter, component, text).unwrap(),
            "the description of the {component:?} row is written"
        );
    }
    let text = |parameter, field| {
        metadata::read(doc, definition, parameter)
            .unwrap()
            .text
            .into_iter()
            .find(|text| text.field == field)
            .map(|text| text.value)
            .unwrap_or_default()
    };
    assert_eq!(
        text(xy, metadata::Field::Description),
        "Pin",
        "an XY parameter's X row writes the horizontal label's own text"
    );
    assert_eq!(
        text(xy, metadata::Field::SecondaryDescription),
        "Height",
        "and its Y row writes the vertical label's own text"
    );
    assert_eq!(text(rotation, metadata::Field::Description), "Turn");
    assert_eq!(text(flip, metadata::Field::Description), "Mirror");
    assert_eq!(
        text(point, metadata::Field::Description),
        "Anchor",
        "a point parameter's rows share the position text"
    );
    assert_eq!(
        crate::scene::block_actions::all_parameters(doc, definition)
            .into_iter()
            .map(|parameter| (parameter.handle, parameter.end))
            .collect::<Vec<_>>(),
        ends,
        "and no parameter's endpoint moves"
    );
}

#[test]
fn an_emptied_description_cell_clears_the_text() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Description, "Left edge");
    answer(&mut app, "Distance1", Cell::Description, "");
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "an emptied cell writes the empty text"
    );
    let _ = app.update(Message::Undo);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "Left edge",
        "and one undo step brings the text back"
    );
}

#[test]
fn an_emptied_description_commits_when_the_click_moves_away() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Description, "Left edge");
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Description,
    ));
    let _ = app.update(Message::ParametersManagerInput(String::new()));
    let _ = app.update(Message::ParametersManagerFocus(None));
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "the field closes with the click"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "an emptied description is a text to write, so moving the click away commits it"
    );
}

#[test]
fn an_untouched_description_waits_for_its_own_field() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Description, "Left edge");
    let field = crate::ui::window::parameters_manager::edit_input_id();
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Description,
    ));
    let _ = app.update(Message::ParametersManagerFocus(None));
    assert!(
        app.tabs[tab].parameters_panel.editing.is_some(),
        "a field that has not been typed into stays open with the text it shows"
    );
    let _ = app.update(Message::ParametersManagerInput("Right edge".to_owned()));
    let _ = app.update(Message::ParametersManagerFocus(Some(field)));
    assert!(
        app.tabs[tab].parameters_panel.editing.is_some(),
        "focus moving onto the field itself commits nothing"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "Left edge",
        "and writes none of what was typed so far"
    );
    let _ = app.update(Message::ParametersManagerCommit);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "Right edge",
        "Enter still writes the typed text"
    );
}

#[test]
fn a_description_undone_under_the_open_field_is_not_written_back() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    answer(&mut app, "Distance1", Cell::Description, "Left edge");
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Description,
    ));
    let _ = app.update(Message::Undo);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "the undo takes the text back under the field that shows it"
    );
    let _ = app.update(Message::ParametersManagerFocus(None));
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "leaving the field alone writes nothing back"
    );
}

#[test]
fn a_polar_parameters_two_rows_keep_their_own_descriptions() {
    let (mut app, tab, definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    answer(&mut app, "Angle1", Cell::Description, "Swing");
    assert_eq!(
        printed(&app, tab, definition, "Angle1").description,
        "Swing"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "",
        "the angle text does not land on the distance row"
    );
    answer(&mut app, "Distance1", Cell::Description, "Reach");
    assert_eq!(
        printed(&app, tab, definition, "Angle1").description,
        "Swing"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance1").description,
        "Reach"
    );
}

#[test]
fn an_attribute_row_opens_no_description_field() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &["TAG1"]);
    let before = manager::rows(&app.tabs[tab].scene.document, definition);
    assert!(
        !row(&before, "TAG1").description_writable,
        "an attribute row names no parameter text"
    );
    assert!(
        row(&before, "Distance1").description_writable,
        "a parameter row names text the manager writes"
    );
    answer(&mut app, "TAG1", Cell::Description, "note");
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "an attribute's row names no parameter text the manager writes"
    );
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "and nothing is written"
    );
}

#[test]
fn a_cell_that_is_not_a_number_is_refused_and_writes_nothing() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &["TAG1"]);
    let before = manager::rows(&app.tabs[tab].scene.document, definition);
    answer(&mut app, "Distance1", Cell::Value, "twelve");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Specify a finite numeric parameter value."),
        "the refusal says what the cell wanted"
    );
    assert_eq!(
        app.tabs[tab].parameters_panel.edit_buf.as_str(),
        "twelve",
        "the refused cell stays open with what was typed, so only the text needs fixing"
    );
    answer(&mut app, "Distance1", Cell::Value, "-5");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Specify a positive parameter length."),
        "a length cannot be typed as a negative"
    );
    answer(&mut app, "TAG1", Cell::Value, "5");
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "nothing a refusal touches reaches the definition"
    );
}

#[test]
fn the_show_column_toggles_the_parameter_it_names() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    assert!(printed(&app, tab, definition, "Distance1").show);
    let _ = app.update(Message::ParametersManagerShow("Distance1".to_owned()));
    assert!(
        !printed(&app, tab, definition, "Distance1").show,
        "the cell is the parameter's own show-in-properties flag"
    );
    let _ = app.update(Message::ParametersManagerShow("Distance1".to_owned()));
    assert!(
        printed(&app, tab, definition, "Distance1").show,
        "and it goes back"
    );
    let _ = app.update(Message::ParametersManagerShow("TAG1".to_owned()));
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("This row has no visibility flag to change."),
        "an attribute row is not a parameter"
    );
}

#[test]
fn a_manager_edit_runs_the_actions_attached_to_the_parameter() {
    use crate::scene::block_actions::{self as actions, instance};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let tab = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 6,6");
    let definition = app.tabs[tab].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[tab].scene.document, definition)[0].handle;
    let target = members(&app)[0];
    actions::create(
        &mut app.tabs[tab].scene.document,
        definition,
        actions::Kind::Move,
        parameter,
        &[target],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    // A reference gets a record of its own once a value is written into it; from
    // then on the editor rebuilds that copy whenever the definition changes.
    let staged = instance::evaluate(
        &app.tabs[tab].scene.document,
        reference,
        parameter,
        glam::DVec3::new(20., 0., 0.),
    )
    .unwrap()
    .unwrap();
    app.apply_action_document(tab, staged);
    begin(&mut app, reference);
    let copy = |app: &OpenCADStudio| {
        let doc = &app.tabs[tab].scene.document;
        let document::EntityType::Insert(native) = doc.get_entity(reference).unwrap() else {
            panic!("the reference is no longer an insert");
        };
        doc.block_records
            .get(&native.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|handle| match doc.get_entity(*handle) {
                Some(document::EntityType::Line(line)) => Some(line.start),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the reference has no line to move"))
    };
    let before = copy(&app);
    let authored = match app.tabs[tab].scene.document.get_entity(target) {
        Some(document::EntityType::Line(line)) => line.start,
        other => panic!("the definition member is not a plain line: {other:?}"),
    };
    assert!(
        (before.x - (authored.x + 10.)).abs() < 1e-9 && (before.y - authored.y).abs() < 1e-9,
        "the reference holds the 20 written into it, so its copy sits 10 past the \
         definition's authored member: {before:?} vs {authored:?}"
    );
    answer(&mut app, "Distance1", Cell::Value, "40");
    let after = copy(&app);
    // The reference keeps the value written into it, and the definition's authored
    // length changed by 30, so the rebuilt copy moves 30 along the parameter's
    // axis. Which way it goes when the reference and the definition disagree is not
    // yet measured, so this pins the amount and the axis rather than the direction.
    let shift = after - before;
    assert!(
        (shift.x.abs() - 30.).abs() < 1e-9 && shift.y.abs() < 1e-9 && shift.z.abs() < 1e-9,
        "the manager edit rebuilt the reference along the parameter's axis, got {shift:?}"
    );
    match app.tabs[tab].scene.document.get_entity(target) {
        Some(document::EntityType::Line(line)) => assert_eq!(
            line.start, authored,
            "and the definition's own member stays where it was authored"
        ),
        other => panic!("the definition member is not a plain line: {other:?}"),
    }
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn an_edit_left_open_by_a_closed_session_writes_nothing() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let before = manager::rows(&app.tabs[tab].scene.document, definition);
    // A tab can hold more than one Block Editor session, and the panel is rebuilt
    // from whichever one is active. A field left open from another session must
    // not write onto a same-named parameter of this one.
    app.tabs[tab].parameters_panel.editing = Some(crate::ui::window::parameters_manager::Edit {
        definition: document::Handle::NULL,
        parameter: crate::scene::block_actions::all_parameters(
            &app.tabs[tab].scene.document,
            definition,
        )[0]
        .handle,
        component: Component::Distance,
        name: "Distance1".to_owned(),
        cell: Cell::Value,
    });
    app.tabs[tab].parameters_panel.edit_buf = "40".to_owned();
    let _ = app.update(Message::ParametersManagerCommit);
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("The Block Editor session is no longer active."),
        "the refusal says the edit's session is gone"
    );
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "and the open session's parameter keeps its value"
    );
}

#[test]
fn a_cell_writes_to_the_parameter_it_was_opened_on() {
    use crate::scene::block_actions as actions;
    let (mut app, tab, definition) = open(
        &[
            "BPARAMETER LINEAR 0,0 10,0 6,6",
            "BPARAMETER LINEAR 0,0 10,0 7,7",
        ],
        &[],
    );
    let parameters = actions::all_parameters(&app.tabs[tab].scene.document, definition);
    let (first, second) = (parameters[0].handle, parameters[1].handle);
    // The cell is opened on Distance1, then aimed at the other parameter's handle
    // the way an edit opened before that row moved would be: the number follows
    // the parameter, not the name it prints.
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    app.tabs[tab]
        .parameters_panel
        .editing
        .as_mut()
        .unwrap()
        .parameter = second;
    let _ = app.update(Message::ParametersManagerInput("40".to_owned()));
    let _ = app.update(Message::ParametersManagerCommit);
    assert_eq!(
        printed(&app, tab, definition, "Distance2").expression,
        "40",
        "the number landed on the parameter the cell was opened on"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "10",
        "and the row the field is printed in is not the one that took it"
    );
    assert_eq!(first, parameters[0].handle);
    assert_ne!(
        first, second,
        "the definition holds two distinct parameters"
    );
}

#[test]
fn a_number_typed_before_the_next_cell_opens_is_not_lost() {
    let (mut app, tab, definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    // Typing into one row and then clicking another row's cell, without pressing
    // Enter, is the ordinary way to fill two cells in a row. Whichever order the
    // click's messages arrive in, the first number has to reach its parameter.
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("20".to_owned()));
    let _ = app.update(Message::ParametersManagerEdit(
        "Angle1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("90".to_owned()));
    let _ = app.update(Message::ParametersManagerCommit);
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "20",
        "the distance typed in the first cell was written when its cell closed"
    );
    assert_eq!(
        printed(&app, tab, definition, "Angle1").value,
        "90",
        "and the second cell took its own number"
    );
}

#[test]
fn a_row_that_prints_no_writable_number_does_not_open_a_field() {
    let (mut app, tab, definition) = open(&["BPARAMETER POINT 0,0 5,5"], &[]);
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    let point = rows
        .iter()
        .find(|r| r.type_name == "Point")
        .unwrap_or_else(|| {
            panic!(
                "no point row in {:?}",
                rows.iter()
                    .map(|r| (r.name.as_str(), r.type_name))
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !point.numeric,
        "a point's axes print a position, not a number the manager writes back"
    );
    let before = rows.clone();
    answer(&mut app, &point.name, Cell::Value, "40");
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "the cell never opens"
    );
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "and nothing is written"
    );
}

#[test]
fn a_point_row_prints_the_label_and_description_taken_at_placement() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let tab = app.active_tab;
    let _ = app.dispatch_command("BPARAMETER POINT");
    for typed in ["N", "Pt9", "L", "Pin9", "D", "audit note", "P", "No"] {
        feed(&mut app, typed);
    }
    feed(&mut app, "0,0");
    feed(&mut app, "5,5");
    let definition = app.tabs[tab].active_block_edit_session().unwrap().br_handle;
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    for name in ["Pin9 X", "Pin9 Y"] {
        let point = row(&rows, name);
        assert_eq!(point.type_name, "Point");
        assert_eq!(point.description, "audit note");
        assert!(
            !point.show,
            "a palette answer of No keeps the row out of the property list"
        );
    }
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
fn an_angle_the_drawing_rounds_to_a_full_turn_prints_zero() {
    let rows = edited(&["BPARAMETER POLAR 0,0 10,-0.05 5,2"], &[]);
    let angle = row(&rows, "Angle1");
    assert!(
        angle.expression.starts_with("359."),
        "the angle is a hair short of a full turn: {}",
        angle.expression
    );
    assert_eq!(
        angle.value, "0",
        "the value cell rounds it to a full turn at the drawing's angle precision \
         and prints that as zero"
    );
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
        manager::displayed(&rows, manager::FilterMode::All, "*", "").len(),
        4,
        "the default filter lists every row"
    );
    assert_eq!(
        manager::displayed(&rows, manager::FilterMode::All, "*1", "").len(),
        3,
        "Angle1, Distance1 and TAG1 end in 1"
    );
    assert_eq!(manager::displayed(&rows, manager::FilterMode::All, "Distance*", "").len(), 2);
    assert_eq!(manager::displayed(&rows, manager::FilterMode::All, "*", "angle").len(), 1);
    assert_eq!(manager::displayed(&rows, manager::FilterMode::All, "Angle*", "distance").len(), 0);
    assert_eq!(
        manager::footer(manager::FilterMode::All, rows.len(), manager::displayed(&rows, manager::FilterMode::All, "Angle*", "").len()),
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
fn a_renamed_lookup_row_shows_the_parameter_name() {
    let (mut app, tab, definition) = open(&["BPARAMETER LOOKUP 3,3"], &[]);
    let lookup = crate::scene::block_lookup::parameters(&app.tabs[tab].scene.document, definition)
        .into_iter()
        .next()
        .map(|(handle, _)| handle)
        .expect("one lookup parameter");
    crate::scene::block_authoring::metadata::set_parameter_name(
        &mut app.tabs[tab].scene.document,
        definition,
        lookup,
        "Grip9",
    )
    .expect("the lookup takes a name");
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    let row = rows
        .iter()
        .find(|row| row.type_name == "Lookup")
        .expect("the lookup still lists a row");
    assert_eq!(
        row.name, "Grip9",
        "the row shows the parameter's own name, not its lookup label"
    );
}

/// The rename is a property of the definition, so it has to survive the
/// drawing being written and read back; the row must not fall back to the
/// lookup's own label or to the name the parameter was placed with.
#[test]
fn a_renamed_lookup_row_keeps_the_name_across_a_save() {
    let (mut app, tab, definition) = open(&["BPARAMETER LOOKUP 3,3"], &[]);
    let lookup = crate::scene::block_lookup::parameters(&app.tabs[tab].scene.document, definition)
        .into_iter()
        .next()
        .map(|(handle, _)| handle)
        .expect("one lookup parameter");
    crate::scene::block_authoring::metadata::set_parameter_name(
        &mut app.tabs[tab].scene.document,
        definition,
        lookup,
        "Grip9",
    )
    .expect("the lookup takes a name");
    let document = app.tabs[tab].scene.document_for_save();
    let version = document.version;
    let prepared = crate::io::prepare_document_for_save(document, version);
    let bytes = document::DxfWriter::new(&prepared).write_to_vec().unwrap();
    let reopened = document::DxfReader::from_reader(std::io::Cursor::new(bytes))
        .unwrap()
        .read()
        .unwrap();
    let row = manager::rows(&reopened, definition)
        .into_iter()
        .find(|row| row.type_name == "Lookup")
        .expect("the lookup still lists a row after the save");
    assert_eq!(
        row.name, "Grip9",
        "the reopened row shows the parameter's own name"
    );
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[tab].scene.document).unwrap();
    let reopened = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    let row = manager::rows(&reopened, definition)
        .into_iter()
        .find(|row| row.type_name == "Lookup")
        .expect("the lookup still lists a row after the DWG round trip");
    assert_eq!(
        row.name, "Grip9",
        "the DWG round trip keeps the parameter's own name too"
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
    assert!(
        !lookup.description_writable,
        "and no parameter component whose text its description cell could write"
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

#[test]
fn moving_the_click_off_the_field_keeps_what_was_typed() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    // Clicking another cell is the ordinary way to leave a field, and it is a
    // different message from clicking the cell itself: the number has to reach
    // the parameter either way.
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("20".to_owned()));
    let _ = app.update(Message::ParametersManagerFocus(None));
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "20",
        "the blur wrote the number, as Enter would"
    );
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "and the field closed with it"
    );
}

#[test]
fn a_field_that_keeps_the_click_writes_nothing() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("20".to_owned()));
    let _ = app.update(Message::ParametersManagerFocus(Some(
        crate::ui::window::parameters_manager::edit_input_id(),
    )));
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "10",
        "the field still holds the click, so nothing has been applied"
    );
    assert_eq!(
        app.tabs[tab].parameters_panel.edit_buf, "20",
        "and the number is still being typed"
    );
}

#[test]
fn a_refused_number_is_not_left_open_once_the_field_is_gone() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let before = manager::rows(&app.tabs[tab].scene.document, definition);
    // Enter leaves a bad number in the field to be corrected. A click away takes
    // the field with it, so there is nothing left to correct.
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("twelve".to_owned()));
    let _ = app.update(Message::ParametersManagerFocus(None));
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Specify a finite numeric parameter value."),
        "the blur still says what the cell wanted"
    );
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "but no field stays open on a row that is not being edited"
    );
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "and the refused number never reaches the definition"
    );
}

#[test]
fn a_cell_left_open_when_the_session_ends_writes_nothing() {
    let (mut app, tab, _) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let definition = app.tabs[tab].active_block_edit_session().unwrap().br_handle;
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("40".to_owned()));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "closing the editor takes the panel's field with the session"
    );
    let _ = app.update(Message::ParametersManagerCommit);
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    assert!(
        rows.is_empty(),
        "the abandoned commit wrote nothing back: the discarded session's parameters \
         are gone with it, got {:?}",
        rows.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    // A field that outlives its session by any route is refused the same way.
    app.tabs[tab].parameters_panel.editing = Some(crate::ui::window::parameters_manager::Edit {
        definition,
        parameter: document::Handle::new(1),
        component: Component::Distance,
        name: "Distance1".to_owned(),
        cell: Cell::Value,
    });
    app.tabs[tab].parameters_panel.edit_buf = "40".to_owned();
    let _ = app.update(Message::ParametersManagerCommit);
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("The Block Editor session is no longer active."),
        "with no session left to edit, the commit says so instead of writing"
    );
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "and it takes the field with the refusal"
    );
}

#[test]
fn an_attribute_tagged_like_a_parameter_is_listed_once() {
    // Both rows answer to one name, and a click carries only the name, so the
    // parameter row wins: the panel lists it as the number it is, and the
    // attribute keeps being editable where attributes are edited.
    let rows = edited(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &["Distance1"]);
    assert_eq!(
        rows.iter()
            .map(|r| (r.name.as_str(), r.group))
            .collect::<Vec<_>>(),
        [("Distance1", manager::Group::ActionParameters)],
        "the colliding attribute row does not double the parameter's name"
    );
    assert!(
        rows.iter().all(|row| row.numeric),
        "and the surviving row is the one the manager writes"
    );
}

#[test]
fn the_parameters_command_reaches_the_manager_in_the_editor_and_the_modal_outside_it() {
    use crate::ui::dock::PanelId;
    let (mut app, reference) = fixture();
    // With no definition open there are no definition parameters to list, so the
    // command keeps its global route to the formula modal. That modal is an effect
    // the headless test cannot observe, so what is pinned here is the part the
    // manager could get wrong: the command asks for no pane outside a definition.
    let _ = app.dispatch_command("PARAMETERS");
    assert!(
        !app.show_parameters_manager,
        "outside the editor the command asks for no manager pane"
    );
    let _ = app.dispatch_command("PARAMETERSCLOSE");
    assert!(
        !app.show_parameters_manager,
        "and the paired command asks for nothing either"
    );

    begin(&mut app, reference);
    let _ = app.dispatch_command("PARAMETERS");
    assert!(
        app.dock_panel_visible(PanelId::ParametersManager),
        "inside a definition the same command opens the manager"
    );
    assert!(
        app.active_modal.is_none(),
        "without raising the global modal as well"
    );
    let click = || Message::RibbonToolClick {
        tool_id: "PARAMETERS".into(),
        event: crate::modules::ModuleEvent::Command("PARAMETERS".into()),
    };
    let _ = app.update(click());
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "the same tool clicked again closes the pane"
    );
    assert!(
        !app.ribbon.show_parameters_manager,
        "and the light goes out on the frame that renders it"
    );
    let _ = app.update(click());
    assert!(
        app.ribbon.show_parameters_manager,
        "and comes back with the pane"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_manager_stays_open_while_the_authoring_palettes_switch_tabs() {
    use crate::ui::dock::PanelId;
    use crate::ui::window::authoring_palettes::Tab;
    let (mut app, _tab, _definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    assert!(
        app.dock_panel_visible(PanelId::ParametersManager),
        "the manager came up with the definition"
    );
    for tab in [
        Tab::Actions,
        Tab::ParameterSets,
        Tab::Constraints,
        Tab::Parameters,
    ] {
        let _ = app.update(Message::AuthoringPaletteTabSelected(tab));
        assert!(
            app.dock_panel_visible(PanelId::ParametersManager),
            "switching the authoring palette to {:?} leaves the manager open",
            tab
        );
    }
    let _ = app.dispatch_command("BAUTHORPALETTECLOSE");
    assert!(
        app.dock_panel_visible(PanelId::ParametersManager),
        "and closing the palettes does not take it either"
    );
}

#[test]
fn the_manager_stays_wanted_until_the_user_closes_it() {
    use crate::ui::dock::PanelId;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("PARAMETERS");
    assert!(app.dock_panel_visible(PanelId::ParametersManager));
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "outside a definition the manager has nothing to list"
    );
    begin(&mut app, reference);
    assert!(
        app.dock_panel_visible(PanelId::ParametersManager),
        "a pane the user never closed is still wanted by the next definition opened"
    );
    let _ = app.dispatch_command("BPARAMETERSCLOSE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    begin(&mut app, reference);
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "and once the user closes it, it does not come back on its own"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_ribbon_light_goes_out_with_the_definition() {
    let (mut app, _tab, _definition) = open(&[], &[]);
    // The command line changed the state without a frame, so this is the frame
    // that renders it.
    let _ = app.update(Message::Tick(std::time::Instant::now()));
    assert!(
        app.ribbon.show_parameters_manager,
        "the tool is lit while its pane is open"
    );
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(
        app.show_parameters_manager,
        "the pane is still the one the user left open"
    );
    // The light is derived from what is rendered, and rendering happens on the
    // next frame.
    let _ = app.update(Message::Tick(std::time::Instant::now()));
    assert!(
        !app.ribbon.show_parameters_manager,
        "but with no definition open the pane prints nothing, so its tool is dark"
    );
}

#[test]
fn the_parameters_toggle_keeps_the_running_drawing_command() {
    let (mut app, _tab, _definition) = open(&[], &[]);
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    assert!(app.tabs[i].active_cmd.is_some(), "a line is being drawn");
    let _ = app.dispatch_command("PARAMETERSCLOSE");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "closing the manager from the command line leaves the line running"
    );
    let _ = app.dispatch_command("PARAMETERS");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "and opening it again does not abandon it either"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_parameters_toggle_changes_nothing_but_the_pane() {
    use crate::ui::dock::{DockMsg, PanelId};
    let (mut app, tab, _definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let before = app.tabs[tab].scene.document.clone();
    let history = app.tabs[tab].history.undo_stack.len();
    let dirty = app.block_edit_dirty(tab);
    assert!(dirty, "the authored parameter is already a change to save");
    // A cell the user opened but never typed into rides along with the pane: it
    // has no number to apply, so the toggles must not change the definition.
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let click = || Message::RibbonToolClick {
        tool_id: "PARAMETERS".into(),
        event: crate::modules::ModuleEvent::Command("PARAMETERS".into()),
    };
    let _ = app.update(click());
    assert!(!app.dock_panel_visible(PanelId::ParametersManager));
    assert!(
        app.tabs[tab].parameters_panel.editing.is_none(),
        "the open cell goes with the pane"
    );
    let _ = app.update(click());
    assert!(app.dock_panel_visible(PanelId::ParametersManager));
    let _ = app.on_dock(DockMsg::Close(PanelId::ParametersManager));
    assert!(!app.show_parameters_manager);
    assert!(!app.dock_panel_visible(PanelId::ParametersManager));
    assert_eq!(app.tabs[tab].scene.document, before);
    assert_eq!(app.tabs[tab].history.undo_stack.len(), history);
    assert_eq!(app.block_edit_dirty(tab), dirty);
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn a_cell_the_pane_does_not_own_closes_without_a_refusal() {
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let before = manager::rows(&app.tabs[tab].scene.document, definition);
    // The panel holds the definition a cell was opened on, so a cell left from a
    // definition this pane is not showing is not its to apply. Closing prints no
    // refusal for it and writes nothing. (Which real route leaves such a cell
    // behind is not measured; the guard is the pane's own promise.)
    app.tabs[tab].parameters_panel.editing = Some(crate::ui::window::parameters_manager::Edit {
        definition: document::Handle::NULL,
        parameter: crate::scene::block_actions::all_parameters(
            &app.tabs[tab].scene.document,
            definition,
        )[0]
        .handle,
        component: Component::Distance,
        name: "Distance1".to_owned(),
        cell: Cell::Value,
    });
    app.tabs[tab].parameters_panel.edit_buf = "40".to_owned();
    let _ = app.dispatch_command("BPARAMETERSCLOSE");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        None,
        "closing the pane prints no refusal for a cell it never held"
    );
    assert!(app.tabs[tab].parameters_panel.editing.is_none());
    assert_eq!(
        manager::rows(&app.tabs[tab].scene.document, definition),
        before,
        "and the open definition keeps its parameter"
    );
}

#[test]
fn the_funnel_menu_opens_and_a_choice_sets_the_filter_and_closes_it() {
    let (mut app, tab, _definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    assert!(
        !app.tabs[tab].parameters_panel.filter_menu,
        "the menu starts closed"
    );
    let _ = app.update(Message::ParametersManagerFilterMenu);
    assert!(
        app.tabs[tab].parameters_panel.filter_menu,
        "the funnel opens it"
    );
    let _ = app.update(Message::ParametersManagerFilterMode(
        manager::FilterMode::UsedInExpressions,
    ));
    assert_eq!(
        app.tabs[tab].parameters_panel.mode,
        manager::FilterMode::UsedInExpressions
    );
    assert!(
        !app.tabs[tab].parameters_panel.filter_menu,
        "choosing an entry closes the menu"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_header_menu_opens_and_an_entry_toggles_its_column_and_closes_the_menu() {
    use crate::ui::window::parameters_manager::Column;
    let (mut app, tab, _definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    assert!(
        !app.tabs[tab].parameters_panel.header_menu,
        "the menu starts closed"
    );
    let _ = app.update(Message::ParametersManagerHeaderMenu);
    assert!(
        app.tabs[tab].parameters_panel.header_menu,
        "a right-click on the header opens it"
    );
    let _ = app.update(Message::ParametersManagerColumn(Column::Type));
    assert!(
        !app.tabs[tab].parameters_panel.columns[Column::Type.index()],
        "choosing Type hides that column"
    );
    assert!(
        !app.tabs[tab].parameters_panel.header_menu,
        "and closes the menu"
    );
    let _ = app.update(Message::ParametersManagerColumn(Column::Type));
    assert!(
        app.tabs[tab].parameters_panel.columns[Column::Type.index()],
        "choosing it again brings the column back"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_two_fields_narrow_the_list_and_the_count_follows() {
    let (mut app, tab, definition) = open(
        &[
            "BPARAMETER POLAR 0,0 10,0 5,2",
            "BPARAMETER LINEAR 0,0 3,4 6,6",
        ],
        &["TAG1"],
    );
    let count = |app: &OpenCADStudio| {
        let panel = &app.tabs[tab].parameters_panel;
        let rows = manager::rows(&app.tabs[tab].scene.document, definition);
        let shown = manager::displayed(&rows, manager::FilterMode::All, &panel.filter, &panel.search);
        manager::footer(manager::FilterMode::All, rows.len(), shown.len())
    };
    assert_eq!(
        app.tabs[tab].parameters_panel.filter, "*",
        "the filter field carries the recorded default, not an empty one"
    );
    assert_eq!(
        count(&app),
        "All: 4 of 4 parameters displayed",
        "and it lists every row"
    );
    let _ = app.update(Message::ParametersManagerFilter("*1".to_owned()));
    assert_eq!(
        count(&app),
        "All: 3 of 4 parameters displayed",
        "Angle1, Distance1 and TAG1 end in 1"
    );
    let _ = app.update(Message::ParametersManagerSearch("angle".to_owned()));
    assert_eq!(
        count(&app),
        "All: 1 of 4 parameters displayed",
        "the search narrows what the filter left"
    );
    let _ = app.update(Message::ParametersManagerFilter(String::new()));
    assert_eq!(
        count(&app),
        "All: 1 of 4 parameters displayed",
        "an empty filter lists everything, so only the search is narrowing"
    );
    let _ = app.update(Message::ParametersManagerSearch(String::new()));
    assert_eq!(
        count(&app),
        "All: 4 of 4 parameters displayed",
        "and clearing both brings the list back whole"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn a_row_named_through_the_filter_is_still_the_row_the_cell_writes() {
    let (mut app, tab, definition) = open(
        &[
            "BPARAMETER LINEAR 0,0 10,0 6,6",
            "BPARAMETER LINEAR 0,0 3,4 7,7",
        ],
        &[],
    );
    let _ = app.update(Message::ParametersManagerFilter("Distance1".to_owned()));
    answer(&mut app, "Distance1", Cell::Value, "20");
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "20",
        "the number reaches the row the filter is showing"
    );
    assert_eq!(
        printed(&app, tab, definition, "Distance2").expression,
        "5",
        "and the row the filter hides keeps its own value"
    );
    assert_eq!(
        app.tabs[tab].parameters_panel.filter, "Distance1",
        "applying a number leaves the list the operator narrowed"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn closing_the_pane_applies_the_number_typed_in_its_open_cell() {
    use crate::ui::dock::PanelId;
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("20".to_owned()));
    let _ = app.dispatch_command("BPARAMETERSCLOSE");
    assert!(
        !app.dock_panel_visible(PanelId::ParametersManager),
        "the command closed the pane"
    );
    let _ = app.dispatch_command("BPARAMETERS");
    assert_eq!(
        printed(&app, tab, definition, "Distance1").expression,
        "20",
        "and the number typed in its open cell was applied on the way out"
    );
}

#[test]
fn closing_the_manager_in_clean_screen_applies_its_open_cell() {
    use crate::ui::dock::PanelId;
    let (mut app, tab, definition) = open(&["BPARAMETER LINEAR 0,0 10,0 6,6"], &[]);
    let _ = app.update(Message::ToggleCleanScreen);
    let _ = app.dispatch_command("PARAMETERS");
    assert!(app.dock_panel_visible(PanelId::ParametersManager));
    let history = app.tabs[tab].history.undo_stack.len();
    let _ = app.update(Message::ParametersManagerEdit(
        "Distance1".to_owned(),
        Cell::Value,
    ));
    let _ = app.update(Message::ParametersManagerInput("20".to_owned()));
    let _ = app.dispatch_command("PARAMETERSCLOSE");
    assert!(app.clean_screen);
    assert!(!app.dock_panel_visible(PanelId::ParametersManager));
    assert!(app.tabs[tab].parameters_panel.editing.is_none());
    assert!(app.tabs[tab].parameters_panel.edit_buf.is_empty());
    assert_eq!(printed(&app, tab, definition, "Distance1").expression, "20");
    assert_eq!(endpoints(&app, tab, definition).1, glam::DVec3::X * 20.);
    assert_eq!(app.tabs[tab].history.undo_stack.len(), history + 1);
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn manager_edit_refreshes_ordered_actions_and_transported_parameters_with_undo() {
    use crate::scene::block_actions::{self as actions, instance, Kind, Value};
    use crate::scene::{block_distance_parameter as distance, block_point_parameter as point};
    use glam::DVec3;

    for (scale_first, child_linear) in [false, true]
        .into_iter()
        .flat_map(|order| [false, true].map(|linear| (order, linear)))
    {
        let (mut app, insert) = fixture();
        let tab = app.active_tab;
        let other = app.tabs[tab]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "EDIT_ME",
                Vector3::UNIT_X * 50.,
            )))
            .unwrap();
        begin(&mut app, insert);
        let definition = app.tabs[tab].active_block_edit_session().unwrap().br_handle;
        let member = members(&app)[0];
        let doc = &mut app.tabs[tab].scene.document;
        let parent = distance::create(
            doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::UNIT_X * 10.,
            Vector3::UNIT_Y,
        )
        .unwrap();
        let child = if child_linear {
            distance::create(
                doc,
                definition,
                distance::Kind::Linear,
                Vector3::ZERO,
                Vector3::UNIT_X * 10.,
                Vector3::UNIT_Y,
            )
            .unwrap()
        } else {
            point::create(doc, definition, Vector3::ZERO, Vector3::UNIT_Y).unwrap()
        };
        for handle in [parent, child] {
            let document::objects::ObjectType::DynamicBlock(object) =
                doc.objects.get_mut(&handle).unwrap()
            else {
                panic!()
            };
            match &mut object.data {
                document::objects::DynamicBlockData::LinearParameter(p) => {
                    p.parameter.parameter.chain_actions = true
                }
                document::objects::DynamicBlockData::PointParameter(p) => {
                    p.parameter.parameter.chain_actions = true
                }
                _ => panic!(),
            }
        }
        let moved =
            actions::create(doc, definition, Kind::Move, parent, &[member], DVec3::ZERO).unwrap();
        let scaled =
            actions::create(doc, definition, Kind::Scale, parent, &[member], DVec3::ZERO).unwrap();
        actions::create(doc, definition, Kind::Move, child, &[member], DVec3::ZERO).unwrap();
        let document::objects::ObjectType::DynamicBlock(object) = doc.objects.get_mut(&moved).unwrap()
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::MoveAction(action) = &mut object.data else {
            panic!()
        };
        action.action.dependencies = vec![member, child];
        let (head, tail) = if scale_first {
            (scaled, moved)
        } else {
            (moved, scaled)
        };
        assert!(actions::chain::set(doc, definition, head, Some(tail)).unwrap());
        let point = actions::all_parameters(doc, definition)
            .into_iter()
            .find(|p| p.handle == child)
            .unwrap();
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        let changed = instance::evaluate_value(
            &app.tabs[tab].scene.document,
            insert,
            parent,
            Value::Point(DVec3::new(20., 0., 0.)),
        )
        .unwrap()
        .unwrap();
        let changed = instance::evaluate_value(
            &changed,
            insert,
            child,
            Value::Point(point.end + DVec3::new(15., 0., 0.)),
        )
        .unwrap()
        .unwrap();
        app.apply_action_document(tab, changed);
        begin(&mut app, insert);
        let line = |doc: &document::Document, handle: Handle| {
            let EntityType::Insert(reference) = doc.get_entity(handle).unwrap() else {
                panic!()
            };
            doc.block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .find_map(|h| {
                    if let Some(EntityType::Line(line)) = doc.get_entity(*h) {
                        Some((actions::v(line.start), actions::v(line.end)))
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        let before = app.tabs[tab].scene.document.clone();
        let original = line(&before, other);
        let depth = app.tabs[tab].history.undo_stack.len();
        answer(&mut app, "Distance1", Cell::Value, "15");
        assert_eq!(app.tabs[tab].history.undo_stack.len(), depth + 1);
        let after = &app.tabs[tab].scene.document;
        let frame = instance::frame(after, insert, &point).unwrap();
        assert!((frame.end - (point.end + DVec3::X * 10.)).length() < 1e-9);
        assert!(
            (frame.base
                - (point.base
                    + if child_linear {
                        DVec3::X * 5.
                    } else {
                        DVec3::ZERO
                    }))
            .length()
                < 1e-9
        );
        let start = if scale_first { 15. } else { 10. + 20. / 3. };
        let expected = (DVec3::X * start, DVec3::X * (start + 4. / 3.));
        let actual = line(after, insert);
        assert!(
            (actual.0 - expected.0).length() < 1e-9 && (actual.1 - expected.1).length() < 1e-9,
            "scale_first={scale_first}: expected {expected:?}, got {actual:?}"
        );
        assert_eq!(line(after, other), original);
        app.undo_steps(1);
        assert_eq!(
            line(&app.tabs[tab].scene.document, insert),
            line(&before, insert)
        );
        assert_eq!(
            instance::frame(&app.tabs[tab].scene.document, insert, &point).unwrap(),
            instance::frame(&before, insert, &point).unwrap()
        );
        assert_eq!(line(&app.tabs[tab].scene.document, other), original);
        app.redo_steps(1);
        let restored = line(&app.tabs[tab].scene.document, insert);
        assert!(
            (restored.0 - expected.0).length() < 1e-9 && (restored.1 - expected.1).length() < 1e-9
        );
        assert_eq!(
            instance::frame(&app.tabs[tab].scene.document, insert, &point).unwrap(),
            frame
        );
        assert_eq!(line(&app.tabs[tab].scene.document, other), original);
    }
}

/// The definition's evaluation graph — the fixture's definition holds one.
fn evaluation_graph(document: &document::Document) -> document::Handle {
    document
        .objects
        .values()
        .find_map(|object| match object {
            document::objects::ObjectType::DynamicBlock(object) => matches!(
                object.data,
                document::objects::DynamicBlockData::EvaluationGraph(_)
            )
            .then_some(object.handle),
            _ => None,
        })
        .expect("the authored definition carries an evaluation graph")
}

/// Leave the graph naming one node twice: every parameter it held is still
/// named, and no authoring read will edit it.
fn duplicate_a_node(document: &mut document::Document, graph: document::Handle) -> bool {
    let Some(document::objects::ObjectType::DynamicBlock(object)) = document.objects.get_mut(&graph)
    else {
        return false;
    };
    match &mut object.data {
        document::objects::DynamicBlockData::EvaluationGraph(graph) => {
            match graph.nodes.first().cloned() {
                Some(node) => {
                    graph.nodes.push(node);
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}

/// A definition whose evaluation graph refuses to be edited still holds the
/// parameters it was authored with, and the manager's rule is one row per
/// parameter, so it has to keep printing them. Dropping the row leaves the
/// operator with a short table and no word about why.
#[test]
fn an_uneditable_graph_keeps_the_parameters_it_holds_listed() {
    let (mut app, tab, definition) = open(&["BPARAMETER VISIBILITY 0,0"], &[]);
    let graph = evaluation_graph(&app.tabs[tab].scene.document);
    assert_eq!(
        crate::scene::block_visibility::parameter(&app.tabs[tab].scene.document, definition)
            .expect("the placed visibility parameter sits on the definition")
            .owner,
        graph,
        "and it hangs off the definition's graph"
    );
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Visibility1").is_some(),
        "the placed parameter is addressable while the graph is sound"
    );
    assert!(
        duplicate_a_node(&mut app.tabs[tab].scene.document, graph),
        "the graph has a node to name twice"
    );
    // The refusal is what makes this the uneditable case: the same row the list
    // prints is one nothing can be addressed through.
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Visibility1").is_none(),
        "and the definition's graph really does refuse"
    );
    let names: Vec<String> = manager::rows(&app.tabs[tab].scene.document, definition)
        .into_iter()
        .map(|row| row.name)
        .collect();
    assert!(
        names.iter().any(|name| name == "Visibility1"),
        "the parameter the definition still holds keeps its row: {names:?}"
    );
}

/// The other half of the same rule: a definition's lookup row is read off its
/// evaluation graph, so a graph that will not take an edit still has to keep that
/// row listed. Listing it is not editing it — the row stays one nothing can be
/// addressed through, which is what the refusal below says.
#[test]
fn an_uneditable_graph_keeps_the_lookup_it_holds_listed() {
    let (mut app, tab, definition) = open(
        &["BPARAMETER POLAR 0,0 10,0 5,2", "BPARAMETER LOOKUP 3,3"],
        &[],
    );
    let graph = evaluation_graph(&app.tabs[tab].scene.document);
    // Before the corruption, the lookup is addressable through the definition, so
    // the refusal below is a refusal on this row and not a name that never matched.
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Lookup1").is_some(),
        "the placed lookup is addressable while the graph is sound"
    );
    assert!(
        duplicate_a_node(&mut app.tabs[tab].scene.document, graph),
        "the graph has a node to name twice"
    );
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Lookup1").is_none(),
        "and the definition's graph really does refuse"
    );
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    assert!(
        rows.iter()
            .any(|row| row.name == "Lookup1" && row.type_name == "Lookup"),
        "the lookup the definition still holds keeps its row: {:?}",
        rows.iter()
            .map(|row| (row.name.as_str(), row.type_name))
            .collect::<Vec<_>>()
    );
}

/// A parameter the definition holds in its own components is the same case as
/// one hanging off the graph: the manager's rule is one row per component, so a
/// graph that will not take an edit has to keep both of a polar parameter's rows
/// listed, with the numbers they printed. A short table with no reason is the
/// defect either way; only addressing a row by name keeps refusing.
#[test]
fn an_uneditable_graph_keeps_the_component_rows_it_holds_listed() {
    let (mut app, tab, definition) = open(&["BPARAMETER POLAR 0,0 10,0 5,2"], &[]);
    let table = |app: &OpenCADStudio| {
        manager::rows(&app.tabs[tab].scene.document, definition)
            .into_iter()
            .map(|row| (row.name, row.type_name, row.value))
            .collect::<Vec<_>>()
    };
    let before = table(&app);
    for name in ["Distance1", "Angle1"] {
        assert!(
            before.iter().any(|(row, _, _)| row.as_str() == name),
            "a sound graph lists both of the polar parameter's rows: {before:?}"
        );
    }
    let graph = evaluation_graph(&app.tabs[tab].scene.document);
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Distance1").is_some(),
        "and a row is addressable while the graph is sound"
    );
    assert!(
        duplicate_a_node(&mut app.tabs[tab].scene.document, graph),
        "the graph has a node to name twice"
    );
    assert!(
        manager::target_of(&app.tabs[tab].scene.document, definition, "Distance1").is_none(),
        "and the definition's graph really does refuse"
    );
    let after = table(&app);
    assert_eq!(
        after, before,
        "the parameter the definition holds keeps its rows, and the numbers they printed"
    );
    assert!(
        after.iter().all(|(_, type_name, _)| *type_name == "Polar"),
        "and they still read as the parameter type they came from: {after:?}"
    );
}

/// An XY parameter's two rows are its two offsets from the base point, and each
/// row writes only its own axis. The two branches share the writer with the
/// linear and polar rows, so an axis mapped the wrong way would put the endpoint
/// somewhere neither number asked for.
#[test]
fn an_xy_row_writes_its_own_axis_as_an_offset_from_the_base() {
    let (mut app, tab, definition) = open(&["BPARAMETER XY 2,3 12,13 5,5"], &[]);
    let rows = manager::rows(&app.tabs[tab].scene.document, definition);
    let xy: Vec<_> = rows.iter().filter(|row| row.type_name == "XY").collect();
    assert_eq!(
        xy.len(),
        2,
        "an XY parameter lists one row per axis: {rows:?}"
    );
    assert_eq!(xy[0].name, "X Distance1");
    assert_eq!(xy[1].name, "Y Distance1");
    assert_eq!(
        xy[0].expression, "10",
        "the X row reads the offset it was placed at"
    );
    assert_eq!(xy[1].expression, "10", "and so does the Y row");

    answer(&mut app, "X Distance1", Cell::Value, "4");
    let (base, end) = endpoints(&app, tab, definition);
    assert!(
        (base - glam::DVec3::new(2.0, 3.0, 0.0)).length() < 1e-9,
        "the base point stays put"
    );
    assert!(
        (end - glam::DVec3::new(6.0, 13.0, 0.0)).length() < 1e-9,
        "the X row adds its offset to the base and leaves Y: {end:?}"
    );

    answer(&mut app, "Y Distance1", Cell::Value, "3");
    let (_, end) = endpoints(&app, tab, definition);
    assert!(
        (end - glam::DVec3::new(6.0, 6.0, 0.0)).length() < 1e-9,
        "the Y row adds its offset to the base and keeps the X just written: {end:?}"
    );
    assert_eq!(
        printed(&app, tab, definition, "X Distance1").expression,
        "4"
    );
    assert_eq!(
        printed(&app, tab, definition, "Y Distance1").expression,
        "3"
    );
}

/// A rotation parameter's row is its angle, written as a bearing from the base
/// point: the endpoint keeps the parameter's radius and turns to the typed angle.
#[test]
fn a_rotation_row_writes_its_angle_and_keeps_the_radius() {
    let (mut app, tab, definition) = open(&["BPARAMETER ROTATION 2,3 10 0 5,5"], &[]);
    let row = manager::rows(&app.tabs[tab].scene.document, definition)
        .into_iter()
        .find(|row| row.type_name == "Rotation")
        .expect("a rotation parameter gets a row");
    assert!(row.numeric, "the angle is a number the manager writes back");
    assert_eq!(
        row.expression, "0",
        "the row reads the angle it was placed at"
    );
    let (base, end) = endpoints(&app, tab, definition);
    assert!(
        ((end - base).length() - 10.0).abs() < 1e-9,
        "the parameter starts at its authored radius: {end:?}"
    );

    answer(&mut app, &row.name, Cell::Value, "90");
    assert_eq!(
        printed(&app, tab, definition, &row.name).value,
        "90",
        "the value cell prints the angle just written"
    );
    assert_eq!(
        printed(&app, tab, definition, &row.name).expression,
        "90",
        "and the definition stores that angle"
    );
    let (base, end) = endpoints(&app, tab, definition);
    assert!(
        (base - glam::DVec3::new(2.0, 3.0, 0.0)).length() < 1e-9,
        "the base point stays put"
    );
    assert!(
        (end - glam::DVec3::new(2.0, 13.0, 0.0)).length() < 1e-9,
        "the endpoint turned about the base to the typed bearing at the same radius: {end:?}"
    );
}
