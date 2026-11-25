#[cfg(test)]
mod layer_filter_tests {
    use crate::app::OpenCADStudio;

    fn app_with_layers() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for (name, color) in [("DIMS-A", 1), ("DIMS-B", 7), ("WALLS", 1)] {
            let _ = app.run_command_line(&format!("-LAYER NEW {name}"));
            let _ = app.run_command_line(&format!("-LAYER COLOR {color} {name}"));
        }
        app
    }

    fn matching(app: &OpenCADStudio, filter: &str) -> Vec<String> {
        let filter = app
            .layer_filters
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(filter))
            .expect("filter");
        let mut names: Vec<String> = app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .iter()
            .filter(|layer| filter.matches(layer))
            .map(|layer| layer.name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn headless_run_reaches_the_filter_command() {
        let mut app = app_with_layers();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LAYERFILTER NEW DIMS DIMS*"}"#);
        println!("DBG reply={reply}");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A", "DIMS-B"]);
    }

    /// #2610 / #2611: layer filters match on the shared filter engine, so a
    /// name takes the same patterns as Quick Select and FILTER.
    #[test]
    fn layer_filter_names_take_the_shared_patterns() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW SOME DIMS-A,WALLS");
        assert_eq!(matching(&app, "SOME"), ["DIMS-A", "WALLS"]);
        let _ = app.run_command_line("LAYERFILTER NEW OTHERS ~DIMS*");
        assert!(matching(&app, "OTHERS").contains(&"WALLS".to_string()));
        assert!(!matching(&app, "OTHERS")
            .iter()
            .any(|name| name.starts_with("DIMS")));
        let _ = app.run_command_line("-LAYER NEW A1");
        let _ = app.run_command_line("LAYERFILTER NEW NUMBERED A#");
        assert_eq!(matching(&app, "NUMBERED"), ["A1"]);
        let _ = app.run_command_line("LAYERFILTER SET SOME COLOR 1");
        assert_eq!(matching(&app, "SOME"), ["DIMS-A", "WALLS"], "both are red");
    }

    #[test]
    fn name_wildcard_filter_selects_the_discipline() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A", "DIMS-B"]);
    }

    #[test]
    fn colour_predicate_narrows_the_filter_result() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("LAYERFILTER SET DIMS COLOR 1");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A"]);
    }

    #[test]
    fn apply_turns_off_every_layer_in_the_filter() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("LAYERFILTER APPLY DIMS OFF");

        let layers = &app.tabs[app.active_tab].scene.document.layers;
        assert!(layers.get("DIMS-A").unwrap().is_off());
        assert!(layers.get("DIMS-B").unwrap().is_off());
        assert!(!layers.get("WALLS").unwrap().is_off());
    }

    #[test]
    fn dash_layer_accepts_a_filter_as_its_name_list() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("-LAYER OFF FILTER:DIMS");

        let layers = &app.tabs[app.active_tab].scene.document.layers;
        assert!(layers.get("DIMS-A").unwrap().is_off());
        assert!(layers.get("DIMS-B").unwrap().is_off());
        assert!(!layers.get("WALLS").unwrap().is_off());
    }
}
