
    use super::{Message, OpenCADStudio};
    use crate::app::ModalKind;

    fn open_with_values(mode: i16, size: f64) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[app.active_tab]
            .scene
            .document
            .header
            .point_display_mode = mode;
        app.tabs[app.active_tab]
            .scene
            .document
            .header
            .point_display_size = size;
        app.begin_point_style_dialog();
        app
    }

    #[test]
    fn closing_point_style_discards_glyph_and_size_edits() {
        let mut app = open_with_values(2, 3.0);

        let _ = app.update(Message::PointStyleSetMode(3));
        let _ = app.update(Message::PointStyleSizeInput("8".into()));
        let _ = app.update(Message::PointStyleApplySize);
        let _ = app.update(Message::CloseModal);

        let header = &app.tabs[app.active_tab].scene.document.header;
        assert_eq!(header.point_display_mode, 2);
        assert_eq!(header.point_display_size, 3.0);
    }

    #[test]
    fn invalid_point_size_is_reported_and_does_not_close_or_write() {
        let mut app = open_with_values(2, 3.0);
        let old_error_revision = app.command_line.error_revision;

        let _ = app.update(Message::PointStyleSizeInput("abc".into()));
        let _ = app.update(Message::PointStyleApplySize);

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .point_display_size,
            3.0
        );
        assert_eq!(app.active_modal, Some(ModalKind::PointStyle));
        assert!(app.command_line.error_revision > old_error_revision);

        let _ = app.update(Message::PointStyleOk);
        assert_eq!(app.active_modal, Some(ModalKind::PointStyle));
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .point_display_mode,
            2
        );
    }

    #[test]
    fn accepting_point_style_commits_both_values_as_one_undo_step() {
        let mut app = open_with_values(2, 3.0);
        let i = app.active_tab;
        let undo_depth = app.tabs[i].history.undo_stack.len();

        let _ = app.update(Message::PointStyleSetMode(35));
        let _ = app.update(Message::PointStyleSizeInput("8".into()));
        let _ = app.update(Message::PointStyleOk);

        let header = &app.tabs[i].scene.document.header;
        assert_eq!(header.point_display_mode, 35);
        assert_eq!(header.point_display_size, 8.0);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_depth + 1);
        assert!(app.tabs[i].dirty);
        assert_eq!(app.active_modal, None);
    }

    #[test]
    fn accepting_relative_point_size_preserves_the_relative_sign() {
        let mut app = open_with_values(2, 3.0);

        let _ = app.update(Message::PointStyleSizeRelative(true));
        let _ = app.update(Message::PointStyleSizeInput("5".into()));
        let _ = app.update(Message::PointStyleOk);

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .point_display_size,
            -5.0
        );
    }
