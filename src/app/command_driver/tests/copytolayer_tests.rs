#[cfg(test)]
mod copytolayer_tests {
    use super::super::*;
    use document::entities::{Circle, Line};
    use document::types::Vector3;
    use glam::DVec3;

    fn fixture() -> (OpenCADStudio, Handle, Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let scene = &mut app.tabs[app.active_tab].scene;
        scene
            .document
            .layers
            .add(document::tables::Layer::new("RED"))
            .unwrap();
        scene
            .document
            .layers
            .add(document::tables::Layer::new("BLUE"))
            .unwrap();
        let mut circle = Circle::from_coords(1.0, 2.0, 0.0, 2.0);
        circle.common.layer = "RED".into();
        let source = scene.add_entity(document::EntityType::Circle(circle));
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        line.common.layer = "BLUE".into();
        let dest = scene.add_entity(document::EntityType::Line(line));
        (app, source, dest)
    }

    // Like viewport picks / boxes: selection accumulates, and the command receives
    // the WHOLE scene selection, never just the handle picked this time.
    fn pick(app: &mut OpenCADStudio, handles: &[Handle]) {
        let scene = &mut app.tabs[app.active_tab].scene;
        for h in handles {
            scene.select_entity(*h, false);
        }
        selection_changed(app);
    }

    fn selection_changed(app: &mut OpenCADStudio) {
        let handles = app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        feed(app, StepInput::SelectionComplete(handles));
    }

    fn feed(app: &mut OpenCADStudio, input: StepInput) {
        let _ = app.feed_command(input);
        // Match update(): close history at every GUI message boundary unless
        // the running command groups it across the copy / placement steps.
        app.finish_all_pending_history();
    }

    fn type_token(app: &mut OpenCADStudio, text: &str) {
        let _ = app.feed_active_cmd(text);
        app.finish_all_pending_history();
    }

    fn copies(app: &OpenCADStudio, originals: &[Handle]) -> Vec<Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .map(|e| e.common().handle)
            .filter(|h| !originals.contains(h))
            .collect()
    }

    fn assert_circle(app: &OpenCADStudio, h: Handle, layer: &str, offset: DVec3) {
        let e = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(h)
            .unwrap();
        assert_eq!(
            e.common().layer,
            layer,
            "destination layer must come from the phase-2 pick"
        );
        let document::EntityType::Circle(c) = e else {
            panic!("expected circle")
        };
        assert_eq!(
            (c.center.x, c.center.y, c.center.z),
            (1.0 + offset.x, 2.0 + offset.y, offset.z)
        );
    }

    fn ready() -> (OpenCADStudio, Handle, Handle, Handle) {
        let (mut app, src, dest) = fixture();
        pick(&mut app, &[src]);
        let _ = app.dispatch_command("COPYTOLAYER");
        assert!(app.tabs[app.active_tab].scene.selected.is_empty());
        pick(&mut app, &[dest]);
        let copied = copies(&app, &[src, dest]);
        assert_eq!(copied.len(), 1);
        assert_circle(&app, copied[0], "BLUE", DVec3::ZERO);
        assert!(
            app.tabs[app.active_tab].active_cmd.is_some(),
            "copies must exist at the base point prompt"
        );
        (app, src, dest, copied[0])
    }

    #[test]
    fn copytolayer_destination_after_accumulated_gui_selection() {
        let (mut app, src, dest) = fixture();
        let _ = app.dispatch_command("COPYTOLAYER");
        pick(&mut app, &[src]);
        feed(&mut app, StepInput::Enter);
        pick(&mut app, &[dest]);
        let copied = copies(&app, &[src, dest]);
        assert_eq!(copied.len(), 1);
        assert_circle(&app, copied[0], "BLUE", DVec3::ZERO);
        assert_circle(&app, src, "RED", DVec3::ZERO);
    }

    #[test]
    fn laymatch_destination_after_accumulated_gui_selection() {
        let (mut app, src, dest) = fixture();
        let _ = app.dispatch_command("LAYMATCH");
        pick(&mut app, &[src]);
        feed(&mut app, StepInput::Enter);
        pick(&mut app, &[dest]);
        assert_circle(&app, src, "BLUE", DVec3::ZERO);
        assert!(copies(&app, &[src, dest]).is_empty());
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn copytolayer_and_laymatch_gather_picks_and_box_until_enter() {
        for name in ["COPYTOLAYER", "LAYMATCH"] {
            let (mut app, src, dest) = fixture();
            let scene = &mut app.tabs[app.active_tab].scene;
            let extra: Vec<_> = (0..3)
                .map(|n| {
                    scene.add_entity(document::EntityType::Circle(Circle::from_coords(
                        20.0 + n as f64,
                        0.0,
                        0.0,
                        1.0,
                    )))
                })
                .collect();
            let _ = app.dispatch_command(name);
            let prompt = app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .unwrap()
                .prompt();
            pick(&mut app, &[src]);
            pick(&mut app, &extra[..1]);
            pick(&mut app, &extra[1..]); // A window selecting two more objects.
            assert_eq!(
                app.tabs[app.active_tab]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt(),
                prompt,
                "several picks must stay at the objects prompt until Enter"
            );
            assert_eq!(
                copies(&app, &[src, dest]).len(),
                3,
                "no copies before Enter / destination"
            );
            // A removal must replace the command's cached set, not be unioned into it.
            app.tabs[app.active_tab].scene.deselect_entity(extra[2]);
            selection_changed(&mut app);
            feed(&mut app, StepInput::Enter);
            assert!(app.tabs[app.active_tab].scene.selected.is_empty());
            pick(&mut app, &[dest]);
            let all_originals = [src, dest, extra[0], extra[1], extra[2]];
            if name == "COPYTOLAYER" {
                assert_eq!(copies(&app, &all_originals).len(), 3);
                assert_circle(&app, src, "RED", DVec3::ZERO);
            } else {
                for h in [src, extra[0], extra[1]] {
                    assert_eq!(
                        app.tabs[app.active_tab]
                            .scene
                            .document
                            .get_entity(h)
                            .unwrap()
                            .common()
                            .layer,
                        "BLUE"
                    );
                }
                assert_eq!(
                    app.tabs[app.active_tab]
                        .scene
                        .document
                        .get_entity(extra[2])
                        .unwrap()
                        .common()
                        .layer,
                    "0"
                );
            }
        }
    }

    #[test]
    fn copytolayer_and_laymatch_empty_selection_enter_cancels() {
        for name in ["COPYTOLAYER", "LAYMATCH"] {
            let (mut app, src, _) = fixture();
            let _ = app.dispatch_command(name);
            pick(&mut app, &[src]);
            app.tabs[app.active_tab].scene.deselect_all();
            selection_changed(&mut app);
            feed(&mut app, StepInput::Enter);
            assert!(
                app.tabs[app.active_tab].active_cmd.is_none(),
                "empty selection must cancel"
            );
            assert!(app.tabs[app.active_tab].history.undo_stack.is_empty());
        }
    }

    #[test]
    fn copytolayer_preselection_and_exit_keep_copies_in_place() {
        for exit in ["", "X", "eXit", "Esc"] {
            let (mut app, src, dest, copy) = ready();
            if exit == "Esc" {
                feed(&mut app, StepInput::Escape);
            } else {
                type_token(&mut app, exit);
            }
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
            assert_circle(&app, copy, "BLUE", DVec3::ZERO);
            assert_circle(&app, src, "RED", DVec3::ZERO);
            assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
            app.undo_active_tab();
            assert!(copies(&app, &[src, dest]).is_empty());
            assert_circle(&app, src, "RED", DVec3::ZERO);
        }
        let (mut app, src, dest) = fixture();
        pick(&mut app, &[src]);
        let _ = app.dispatch_command("LAYMATCH");
        pick(&mut app, &[dest]);
        assert_circle(&app, src, "BLUE", DVec3::ZERO);
    }

    #[test]
    fn copytolayer_base_second_point_preview_move_and_one_undo() {
        let (mut app, src, dest, copy) = ready();
        let i = app.active_tab;
        let originals: Vec<_> = [src, dest]
            .iter()
            .map(|h| {
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap()
            })
            .collect();
        feed(&mut app, StepInput::Point(DVec3::new(2.0, 3.0, 0.0)));
        let cmd = app.tabs[i].active_cmd.as_mut().unwrap();
        assert_eq!(cmd.preview_hidden_handles(), &[copy]);
        let preview = cmd.on_preview_wires(DVec3::new(7.0, 9.0, 0.0));
        assert!(preview.len() > 1, "copied geometry plus COPY rubber band");
        feed(&mut app, StepInput::Point(DVec3::new(7.0, 9.0, 0.0)));
        assert_circle(&app, copy, "BLUE", DVec3::new(5.0, 6.0, 0.0));
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            app.tabs[i].history.undo_stack.len(),
            1,
            "copy + layer + move is one undo"
        );
        for (n, h) in [src, dest].iter().enumerate() {
            assert_eq!(
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap(),
                originals[n]
            );
        }
        app.undo_active_tab();
        assert!(copies(&app, &[src, dest]).is_empty());
        for (n, h) in [src, dest].iter().enumerate() {
            assert_eq!(
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap(),
                originals[n]
            );
        }
        app.redo_active_tab();
        assert_circle(&app, copy, "BLUE", DVec3::new(5.0, 6.0, 0.0));
    }

    #[test]
    fn copytolayer_displacement_coordinates_and_default() {
        for (text, delta) in [
            ("3,4", DVec3::new(3.0, 4.0, 0.0)),
            ("3,4,5", DVec3::new(3.0, 4.0, 5.0)),
            ("", DVec3::ZERO),
        ] {
            let (mut app, src, _, copy) = ready();
            type_token(&mut app, "D");
            type_token(&mut app, text);
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
            assert_circle(&app, copy, "BLUE", delta);
            assert_circle(&app, src, "RED", DVec3::ZERO);
            assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
        }
    }

    #[test]
    fn copytolayer_second_point_enter_uses_first_point_as_displacement() {
        let (mut app, src, _, copy) = ready();
        feed(&mut app, StepInput::Point(DVec3::new(3.0, 4.0, 5.0)));
        feed(&mut app, StepInput::Enter);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert_circle(&app, copy, "BLUE", DVec3::new(3.0, 4.0, 5.0));
        assert_circle(&app, src, "RED", DVec3::ZERO);
        assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
    }
}
