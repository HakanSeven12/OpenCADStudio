
    use super::Message;
    use crate::app::OpenCADStudio;
    use crate::ui::dock::PanelId;

    fn drawing_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn hidden_properties_panel_skips_the_focus_sweep() {
        // A click while the panel is closed must not run the widget-tree sweep:
        // the returned task is a bare `Task::none` (units == 0).
        let mut app = drawing_app();
        assert!(app.dock_panel_visible(PanelId::Properties));
        app.show_properties = false;
        assert!(!app.dock_panel_visible(PanelId::Properties));

        let task = app.update(Message::PropPointerPressed);
        assert_eq!(task.units(), 0);
    }

    #[test]
    fn visible_properties_panel_runs_the_focus_sweep() {
        // A click while the panel is open must still fire the sweep (units > 0)
        // so the select-whole-value feature keeps working.
        let mut app = drawing_app();
        app.show_properties = true;
        assert!(app.dock_panel_visible(PanelId::Properties));

        let task = app.update(Message::PropPointerPressed);
        assert!(task.units() > 0);
    }
