use super::*;

fn prompt_of(app: &OpenCADStudio, i: usize) -> String {
    app.tabs[i]
        .active_cmd
        .as_ref()
        .expect("the action is still being authored")
        .prompt()
        .to_string()
}

fn options_of(app: &OpenCADStudio, i: usize) -> Vec<String> {
    app.tabs[i]
        .active_cmd
        .as_ref()
        .expect("the action is still being authored")
        .options()
        .into_iter()
        .map(|option| option.label)
        .collect()
}

/// A definition carrying one Linear parameter, whose name is read back rather
/// than assumed.
fn linear_action_fixture() -> (OpenCADStudio, Handle, String) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.dispatch_command("BPARAMETER LINEAR");
    for point in ["0,0", "10,0", "5,2", "2"] {
        feed(&mut app, point);
    }
    assert!(
        app.tabs[app.active_tab].active_cmd.is_none(),
        "the parameter is placed"
    );
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let name = crate::scene::block_actions::all_parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .next()
        .expect("the definition has a parameter")
        .name;
    (app, definition, name)
}

#[test]
fn a_bare_action_asks_for_the_parameter_before_the_action_type() {
    let (mut app, _, name) = linear_action_fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("BACTIONTOOL");
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Select parameter:",
        "the parameter comes before the action type"
    );
    assert!(
        options_of(&app, i).is_empty(),
        "the action types wait for the parameter"
    );

    feed(&mut app, &name);
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Enter action type [Array/Move/Scale/sTretch]:",
        "a linear parameter offers the action types it supports"
    );
}

#[test]
fn a_linear_parameter_offers_only_the_action_types_it_supports() {
    let (mut app, _, name) = linear_action_fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("BACTIONTOOL");
    feed(&mut app, &name);
    assert_eq!(
        options_of(&app, i),
        vec!["Array", "Move", "Scale", "sTretch"],
        "only the four types a linear parameter supports are offered"
    );
}

#[test]
fn an_action_type_the_parameter_does_not_support_is_refused() {
    let (mut app, _, name) = linear_action_fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BACTIONTOOL");
    feed(&mut app, &name);
    feed(&mut app, "Rotate");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Choose one of the offered action types."),
        "a linear parameter cannot take a rotation"
    );
    assert_eq!(app.tabs[i].scene.document, before, "nothing is recorded");
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Enter action type [Array/Move/Scale/sTretch]:",
        "the refusal leaves the question standing"
    );
}

#[test]
fn a_chosen_parameter_completes_its_array_action() {
    let (mut app, definition, name) = linear_action_fixture();
    let i = app.active_tab;
    let count = crate::scene::block_actions::actions(&app.tabs[i].scene.document, definition).len();
    let _ = app.dispatch_command("BACTIONTOOL");
    feed(&mut app, &name);
    feed(&mut app, "Array");
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Select objects for the action, then press Enter:",
        "the chosen type moves on to its objects"
    );
    let targets = members(&app);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Enter the distance between columns (|||):",
        "the array asks for its spacing"
    );
    feed(&mut app, "10");
    assert!(app.tabs[i].active_cmd.is_none());
    let recorded =
        crate::scene::block_actions::actions(&app.tabs[i].scene.document, definition).len();
    assert_eq!(recorded, count + 1, "the array action is recorded");
}
