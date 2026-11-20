use super::*;

// The free-form text prompts (table cell content, TEXT / MTEXT bodies)
// change what Space / Enter / typing mean at the command line. These
// tests pin the routing through `text_entry_mode` so the three key
// routes can't drift apart again.
    use crate::app::{OpenCADStudio, TextEntryMode};
    use crate::modules::annotate::table_cmd::TableCellEditCommand;
    use document::entities::Table;
    use document::types::Vector3;
    use document::{EntityType, Handle};

    /// A test drawing with a 2×2 table and the cell editor active on [0,0],
    /// seeded with `cell_text`.
    fn app_with_cell_command(cell_text: &str) -> (OpenCADStudio, Handle) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let mut table = Table::new(Vector3::new(0.0, 0.0, 0.0), 2, 2);
        table.cell_mut(0, 0).unwrap().set_text(cell_text);
        let handle = app.tabs[0]
            .scene
            .document
            .add_entity(EntityType::Table(Box::new(table.clone())))
            .unwrap();
        app.set_active_command(0, Box::new(TableCellEditCommand::new(handle, &table, 0, 0)));
        (app, handle)
    }

    fn cell_text(app: &OpenCADStudio, _handle: Handle) -> String {
        // ReplaceMany commits by erasing the old handle and re-adding the
        // entity under a new one, so look the (single) table up by type.
        app.tabs[0]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                EntityType::Table(table) => Some(table.cell_text(0, 0).unwrap_or("").to_string()),
                _ => None,
            })
            .expect("table entity missing")
    }

    #[test]
    fn cell_editor_switches_to_free_text_mode() {
        let (mut app, _) = app_with_cell_command("");
        assert_eq!(app.text_entry_mode(), TextEntryMode::FreeText);
        assert!(app.is_free_text_active());
        app.tabs[0].active_cmd = None;
        assert_eq!(app.text_entry_mode(), TextEntryMode::Command);
        assert!(!app.is_free_text_active());
    }

    #[test]
    fn pasted_spreadsheet_rows_enter_one_point_each() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("PLINE");
        // A column copied from a spreadsheet, then X/Y in two columns.
        let _ = app.update(Message::CommandInput("1,4\r\n2,6\r\n3,1\r\n".into()));
        let _ = app.update(Message::CommandInput("4\t4\r\n".into()));
        let _ = app.update(Message::CommandSubmit);
        let points: Vec<(f64, f64)> = app.tabs[0]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::LwPolyline(p) => Some(
                    p.vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect(),
                ),
                _ => None,
            })
            .expect("the pasted rows drew a polyline");
        assert_eq!(points, vec![(1.0, 4.0), (2.0, 6.0), (3.0, 1.0), (4.0, 4.0)]);
    }

    #[test]
    fn cell_edit_typed_space_stays_in_buffer_with_case() {
        let (mut app, handle) = app_with_cell_command("");
        // A typed buffer containing a Space must not auto-submit.
        let _ = app.update(Message::CommandInput("Foo Bar".into()));
        assert_eq!(app.command_line.input, "Foo Bar");
        assert!(app.tabs[0].active_cmd.is_some(), "Space must not submit");
        // The Space key binding inserts a literal space too.
        let _ = app.update(Message::CommandSpace);
        assert_eq!(app.command_line.input, "Foo Bar ");
        // Enter finishes the edit and commits the content, case intact.
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), "Foo Bar");
    }

    #[test]
    fn cell_edit_slash_n_inserts_line_break() {
        let (mut app, handle) = app_with_cell_command("");
        let _ = app.update(Message::CommandInput("Foo/nBar".into()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), "Foo\\PBar");
    }

    #[test]
    fn cell_edit_unfocused_typing_preserves_case() {
        // The CommandAppendChar route (typing while the command line is not
        // focused) must not uppercase free-form text either.
        let (mut app, handle) = app_with_cell_command("");
        for ch in ["F", "o", "o"] {
            let _ = app.update(Message::CommandAppendChar(ch.into()));
        }
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), "Foo");
    }

    #[test]
    fn cell_edit_skips_expression_evaluation() {
        let (mut app, handle) = app_with_cell_command("");
        let _ = app.update(Message::CommandInput("5*2".into()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), "5*2");
    }

    #[test]
    fn cell_edit_leading_greater_than_is_content() {
        // `>` is only a literal-space hint for command prompts; in free
        // text it is content and must not be stripped on submit.
        let (mut app, handle) = app_with_cell_command("");
        let _ = app.update(Message::CommandInput(">Note".into()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), ">Note");
    }

    #[test]
    fn cell_edit_empty_enter_ends_edit_without_wiping() {
        let (mut app, handle) = app_with_cell_command("Keep me");
        // Enter moves the editor to the cell below; on the last row it ends.
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(cell_text(&app, handle), "Keep me");
        assert!(app.tabs[0].active_cmd.is_some(), "Enter moves down a row");
        let _ = app.update(Message::CommandSubmit);
        assert!(
            app.tabs[0].active_cmd.is_none(),
            "Enter on the last row ends the edit"
        );
    }

    #[test]
    fn command_input_preserves_case_and_submits_multi_token_lines() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        // Ordinary command entry now retains its typed case…
        let _ = app.update(Message::CommandInput("lin".into()));
        assert_eq!(app.command_line.input, "lin");
        // …and a pasted multi-token line still submits as a command.
        let _ = app.update(Message::CommandInput("LINE 0,0 10,10".into()));
        assert!(
            app.command_line.input.is_empty(),
            "Space submitted the line"
        );
        assert_eq!(app.text_entry_mode(), TextEntryMode::Command);
    }

    #[test]
    fn typed_command_characters_preserve_case() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.update(Message::CommandAppendChar("MyPath".into()));
        assert_eq!(app.command_line.input, "MyPath");
    }

    #[test]
    fn literal_command_input_preserves_case() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.update(Message::CommandInput(">Plugin MixedCase".into()));
        assert_eq!(app.command_line.input, ">Plugin MixedCase");
    }

    #[test]
    fn paste_shortcut_leaves_a_focused_field_alone() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.command_line.input = "EXISTING".into();
        app.clipboard = vec![document::EntityType::Line(
            document::entities::Line::default(),
        )];

        // A dialog field pasted natively: no command-line text, no PASTECLIP.
        let _ = app.update(Message::PasteShortcutResolved(
            crate::app::PasteFocus::Field,
        ));
        assert_eq!(app.command_line.input, "EXISTING");
        assert!(app.tabs[0].active_cmd.is_none());
    }

    #[test]
    fn paste_shortcut_over_the_command_line_pastes_copied_objects() {
        use crate::app::PasteFocus;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        // Text only: the command line already pasted it natively.
        let _ = app.update(Message::PasteShortcutResolved(PasteFocus::CommandLine));
        assert!(app.command_line.input.is_empty());
        assert!(app.tabs[0].active_cmd.is_none());

        // Copied objects paste even though the command line holds focus.
        app.clipboard = vec![document::EntityType::Line(
            document::entities::Line::default(),
        )];
        let _ = app.update(Message::PasteShortcutResolved(PasteFocus::CommandLine));
        assert!(app.tabs[0]
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.name() == "PASTECLIP"));
    }

    #[test]
    fn space_bound_to_finalize_finalizes_in_free_text_mode() {
        // #1710: when the user rebinds SPACE to FINALIZE in the CUI, pressing
        // SPACE must run FINALIZE even while the command line holds focus,
        // instead of falling through to the command line's CommandSpace
        // (space-insert) behavior. FreeText mode distinguishes the two: a
        // COMMANDSPACE would insert a space, while FINALIZE finishes the edit.
        let (mut app, _) = app_with_cell_command("");
        assert_eq!(app.text_entry_mode(), TextEntryMode::FreeText);

        // Bind SPACE to FINALIZE (the CUI assignment from the issue).
        app.shortcut_bindings
            .insert("SPACE".to_string(), "FINALIZE".to_string());

        // Pressing SPACE runs FINALIZE, which finishes the edit instead of
        // inserting a space.
        let _ = app.update(Message::ShortcutPressed("SPACE".to_string()));
        assert!(
            app.tabs[0].active_cmd.is_none(),
            "SPACE bound to FINALIZE must finalize, not insert a space"
        );
    }
