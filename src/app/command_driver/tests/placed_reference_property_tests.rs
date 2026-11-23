// A real placed reference (not the temporary Test Block reference) takes its
// visibility state from the Properties row: the `ChoiceWithTooltip` row of
// `visibility_property` goes through PropGeomChoiceChanged and applies the
// state to a per-reference evaluated copy.
mod placed_reference_visibility_property_tests {
    use super::super::*;
    use crate::scene::block_visibility;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn the_visibility_property_row_switches_a_placed_reference_state() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let frame = Line::from_points(
            Vector3::new(10.0, 21.0, 0.0),
            Vector3::new(15.0, 21.0, 0.0),
        );
        let other = Line::from_points(
            Vector3::new(10.0, 22.0, 0.0),
            Vector3::new(15.0, 22.0, 0.0),
        );
        app.tabs[i].scene.define_block_raw(
            "WINDOW",
            Vector3::ZERO,
            vec![EntityType::Line(frame), EntityType::Line(other)],
        );
        let owner = app.tabs[i]
            .scene
            .document
            .block_records
            .get("WINDOW")
            .unwrap()
            .handle;
        let members: Vec<_> = app.tabs[i]
            .scene
            .document
            .block_records
            .get("WINDOW")
            .unwrap()
            .entity_handles
            .clone();
        let doc = &mut app.tabs[i].scene.document;
        let parameter = block_visibility::create(doc, owner, Vector3::ZERO).unwrap();
        let hidden = block_visibility::add_state(doc, parameter, "WindowHidden", 0).unwrap();
        block_visibility::set_visible(doc, parameter, hidden, &[members[0]], false);
        let reference = doc
            .add_entity(EntityType::Insert(Insert::new(
                "WINDOW",
                Vector3::new(20.0, 0.0, 0.0),
            )))
            .unwrap();
        app.tabs[i].dirty = false;
        app.tabs[i].scene.select_entity(reference, true);
        app.tabs[i].selected_handle = Some(reference);
        app.refresh_properties();

        let row = OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference)
            .expect("the reference exposes its visibility state in Properties");
        let crate::scene::model::object::PropValue::ChoiceWithTooltip { ref options, .. } =
            row.value
        else {
            panic!("the visibility row is an editable choice: {:?}", row.value)
        };
        assert!(options.iter().any(|name| name == "WindowHidden"));

        let _ = app
            .set_property_value(row, &serde_json::json!("WindowHidden"))
            .unwrap();
        app.finish_pending_history(i);

        let doc = &app.tabs[i].scene.document;
        let document::EntityType::Insert(insert) = doc.get_entity(reference).unwrap() else {
            panic!("the reference remains an INSERT")
        };
        assert_ne!(
            insert.block_name, "WINDOW",
            "the state is applied to a per-reference evaluated copy"
        );
        let copy = doc.entities_in_block(&insert.block_name).collect::<Vec<_>>();
        let frame = copy
            .iter()
            .find(|entity| matches!(entity, EntityType::Line(line) if (line.start.y - 21.0).abs() < 1e-6))
            .expect("the frame member is in the evaluated copy");
        let other = copy
            .iter()
            .find(|entity| matches!(entity, EntityType::Line(line) if (line.start.y - 22.0).abs() < 1e-6))
            .expect("the other member is in the evaluated copy");
        assert!(frame.common().invisible, "the chosen state hides the frame");
        assert!(
            !other.common().invisible,
            "the state leaves the ungoverned member visible"
        );
        let chosen = OpenCADStudio::visibility_property(doc, reference)
            .expect("the row is still offered after the change");
        assert!(
            matches!(&chosen.value, crate::scene::model::object::PropValue::ChoiceWithTooltip { selected, .. } if selected == "WindowHidden"),
            "the row reads back the chosen state: {:?}",
            chosen.value
        );
    }
}


// A real placed reference takes its Lookup choice from the Properties row:
// the Choice row built by `reference_property` routes through
// PropGeomChoiceChanged to `evaluate_reference_choice`, not the temporary
// Test Block path.
mod placed_reference_lookup_property_tests {
    use super::super::*;
    use crate::scene::{block_actions, block_distance_parameter as distance, block_lookup};
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn the_lookup_property_row_switches_a_placed_reference_choice() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "SCALEBLK",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let definition = app.tabs[i]
            .scene
            .document
            .block_records
            .get("SCALEBLK")
            .unwrap()
            .handle;
        let member = app.tabs[i]
            .scene
            .document
            .block_records
            .get("SCALEBLK")
            .unwrap()
            .entity_handles[0];
        let doc = &mut app.tabs[i].scene.document;
        let linear = distance::create(
            doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(5.0, -2.0, 0.0),
        )
        .unwrap();
        block_actions::create(
            doc,
            definition,
            block_actions::Kind::Move,
            linear,
            &[member],
            glam::DVec3::ZERO,
        )
        .unwrap();
        let lookup = block_lookup::create(doc, definition, Vector3::new(5.0, -2.0, 0.0)).unwrap();
        let parameter = block_lookup::parameters(doc, definition)
            .into_iter()
            .find(|(handle, _)| *handle == lookup)
            .unwrap()
            .1;
        let mut table = block_lookup::empty_table(parameter);
        table
            .columns
            .insert(0, block_lookup::properties(doc, definition)[0].column.clone());
        table.column_count = 2;
        table.row_count = 2;
        table.expressions = vec![
            "10".into(),
            "Small".into(),
            "20".into(),
            "Large".into(),
        ];
        block_lookup::apply_table(doc, definition, lookup, table).unwrap();

        let reference = doc
            .add_entity(EntityType::Insert(Insert::new("SCALEBLK", Vector3::ZERO)))
            .unwrap();
        app.tabs[i].dirty = false;
        app.tabs[i].scene.select_entity(reference, true);
        app.tabs[i].selected_handle = Some(reference);
        app.refresh_properties();

        let row = block_lookup::reference_property(&app.tabs[i].scene.document, reference)
            .expect("the placed reference offers a Lookup choice row");
        assert_eq!(row.field, "dyn_lookup");
        assert!(
            matches!(&row.value, crate::scene::model::object::PropValue::Choice { selected, options } if selected == "Small" && options == &["Small", "Large"]),
            "the row lists the table's labelled choices: {:?}",
            row.value
        );

        let _ = app
            .set_property_value(row, &serde_json::json!("Large"))
            .unwrap();
        app.finish_pending_history(i);

        assert!(app.command_line.last_error.is_none());
        let doc = &app.tabs[i].scene.document;
        let original = doc.get_entity(member).cloned();
        let document::EntityType::Insert(insert) = doc.get_entity(reference).unwrap() else {
            panic!("the reference remains an INSERT")
        };
        assert_ne!(
            insert.block_name, "SCALEBLK",
            "the choice is applied to a per-reference evaluated copy"
        );
        let evaluated: Vec<_> = doc.entities_in_block(&insert.block_name).collect();
        let document::EntityType::Line(line) = evaluated[0] else {
            panic!("the evaluated copy holds the member")
        };
        assert_eq!(
            (crate::scene::block_actions::v(line.start).x, crate::scene::block_actions::v(line.end).x),
            (10.0, 20.0),
            "the Large row drives the Move action on this reference only"
        );
        assert_eq!(
            doc.get_entity(member),
            original.as_ref(),
            "the source definition is untouched"
        );
    }
}
