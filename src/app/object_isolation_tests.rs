//! App-level UNISOLATEOBJECTS tests for objects a drawing was saved with as
//! invisible (e.g. hidden by another CAD program's HIDEOBJECTS).

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::entities::Line;
    use codec::types::Vector3;
    use codec::{EntityType, Handle};

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn line(y: f64) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(1.0, y, 0.0),
        ))
    }

    fn invisible(app: &OpenCADStudio, handle: Handle) -> bool {
        app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .invisible
    }

    fn set_invisible(app: &mut OpenCADStudio, handle: Handle) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .get_entity_mut(handle)
            .unwrap()
            .common_mut()
            .invisible = true;
    }

    /// A model-space line saved invisible, a visible one, and a block whose
    /// only child is invisible (a dynamic-block visibility state, which must
    /// stay untouched).
    fn drawing() -> (OpenCADStudio, Handle, Handle, Handle) {
        let mut app = fresh_app();
        let i = app.active_tab;
        let hidden = app.tabs[i].scene.add_entity(line(0.0));
        let shown = app.tabs[i].scene.add_entity(line(1.0));
        app.tabs[i]
            .scene
            .define_block_raw("STATE", Vector3::ZERO, vec![line(2.0)]);
        let child = app.tabs[i]
            .scene
            .document
            .block_records
            .get("STATE")
            .unwrap()
            .entity_handles[0];
        set_invisible(&mut app, hidden);
        set_invisible(&mut app, child);
        app.tabs[i].scene.bump_geometry();
        (app, hidden, shown, child)
    }

    #[test]
    fn unisolate_shows_objects_saved_invisible() {
        let (mut app, hidden, shown, child) = drawing();
        let i = app.active_tab;
        assert!(!app.tabs[i].scene.is_isolation_active());
        assert!(app.tabs[i].scene.has_hidden_objects());
        assert_eq!(app.tabs[i].scene.persisted_hidden_objects(), vec![hidden]);

        let _ = app.dispatch_command("UNISOLATEOBJECTS");
        assert!(!invisible(&app, hidden));
        assert!(!invisible(&app, shown));
        assert!(
            invisible(&app, child),
            "block-definition visibility untouched"
        );
        assert!(!app.tabs[i].scene.has_hidden_objects());
        assert!(app.tabs[i].dirty);

        let _ = app.update(Message::Undo);
        assert!(invisible(&app, hidden));
        assert!(app.tabs[i].scene.has_hidden_objects());
    }

    #[test]
    fn unisolate_ends_session_isolation_and_saved_invisibility_together() {
        let (mut app, hidden, shown, _) = drawing();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(shown, true);
        app.tabs[i].scene.hide_selected();
        assert!(app.tabs[i].scene.is_isolation_active());

        let _ = app.dispatch_command("UNISOLATEOBJECTS");
        assert!(!app.tabs[i].scene.is_isolation_active());
        assert!(!invisible(&app, hidden));
        assert!(!app.tabs[i].scene.has_hidden_objects());
    }

    #[test]
    fn unisolate_without_hidden_objects_changes_nothing() {
        let mut app = fresh_app();
        let i = app.active_tab;
        let _ = app.tabs[i].scene.add_entity(line(0.0));
        assert!(!app.tabs[i].scene.has_hidden_objects());
        let undo_len = app.tabs[i].history.undo_stack.len();
        let _ = app.dispatch_command("UNISOLATEOBJECTS");
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_len);
    }

    #[test]
    fn block_editor_reports_no_saved_hidden_objects() {
        let (mut app, _, _, _) = drawing();
        let i = app.active_tab;
        let block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("STATE")
            .unwrap()
            .handle;
        app.tabs[i].scene.block_edit_block = Some(block);
        assert!(app.tabs[i].scene.persisted_hidden_objects().is_empty());
        assert!(!app.tabs[i].scene.has_hidden_objects());
    }
}
