#[cfg(test)]
mod copy_and_viewport_commands {
    use crate::app::OpenCADStudio;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn lines(app: &mut OpenCADStudio) -> Vec<(f64, f64, f64, f64)> {
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["start"][0].as_f64().unwrap(),
                    e["start"][1].as_f64().unwrap(),
                    e["end"][0].as_f64().unwrap(),
                    e["end"][1].as_f64().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn scale_copy_keeps_the_source_and_adds_a_scaled_duplicate() {
        let mut app = new_app();
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("SCALE 0,0 C 2");
        let mut got = lines(&mut app);
        got.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        assert_eq!(got.len(), 2, "{got:?}");
        assert!((got[0].2 - 10.0).abs() < 1e-9, "{got:?}");
        assert!((got[1].2 - 20.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn rotate_copy_keeps_the_source_and_adds_a_rotated_duplicate() {
        let mut app = new_app();
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("ROTATE 0,0 C 90");
        let mut got = lines(&mut app);
        got.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            (got[0].2 - 0.0).abs() < 1e-6 && (got[0].3 - 10.0).abs() < 1e-6,
            "{got:?}"
        );
        assert!((got[1].2 - 10.0).abs() < 1e-6, "{got:?}");
    }

    fn out(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn enter_layout(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let name = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.set_current_layout(name);
        app.tabs[i].scene.load_current_layout_state();
    }

    fn layout_viewport_handles(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    fn vp(app: &OpenCADStudio, handle: document::Handle) -> &document::entities::Viewport {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
            _ => panic!("viewport missing"),
        }
    }

    fn effective_scale(app: &OpenCADStudio, handle: document::Handle) -> f64 {
        let vp = vp(app, handle);
        crate::scene::vp_effective_scale(vp.custom_scale, vp.view_height, vp.height)
    }

    #[test]
    fn vpscale_sets_the_viewport_plot_scale_and_reports_it() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let _ = app.feed_active_cmd("F");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));

        assert!(app.command_line.last_error.is_none());
        let _ = app.run_command_line("VPSCALE 1:50");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let eff = effective_scale(&app, handles[0]);
        // VPSCALE is a physical sheet scale: metre drawings require
        // 1000 / 50 paper units per model unit, not the raw 1/50.
        let expected = 0.02 * app.tabs[app.active_tab].scene.default_scale_unit_factor();
        assert!(
            (eff - expected).abs() < 1e-9,
            "the viewport is not at 1:50 (got {eff}, expected {expected})"
        );

        let text = out(&app);
        assert!(text.contains("VPSCALE"), "{text}");

        assert!(app.command_line.last_error.is_none());
        let _ = app.run_command_line("VPSCALE ?");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let text = out(&app);
        assert!(text.contains("1:50"), "{text}");
    }

    /// A ratio is a sheet scale whether or not the scale list names it: in a
    /// metre drawing 1:40 and 1:750 sit beside 1:50, not a thousand times
    /// away. A bare number stays the paper / model factor `ZOOM nXP` takes.
    #[test]
    fn vpscale_takes_a_ratio_the_list_does_not_name_as_a_sheet_scale_too() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let _ = app.feed_active_cmd("F");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));
        let i = app.active_tab;
        app.tabs[i].scene.document.header.insertion_units = 6; // metres
        let unit = app.tabs[i].scene.default_scale_unit_factor();
        assert_eq!(unit, 1000.0, "millimetres of paper to a metre");

        for (text, ratio) in [
            ("1:50", 1.0 / 50.0),
            ("1:40", 1.0 / 40.0),
            ("1:750", 1.0 / 750.0),
            ("2:3", 2.0 / 3.0),
        ] {
            let _ = app.run_command_line(&format!("VPSCALE {text}"));
            assert!(app.command_line.last_error.is_none(), "{}", out(&app));
            let eff = effective_scale(&app, handles[0]);
            let expected = ratio * unit;
            assert!(
                (eff - expected).abs() < 1e-9 * expected,
                "VPSCALE {text}: the viewport is at {eff}, a sheet scale of {text} is {expected}"
            );
        }

        let _ = app.run_command_line("VPSCALE 0.02");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let eff = effective_scale(&app, handles[0]);
        assert!(
            (eff - 0.02).abs() < 1e-12,
            "a bare factor is paper units to a model unit, got {eff}"
        );
    }

    /// A drawing in a unit that the units picker does not offer has a size
    /// too: its sheet scales use it. US survey feet go with inches of paper,
    /// decimeters with millimetres.
    #[test]
    fn vpscale_is_a_sheet_scale_in_a_unit_the_picker_does_not_offer() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let _ = app.feed_active_cmd("F");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));
        let i = app.active_tab;

        let survey_foot_in_inches = 1_200_000.0 / 3_937.0 / 25.4;
        for (code, unit_name, expected_unit) in [
            (21, "US survey feet", survey_foot_in_inches),
            (14, "decimeters", 100.0),
        ] {
            app.tabs[i].scene.document.header.insertion_units = code;
            let unit = app.tabs[i].scene.default_scale_unit_factor();
            assert!(
                (unit - expected_unit).abs() < 1e-9 * expected_unit,
                "{unit_name}: one drawing unit is {unit} paper units, expected {expected_unit}"
            );

            let _ = app.run_command_line("VPSCALE 1:50");
            assert!(app.command_line.last_error.is_none(), "{}", out(&app));
            let eff = effective_scale(&app, handles[0]);
            let expected = expected_unit / 50.0;
            assert!(
                (eff - expected).abs() < 1e-9 * expected,
                "{unit_name}: VPSCALE 1:50 gives {eff}, a sheet scale of 1:50 is {expected}"
            );
        }
    }

    /// DWGUNITS converts from a unit that the picker does not offer: a line
    /// of 10 US survey feet is 3.048006 m long, and the drawing says metres.
    #[test]
    fn dwgunits_converts_a_drawing_in_us_survey_feet() {
        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].scene.document.header.insertion_units = 21;
        let _ = app.run_command_line("LINE 0,0 10,0 ");
        assert_eq!(
            lines(&mut app),
            vec![(0.0, 0.0, 10.0, 0.0)],
            "{}",
            out(&app)
        );

        let _ = app.run_command_line("DWGUNITS m");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        assert_eq!(app.tabs[i].scene.document.header.insertion_units, 6);
        let converted = lines(&mut app);
        let expected = 10.0 * 1_200.0 / 3_937.0;
        assert!(
            (converted[0].2 - expected).abs() < 1e-9,
            "the line ends at x = {}, expected {expected}\n{}",
            converted[0].2,
            out(&app)
        );
        let text = out(&app);
        assert!(text.contains("US Survey Feet"), "{text}");
    }

    #[test]
    fn vp_lock_toggles_the_viewport_display_lock() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let _ = app.feed_active_cmd("F");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));

        let _ = app.run_command_line("VPLOCK ON");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(vp(&app, handles[0]).status.locked);

        let _ = app.run_command_line("VPLOCK TOGGLE");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(!vp(&app, handles[0]).status.locked);

        let _ = app.run_command_line("VPLOCK OFF");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(!vp(&app, handles[0]).status.locked);
    }

    #[test]
    fn vpscale_in_model_space_is_refused() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("VPSCALE 1:50");
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("switch to a paper layout first"),
            "{:?}",
            app.command_line.last_error
        );
    }

    #[test]
    fn objectscale_adds_an_explicit_scale_and_delete_removes_it() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("TEXT 0,0 2 0 hello");
        let _ = app.run_command_line("SELECTALL");
        let i = app.active_tab;
        let handle = app.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .next()
            .expect("the text is selected");

        let _ = app.run_command_line("OBJECTSCALE ADD 1:100");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let after_add =
            crate::scene::annotative::object_scale_memberships(&app.tabs[i].scene.document, handle);
        assert!(
            after_add.iter().any(|(name, _)| name == "1:100"),
            "{after_add:?}"
        );

        let _ = app.run_command_line("OBJECTSCALE DELETE 1:100");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let after_del =
            crate::scene::annotative::object_scale_memberships(&app.tabs[i].scene.document, handle);
        assert!(
            !after_del.iter().any(|(name, _)| name == "1:100"),
            "{after_del:?}"
        );
    }

    #[test]
    fn tablestyle_set_applies_an_existing_style() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("TABLESTYLE NEW MYPAPER");
        let _ = app.run_command_line("TABLESTYLE SET MYPAPER");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let current = app.tabs[app.active_tab]
            .scene
            .document
            .header
            .current_table_style_name
            .clone();
        assert_eq!(current, "MYPAPER");
    }

    fn model_lines(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.common.handle),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn group_rename_add_and_remove_edit_an_existing_group() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 10,5");
        let _ = app.run_command_line("LINE 0,9 10,9");
        let i = app.active_tab;
        let lines = model_lines(&app);
        assert_eq!(lines.len(), 3, "{lines:?}");
        app.tabs[i]
            .scene
            .create_group("MYGROUP".to_string(), vec![lines[0], lines[1]]);

        let _ = app.run_command_line("GROUP RENAME MYGROUP TEAM");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let names: Vec<String> = app.tabs[i].scene.groups().map(|g| g.name.clone()).collect();
        assert_eq!(names, vec!["TEAM".to_string()], "{names:?}");
        let renamed = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the renamed group");
        assert_eq!(renamed.entities.len(), 2, "{:?}", renamed.entities);
        let keyed = app.tabs[i]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                document::objects::ObjectType::Dictionary(dict) => dict
                    .entries
                    .iter()
                    .find(|(key, _)| key == "TEAM")
                    .map(|_| ()),
                _ => None,
            });
        assert!(keyed.is_some(), "the group dictionary lost the new name");

        let add = format!("GROUP ADD TEAM {:X}", lines[2].value());
        let _ = app.run_command_line(&add);
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let group = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the group");
        assert!(group.contains(lines[2]), "{:?}", group.entities);

        let remove = format!("GROUP REMOVE TEAM {:X}", lines[0].value());
        let _ = app.run_command_line(&remove);
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let group = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the group");
        assert!(!group.contains(lines[0]), "{:?}", group.entities);
        assert!(group.contains(lines[1]), "{:?}", group.entities);

        let _ = app.run_command_line("GROUP RENAME NOSUCH OTHER");
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("no group named"),
            "{:?}",
            app.command_line.last_error
        );
    }

    /// #2805: GROUPEDIT picks a group by a member, takes members out one
    /// pick at a time, adds a selection and renames, each one undo step.
    #[test]
    fn groupedit_removes_adds_and_renames() {
        use crate::app::Message;
        use crate::command::StepInput;
        use glam::DVec3;
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 10,5");
        let _ = app.run_command_line("LINE 0,9 10,9");
        let lines = model_lines(&app);
        app.tabs[i]
            .scene
            .create_group("G".to_string(), vec![lines[0], lines[1]]);
        let members = |app: &OpenCADStudio| {
            app.tabs[i]
                .scene
                .groups()
                .map(|g| (g.name.clone(), g.entities.clone()))
                .collect::<Vec<_>>()
        };

        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::EntityPick(lines[1], DVec3::ZERO));
        let _ = app.feed_command(StepInput::Text("R".into()));
        let _ = app.feed_command(StepInput::EntityPick(lines[0], DVec3::ZERO));
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(members(&app), vec![("G".to_string(), vec![lines[1]])]);

        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::Text("N".into()));
        let _ = app.feed_command(StepInput::Text("G".into()));
        let _ = app.feed_command(StepInput::Text("A".into()));
        let _ = app.feed_command(StepInput::SelectionComplete(vec![lines[2]]));
        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::Text("N".into()));
        let _ = app.feed_command(StepInput::Text("G".into()));
        let _ = app.feed_command(StepInput::Text("REN".into()));
        let _ = app.feed_command(StepInput::Text("H".into()));
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        assert_eq!(members(&app), vec![("H".to_string(), vec![lines[1], lines[2]])]);

        let _ = app.update(Message::Undo);
        assert_eq!(members(&app), vec![("G".to_string(), vec![lines[1], lines[2]])]);
    }

    /// UNGROUP on a group picked whole drops its box and centre grip at
    /// once, not at the next click.
    #[test]
    fn ungroup_clears_the_group_box_and_grip() {
        crate::app::commands::set_group_display_mode(2);
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 10,5");
        let lines = model_lines(&app);
        let group_grips = |app: &OpenCADStudio| {
            app.tabs[i]
                .selected_grips
                .iter()
                .filter(|grip| grip.id == crate::scene::GROUP_GRIP_ID)
                .count()
        };
        app.tabs[i]
            .scene
            .create_group("G".to_string(), vec![lines[0], lines[1]]);
        app.tabs[i].scene.select_entities(&lines);
        app.refresh_selected_grips();
        assert_eq!(app.tabs[i].group_boxes.len(), 1, "{}", out(&app));
        assert!(group_grips(&app) > 0, "{}", out(&app));

        let _ = app.run_command_line("UNGROUP");
        assert_eq!(app.tabs[i].scene.groups().count(), 0, "{}", out(&app));
        assert!(app.tabs[i].group_boxes.is_empty(), "{}", out(&app));
        assert_eq!(group_grips(&app), 0, "{}", out(&app));
        // The members stay picked, now with their own grips.
        assert_eq!(
            app.tabs[i].scene.selected_entities().len(),
            2,
            "{}",
            out(&app)
        );
        assert!(!app.tabs[i].selected_grips.is_empty(), "{}", out(&app));
    }

    #[test]
    fn qselect_command_line_compares_negates_and_matches_wildcards() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 40,5");
        let _ = app.run_command_line("CIRCLE 100,100 5");
        let count = |app: &OpenCADStudio| app.tabs[app.active_tab].scene.selected_entities().len();

        // Numeric comparison: only the 40-long line.
        let _ = app.run_command_line("QSELECT LENGTH > 20");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        assert_eq!(count(&app), 1, "{}", out(&app));

        // Type equality ignores case.
        let _ = app.run_command_line("QSELECT TYPE line");
        assert_eq!(count(&app), 2, "{}", out(&app));

        // Wildcards work on the type name too.
        let _ = app.run_command_line("QSELECT TYPE L*");
        assert_eq!(count(&app), 2, "{}", out(&app));

        // NOT turns the test around: everything that is not a line.
        let _ = app.run_command_line("QSELECT NOT TYPE LINE");
        assert_eq!(count(&app), 1, "{}", out(&app));
        let selected = app.tabs[app.active_tab].scene.selected_entities();
        assert!(
            matches!(selected[0].1, document::EntityType::Circle(_)),
            "the circle should be the only match"
        );

        // A malformed line keeps the selection and states the form.
        let _ = app.run_command_line("QSELECT LENGTH >");
        assert_eq!(count(&app), 1, "{}", out(&app));
        assert!(out(&app).contains("a value is needed"), "{}", out(&app));
    }

    #[test]
    fn burst_explodes_a_block_and_keeps_attribute_values_as_text() {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;

        let mut record = document::tables::BlockRecord::new("TAGBLOCK");
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = document::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut variable =
            AttributeDefinition::new("STATUS".into(), "Status".into(), "DEFAULT".into());
        variable.insertion_point = Vector3::new(1.0, 1.0, 0.0);
        variable.alignment_point = variable.insertion_point;
        variable.height = 2.5;
        variable.common.owner_handle = owner;
        let variable_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(variable.clone()))
            .unwrap();

        let mut constant = AttributeDefinition::new("SHEET".into(), String::new(), "A-101".into());
        constant.insertion_point = Vector3::new(1.0, 4.0, 0.0);
        constant.alignment_point = constant.insertion_point;
        constant.flags.constant = true;
        constant.common.owner_handle = owner;
        let constant_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(constant))
            .unwrap();

        let record = doc.block_records.get_mut("TAGBLOCK").unwrap();
        record.entity_handles = vec![line_handle, variable_handle, constant_handle];

        let mut insert = Insert::new("TAGBLOCK", Vector3::new(100.0, 50.0, 0.0));
        let mut attribute = AttributeEntity::from_definition(&variable, Some("ACTIVE".into()));
        attribute.insertion_point = Vector3::new(101.0, 51.0, 0.0);
        attribute.alignment_point = attribute.insertion_point;
        insert.attributes.push(attribute);
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("BURST");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        assert!(
            doc.get_entity(insert_handle).is_none(),
            "the reference itself must be gone"
        );
        let texts: Vec<(String, f64, f64)> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Text(text) => Some((
                    text.value.clone(),
                    text.insertion_point.x,
                    text.insertion_point.y,
                )),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&("ACTIVE".to_string(), 101.0, 51.0)),
            "the reference's value must survive as text: {texts:?}"
        );
        assert!(
            texts.contains(&("A-101".to_string(), 101.0, 54.0)),
            "a constant attribute's value must survive at its placed point: {texts:?}"
        );
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(
            !doc.entities()
                .any(|entity| matches!(entity, document::EntityType::AttributeEntity(_))),
            "no attribute instance may remain"
        );
    }

    #[test]
    fn attredef_takes_the_reference_attributes_as_the_new_definitions() {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;

        let mut record = document::tables::BlockRecord::new("TAGBLOCK");
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = document::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut definition =
            AttributeDefinition::new("STATUS".into(), "Status".into(), "DEFAULT".into());
        definition.insertion_point = Vector3::new(1.0, 1.0, 0.0);
        definition.alignment_point = definition.insertion_point;
        definition.height = 2.5;
        definition.common.owner_handle = owner;
        let definition_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(
                definition.clone(),
            ))
            .unwrap();
        doc.block_records
            .get_mut("TAGBLOCK")
            .unwrap()
            .entity_handles = vec![line_handle, definition_handle];

        let mut insert = Insert::new("TAGBLOCK", Vector3::new(100.0, 50.0, 0.0));
        let mut attribute = AttributeEntity::from_definition(&definition, Some("ACTIVE".into()));
        attribute.insertion_point = Vector3::new(101.0, 51.0, 0.0);
        attribute.alignment_point = attribute.insertion_point;
        insert.attributes.push(attribute);
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        // The reference is the authority: its value and placement changed.
        if let Some(document::EntityType::Insert(insert)) = doc.get_entity_mut(insert_handle) {
            insert.attributes[0].value = "CHANGED".into();
            insert.attributes[0].insertion_point = Vector3::new(105.0, 52.0, 0.0);
            insert.attributes[0].alignment_point = Vector3::new(105.0, 52.0, 0.0);
        }

        let _ = app.run_command_line("ATTREDEF TAGBLOCK");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        let definitions: Vec<_> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::AttributeDefinition(definition) => Some(definition),
                _ => None,
            })
            .collect();
        assert_eq!(definitions.len(), 1, "the old definition must be replaced");
        let definition = definitions[0];
        assert_eq!(definition.tag, "STATUS");
        assert_eq!(
            definition.default_value, "CHANGED",
            "the reference value becomes the new default"
        );
        assert!(
            (definition.insertion_point.x - 5.0).abs() < 1e-9
                && (definition.insertion_point.y - 2.0).abs() < 1e-9,
            "the placement is folded back into block space: {:?}",
            definition.insertion_point
        );

        let Some(document::EntityType::Insert(insert)) = doc.get_entity(insert_handle) else {
            panic!("reference gone");
        };
        assert_eq!(insert.attributes.len(), 1);
        assert_eq!(insert.attributes[0].value, "CHANGED");
        assert!(
            (insert.attributes[0].insertion_point.x - 105.0).abs() < 1e-9
                && (insert.attributes[0].insertion_point.y - 52.0).abs() < 1e-9,
            "the synchronised attribute keeps the reference's placement: {:?}",
            insert.attributes[0].insertion_point
        );
    }

    /// A block with one line and the given attribute definitions.
    fn block_with_attributes(
        app: &mut crate::app::OpenCADStudio,
        name: &str,
        definitions: &[(&str, &str, &str)],
    ) -> (document::Handle, Vec<document::Handle>) {
        use document::entities::{AttributeDefinition, Line};
        use document::types::Vector3;

        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let mut record = document::tables::BlockRecord::new(name);
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut handles = vec![line_handle];
        for (tag, prompt, default) in definitions {
            let mut definition =
                AttributeDefinition::new((*tag).into(), (*prompt).into(), (*default).into());
            definition.insertion_point = Vector3::new(1.0, 1.0, 0.0);
            definition.alignment_point = definition.insertion_point;
            definition.common.owner_handle = owner;
            let handle = doc
                .add_entity(document::EntityType::AttributeDefinition(definition))
                .unwrap();
            handles.push(handle);
        }
        doc.block_records.get_mut(name).unwrap().entity_handles = handles.clone();
        (owner, handles)
    }

    #[test]
    fn xplode_stamps_the_named_properties_on_the_pieces() {
        use document::entities::Insert;
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        block_with_attributes(&mut app, "XUNIT", &[]);
        let doc = &mut app.tabs[i].scene.document;
        doc.layers
            .add(document::tables::Layer::new("DEST"))
            .unwrap();
        let insert = Insert::new("XUNIT", Vector3::new(5.0, 5.0, 0.0));
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();
        app.tabs[i].scene.selected.insert(insert_handle);

        let _ = app.run_command_line("XPLODE COLOR 1 LAYER DEST LINETYPE DASHED LINEWEIGHT 0.50");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        assert!(
            doc.get_entity(insert_handle).is_none(),
            "the reference itself must be gone"
        );
        let pieces: Vec<_> = doc
            .entities()
            .filter(|entity| {
                matches!(entity, document::EntityType::Line(_)) && entity.common().layer == "DEST"
            })
            .collect();
        assert_eq!(pieces.len(), 1, "one line comes out");
        let common = pieces[0].common();
        assert_eq!(common.color, document::types::Color::Index(1));
        assert_eq!(common.layer, "DEST");
        assert_eq!(common.linetype, "DASHED");
        assert_eq!(
            common.line_weight,
            document::types::LineWeight::Value(50),
            "0.50 mm is 50 hundredths"
        );
    }

    #[test]
    fn blockreplace_retargets_references_and_keeps_values_by_tag() {
        use document::entities::{AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        block_with_attributes(
            &mut app,
            "OLDBLOCK",
            &[("TAG", "Tag?", "OLD"), ("OLDONLY", "Old?", "X")],
        );
        block_with_attributes(
            &mut app,
            "NEWBLOCK",
            &[("TAG", "Tag?", "NEW"), ("NEWONLY", "New?", "D")],
        );

        let doc = &mut app.tabs[i].scene.document;
        let definition =
            match doc.get_entity(doc.block_records.get("OLDBLOCK").unwrap().entity_handles[1]) {
                Some(document::EntityType::AttributeDefinition(definition)) => definition.clone(),
                _ => panic!("the old definition is there"),
            };
        let mut insert = Insert::new("OLDBLOCK", Vector3::new(20.0, 10.0, 0.0));
        insert.attributes.push(AttributeEntity::from_definition(
            &definition,
            Some("KEPT".into()),
        ));
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        let _ = app.run_command_line("BLOCKREPLACE");
        let _ = app.feed_active_cmd("OLDBLOCK");
        let _ = app.feed_active_cmd("NEWBLOCK");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        let Some(document::EntityType::Insert(insert)) = doc.get_entity(insert_handle) else {
            panic!("the reference is still there");
        };
        assert_eq!(insert.block_name, "NEWBLOCK");
        let mut tags: Vec<_> = insert
            .attributes
            .iter()
            .map(|attribute| attribute.tag.as_str())
            .collect();
        tags.sort_unstable();
        assert_eq!(tags, ["NEWONLY", "TAG"], "old-only tags are dropped");
        let tag = insert
            .attributes
            .iter()
            .find(|attribute| attribute.tag == "TAG")
            .unwrap();
        assert_eq!(tag.value, "KEPT");
        let new_only = insert
            .attributes
            .iter()
            .find(|attribute| attribute.tag == "NEWONLY")
            .unwrap();
        assert_eq!(new_only.value, "D");
    }
}
