#[cfg(test)]
mod dispatched_command_task_tests {
    use super::super::*;
    use crate::command::CmdResult;
    use document::entities::ViewportRenderMode as Mode;

    /// `CmdResult::Dispatch` and `CmdResult::Relaunch` run the assembled line
    /// through `dispatch_command`, and the Task it returns must reach the
    /// runtime: the render mode a VSCURRENT keyword picker chooses is applied
    /// by a message that Task carries.
    #[test]
    fn dispatch_and_relaunch_keep_the_task_the_command_returns() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Dispatch("VSCURRENT GOURAUDSHADED".into()));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Dispatch dropped the dispatched command's task"
        );

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Relaunch(
            "VSCURRENT GOURAUDSHADED".into(),
            Vec::new(),
        ));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Relaunch dropped the dispatched command's task"
        );
    }

    #[test]
    fn dispatch_move_with_handle_enter_and_points() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let handle = app.tabs[i].scene.add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::new(10.0, 20.0, 0.0),
                document::types::Vector3::new(30.0, 40.0, 0.0),
            ),
        ));

        // Unselected entity - AutoLISP (command "_MOVE" ss "" "_NONE" pt "_NONE" '(0 0 0)) produces:
        assert!(app.tabs[i].scene.selected_entities().is_empty());
        let cmd = format!("_MOVE {:X} ENTER _NONE 10,20,0 _NONE 0,0,0", handle.value());
        let task = app.apply_cmd_result(CmdResult::Script(cmd));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let document::EntityType::Line(l) = moved {
            assert!(
                (l.start.x - 0.0).abs() < 1e-4,
                "Expected start.x == 0, got {}",
                l.start.x
            );
            assert!(
                (l.start.y - 0.0).abs() < 1e-4,
                "Expected start.y == 0, got {}",
                l.start.y
            );
            assert!(
                (l.end.x - 20.0).abs() < 1e-4,
                "Expected end.x == 20, got {}",
                l.end.x
            );
            assert!(
                (l.end.y - 20.0).abs() < 1e-4,
                "Expected end.y == 20, got {}",
                l.end.y
            );
        } else {
            panic!("Expected line entity");
        }
    }

    #[test]
    fn preselection_move_with_base_and_dest_points() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let handle = app.tabs[i].scene.add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::new(10.0, 20.0, 0.0),
                document::types::Vector3::new(30.0, 40.0, 0.0),
            ),
        ));

        // User pre-selects the entity in the GUI
        app.tabs[i].scene.select_entity(handle, true);
        assert_eq!(app.tabs[i].scene.selected_entities().len(), 1);

        // Pre-selection MOVE command only takes base and destination points
        let cmd = "_MOVE _NONE 10,20,0 _NONE 0,0,0";
        let task = app.apply_cmd_result(CmdResult::Script(cmd.to_string()));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let document::EntityType::Line(l) = moved {
            assert!(
                (l.start.x - 0.0).abs() < 1e-4,
                "Expected start.x == 0, got {}",
                l.start.x
            );
            assert!(
                (l.start.y - 0.0).abs() < 1e-4,
                "Expected start.y == 0, got {}",
                l.start.y
            );
            assert!(
                (l.end.x - 20.0).abs() < 1e-4,
                "Expected end.x == 20, got {}",
                l.end.x
            );
            assert!(
                (l.end.y - 20.0).abs() < 1e-4,
                "Expected end.y == 20, got {}",
                l.end.y
            );
        } else {
            panic!("Expected line entity");
        }
    }
}
