#[cfg(test)]
mod selection_and_attribute_sync_tests {
    use crate::app::OpenCADStudio;

    fn out_text(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> document::Handle {
        use document::entities::Line;
        use document::types::Vector3;
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let line = Line::from_points(Vector3::new(x1, y1, 0.0), Vector3::new(x2, y2, 0.0));
        doc.add_entity(document::EntityType::Line(line)).unwrap()
    }

    /// M086: SELECTSIMILAR is a selection command, so it may only add objects
    /// the user can see and pick. An object on an off layer is never drawn and
    /// never hit-tested; the old path walked every top-level handle and added
    /// it anyway, so a following MOVE or ERASE silently changed geometry the
    /// user could not see.
    #[test]
    fn selectsimilar_skips_objects_on_an_off_layer() {
        let mut app = new_app();
        let i = app.active_tab;
        let visible = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let hidden = add_line(&mut app, 0.0, 5.0, 10.0, 5.0);

        let _ = app.run_command_line("-LAYER NEW HIDDEN");
        let _ = app.run_command_line("-LAYER OFF HIDDEN");
        app.tabs[i]
            .scene
            .document
            .get_entity_mut(hidden)
            .unwrap()
            .common_mut()
            .layer = "HIDDEN".to_string();

        app.tabs[i].scene.select_entities(&[visible]);
        // Type-only similarity: both objects are lines, so layer is the only
        // thing that can keep the hidden one out.
        app.select_similar_mode = 0;
        let _ = app.run_command_line("SELECTSIMILAR");

        let out = out_text(&app);
        assert!(out.contains("Select Similar: 0 added"), "{out}");
        assert!(app.tabs[i].scene.selected.contains(&visible));
        assert!(
            !app.tabs[i].scene.selected.contains(&hidden),
            "an object on an off layer must not be selected"
        );
    }

    /// A field on an attribute definition is what makes its references show
    /// live text. ATTSYNC rebuilds every reference from the definitions, but
    /// it built fresh attributes without copying the definition's field, so a
    /// field added after the references were placed never reached them and a
    /// field they already had was dropped.
    #[test]
    fn attsync_copies_a_field_from_the_attribute_definition() {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert};
        use document::types::Vector3;
        use document::EntityType;

        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = Some(r"C:\work\PLAN.dwg".to_string());
        app.tabs[i].scene.define_block_raw(
            "TAGGED",
            Vector3::ZERO,
            vec![EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TITLE".into(), "Title".into(), String::new())
            })],
        );
        let mut insert = Insert::new("TAGGED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TITLE".into(), String::new()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        assert!(!reference.is_null(), "the reference is placed");

        let definition = app.tabs[i]
            .scene
            .document
            .block_records
            .get("TAGGED")
            .expect("the definition")
            .entity_handles
            .iter()
            .copied()
            .find(|handle| {
                matches!(
                    app.tabs[i].scene.document.get_entity(*handle),
                    Some(EntityType::AttributeDefinition(_))
                )
            })
            .expect("the definition carries an ATTDEF");
        crate::entities::field::set_text_field(
            &mut app.tabs[i].scene.document,
            definition,
            Some((r#"\AcVar Filename \f "%fn6""#.to_string(), Vec::new())),
            "PLAN.dwg",
        );

        let _ = app.run_command_line("ATTSYNC TAGGED");

        let document = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = document.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let attribute = insert
            .attributes
            .iter()
            .find(|a| a.tag == "TITLE")
            .expect("the rebuilt attribute");
        assert!(
            crate::entities::field::common_hosts_field(document, &attribute.common),
            "the rebuilt attribute must host the definition's field"
        );
        assert_eq!(
            attribute.get_value(),
            "PLAN.dwg",
            "the field is resolved for the reference"
        );
    }
}
