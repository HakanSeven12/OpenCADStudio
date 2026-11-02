use super::*;

/// The `Lookup Set` row has to reach the table editor on its own: the command
/// it used to run is the one the action-bar preference refuses.
#[test]
fn lookup_set_row_opens_the_table_editor_without_the_action_bar_refusal() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 1");
    let i = app.active_tab;
    let _ = app.dispatch_command("BPARAMETERSET Lookup Set");
    feed(&mut app, "1,1");
    feed(&mut app, "");
    assert!(
        app.command_line.last_error.is_none(),
        "the row is not refused: {:?}",
        app.command_line.last_error
    );
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the lookup placement finished, still prompting {:?}",
        app.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt().to_string())
    );
    assert!(
        app.block_lookup.is_some(),
        "the table editor opens for the parameter the row just placed"
    );
    assert_eq!(
        app.active_modal,
        Some(crate::app::ModalKind::BlockLookup)
    );
    assert_eq!(
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, app.tabs[i]
            .active_block_edit_session()
            .unwrap()
            .br_handle)
        .len(),
        1,
        "one parameter, authored once"
    );
}

/// The row places a parameter of its own, so it has to open that parameter's
/// table even when the definition already carries another one.
#[test]
fn lookup_set_row_opens_the_table_of_the_parameter_it_placed() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER LOOKUP 0,0");
    let i = app.active_tab;
    let definition = app.tabs[i]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let handles = |app: &OpenCADStudio| {
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|(handle, _)| handle)
            .collect::<Vec<_>>()
    };
    let first = handles(&app)[0];
    let _ = app.dispatch_command("BPARAMETERSET Lookup Set");
    feed(&mut app, "1,1");
    feed(&mut app, "");
    let all = handles(&app);
    assert_eq!(all.len(), 2, "the row authored the second parameter");
    let placed = *all.iter().find(|handle| **handle != first).unwrap();
    let state = app.block_lookup.as_ref().expect("the table editor opens");
    assert_eq!(state.drafts.len(), 2, "both tables are listed");
    assert_eq!(
        state.drafts[state.selected].parameter, placed,
        "the table of the parameter the row just placed, not the older one"
    );
}

/// The editor's Actions tab offers the same table; with the action bars on it
/// must reach the editor, not the command the bars refuse.
#[test]
fn actions_tab_lookup_tool_opens_the_table_with_the_action_bars_on() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 1");
    let _ = app.run_command_line("BPARAMETER LOOKUP 1,1");
    let _ = app.update(Message::RibbonToolClick {
        tool_id: "BLOOKUPTABLE".to_string(),
        event: crate::modules::ModuleEvent::Command("BLOOKUPTABLE".to_string()),
    });
    assert!(
        app.command_line.last_error.is_none(),
        "the palette tool is not refused: {:?}",
        app.command_line.last_error
    );
    assert!(
        app.block_lookup.is_some(),
        "the palette's Lookup tool opens the table editor"
    );
}

/// Two lookup parameters put two rows on the action bars and each row names a
/// table of its own, so the row that was clicked has to be the one that opens.
#[test]
fn action_bar_lookup_rows_open_the_table_of_the_row_clicked() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 1");
    let _ = app.run_command_line("BPARAMETER LOOKUP 0,0");
    let _ = app.run_command_line("BPARAMETER LOOKUP 4,0");
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handles = crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .map(|(handle, _)| handle)
        .collect::<Vec<_>>();
    assert_eq!(handles.len(), 2, "the fixture places two lookup parameters");
    for clicked in handles {
        let _ = app.update(Message::CloseModal);
        let _ = app.update(Message::BlockActionBarEdit(clicked));
        let state = app
            .block_lookup
            .as_ref()
            .expect("the lookup row opens the table editor");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockLookup),
            "the row's table opens over the editor"
        );
        assert_eq!(state.drafts.len(), 2, "both tables are listed");
        assert_eq!(
            state.drafts[state.selected].parameter, clicked,
            "the table of the row that was clicked"
        );
        assert_eq!(
            app.tabs[i].selected_authoring,
            Some(clicked),
            "the same row is what that tab now reports as selected"
        );
    }
}
