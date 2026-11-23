#[cfg(test)]
mod insert_and_dimstyle_commands {
    use crate::app::OpenCADStudio;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn classic_insert_advertises_its_point_keywords_as_structured_options() {
        use crate::command::CadCommand;
        use crate::modules::insert::insert_block::InsertBlockCommand;

        let command = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let options = command.options();
        let keywords: Vec<&str> = options.iter().map(|option| option.keyword.as_str()).collect();
        assert_eq!(keywords, ["B", "S", "X", "Y", "Z", "R", "E", "RE"]);
        let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
        assert!(labels.contains(&"Basepoint"));
        assert!(labels.contains(&"Scale"));
        assert!(labels.contains(&"Rotate"));
        assert!(labels.contains(&"Explode"));
        assert!(labels.contains(&"Repeat"));

        let scale_options = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let step = {
            // Reach the ScaleX step: choose a point, then answer the prompts.
            let mut command = scale_options;
            let _ = command.on_point(glam::DVec3::ZERO);
            command
        };
        assert_eq!(step.options().len(), 2);
    }
    fn out(app: &OpenCADStudio) -> String {
        app.command_line
            .history_plain_text()
            .trim()
            .to_string()
    }

    #[test]
    fn dimstyle_variables_accepts_a_write_and_keeps_it() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "4.5"]"#);
        let out = out(&app);
        assert!(out.contains("DIMTXT = 4.5"), "{out}");
        let doc = &app.tabs[app.active_tab].scene.document;
        let style = doc.dim_styles.get("ISO-25").expect("ISO-25 exists");
        assert_eq!(style.dimtxt, 4.5);
    }

    #[test]
    fn dimstyle_compare_reports_the_differing_variables() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "Standard"]"#);
        let out = out(&app);
        assert!(out.contains("DIMTXT"), "{out}");
        assert!(out.contains("DIMASZ"), "{out}");
    }

    #[test]
    fn associative_array_honours_the_base_point() {

        let mut app = new_app();
        let before: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .find(|handle| !before.contains(handle))
            .unwrap();
        app.handle_assoc_array(
            vec![line],
            crate::scene::assoc_array::AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
            Some(glam::DVec3::new(50.0, 50.0, 0.0)),
        );
        let inserts: Vec<document::types::Vector3> = app
            .tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.insert_point),
                _ => None,
            })
            .collect();
        assert!(
            inserts.contains(&document::types::Vector3::new(50.0, 50.0, 0.0)),
            "the array insert is at the named base point: {inserts:?}"
        );

        let before: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .find(|handle| !before.contains(handle))
            .unwrap();
        app.handle_assoc_array(
            vec![line],
            crate::scene::assoc_array::AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
            None,
        );
        let inserts: Vec<document::types::Vector3> = app
            .tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.insert_point),
                _ => None,
            })
            .collect();
        assert!(
            inserts.contains(&document::types::Vector3::new(0.0, 0.0, 0.0)),
            "the default anchors at the source's bounding corner: {inserts:?}"
        );
    }

    #[test]
    fn setbylayer_reset_is_scriptable() {

        let mut app = new_app();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .next()
            .unwrap()
            .common()
            .handle;
        // An empty selection is reported, not silently ignored.
        let empty = app.automation_op(r#"{"op":"run","cmd":"-SETBYLAYER"}"#);
        let text = out(&app);
        assert!(text.contains("SETBYLAYER"), "{empty}: {text}");

        let _ = app.automation_op(r#"{"op":"run","cmd":"SELECTALL"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"CHPROP COLOR 1"}"#);
        let pre = app.tabs[i].scene.document.get_entity(line).unwrap();
        assert_eq!(pre.common().color, document::types::Color::Index(1));

        let _ = app.automation_op(r#"{"op":"run","cmd":"-SETBYLAYER"}"#);
        let post = app.tabs[i].scene.document.get_entity(line).unwrap();
        assert_eq!(post.common().color, document::types::Color::ByLayer);
        let text = out(&app);
        assert!(text.contains("reset 1 entity"), "{text}");
    }
}
