
    use super::Message;
    use crate::app::OpenCADStudio;

    fn app_with_three_tabs() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        for _ in 0..3 {
            let _ = app.update(Message::TabNew);
        }
        app
    }

    #[test]
    fn closing_a_tab_closes_its_layouts_list() {
        let mut app = app_with_three_tabs();
        let last = app.tabs.len() - 1;
        app.doc_tab_layouts = Some(last);
        let id = app.tabs[last - 1].id;
        let _ = app.update(Message::TabClose(id));
        assert_eq!(app.doc_tab_layouts, None);
    }

    #[test]
    fn moving_a_tab_closes_the_layouts_list() {
        let mut app = app_with_three_tabs();
        let last = app.tabs.len() - 1;
        app.doc_tab_layouts = Some(last);
        let _ = app.update(Message::TabReorder {
            from: last,
            to: last - 1,
            after: false,
        });
        assert_eq!(app.doc_tab_layouts, None);
    }
