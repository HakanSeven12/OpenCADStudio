#[cfg(test)]
mod layer_and_style_commands {
    use crate::app::OpenCADStudio;

    fn out_text(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn add_layer(app: &mut OpenCADStudio, name: &str, color: document::types::Color) -> document::Handle {
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let mut layer = document::tables::Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = color;
        let handle = layer.handle;
        doc.layers.add(layer).unwrap();
        handle
    }

    /// Add a layout (with its page setup) and a viewport to it; call again
    /// with the same name to add another viewport to that layout.
    fn add_viewport(app: &mut OpenCADStudio, layout: &str) -> document::Handle {
        let i = app.active_tab;
        if app.tabs[i].scene.add_layout(layout).is_ok() {
            app.tabs[i].scene.initialize_layout_page_setup(layout);
        }
        let next_id = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Viewport(vp) => Some(vp.id),
                _ => None,
            })
            .max()
            .map(|id| id + 1)
            .unwrap_or(2);
        let mut viewport = document::entities::Viewport::new();
        viewport.center = document::types::Vector3::new(0.0, 0.0, 0.0);
        viewport.width = 200.0;
        viewport.height = 150.0;
        viewport.id = next_id;
        viewport.status = document::entities::ViewportStatusFlags::default_on();
        app.tabs[i]
            .scene
            .document
            .add_entity_to_layout(document::EntityType::Viewport(Box::new(viewport)), layout)
            .expect("the layout accepts a viewport")
    }

    #[test]
    fn vplayer_sets_colour_linetype_lineweight_and_alpha_overrides() {
        use document::objects::KnownXRecordKind as Kind;
        type Channel = crate::command::VpOverrideChannel;

        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet1");
        let other_viewport = add_viewport(&mut app, "Sheet1");

        // COLOR
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Color, "3");
        let color_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportColorOverride,
        );
        assert_eq!(color_overrides, vec![(viewport, document::objects::XRecordValue::Int32(0xC300_0003u32 as i32))]);
        assert!(
            !color_overrides.iter().any(|(handle, _)| *handle == other_viewport),
            "viewport 2 must not carry the override: {color_overrides:?}"
        );

        // LINETYPE (dashed ships in the default linetypes)
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Linetype, "DASHED");
        let line_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportLinetypeOverride,
        );
        let lt_handle = line_overrides.iter().next().cloned();
        assert!(lt_handle.is_some(), "{line_overrides:?}");

        // LINEWEIGHT
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Lineweight, "0.53");
        let lw_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportLineweightOverride,
        );
        assert!(lw_overrides
            .iter()
            .any(|(handle, value)| *handle == viewport && *value == document::objects::XRecordValue::Int32(53)));

        // ALPHA (77% transparent)
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Alpha, "77");
        let alpha_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportAlphaOverride,
        );
        assert!(alpha_overrides.iter().any(|(handle, _)| *handle == viewport));
    }

    #[test]
    fn vplayer_rejects_a_bad_value_and_changes_nothing() {
        type Channel = crate::command::VpOverrideChannel;

        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet2");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Color, "not-a-color");
        let color_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportColorOverride,
        );
        assert!(color_overrides.is_empty(), "{color_overrides:?}");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Linetype, "NoSuchLinetype");
        let lt_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
        );
        assert!(lt_overrides.is_empty(), "{lt_overrides:?}");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Alpha, "99");
        let alpha_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportAlphaOverride,
        );
        assert!(alpha_overrides.is_empty(), "{alpha_overrides:?}");
    }

    #[test]
    fn vplayer_freeze_stays_in_the_viewport_and_leaves_the_layer_thawed() {
        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet3");
        let other_viewport = add_viewport(&mut app, "Sheet3");

        app.handle_vp_layer_update(viewport, vec!["WALLS".into()], vec![]);

        // VP Freeze is a separate property: the global layer stays thawed.
        let global = app
            .tabs[i]
            .scene
            .document
            .layers
            .get("WALLS")
            .expect("the layer exists");
        assert!(!global.flags.frozen, "the layer itself must stay thawed");
        let frozen_of = |app: &OpenCADStudio, handle: document::Handle| match app.tabs[i].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp.frozen_layers.clone(),
            other => panic!("expected a viewport, got {other:?}"),
        };
        assert!(
            frozen_of(&app, viewport).contains(&walls),
            "viewport 1 carries the VP freeze"
        );
        assert!(
            !frozen_of(&app, other_viewport).contains(&walls),
            "viewport 2 is unchanged"
        );

        // Thawing takes the freeze out of the viewport again.
        app.handle_vp_layer_update(viewport, vec![], vec!["WALLS".into()]);
        assert!(
            !frozen_of(&app, viewport).contains(&walls),
            "viewport 1 is thawed again"
        );
    }

    #[test]
    fn dimstyle_variables_dumps_the_variable_table() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25"]"#);
        let out = out_text(&app);
        // The history holds 64 entries, so the listing arrives as a suffix of
        // the 79-line dump; the complete content is checked in the module's
        // unit tests. Here the trailing rows must be there, aligned, with
        // current values.
        let rows = crate::app::dim_variables::DIM_VARIABLES
            .iter()
            .filter(|variable| out.contains(variable.name))
            .count();
        assert!(rows >= 60, "expected the tail of the table, got {rows}: {out}");
        let line = out
            .lines()
            .find(|line| line.trim_start().starts_with("DIMTXT"))
            .expect("the DIMTXT row prints");
        assert!(line.contains("2.5"), "{line}");
        let line = out
            .lines()
            .find(|line| line.trim_start().starts_with("DIMASZ"))
            .expect("the DIMASZ row prints");
        assert!(line.contains("2.5"), "{line}");
        assert!(
            out.contains("Dimension text height"),
            "descriptions print: {out}"
        );
    }

    #[test]
    fn dimstyle_create_with_variables_set_current_marks_new_dimensions() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-1-50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "SHEET-1-50", "DIMTXT", "2.5"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "SHEET-1-50"]"#);
        let out = out_text(&app);
        assert!(out.contains("SHEET-1-50"), "{out}");
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .header
                .current_dimstyle_name,
            "SHEET-1-50"
        );

        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let dimension = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension),
                _ => None,
            })
            .expect("the DIMLINEAR command placed a dimension");
        assert_eq!(
            dimension.base().style_name,
            "SHEET-1-50",
            "the new dimension carries the new style"
        );
    }

    #[test]
    fn blocks_palette_menu_renames_a_definition_and_rekeys_its_references() {
        use crate::app::Message;
        use crate::ui::window::block_palette::{BlockPaletteMsg, Item, MenuAction};

        let mut app = new_app();
        let i = app.active_tab;
        {
            let doc = &mut app.tabs[i].scene.document;
            let mut record = document::tables::BlockRecord::new("VALVE");
            record.handle = doc.allocate_handle();
            let owner = record.handle;
            doc.block_records.add(record).unwrap();
            let mut line = document::entities::Line::new();
            line.start = document::types::Vector3::ZERO;
            line.end = document::types::Vector3::new(10.0, 0.0, 0.0);
            line.common.owner_handle = owner;
            let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();
            doc.block_records.get_mut("VALVE").unwrap().entity_handles = vec![line_handle];
            let insert = document::entities::Insert::new(
                "VALVE",
                document::types::Vector3::new(5.0, 5.0, 0.0),
            );
            let _ = doc.add_entity(document::EntityType::Insert(insert)).unwrap();
        }

        // Right-click the current-drawing entry and choose Rename Block.
        let _ = app.update(Message::BlockPalette(BlockPaletteMsg::Menu(
            Item::Current("VALVE".into()),
            MenuAction::Rename,
        )));
        let out = out_text(&app);
        assert!(out.contains("Enter new block name"), "{out}");

        // Type the new name; nothing happens until it is submitted.
        let _ = app.feed_active_cmd("GATE-VALVE");
        let out = out_text(&app);
        assert!(out.contains("RENAME"), "{out}");
        let doc = &app.tabs[i].scene.document;
        assert!(doc.block_records.get("VALVE").is_none(), "old name is gone");
        assert!(
            doc.block_records.get("GATE-VALVE").is_some(),
            "the new name exists in the table"
        );
        let insert = doc
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert),
                _ => None,
            })
            .expect("the reference exists");
        assert_eq!(insert.block_name, "GATE-VALVE", "the reference follows");
    }

    #[test]
    fn blocks_palette_rename_cancelled_with_empty_input_changes_nothing() {
        use crate::app::Message;
        use crate::ui::window::block_palette::{BlockPaletteMsg, Item, MenuAction};

        let mut app = new_app();
        let i = app.active_tab;
        {
            let doc = &mut app.tabs[i].scene.document;
            let mut record = document::tables::BlockRecord::new("VALVE");
            record.handle = doc.allocate_handle();
            doc.block_records.add(record).unwrap();
        }

        let _ = app.update(Message::BlockPalette(BlockPaletteMsg::Menu(
            Item::Current("VALVE".into()),
            MenuAction::Rename,
        )));
        assert!(app.tabs[i].active_cmd.is_some(), "the rename command starts");
        let _ = app.feed_active_cmd("");
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .contains("VALVE"),
            "the empty input leaves the definition alone"
        );
    }

    #[test]
    fn dimstyle_compare_reports_all_differing_variables() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-A"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "SHEET-A", "DIMTXT", "9.0"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "SHEET-A"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMTXT"), "{out}");

        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "ISO-25"]"#);
        let out = out_text(&app);
        assert!(out.contains("identical"), "{out}");
    }

    #[test]
    fn dimstyle_variables_sets_dimtxt_to_3_and_keeps_it() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "3.0"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMTXT = 3"), "{out}");
        let style = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .expect("ISO-25 exists");
        assert_eq!(style.dimtxt, 3.0);
    }

    #[test]
    fn dimstyle_variables_rejects_an_out_of_range_value_and_keeps_the_style() {
        let mut app = new_app();
        let i = app.active_tab;
        let before = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap()
            .dimtxt;
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "-1"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMSTYLE"), "{out}");
        assert!(out.contains("must be greater than 0"), "{out}");
        let after = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap()
            .dimtxt;
        assert_eq!(before, after, "the refused value is not written");

        // And a boolean keyword with a wrong kind of value is refused too.
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTOL", "banana"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMSTYLE"), "{out}");
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .dim_styles
                .get("ISO-25")
                .unwrap()
                .dimtol,
            false,
            "the refused value is not written"
        );
    }

    #[test]
    fn dimstyle_rename_and_delete_through_the_command_line() {
        let mut app = new_app();
        let i = app.active_tab;
        // Create a style from the current one, rename it, restyle a
        // dimension to it, and delete it after deleting the dimension.
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-1-50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["RENAME", "SHEET-1-50", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("SHEET_1_50"), "{out}");
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .get("SHEET-1-50")
            .is_none());
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"));

        // Set it current, confirm it is current, then delete it.
        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "SHEET_1_50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["DELETE", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("current style"), "{out}");
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"), "the current style is not deleted");

        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "ISO-25"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["DELETE", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("deleted"), "{out}");
        assert!(!app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"));
    }
}
