#[cfg(test)]
mod attedit_dash_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeEntity, Insert};
    use document::types::Vector3;

    fn inserts_with_title() -> (OpenCADStudio, Vec<document::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                let mut insert = Insert::new("TITLEBLOCK", Vector3::new(n as f64 * 50.0, 0.0, 0.0));
                insert
                    .attributes
                    .push(AttributeEntity::new("TITLE".into(), "old".into()));
                app.tabs[i]
                    .scene
                    .add_entity(document::EntityType::Insert(insert))
            })
            .collect();
        (app, handles)
    }

    fn attribute(app: &OpenCADStudio, handle: document::Handle) -> (String, String) {
        let entity = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("insert");
        let document::EntityType::Insert(insert) = entity else {
            panic!("not an insert");
        };
        let attr = insert.attributes.first().expect("attribute");
        (attr.tag.clone(), attr.get_value().to_string())
    }

    #[test]
    fn dash_attedit_accepts_a_handle_argument() {
        let (mut app, handles) = inserts_with_title();
        let line = format!("-ATTEDIT {:X} TITLE new", handles[1].value());
        let _ = app.run_command_line(&line);

        assert_eq!(attribute(&app, handles[1]), ("TITLE".into(), "new".into()));
        assert_eq!(attribute(&app, handles[0]), ("TITLE".into(), "old".into()));
    }

    #[test]
    fn eattedit_opens_the_attribute_editor_on_a_selected_reference() {
        let (mut app, handles) = inserts_with_title();
        let i = app.active_tab;
        app.tabs[i].scene.select_entities(&[handles[1]]);

        let _ = app.run_command_line("EATTEDIT");

        assert_eq!(
            app.attr_editor_rows.len(),
            1,
            "the editor opens on the selected reference's one attribute"
        );
    }

    #[test]
    fn dash_attedit_asks_whether_to_edit_one_at_a_time() {
        let (mut app, _) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT");

        let i = app.active_tab;
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("ATTEDIT must ask its first question");
        assert_eq!(
            command.prompt(),
            "Edit attributes one at a time? [Yes/No] <Y>:"
        );
    }

    #[test]
    fn dash_attedit_global_branch_swaps_a_string_after_its_prompts() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT");
        let _ = app.feed_script_line("N");
        let _ = app.feed_script_line("N");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("old");
        let _ = app.feed_script_line("new");

        for handle in handles {
            assert_eq!(attribute(&app, handle).1, "new");
        }
    }

    #[test]
    fn dash_attedit_renames_a_tag_across_every_reference() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT RENAME TITLE SHEET");

        for handle in handles {
            assert_eq!(attribute(&app, handle), ("SHEET".into(), "old".into()));
        }
    }

    /// A definition change reaches its references through `ATTSYNC`: each
    /// reference takes the definition's attributes in their order, a tag it
    /// already carried keeps the value it held, and a newly defined tag
    /// arrives with its default.
    #[test]
    fn attsync_rebuilds_a_reference_from_its_changed_definition() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let tag_one = || {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TAG1".into(), "Tag one".into(), String::new())
            })
        };
        app.tabs[i]
            .scene
            .define_block_raw("SYNCED", Vector3::new(0.0, 0.0, 0.0), vec![tag_one()]);
        let mut insert = Insert::new("SYNCED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAG1".into(), "kept".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        assert!(!reference.is_null(), "the reference is placed");
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "SYNCED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![
                    tag_one(),
                    EntityType::AttributeDefinition(AttributeDefinition {
                        insertion_point: Vector3::new(2.0, 5.0, 0.0),
                        height: 1.0,
                        ..AttributeDefinition::new("TAG2".into(), "Tag two".into(), "two".into())
                    }),
                ],
            )
            .expect("the definition takes the new tag");

        let _ = app.run_command_line("ATTSYNC SYNCED");

        let doc = &app.tabs[i].scene.document;
        let defs: usize = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .filter_map(|h| doc.get_entity(*h))
            .filter(|e| matches!(e, EntityType::AttributeDefinition(_)))
            .count();
        assert_eq!(defs, 2, "the definition carries both tags");
        let table = crate::modules::insert::block_cmdline::BlockTable::of(doc);
        assert!(
            table.attributed.iter().any(|b| b == "SYNCED"),
            "the definition is listed as attributed: {:?}",
            table.attributed
        );
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the command ran without waiting for a prompt"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("TAG1".into(), "kept".into()),
                ("TAG2".into(), "two".into())
            ],
            "the reference follows the definition without losing the value it carried"
        );
    }

    /// A tag the definition renames is a new tag with the definition's default:
    /// the value the old tag carried is dropped rather than moved onto it.
    #[test]
    fn attsync_drops_the_value_of_a_tag_the_definition_renamed() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let definition = |tag: &str, default: &str| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), default.into())
            })
        };
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "RENAMED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TITLE", "")],
        );
        let mut insert = Insert::new("RENAMED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TITLE".into(), "kept".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        assert!(!reference.is_null(), "the reference is placed");
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "RENAMED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![definition("SHEET", "fresh")],
            )
            .expect("the definition renames its tag");

        let _ = app.run_command_line("ATTSYNC RENAMED");

        let document = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = document.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![("SHEET".into(), "fresh".into())],
            "the renamed tag arrives with its default and the old tag leaves"
        );
    }

    /// BATTMAN opens on the drawing's attributed block, counts its references
    /// and lists the block's definitions in their own order.
    #[test]
    fn battman_lists_the_attributed_block_and_its_definitions() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        assert!(!reference.is_null(), "the reference is placed");

        let _ = app.dispatch_command("BATTMAN");

        let state = app.attman.as_ref().expect("the manager opens");
        let blocks: Vec<(&str, usize)> = state
            .blocks
            .iter()
            .map(|b| (b.name.as_str(), b.found))
            .collect();
        assert_eq!(
            blocks,
            vec![("SYNCED", 1)],
            "the manager shows the attributed block with its one reference"
        );
        let tags: Vec<&str> = state.rows.iter().map(|r| r.original_tag.as_str()).collect();
        assert_eq!(
            tags,
            vec!["TAG1", "TAG2"],
            "the definitions are listed in the block's own order"
        );
    }

    /// Renaming a tag in the manager and syncing writes the new tag to the
    /// definition and to the references that already carry it.
    #[test]
    fn battman_sync_renames_the_definition_tag_and_its_references() {
        use crate::ui::window::attman_dialog::AttManMsg;
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TAG1".into(), "Tag one".into(), String::new())
            })],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        let _ = app.dispatch_command("BATTMAN");

        let _ = app.on_attman(AttManMsg::Tag("SHEET".into()));
        let _ = app.on_attman(AttManMsg::Sync);

        let doc = &app.tabs[i].scene.document;
        let definition_tag = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .find_map(|h| match doc.get_entity(*h) {
                Some(EntityType::AttributeDefinition(def)) => Some(def.tag.clone()),
                _ => None,
            });
        assert_eq!(
            definition_tag.as_deref(),
            Some("SHEET"),
            "the definition takes the renamed tag"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        assert_eq!(
            insert.attributes[0].tag, "SHEET",
            "and the reference is rebuilt against the renamed definition"
        );
    }

    /// Removing a row from the manager and syncing drops the definition from
    /// the block and the attribute it produced from every reference.
    #[test]
    fn battman_sync_drops_the_removed_definition_and_its_attributes() {
        use crate::ui::window::attman_dialog::AttManMsg;
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        let _ = app.dispatch_command("BATTMAN");

        let _ = app.on_attman(AttManMsg::Select(1));
        let _ = app.on_attman(AttManMsg::Remove);
        let _ = app.on_attman(AttManMsg::Sync);

        let doc = &app.tabs[i].scene.document;
        let definition_tags: Vec<String> = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .filter_map(|h| match doc.get_entity(*h) {
                Some(EntityType::AttributeDefinition(def)) => Some(def.tag.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            definition_tags,
            vec!["TAG1".to_string()],
            "the block keeps only the definition left in the list"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let reference_tags: Vec<&str> = insert.attributes.iter().map(|a| a.tag.as_str()).collect();
        assert_eq!(
            reference_tags,
            vec!["TAG1"],
            "and the reference no longer carries the removed attribute"
        );
    }

    /// A definition dropped from the block takes the attribute it produced off
    /// every reference, while the tags that remain keep their values.
    #[test]
    fn attsync_drops_the_attribute_whose_definition_is_gone() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let mut insert = Insert::new("SYNCED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAG1".into(), "kept".into()));
        insert
            .attributes
            .push(AttributeEntity::new("TAG2".into(), "old".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "SYNCED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![definition("TAG1", 2.0)],
            )
            .expect("the definition loses its second tag");

        let _ = app.run_command_line("ATTSYNC SYNCED");

        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![("TAG1".into(), "kept".into())],
            "the reference follows the definition it has now, keeping the value that remains"
        );
    }

    fn set_values(app: &mut OpenCADStudio, handles: &[document::Handle], values: &[&str]) {
        let i = app.active_tab;
        for (handle, value) in handles.iter().zip(values) {
            let entity = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|entity| entity.common().handle == *handle)
                .expect("insert");
            let document::EntityType::Insert(insert) = entity else {
                panic!("not an insert");
            };
            insert.attributes[0].set_value(*value);
        }
    }

    #[test]
    fn dash_attedit_replaces_a_string_inside_every_attribute_value() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE old new");

        for handle in handles {
            assert_eq!(attribute(&app, handle), ("TITLE".into(), "new".into()));
        }
    }

    #[test]
    fn dash_attedit_replacement_keeps_the_rest_of_each_value() {
        let (mut app, handles) = inserts_with_title();
        set_values(&mut app, &handles, &["old north", "old south", "keep"]);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new");

        assert_eq!(attribute(&app, handles[0]).1, "new north");
        assert_eq!(attribute(&app, handles[1]).1, "new south");
        assert_eq!(attribute(&app, handles[2]).1, "keep");
    }

    #[test]
    fn dash_attedit_replacement_takes_a_handle_before_the_verb() {
        let (mut app, handles) = inserts_with_title();
        let line = format!("-ATTEDIT {:X} REPLACE old new", handles[1].value());
        let _ = app.run_command_line(&line);

        assert_eq!(attribute(&app, handles[1]).1, "new");
        assert_eq!(attribute(&app, handles[0]).1, "old");
        assert_eq!(attribute(&app, handles[2]).1, "old");
    }

    #[test]
    fn dash_attedit_replacement_reports_a_string_that_matches_nothing() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE zzz new");

        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or("")
                .contains("zzz"),
            "error was {:?}",
            app.command_line.last_error
        );
        for handle in handles {
            assert_eq!(attribute(&app, handle).1, "old");
        }
    }

    fn add_note_attributes(app: &mut OpenCADStudio, handles: &[document::Handle]) {
        let i = app.active_tab;
        for handle in handles {
            let entity = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|entity| entity.common().handle == *handle)
                .expect("insert");
            let document::EntityType::Insert(insert) = entity else {
                panic!("not an insert");
            };
            insert
                .attributes
                .push(AttributeEntity::new("NOTE".into(), "old".into()));
        }
    }

    fn attribute_value(app: &OpenCADStudio, handle: document::Handle, tag: &str) -> String {
        let entity = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("insert");
        let document::EntityType::Insert(insert) = entity else {
            panic!("not an insert");
        };
        insert
            .attributes
            .iter()
            .find(|attr| attr.tag == tag)
            .map(|attr| attr.get_value().to_string())
            .unwrap_or_default()
    }

    #[test]
    fn dash_attedit_replacement_filters_by_tag() {
        let (mut app, handles) = inserts_with_title();
        add_note_attributes(&mut app, &handles);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new * TITLE");

        assert_eq!(attribute_value(&app, handles[0], "TITLE"), "new");
        assert_eq!(attribute_value(&app, handles[0], "NOTE"), "old");
    }

    #[test]
    fn dash_attedit_replacement_filters_by_value() {
        let (mut app, handles) = inserts_with_title();
        set_values(&mut app, &handles, &["old north", "north old", "keep"]);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new * * old*");

        assert_eq!(attribute(&app, handles[0]).1, "new north");
        assert_eq!(attribute(&app, handles[1]).1, "north old");
        assert_eq!(attribute(&app, handles[2]).1, "keep");
    }

    #[test]
    fn dash_attedit_replacement_filters_by_block_name() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE old new OTHERBLOCK");
        for handle in &handles {
            assert_eq!(attribute(&app, *handle).1, "old");
        }

        let _ = app.run_command_line("-ATTEDIT REPLACE old new TITLEBLOCK");
        for handle in &handles {
            assert_eq!(attribute(&app, *handle).1, "new");
        }
    }
}
