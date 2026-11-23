
    // #57: one visibility predicate for edge rendering and drag math. Pins
    // the full 8-panel table so a newly added panel cannot silently diverge
    // (behavior-preserving dedup: passes before and after by construction).
    use crate::app::OpenCADStudio;
    use crate::ui::dock::PanelId;

    const ALL: [PanelId; 8] = [
        PanelId::Properties,
        PanelId::BlockPalette,
        PanelId::ExternalReferences,
        PanelId::Browser,
        PanelId::NodeGraph,
        PanelId::PointCloudManager,
        PanelId::Count,
        PanelId::SheetSetManager,
    ];

    #[test]
    fn dock_panel_visible_covers_every_panel() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.show_properties = true;
        app.show_block_palette = true;
        app.show_external_references = true;
        app.show_browser = true;
        app.show_node_graph = true;
        app.pc_manager.show = true;
        app.count_palette.show = true;
        app.sheet_set.show = true;
        for id in ALL {
            assert!(app.dock_panel_visible(id), "{id:?} must be visible");
        }
        app.clean_screen = true;
        for id in ALL {
            assert!(
                !app.dock_panel_visible(id),
                "{id:?} must hide on clean screen"
            );
        }
    }

    #[test]
    fn dock_panel_hidden_on_start_tab() {
        // Fresh app, no drawing opened: the welcome tab shows no panels.
        let app = OpenCADStudio::new_for_test();
        assert!(app.tabs[app.active_tab].is_start);
        for id in ALL {
            assert!(!app.dock_panel_visible(id), "{id:?} must hide on Start");
        }
    }
