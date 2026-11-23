
    // #59: silent no-op toggles must explain themselves on the command line.
    use super::Message;
    use crate::app::OpenCADStudio;

    fn app_with_target() -> (OpenCADStudio, usize) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::ZERO;
        line.end = document::types::Vector3::new(1.0, 0.0, 0.0);
        let h = app
            .commit_entity_handle(document::EntityType::Line(line))
            .expect("line commits");
        app.anno_object_scale_target = Some(h);
        (app, i)
    }

    #[test]
    fn locked_layer_toggle_explains_itself() {
        let (mut app, i) = app_with_target();
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut("0")
            .expect("layer 0 exists")
            .flags
            .locked = true;
        let before = app.command_line.history.len();
        let _ = app.update(Message::AnnoObjectScaleToggle("1:50".to_string()));
        let new: Vec<String> = app.command_line.history[before..]
            .iter()
            .map(|e| e.text.clone())
            .collect();
        assert!(
            new.iter().any(|t| t.to_lowercase().contains("locked")),
            "locked-layer toggle must say so, got {new:?}"
        );
    }

    #[test]
    fn unknown_scale_toggle_explains_itself() {
        let (mut app, _i) = app_with_target();
        let before = app.command_line.history.len();
        let _ = app.update(Message::AnnoObjectScaleToggle("NOPE".to_string()));
        let new: Vec<String> = app.command_line.history[before..]
            .iter()
            .map(|e| e.text.clone())
            .collect();
        assert!(
            !new.is_empty(),
            "unknown-scale toggle must say something, got silence"
        );
    }

    #[test]
    fn missing_target_toggle_explains_itself() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.anno_object_scale_target = None;
        let before = app.command_line.history.len();
        let _ = app.update(Message::AnnoObjectScaleToggle("1:50".to_string()));
        let new: Vec<String> = app.command_line.history[before..]
            .iter()
            .map(|e| e.text.clone())
            .collect();
        assert!(
            !new.is_empty(),
            "target-less toggle must say something, got silence"
        );
    }
