#[cfg(test)]
mod bedit_characterization_tests {
    use super::super::*;
    use document::entities::{Circle, Insert, Line};
    use document::types::Vector3;
    use document::EntityType;

    /// Fresh document with one block definition `BLK` (one line) and one
    /// model-space INSERT referencing it. Returns the INSERT handle and the
    /// definition's member handles.
    fn setup_block_and_insert(app: &mut OpenCADStudio) -> (Handle, Vec<Handle>) {
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab].scene.define_block_raw(
            "BLK",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let insert = app.tabs[tab]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "BLK",
                Vector3::new(100.0, 0.0, 0.0),
            )))
            .expect("insert");
        let members = block_members(app, "BLK");
        (insert, members)
    }

    fn block_members(app: &OpenCADStudio, name: &str) -> Vec<Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get(name)
            .map(|br| br.entity_handles.clone())
            .unwrap_or_default()
    }

    /// Definition state as (handle, Debug form) in member order. The Debug
    /// form stands in for PartialEq, which EntityType does not provide.
    fn definition_state(app: &OpenCADStudio, name: &str) -> Vec<(Handle, String)> {
        let tab = app.active_tab;
        block_members(app, name)
            .into_iter()
            .map(|h| {
                let e = app.tabs[tab].scene.document.get_entity(h);
                (h, format!("{e:?}"))
            })
            .collect()
    }

    fn undo_depth(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn begin_bedit(app: &mut OpenCADStudio, insert: Handle) {
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[app.active_tab].active_block_edit.is_some(),
            "setup: BEDIT_BEGIN should open a block-edit session"
        );
    }

    fn add_circle_in_bedit(app: &mut OpenCADStudio) -> Handle {
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)))
    }

    // ---- locked: must survive Phase 1 ----

    /// Locked: save keeps the entity added inside the session.
    #[test]
    fn locked_save_keeps_entity_added_in_session() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        add_circle_in_bedit(&mut app);
        let _ = app.dispatch_command("BEDIT_SAVE");

        assert!(app.tabs[app.active_tab].active_block_edit.is_none());
        assert_eq!(
            block_members(&app, "BLK").len(),
            2,
            "save keeps the new entity"
        );
    }

    /// Locked: discard restores the definition to its entry state exactly
    /// (same handles, same geometry, same order).
    #[test]
    fn locked_discard_restores_definition_to_entry_state() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let entry = definition_state(&app, "BLK");
        begin_bedit(&mut app, insert);
        add_circle_in_bedit(&mut app);
        let _ = app.dispatch_command("BEDIT_DISCARD");

        assert!(app.tabs[app.active_tab].active_block_edit.is_none());
        assert_eq!(definition_state(&app, "BLK"), entry);
    }

    /// Locked: refusals. A non-INSERT is rejected and no session opens.
    #[test]
    fn locked_bedit_refuses_non_insert_object() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let line = app.tabs[tab]
            .scene
            .document
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )))
            .expect("line");
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", line.value()));
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// Locked: refusal. The `BEDIT` picker command is rejected while a
    /// REFEDIT session is active and no BEDIT session opens.
    #[test]
    fn locked_bedit_picker_refused_while_refedit_active() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN should open a REFEDIT session"
        );
        let _ = app.dispatch_command("BEDIT");
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// Every entry point must refuse while REFEDIT is active.
    #[test]
    fn locked_bedit_begin_refuses_active_refedit() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].refedit_session.is_some());
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].active_block_edit.is_none(),
            "begin must refuse while REFEDIT is active"
        );
    }

    /// Locked: refusal. An xref definition cannot be edited.
    #[test]
    fn locked_bedit_refuses_xref_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get_mut("BLK")
            .expect("block record")
            .flags
            .is_xref = true;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// V7 (reference check): while a block editor session is open this
    /// command set is refused outright with "not allowed in block editor";
    /// the refusal pushes no undo entry and starts no command.
    #[test]
    fn locked_deny_list_refuses_while_session_open() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "setup: BEDIT_BEGIN should open a session"
        );
        let undo_depth = app.tabs[tab].history.undo_stack.len();
        for verb in [
            "LAYOUT",
            "VPORTS",
            "MVIEW",
            "WBLOCK",
            "PURGE",
            "RENAME",
            "REFEDIT",
            "REFEDIT_BEGIN",
            "PLOT",
        ] {
            let revision = app.command_line.error_revision;
            let _ = app.dispatch_command(verb);
            assert_eq!(
                app.command_line.error_revision,
                revision.wrapping_add(1),
                "{verb} must refuse with a command-line error"
            );
            assert!(
                app.command_line
                    .last_error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("command not allowed in block editor"),
                "{verb}: got {:?}",
                app.command_line.last_error
            );
            assert!(
                app.tabs[tab].active_block_edit.is_some(),
                "{verb} must keep the session open"
            );
            assert!(
                app.tabs[tab].active_cmd.is_none(),
                "{verb} must not install a command"
            );
            assert_eq!(
                app.tabs[tab].history.undo_stack.len(),
                undo_depth,
                "{verb} must push no undo entry"
            );
        }
    }

    /// Spec 8: a rejected command must not cancel a running command — the
    /// deny hook sits before the running-command teardown.
    #[test]
    fn locked_deny_list_does_not_cancel_a_running_command() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].active_block_edit.is_some());
        let _ = app.dispatch_command("LINE");
        assert_eq!(
            app.tabs[tab].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE"),
            "setup: LINE should be running inside the session"
        );
        let _ = app.dispatch_command("LAYOUT");
        assert!(app
            .command_line
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("command not allowed in block editor"));
        assert_eq!(
            app.tabs[tab].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE"),
            "the refused command must leave the running LINE untouched"
        );
    }

    /// V7: the reference keeps XREF, NEW, OPEN, SAVEAS and BEDIT available
    /// inside the editor — the deny list must not touch them.
    #[test]
    fn locked_deny_list_leaves_v7_allowed_commands_alone() {
        for verb in ["XREF", "NEW", "OPEN", "SAVEAS", "BEDIT"] {
            let mut app = OpenCADStudio::new_for_test();
            let (insert, _) = setup_block_and_insert(&mut app);
            let tab = app.active_tab;
            let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
            assert!(app.tabs[tab].active_block_edit.is_some());
            let _ = app.dispatch_command(verb);
            assert!(
                !app.command_line
                    .last_error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("not allowed in block editor"),
                "{verb} is available in the reference session and must not be denied: {:?}",
                app.command_line.last_error
            );
        }
    }

    // ---- documenting: today's behavior, expected to flip in Phase 1 ----

    /// Discard removes local before-images before drawing undo resumes.
    #[test]
    fn locked_modify_discard_then_undo_does_not_restore_local_edits() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, members) = setup_block_and_insert(&mut app);
        let line = members[0];
        let entry = definition_state(&app, "BLK");
        begin_bedit(&mut app, insert);

        let tab = app.active_tab;
        // Record the undo step before the mutation so the recorder captures it.
        app.push_undo_snapshot(tab, "modify inside BEDIT");
        app.tabs[tab]
            .scene
            .document
            .get_entity_mut(line)
            .map(|e| {
                if let EntityType::Line(l) = e {
                    l.end = Vector3::new(99.0, 0.0, 0.0);
                }
            })
            .expect("line in definition");
        app.finish_pending_history(tab);

        let _ = app.dispatch_command("BEDIT_DISCARD");
        app.finish_pending_history(tab);
        assert_eq!(
            definition_state(&app, "BLK"),
            entry,
            "discard restores entry"
        );

        app.undo_steps(1);
        // Pinned: the state after one undo, observed on current code.
        assert_eq!(
            definition_state(&app, "BLK"),
            entry,
            "documenting: undo after discard restores the entry state (ISS-02 not reproduced here)"
        );
    }

    /// Local undo restores both the entity store and block membership.
    #[test]
    fn locked_session_undo_restores_membership_without_crossing_entry() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        let tab = app.active_tab;
        app.push_undo_snapshot(tab, "add circle in BEDIT");
        add_circle_in_bedit(&mut app);
        app.finish_pending_history(tab);
        assert_eq!(block_members(&app, "BLK").len(), 2);

        app.undo_steps(1);
        let members = block_members(&app, "BLK");
        assert_eq!(
            members.len(),
            1,
            "block membership must match the undone entity store"
        );
        let dangling = members
            .iter()
            .filter(|h| app.tabs[tab].scene.document.get_entity(**h).is_none())
            .count();
        assert_eq!(dangling, 0, "every member must have an entity");
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "documenting: undo does not close the session"
        );
    }

    /// Opening starts an empty local history; undo cannot cross entry.
    #[test]
    fn locked_session_entry_has_no_undo_step() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        begin_bedit(&mut app, insert);
        app.finish_pending_history(tab);
        let depth_after_begin = undo_depth(&app);
        assert_eq!(depth_after_begin, 0, "opening must not record an undo step");

        app.undo_steps(1);
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "documenting: undoing BEDIT_BEGIN does not close the open session"
        );
    }

    /// Switching a clean block replaces the single session.
    #[test]
    fn locked_clean_switch_replaces_the_session() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        app.tabs[tab].scene.define_block_raw(
            "OTHER",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(5.0, 0.0, 0.0),
            ))],
        );
        let other_insert = app.tabs[tab]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "OTHER",
                Vector3::new(200.0, 0.0, 0.0),
            )))
            .expect("insert");
        begin_bedit(&mut app, insert);
        begin_bedit(&mut app, other_insert);
        assert_eq!(
            app.tabs[tab].block_edits.len(),
            1,
            "a drawing has only one block-editor session"
        );
    }

    /// No-change Save and Discard leave the drawing history untouched.
    #[test]
    fn locked_clean_save_and_discard_leave_history_unchanged() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        app.finish_pending_history(app.active_tab);
        let after_begin = undo_depth(&app);

        let _ = app.dispatch_command("BEDIT_SAVE");
        app.finish_pending_history(app.active_tab);
        assert_eq!(undo_depth(&app), after_begin, "save pushes no undo entry");

        begin_bedit(&mut app, insert);
        app.finish_pending_history(app.active_tab);
        let before_discard = undo_depth(&app);
        let _ = app.dispatch_command("BEDIT_DISCARD");
        app.finish_pending_history(app.active_tab);
        assert_eq!(
            undo_depth(&app),
            before_discard,
            "discard pushes no undo entry (not undoable)"
        );
    }

    /// PICKLOCKED 0 takes objects on a locked layer out of selection, and
    /// what was selected there leaves the selection; 1 (default) keeps the
    /// earlier behaviour.
    #[test]
    fn picklocked_off_makes_locked_layer_objects_unselectable() {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::scene::set_pick_locked(self.0);
            }
        }
        let _restore = Restore(crate::scene::pick_locked());

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let free = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        let mut line =
            Line::from_points(Vector3::new(0.0, 10.0, 0.0), Vector3::new(10.0, 10.0, 0.0));
        line.common.layer = "BASE".to_string();
        let locked = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(line));
        app.tabs[tab]
            .scene
            .document
            .layers
            .get_mut("BASE")
            .expect("the layer of the line exists")
            .flags
            .locked = true;

        let _ = app.run_command_line("SETVAR PICKLOCKED 1");
        assert!(app.tabs[tab].scene.passes_selection_filter(locked));
        app.tabs[tab].scene.select_entity(locked, false);
        app.tabs[tab].scene.select_entity(free, false);

        let _ = app.run_command_line("SETVAR PICKLOCKED 0");
        assert!(!crate::scene::pick_locked());
        let scene = &mut app.tabs[tab].scene;
        assert!(scene.passes_selection_filter(free));
        assert!(!scene.passes_selection_filter(locked));
        let selected: Vec<_> = scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        assert_eq!(selected, vec![free], "the locked line left the selection");
        scene.deselect_all();
        assert_eq!(
            scene.select_all_visible(),
            1,
            "select-all takes the free line only"
        );
    }

    #[test]
    fn locked_layer_selection_rejection() {
        use crate::command::{CadCommand, CmdResult};

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;

        // Create line1 on layer "0" (unlocked)
        let h_unlocked =
            app.tabs[tab]
                .scene
                .add_entity(document::EntityType::Line(Line::from_points(
                    Vector3::ZERO,
                    Vector3::new(10.0, 0.0, 0.0),
                )));

        // Create line2 on layer "LOCKED_LAYER" and lock it
        let mut line2 =
            Line::from_points(Vector3::new(0.0, 10.0, 0.0), Vector3::new(10.0, 10.0, 0.0));
        line2.common.layer = "LOCKED_LAYER".to_string();
        let h_locked = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(line2));

        app.tabs[tab]
            .scene
            .document
            .layers
            .get_mut("LOCKED_LAYER")
            .expect("LOCKED_LAYER must exist")
            .flags
            .locked = true;

        assert!(!app.tabs[tab].scene.is_layer_locked(h_unlocked));
        assert!(app.tabs[tab].scene.is_layer_locked(h_locked));

        struct MockCmd {
            rejects: bool,
        }

        impl CadCommand for MockCmd {
            fn name(&self) -> &'static str {
                "MOCK_REJECT"
            }
            fn prompt(&self) -> String {
                "Select objects: ".into()
            }
            fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
                CmdResult::Done
            }
            fn on_enter(&mut self) -> CmdResult {
                CmdResult::Done
            }
            fn selection_rejects_locked_layers(&self) -> bool {
                self.rejects
            }
        }

        // Test 1: Command with rejects = true drops locked handles and reports count
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: true }));

        let mut handles = vec![h_unlocked, h_locked];
        app.filter_handles_reject_locked(tab, &mut handles);
        assert_eq!(handles, vec![h_unlocked]);
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("1 was on a locked layer."));

        // Test 2: Command with rejects = false keeps all handles
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: false }));

        let mut handles2 = vec![h_unlocked, h_locked];
        app.filter_handles_reject_locked(tab, &mut handles2);
        assert_eq!(handles2, vec![h_unlocked, h_locked]);

        // Test 3: filter_locked_layer_selection drops locked handles from SelectionComplete
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: true }));
        let input = StepInput::SelectionComplete(vec![h_unlocked, h_locked]);
        let filtered = app.filter_locked_layer_selection(input);
        match filtered {
            StepInput::SelectionComplete(res) => assert_eq!(res, vec![h_unlocked]),
            _ => panic!("Expected SelectionComplete"),
        }
    }
}
