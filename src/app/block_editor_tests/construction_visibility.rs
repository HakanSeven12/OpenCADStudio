use super::*;
use crate::command::CmdResult;
use crate::modules::block_editor::construction::Operation;
use crate::scene::{block_construction as construction, block_visibility as visibility};

#[test]
fn reverting_construction_uses_current_visibility_state_and_history() {
    for initially_visible in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let member = members(&app)[0];
        let document = &mut app.tabs[i].scene.document;
        let parameter = visibility::create(document, definition, Vector3::ZERO).unwrap();
        visibility::add_state(document, parameter, "Alternative", 0).unwrap();
        visibility::set_visible(document, parameter, 0, &[member], initially_visible);
        visibility::set_visible(document, parameter, 1, &[member], !initially_visible);
        visibility::apply_state(document, parameter, 0);
        app.tabs[i].scene.visibility_states.insert(definition, 0);
        let edit = |app: &mut OpenCADStudio, operation| {
            let _ = app.set_block_construction(CmdResult::SetBlockConstruction {
                document: app.tabs[i].id,
                definition,
                handles: vec![member],
                operation,
            });
        };
        edit(&mut app, Operation::Convert);
        let common = app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common();
        assert!(construction::is_construction(common));
        assert!(common.invisible);
        visibility::apply_state(&mut app.tabs[i].scene.document, parameter, 1);
        app.tabs[i].scene.visibility_states.insert(definition, 1);
        let depth = app.tabs[i].history.undo_stack.len();
        edit(&mut app, Operation::Revert);
        let check = |app: &OpenCADStudio, converted, hidden| {
            let common = app.tabs[i]
                .scene
                .document
                .get_entity(member)
                .unwrap()
                .common();
            assert_eq!(construction::is_construction(common), converted);
            assert_eq!(common.invisible, hidden);
        };
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        check(&app, false, initially_visible);
        app.undo_steps(1);
        check(&app, true, true);
        app.redo_steps(1);
        check(&app, false, initially_visible);
    }
}
