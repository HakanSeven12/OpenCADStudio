use super::*;

#[test]
fn add_row_opens_a_blank_row_and_typing_a_value_keeps_a_blank_one_after_it() {
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, definition, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::Property(0)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::AddRow));
    let draft = &app.block_lookup.as_ref().unwrap().drafts[0];
    assert_eq!(draft.edited.row_count, 1, "the button adds one row");
    assert_eq!(
        draft.edited.expressions,
        vec![String::new(), String::new()],
        "the new row is blank rather than carrying the current value and a generated label"
    );
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "20".into())));
    let draft = &app.block_lookup.as_ref().unwrap().drafts[0];
    assert_eq!(
        draft.edited.row_count, 2,
        "a row that takes a value keeps a blank one below it for the next entry"
    );
    assert_eq!(draft.edited.expressions[2..], ["", ""]);
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Small".into())));
    assert_eq!(
        app.block_lookup.as_ref().unwrap().drafts[0]
            .edited
            .row_count,
        2,
        "editing a row that is not last adds no further row"
    );
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    let (_, stored) =
        crate::scene::block_lookup::table(&app.tabs[i].scene.document, definition, parameter)
            .unwrap()
            .unwrap();
    assert_eq!(stored.row_count, 1, "the blank entry row is not stored");
    assert_eq!(
        stored.expressions,
        vec!["20".to_string(), "Small".to_string()]
    );
}

#[test]
fn a_partly_filled_row_is_refused_though_the_blank_one_is_dropped() {
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, _definition, _parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::Property(0)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::AddRow));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "20".into())));
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::BlockLookup(A::Apply));
    let state = app.block_lookup.as_ref().expect("the dialog stays open");
    assert_eq!(
        state.error.as_deref(),
        Some("Each cell must be a finite number."),
        "the partly filled row is the one refused"
    );
    assert!(state.confirm_close, "the operator is offered the way out");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "nothing was written while the row is refused"
    );
}

#[test]
fn a_blank_entry_row_alone_leaves_the_table_as_it_was() {
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, definition, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Apply));
    let depth = app.tabs[i].history.undo_stack.len();
    let saved = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::AddRow));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none(), "the dialog still closes");
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        depth,
        "an untouched blank row is not an edit"
    );
    assert_eq!(app.tabs[i].scene.document, saved);
    assert_eq!(
        crate::scene::block_lookup::table(&app.tabs[i].scene.document, definition, parameter)
            .unwrap()
            .unwrap()
            .1
            .expressions,
        vec!["20", "Small"]
    );
}

#[test]
fn lookup_editor_cancel_apply_one_undo_and_stale_guard() {
    use crate::scene::block_lookup;
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, definition, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for cancel in [Message::CloseModal, Message::CommandEscape] {
        let _ = app.dispatch_command("BLOOKUPTABLE");
        edit_lookup(&mut app);
        assert_eq!(app.tabs[i].scene.document, before, "draft is isolated");
        let _ = app.update(cancel);
        assert!(app.block_lookup.is_none());
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        depth,
        "unchanged OK is a no-op"
    );
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Audit));
    assert!(app.block_lookup.as_ref().unwrap().error.is_none());
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        block_lookup::table(&app.tabs[i].scene.document, definition, parameter)
            .unwrap()
            .unwrap()
            .1
            .expressions,
        vec!["20", "Small"]
    );
    let saved = app.tabs[i].scene.document.clone();
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, saved);
    let _ = app.dispatch_command("BLOOKUPTABLE Lookup1");
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Changed".into())));
    app.tabs[i].scene.bump_geometry();
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app
        .block_lookup
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("changed"));
    assert_eq!(app.tabs[i].scene.document, saved);
    let _ = app.update(Message::CloseModal);
}

#[test]
fn lookup_editor_multiple_tables_apply_atomically_and_parameter_cancel_is_clean() {
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, definition, _) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for kind in ["BASEPOINT", "LOOKUP"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Select(1)));
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "NaN".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.as_ref().unwrap().error.is_some());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "5".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for (h, _) in crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition) {
        assert!(
            crate::scene::block_lookup::table(&app.tabs[i].scene.document, definition, h)
                .unwrap()
                .is_some()
        );
    }
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let _ = app.dispatch_command("LINE");
    for cmd in ["BPARAMETER BASEPOINT", "BPARAMETER LOOKUP", "BLOOKUPTABLE"] {
        let _ = app.dispatch_command(cmd);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn action_type_list_offers_lookup_and_routes_the_chosen_parameter_to_its_table() {
    let (mut app, definition, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let second =
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[1].0;
    assert_ne!(parameter, second);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();

    let _ = app.dispatch_command("BACTIONTOOL");
    let prompt = app.command_line.history.last().unwrap().text.clone();
    assert!(prompt.contains("BACTION  Select parameter:"), "{prompt}");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "BACTION");

    feed(&mut app, "Lookup2");
    assert!(
        app.active_modal.is_none(),
        "the action type comes after the parameter"
    );
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BACTION  Enter action type [Lookup]:",
        "a chosen lookup parameter offers its one action type"
    );
    feed(&mut app, "Lookup");
    assert_eq!(app.active_modal, Some(ModalKind::BlockLookup));
    let state = app.block_lookup.as_ref().unwrap();
    let draft = &state.drafts[state.selected];
    assert_eq!(
        draft.parameter, second,
        "the named parameter, not the first"
    );
    assert_eq!(state.definition, definition);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.update(Message::CloseModal);
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].scene.document, before);

    for command in ["BACTION LOOKUP", "_BACTIONTOOL LOOKUP"] {
        let _ = app.dispatch_command(command);
        let prompt = app.command_line.history.last().unwrap().text.clone();
        assert!(
            prompt.to_ascii_lowercase().contains("lookup"),
            "{command}: {prompt}"
        );
        feed(&mut app, "Lookup2");
        assert_eq!(app.active_modal, Some(ModalKind::BlockLookup), "{command}");
        assert_eq!(
            app.block_lookup.as_ref().unwrap().drafts[app.block_lookup.as_ref().unwrap().selected]
                .parameter,
            second,
            "{command}"
        );
        let _ = app.update(Message::CloseModal);
    }

    let _ = app.dispatch_command("BACTION LOOKUP");
    feed(&mut app, "Missing");
    assert!(app.command_line.last_error.is_some());
    assert!(
        app.tabs[i]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt()
            .contains("Lookup"),
        "an unknown name keeps the parameter prompt active"
    );
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].scene.document, before);
}
