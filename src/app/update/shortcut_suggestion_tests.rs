
    use super::Message;
    use crate::app::OpenCADStudio;
    use crate::ui::window::shortcuts::command_input_id;
    use iced::futures::StreamExt;
    use iced::widget::text_input::{cursor, Value};

    type Paragraph = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

    #[test]
    fn picking_a_suggestion_puts_the_caret_after_the_name() {
        let mut app = OpenCADStudio::new_for_test();
        app.shortcut_editor_rows = vec![
            ("CTRL+L".to_owned(), "LIN".to_owned()),
            ("CTRL+D".to_owned(), "DI".to_owned()),
        ];
        app.shortcut_suggest = Some((0, "LIN".to_owned()));
        // Row 0's box after typing "lin": caret after the third character,
        // focus lost to the list click. Row 1's box must stay untouched.
        let mut picked = iced::widget::text_input::State::<Paragraph>::new();
        picked.move_cursor_to(3);
        let mut other = iced::widget::text_input::State::<Paragraph>::new();
        other.move_cursor_to(1);

        let task = app.update(Message::ShortcutEditorSuggestionPick {
            idx: 0,
            command: "LINE".to_owned(),
        });
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = iced::futures::executor::block_on(stream.next()) {
                if let iced_runtime::Action::Widget(mut operation) = action {
                    for (idx, state) in [(0, &mut picked), (1, &mut other)] {
                        let id = command_input_id(idx);
                        operation.focusable(Some(&id), iced::Rectangle::default(), state);
                        operation.text_input(Some(&id), iced::Rectangle::default(), state);
                    }
                }
            }
        }

        assert_eq!(app.shortcut_editor_rows[0].1, "LINE");
        assert_eq!(app.shortcut_suggest, None);
        assert_eq!(
            picked.cursor().state(&Value::new("LINE")),
            cursor::State::Index(4),
            "the caret must follow the picked name, not the typed prefix"
        );
        assert!(picked.is_focused(), "typing must go on in the picked box");
        assert_eq!(
            other.cursor().state(&Value::new("DI")),
            cursor::State::Index(1)
        );
        assert!(!other.is_focused());
    }
