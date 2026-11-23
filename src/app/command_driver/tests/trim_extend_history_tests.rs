#[cfg(test)]
mod trim_extend_history_tests {
    use super::super::*;
    use crate::modules::draw::modify::trim::{ExtendCommand, TrimCommand};
    use document::entities::Line;
    use document::types::Vector3;

    fn add_line(app: &mut OpenCADStudio, a: [f64; 2], b: [f64; 2]) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
    }

    fn assert_line_exists(app: &OpenCADStudio, a: [f64; 2], b: [f64; 2]) {
        assert!(
            app.tabs[app.active_tab]
                .scene
                .document
                .entities()
                .any(|entity| {
                    matches!(entity, document::EntityType::Line(line)
                if (line.start.x - a[0]).abs() < 1e-9
                    && (line.start.y - a[1]).abs() < 1e-9
                    && (line.end.x - b[0]).abs() < 1e-9
                    && (line.end.y - b[1]).abs() < 1e-9)
                }),
            "line from {a:?} to {b:?} should exist"
        );
    }

    #[test]
    fn trim_command_dispatch_includes_fill_only_hatch_targets() {
        use document::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
        use document::types::Vector2;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
            true,
        )));
        hatch.paths.push(path);
        let i = app.active_tab;
        let hatch_handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Hatch(Box::new(hatch)));
        let boundary = add_line(&mut app, [5.0, -2.0], [5.0, 12.0]);
        // Pick-first supplies a deterministic Standard-mode cutting edge,
        // regardless of the global TRIMMODE setting from other tests.
        app.tabs[i].scene.select_entity(boundary, false);
        assert!(app.tabs[i]
            .scene
            .visible_hatch_handles_for_edit()
            .contains(&hatch_handle));

        // This exercises command dispatch, not just TrimCommand::new().
        // A filled hatch does not appear in entity_wires(): previously the
        // command snapshot omitted it, making every inside click a no-op.
        let _ = app.dispatch_command("TRIM");
        let cmd = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("TRIM command active");
        assert_eq!(cmd.name(), "TRIM");
        let result = cmd.on_entity_pick(hatch_handle, glam::DVec3::new(2.0, 5.0, 0.0));
        assert!(matches!(result, CmdResult::ReplaceEntity(h, edited)
            if h == hatch_handle && matches!(edited.first(), Some(document::EntityType::Hatch(_)))));
    }

    #[test]
    fn trim_replacement_survives_undo_and_redo() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let target = add_line(&mut app, [0.0, 0.0], [10.0, 0.0]);
        let _boundary = add_line(&mut app, [6.0, -2.0], [6.0, 2.0]);
        let all: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .cloned()
            .collect();
        app.tabs[app.active_tab].active_cmd = Some(Box::new(TrimCommand::new(all)));
        let result = app.tabs[app.active_tab]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(8.0, 0.0, 0.0));
        let _ = app.apply_cmd_result(result);
        assert_line_exists(&app, [0.0, 0.0], [6.0, 0.0]);

        app.undo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [10.0, 0.0]);
        app.redo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [6.0, 0.0]);
    }

    #[test]
    fn extend_replacement_survives_undo_and_redo() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let target = add_line(&mut app, [0.0, 0.0], [4.0, 0.0]);
        let _boundary = add_line(&mut app, [8.0, -2.0], [8.0, 2.0]);
        let all: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .cloned()
            .collect();
        app.tabs[app.active_tab].active_cmd = Some(Box::new(ExtendCommand::new(all)));
        let result = app.tabs[app.active_tab]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(3.5, 0.0, 0.0));
        let _ = app.apply_cmd_result(result);
        assert_line_exists(&app, [0.0, 0.0], [8.0, 0.0]);

        app.undo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [4.0, 0.0]);
        app.redo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [8.0, 0.0]);
    }
}
