// Tests for the plugin host IPC surface.

use super::quad::quad_item_icon;
use super::*;
use crate::app::OpenCADStudio;
use crate::entities::traits::RenderConvertible;
use document::entities::{Line, Point};
use document::xdata::XDataValue;
use ocs_plugin_api::host::DocumentReader;

/// Plugins get the service's rules: an attribute counts as a use, and a
/// rename reaches it, as they do on the command line.
#[test]
fn linetype_operations_follow_the_shared_rules() {
    use ocs_plugin_api::host::TableOperation as Op;
    let create = |name: &str, pattern: &[f64]| Op::LinetypeCreate {
        name: name.to_owned(),
        description: String::new(),
        pattern: pattern.to_vec(),
    };
    let rename = |from: &str, to: &str| Op::LinetypeRename {
        from: from.to_owned(),
        to: to.to_owned(),
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    assert!(host
        .linetype_operation(create("Gap First", &[-1.0, 1.0]))
        .is_err());
    assert!(host
        .linetype_operation(create("Comma,Name", &[1.0, -1.0]))
        .is_err());
    assert!(
        host.linetype_operation(create("dashed", &[1.0, -1.0]))
            .is_err(),
        "a standard name is taken"
    );
    let created = host
        .linetype_operation(create("Posts", &[12.0, -3.0, 2.0, -3.0]))
        .unwrap();
    let stored = host.document().line_types.get("Posts").unwrap();
    assert_eq!(stored.handle, created);
    assert_eq!(stored.pattern_length, 20.0);

    assert!(
        host.linetype_operation(rename("Posts", "POSTS")).is_err(),
        "a plugin still cannot change only the case"
    );

    let mut attribute = document::entities::AttributeEntity::new("TAG".into(), "VALUE".into());
    attribute.common.linetype = "Posts".into();
    let mut insert = document::entities::Insert::new("HOLDER", document::Vector3::ZERO);
    insert.attributes.push(attribute);
    let insert = host
        .document_mut()
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    let delete = |name: &str| Op::LinetypeDelete {
        name: name.to_owned(),
    };
    let refusal = host.linetype_operation(delete("Posts")).unwrap_err();
    assert!(refusal.contains("PURGE"), "{refusal}");

    let renamed = host.linetype_operation(rename("Posts", "Rails")).unwrap();
    assert_eq!(renamed, created, "the handle survives");
    let Some(EntityType::Insert(insert)) = host.document().get_entity(insert) else {
        panic!("the insert is gone");
    };
    assert_eq!(insert.attributes[0].common.linetype, "Rails");
}

/// A layout a plugin creates, renames or deletes shows in the layout tab
/// bar at once, as one made from the tab bar does.
#[test]
fn plugin_layout_changes_reach_the_layout_tab_bar() {
    use ocs_plugin_api::host::TableOperation as Op;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let tabs = |app: &OpenCADStudio| app.tabs[0].scene.cached_layout_names().to_vec();
    let before = tabs(&app);
    assert!(!before.iter().any(|n| n == "Plugin Sheet"));

    let mut host = HostSession::new(&mut app, 0);
    host.layout_operation(Op::LayoutCreate {
        name: "Plugin Sheet".to_owned(),
    })
    .unwrap();
    drop(host);
    assert!(
        tabs(&app).iter().any(|n| n == "Plugin Sheet"),
        "the new layout is missing from the tab bar: {:?}",
        tabs(&app)
    );

    let mut host = HostSession::new(&mut app, 0);
    host.layout_operation(Op::LayoutRename {
        from: "Plugin Sheet".to_owned(),
        to: "Renamed Sheet".to_owned(),
    })
    .unwrap();
    drop(host);
    let names = tabs(&app);
    assert!(names.iter().any(|n| n == "Renamed Sheet"), "{names:?}");
    assert!(!names.iter().any(|n| n == "Plugin Sheet"), "{names:?}");

    let mut host = HostSession::new(&mut app, 0);
    host.layout_operation(Op::LayoutDelete {
        name: "Renamed Sheet".to_owned(),
    })
    .unwrap();
    drop(host);
    assert_eq!(tabs(&app), before);
}

/// A plugin sets the drawing's custom properties like DWGPROPS OK: a
/// listed name replaces the value (trimmed, any case) or is appended, the
/// rest stay, a text showing the property updates, and it is one undo step.
#[test]
fn plugin_sets_custom_properties_in_one_undo_step() {
    use document::entities::{EntityType, MText};
    use ocs_plugin_api::host::TableOperation as Op;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let props = |app: &OpenCADStudio| {
        app.tabs[0]
            .scene
            .document
            .summary_info
            .custom_properties
            .clone()
    };
    let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> {
        p.iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    };
    let set = |p: &[(&str, &str)]| Op::CustomPropertiesSet {
        properties: pairs(p),
    };
    let document = &mut app.tabs[0].scene.document;
    document.summary_info.custom_properties = pairs(&[("Cost", "12,5"), ("Keep", "k")]);
    let text = document
        .add_entity(EntityType::MText(Box::new(MText::new())))
        .unwrap();
    crate::entities::field::set_text_field(
        document,
        text,
        Some((r"\AcVar CustomDP.cost".to_string(), vec![])),
        "12,5",
    );
    let before = props(&app);

    let mut host = HostSession::new(&mut app, 0);
    assert!(
        host.table_operation(set(&[(" ", "x")])).is_err(),
        "empty name"
    );
    assert!(
        host.table_operation(set(&[("New", "1"), ("new ", "2")]))
            .is_err(),
        "the same name twice"
    );
    drop(host);
    assert_eq!(props(&app), before, "a refused list changes nothing");

    app.finish_pending_history(0);
    let undo_before = app.tabs[0].history.undo_stack.len();
    app.tabs[0].dirty = false;
    let mut host = HostSession::new(&mut app, 0);
    host.table_operation(set(&[("cost ", "99"), ("Added", "a")]))
        .unwrap();
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(
        props(&app),
        pairs(&[("Cost", "99"), ("Keep", "k"), ("Added", "a")])
    );
    assert!(app.tabs[0].dirty);
    let Some(EntityType::MText(shown)) = app.tabs[0].scene.document.get_entity(text) else {
        panic!("the text stays");
    };
    assert_eq!(shown.value, "99", "the field shows the new value");
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        undo_before + 1,
        "one undo step"
    );

    let mut host = HostSession::new(&mut app, 0);
    host.table_operation(set(&[("COST", "99")])).unwrap();
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        undo_before + 1,
        "no change, no undo step"
    );

    app.undo_steps(1);
    assert_eq!(props(&app), before, "undo puts the old properties back");
}

/// A plugin puts the paper-space layout tabs in a new order in one undo
/// step, and a list that is not exactly the paper-space layouts is refused.
#[test]
fn plugin_sets_the_layout_tab_order_in_one_undo_step() {
    use ocs_plugin_api::host::TableOperation as Op;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let tabs = |app: &OpenCADStudio| app.tabs[0].scene.cached_layout_names().to_vec();
    let mut host = HostSession::new(&mut app, 0);
    for name in ["Sheet A", "Sheet B"] {
        host.layout_operation(Op::LayoutCreate {
            name: name.to_owned(),
        })
        .unwrap();
    }
    drop(host);
    let before = tabs(&app);
    let paper: Vec<String> = before.iter().skip(1).cloned().collect();
    assert!(paper.len() >= 3, "{before:?}");
    let mut wanted = paper.clone();
    wanted.reverse();

    let order = |names: &[&str]| Op::LayoutSetTabOrder {
        names: names.iter().map(|n| (*n).to_owned()).collect(),
    };
    let mut host = HostSession::new(&mut app, 0);
    let first = paper[0].as_str();
    assert!(
        host.table_operation(order(&[first])).is_err(),
        "a layout left out"
    );
    assert!(
        host.table_operation(order(&[first, first])).is_err(),
        "a layout listed twice"
    );
    let mut with_model: Vec<&str> = paper.iter().map(String::as_str).collect();
    with_model.push("Model");
    assert!(
        host.table_operation(order(&with_model)).is_err(),
        "Model listed"
    );
    drop(host);
    assert_eq!(tabs(&app), before, "a refused order changes nothing");

    app.finish_pending_history(0);
    let undo_before = app.tabs[0].history.undo_stack.len();
    let mut host = HostSession::new(&mut app, 0);
    let lower: Vec<String> = wanted.iter().map(|n| n.to_lowercase()).collect();
    let lower: Vec<&str> = lower.iter().map(String::as_str).collect();
    host.table_operation(order(&lower)).unwrap();
    drop(host);
    app.finish_pending_history(0);
    let after = tabs(&app);
    assert_eq!(after[0], "Model");
    assert_eq!(after[1..], wanted[..], "the tab bar shows the new order");
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        undo_before + 1,
        "one undo step"
    );
    app.undo_steps(1);
    assert_eq!(tabs(&app), before, "one undo restores the old order");
}

#[test]
fn selection_query_is_live_and_replacement_is_validated() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let a = host.add_entity(EntityType::Point(Point::new()));
    let b = host.add_entity(EntityType::Point(Point::new()));
    assert!(host.selection().is_empty());
    host.set_selection(&[b, a]).unwrap();
    assert_eq!(host.selection(), vec![b, a]);
    assert!(host.set_selection(&[Handle::new(99999)]).is_err());
    assert!(host.set_selection(&[a, a]).is_err());
    assert_eq!(host.selection(), vec![b, a]);
    host.set_selection(&[]).unwrap();
    assert!(host.selection().is_empty());
}

#[test]
fn nested_insert_attribute_crud_uses_parent_storage_and_undo() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let (insert_handle, attribute_handle);
    {
        let mut host = HostSession::new(&mut app, 0);
        let next = host.document().next_handle();
        let record_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let block_end_handle = Handle::new(next + 2);
        let mut record = document::tables::BlockRecord::new("TAGBLOCK");
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = block_end_handle;
        host.document_mut().block_records.add(record).unwrap();
        let mut block = document::entities::Block::new("TAGBLOCK", document::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut end = document::entities::BlockEnd::new();
        end.common.handle = block_end_handle;
        end.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(end))
            .unwrap();
        let mut definition = document::entities::AttributeDefinition::new(
            "PART_NO".into(),
            "Part number".into(),
            "PN-001".into(),
        );
        definition.common.owner_handle = record_handle;
        let definition_handle = host
            .document_mut()
            .add_entity(EntityType::AttributeDefinition(definition))
            .unwrap();
        insert_handle = host
            .document_mut()
            .add_entity(EntityType::Insert(document::entities::Insert::new(
                "TAGBLOCK",
                document::types::Vector3::new(10.0, 0.0, 0.0),
            )))
            .unwrap();

        let mut attribute =
            document::entities::AttributeEntity::new("PART_NO".into(), "PN-101".into());
        attribute.common.owner_handle = insert_handle;
        attribute.attdef_handle = definition_handle;
        attribute.insertion_point = document::types::Vector3::new(10.0, 2.0, 0.0);
        host.push_undo("Create attribute");
        attribute_handle = host.add_entity(EntityType::AttributeEntity(attribute));
        assert!(!attribute_handle.is_null());
        assert!(
            host.document().get_entity(attribute_handle).is_none(),
            "nested attributes must not become orphan flat entities"
        );
        assert!(matches!(host.host_model_entity(attribute_handle),
                Some(EntityType::AttributeEntity(value)) if value.value == "PN-101"));
        let view = ocs_plugin_api::shm::DocumentViewDataV4::from(host.document());
        assert!(view
            .entities
            .iter()
            .any(|entity| entity.handle == attribute_handle.value()));

        let mut changed = host.host_model_entity(attribute_handle).unwrap();
        let EntityType::AttributeEntity(value) = &mut changed else {
            unreachable!()
        };
        value.value = "PN-102".into();
        value.insertion_point.x = 12.0;
        host.update_entities_transaction("Edit attribute", vec![changed])
            .unwrap();
        assert!(matches!(host.host_model_entity(attribute_handle),
                Some(EntityType::AttributeEntity(value))
                    if value.value == "PN-102" && value.insertion_point.x == 12.0));

        host.push_undo("Delete attribute");
        assert!(host.remove_entity(attribute_handle));
        assert!(host.host_model_entity(attribute_handle).is_none());
    }
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.len() == 1
                && insert.attributes[0].value == "PN-102")
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes[0].value == "PN-101")
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
    );
    app.redo_steps(3);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
    );
}

#[test]
fn scripted_selection_does_not_expand_linked_leader_annotation() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let annotation = host.add_entity(EntityType::Text(document::entities::Text::default()));
    let mut leader = document::entities::Leader::default();
    leader.annotation_handle = annotation;
    let leader_handle = host.add_entity(EntityType::Leader(leader));
    host.set_selection(&[leader_handle]).unwrap();
    assert_eq!(host.selection(), vec![leader_handle]);
}

#[test]
fn entity_transaction_validates_before_mutation_and_undoes_as_one_step() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let (first, second);
    {
        let mut host = HostSession::new(&mut app, 0);
        first = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
            1.0, 0.0, 0.0,
        ))));
        second = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
            2.0, 0.0, 0.0,
        ))));
        let mut a = host.document().get_entity(first).unwrap().clone();
        let mut b = host.document().get_entity(second).unwrap().clone();
        if let EntityType::Point(p) = &mut a {
            p.location.x = 10.0;
        }
        if let EntityType::Point(p) = &mut b {
            p.location.x = 20.0;
        }
        let mut invalid = b.clone();
        invalid.common_mut().owner_handle = Handle::new(99999);
        assert!(host
            .update_entities_transaction("Move points", vec![a.clone(), invalid])
            .is_err());
        let mut invalid_geometry = b.clone();
        if let EntityType::Point(p) = &mut invalid_geometry {
            p.location.x = f64::NAN;
        }
        assert!(host
            .update_entities_transaction("Move points", vec![a.clone(), invalid_geometry])
            .unwrap_err()
            .contains("Point.location"));
        assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
        assert!(
            matches!(host.document().get_entity(first), Some(EntityType::Point(p)) if p.location.x == 1.0)
        );
        host.update_entities_transaction("Move points", vec![a, b])
            .unwrap();
        assert!(
            matches!(host.document().get_entity(first), Some(EntityType::Point(p)) if p.location.x == 10.0)
        );
    }
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(first), Some(EntityType::Point(p)) if p.location.x == 1.0)
    );
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(second), Some(EntityType::Point(p)) if p.location.x == 2.0)
    );
    app.redo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(first), Some(EntityType::Point(p)) if p.location.x == 10.0)
    );
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(second), Some(EntityType::Point(p)) if p.location.x == 20.0)
    );
}

#[test]
fn xrecord_crud_round_trips_refusals_and_list() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "{\"k\":\"road\",\"s\":1}"),
            document::objects::XRecordEntry::double(40, 0.125),
            document::objects::XRecordEntry::int16(70, 3),
            document::objects::XRecordEntry::int32(90, -45000),
            document::objects::XRecordEntry::bool(290, true),
        ]
    };

    // Create seeds the namespace: the dictionary chain is auto-created and
    // reading returns the entries verbatim.
    let alpha = host
        .create_xrecord(&["PLUGIN"], "alpha", entries())
        .unwrap();
    assert!(!alpha.is_null());
    assert_eq!(host.read_xrecord(&["PLUGIN"], "alpha").unwrap(), entries());
    // Dictionary lookup is case-insensitive, both for the path …
    assert_eq!(host.read_xrecord(&["plugin"], "alpha").unwrap(), entries());
    // â€¦ and re-creating an existing name is refused without any change.
    assert!(host
        .create_xrecord(&["plugin"], "ALPHA", entries())
        .unwrap_err()
        .contains("already exists"));

    // Namespace reuse is idempotent: the root still holds one PLUGIN entry.
    let beta = host.create_xrecord(&["PLUGIN"], "beta", entries()).unwrap();
    let root = host.document().header.named_objects_dict_handle;
    let cvifc_entries = match host.document().objects.get(&root) {
        Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
            .entries
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("PLUGIN"))
            .count(),
        _ => 0,
    };
    assert_eq!(cvifc_entries, 1);

    // Listing names only the keys that address XRecords, in dictionary
    // order; a nested dictionary is not an XRecord.
    host.create_xrecord(&["PLUGIN", "LAYOUTS"], "nested", entries())
        .unwrap();
    assert_eq!(
        host.list_xrecords(&["PLUGIN"]).unwrap(),
        vec!["alpha".to_owned(), "beta".to_owned()]
    );
    assert_eq!(
        host.list_xrecords(&["PLUGIN", "LAYOUTS"]).unwrap(),
        vec!["nested".to_owned()]
    );
    // The root dictionary holds non-XRecord entries (ACAD_LAYOUT et al.);
    // listing only reports XRecords.
    let root_names = host.list_xrecords(&[]).unwrap();
    assert!(!root_names
        .iter()
        .any(|n| n.eq_ignore_ascii_case("ACAD_LAYOUT")));

    // Update refuses on a missing name and never creates anything.
    assert!(host
        .update_xrecord(&["PLUGIN"], "gamma", entries())
        .unwrap_err()
        .contains("does not exist"));
    // Case-insensitive resolution finds the stored ALPHA and replaces its
    // entries atomically; the handle is stable.
    assert_eq!(
        host.update_xrecord(
            &["PLUGIN"],
            "ALPHA",
            vec![document::objects::XRecordEntry::string(1, "v2")]
        )
        .unwrap(),
        alpha
    );
    assert_eq!(
        host.read_xrecord(&["PLUGIN"], "alpha").unwrap(),
        vec![document::objects::XRecordEntry::string(1, "v2")]
    );

    // Entries are validated before any change: reserved and out-of-range
    // group codes refuse (5 and 105 are the object handle codes).
    assert!(host
        .create_xrecord(
            &["PLUGIN"],
            "gamma",
            vec![document::objects::XRecordEntry::int16(5, 1)]
        )
        .unwrap_err()
        .contains("reserved"));
    assert!(host
        .create_xrecord(
            &["PLUGIN"],
            "gamma",
            vec![document::objects::XRecordEntry::int16(400, 1)]
        )
        .unwrap_err()
        .contains("out of range"));
    assert!(host
        .update_xrecord(
            &["PLUGIN"],
            "alpha",
            vec![document::objects::XRecordEntry::int16(105, 1)]
        )
        .is_err());
    assert_eq!(
        host.read_xrecord(&["PLUGIN"], "alpha").unwrap(),
        vec![document::objects::XRecordEntry::string(1, "v2")]
    );
    // Dictionary and record names are symbol names.
    assert!(host.create_xrecord(&["PLUGIN"], "  ", entries()).is_err());
    assert!(host
        .create_xrecord(&["PLUGIN", "A|B"], "gamma", entries())
        .is_err());
    assert!(host.create_xrecord(&["PLUGIN"], "a<b", entries()).is_err());

    // Reads distinguish a missing dictionary, a missing name and a name
    // that addresses another object kind.
    assert!(host
        .read_xrecord(&["NOPE"], "alpha")
        .unwrap_err()
        .contains("does not exist"));
    assert!(host
        .read_xrecord(&["PLUGIN", "LAYOUTS"], "alpha")
        .unwrap_err()
        .contains("does not exist"));
    assert!(host
        .delete_xrecord(&["PLUGIN"], "LAYOUTS")
        .unwrap_err()
        .contains("not an XRecord"));

    // Delete removes exactly the named record and returns its content.
    let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
    assert_eq!(removed.handle, beta);
    assert_eq!(removed.entries, entries());
    assert_eq!(
        host.list_xrecords(&["PLUGIN"]).unwrap(),
        vec!["alpha".to_owned()]
    );
    assert!(host
        .read_xrecord(&["PLUGIN"], "beta")
        .unwrap_err()
        .contains("does not exist"));
    assert!(host.delete_xrecord(&["PLUGIN"], "beta").is_err());
}

#[test]
fn xrecord_mutations_are_single_undo_steps_that_restore_objects() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let original_entries = || vec![document::objects::XRecordEntry::string(1, "one")];
    let changed_entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "changed"),
            document::objects::XRecordEntry::int16(70, 9),
        ]
    };
    let (alpha, beta);
    {
        let mut host = HostSession::new(&mut app, 0);
        alpha = host
            .create_xrecord(&["PLUGIN"], "alpha", original_entries())
            .unwrap();
        beta = host
            .create_xrecord(&["PLUGIN"], "beta", original_entries())
            .unwrap();
        // Refusals record no undo step.
        assert!(host
            .create_xrecord(&["PLUGIN"], "alpha", original_entries())
            .is_err());
        assert!(host
            .update_xrecord(&["PLUGIN"], "gamma", original_entries())
            .is_err());
        host.update_xrecord(&["PLUGIN"], "alpha", changed_entries())
            .unwrap();
        let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
        assert_eq!(removed.handle, beta);
    }
    app.finish_pending_history(0);
    // Four mutating calls = four undo steps; the refusals recorded none.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 4);

    let xrecord = |doc: &Document, handle: Handle| -> Option<document::objects::XRecord> {
        match doc.objects.get(&handle) {
            Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
            _ => None,
        }
    };
    let has_dictionary_entry = |doc: &Document, name: &str| -> bool {
        let root = doc.header.named_objects_dict_handle;
        match doc.objects.get(&root) {
            Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
                .entries
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name)),
            _ => false,
        }
    };

    // Undo the delete: beta is back with its exact prior entries.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert_eq!(xrecord(document, beta).unwrap().entries, original_entries());
    // Undo the update: alpha's entries are restored byte-exactly.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert_eq!(
        xrecord(document, alpha).unwrap().entries,
        original_entries()
    );
    // Undo beta's creation.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(xrecord(document, beta).is_none());
    assert!(xrecord(document, alpha).is_some());
    // Undo alpha's creation: the auto-created dictionary went with it,
    // because the pending history snapshot restores the whole objects map.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(xrecord(document, alpha).is_none());
    assert!(
        !has_dictionary_entry(document, "PLUGIN"),
        "undoing the first create removed the auto-created dictionary"
    );
    // Redo re-applies the whole ladder.
    app.redo_steps(4);
    let document = &app.tabs[0].scene.document;
    assert_eq!(xrecord(document, alpha).unwrap().entries, changed_entries());
    assert!(xrecord(document, beta).is_none());
}

/// The entity-write composition contract, for XRecords: a plugin opens one
/// `push_undo` scope per operation and performs every write — XRecord and
/// entity, plus refusals — inside it, and the finished scope is a single
/// undo step under the plugin's own label that restores (and re-applies)
/// the whole operation at once.
#[test]
fn xrecord_writes_join_a_plugin_initiated_undo_scope() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let original_entries = || vec![document::objects::XRecordEntry::string(1, "one")];
    let changed_entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "changed"),
            document::objects::XRecordEntry::int16(70, 9),
        ]
    };
    let (alpha, nested, entity);
    {
        let mut host = HostSession::new(&mut app, 0);
        host.push_undo("PLUGIN: import");
        alpha = host
            .create_xrecord(&["PLUGIN"], "alpha", original_entries())
            .unwrap();
        nested = host
            .create_xrecord(&["PLUGIN", "LAYOUTS"], "nested", original_entries())
            .unwrap();
        // A refusal inside the scope records nothing at all.
        assert!(host
            .create_xrecord(&["PLUGIN"], "ALPHA", original_entries())
            .is_err());
        assert_eq!(
            host.update_xrecord(&["PLUGIN"], "alpha", changed_entries())
                .unwrap(),
            alpha
        );
        let beta = host
            .create_xrecord(&["PLUGIN"], "beta", original_entries())
            .unwrap();
        let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
        assert_eq!(removed.handle, beta);
        // Entity writes join the same scope and land in the same step.
        entity = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
            3.0, 0.0, 0.0,
        ))));
    }
    app.finish_pending_history(0);
    // The whole plugin operation is ONE undo step under the plugin's label.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    assert_eq!(app.tabs[0].history.undo_stack[0].label(), "PLUGIN: import");

    let xrecord = |doc: &Document, handle: Handle| -> Option<document::objects::XRecord> {
        match doc.objects.get(&handle) {
            Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
            _ => None,
        }
    };
    let has_dictionary_entry = |doc: &Document, name: &str| -> bool {
        let root = doc.header.named_objects_dict_handle;
        match doc.objects.get(&root) {
            Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
                .entries
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name)),
            _ => false,
        }
    };

    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(document.get_entity(entity).is_none());
    assert!(xrecord(document, alpha).is_none());
    assert!(xrecord(document, nested).is_none());
    assert!(
        !has_dictionary_entry(document, "PLUGIN"),
        "undoing the operation removed its auto-created dictionary namespace"
    );
    app.redo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(matches!(
        document.get_entity(entity),
        Some(EntityType::Point(_))
    ));
    let alpha_record = xrecord(document, alpha).unwrap();
    assert_eq!(alpha_record.entries, changed_entries());
    assert!(xrecord(document, nested).is_some());
    assert!(
        has_dictionary_entry(document, "PLUGIN"),
        "redoing the operation re-seeded its namespace"
    );
}

#[test]
fn xrecords_survive_dxf_and_dwg_round_trips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "{\"k\":\"road\",\"s\":1}"),
            document::objects::XRecordEntry::double(40, 0.125),
            document::objects::XRecordEntry::int16(70, 3),
            document::objects::XRecordEntry::int32(90, -45000),
            document::objects::XRecordEntry::bool(290, true),
        ]
    };
    // Create carries a first payload, then the update replaces it: if the
    // writers preferred stale raw bytes over the typed entries, the loaded
    // record would still read "stale".
    host.create_xrecord(
        &["PLUGIN", "RECORDS"],
        "alpha",
        vec![document::objects::XRecordEntry::string(1, "stale")],
    )
    .unwrap();
    host.update_xrecord(&["PLUGIN", "RECORDS"], "alpha", entries())
        .unwrap();

    let find_xrecord =
        |doc: &Document, path: &[&str], name: &str| -> Option<document::objects::XRecord> {
            let mut current = doc.header.named_objects_dict_handle;
            for fragment in path {
                match doc.objects.get(&current) {
                    Some(document::objects::ObjectType::Dictionary(dictionary)) => {
                        current = dictionary.get(fragment)?
                    }
                    _ => return None,
                }
            }
            let Some(document::objects::ObjectType::Dictionary(dictionary)) =
                doc.objects.get(&current)
            else {
                return None;
            };
            let handle = dictionary.get(name)?;
            match doc.objects.get(&handle) {
                Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
                _ => None,
            }
        };

    let dwg = crate::io::load_bytes(
        "xrec.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let reloaded =
        find_xrecord(&dwg, &["PLUGIN", "RECORDS"], "alpha").expect("XRecord in the reloaded DWG");
    assert_eq!(reloaded.entries, entries());

    let dxf = crate::io::load_bytes(
        "xrec.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    let reloaded =
        find_xrecord(&dxf, &["PLUGIN", "RECORDS"], "alpha").expect("XRecord in the reloaded DXF");
    assert_eq!(reloaded.entries, entries());
}

/// Opt-in end-to-end check using the staged Python cdylib and the actual
/// OCS executable as its out-of-process runner. Run with OCS_TEST_PYTHON_PLUGIN
/// and OCS_PLUGIN_RUNNER_EXE set; unlike the local-socket protocol test,
/// this executes Python and calls the app's HostSession over real IPC.
#[test]
fn staged_python_plugin_line_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let plugin_path = std::path::PathBuf::from(plugin_path);
    assert!(
        plugin_path.is_file(),
        "missing staged Python plugin: {}",
        plugin_path.display()
    );
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        &plugin_path,
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .expect("spawn staged Python plugin");
    assert_eq!(process.id(), "opencad.python");
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    dispatch(
        &mut host,
        concat!(
            "PY_EVAL ocs.active_document.create_entity('Line',",
            "start={'x':0.0,'y':0.0,'z':0.0},",
            "end={'x':1.0,'y':0.0,'z':0.0}).handle"
        ),
    );
    let handles: Vec<_> = host
        .document()
        .entities()
        .map(|entity| entity.common().handle)
        .collect();
    assert_eq!(handles.len(), 1, "Python add did not reach the host");
    let handle = handles[0];
    assert!(
        matches!(host.document().get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 1.0)
    );
    let before_edit = host.document().get_entity(handle).unwrap().clone();
    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Move line', [{{'handle':{},'end':{{'x':5.0,'y':0.0,'z':0.0}}}}])",
            handle.value()
        ));
    let mut expected_after_edit = before_edit;
    let EntityType::Line(expected_line) = &mut expected_after_edit else {
        unreachable!()
    };
    expected_line.end.x = 5.0;
    assert_eq!(
        host.document().get_entity(handle),
        Some(&expected_after_edit),
        "partial Python edit must preserve every unmentioned common and geometry field"
    );
    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject NaN', [{{'handle':{},'start':{{'x':float('nan'),'y':0.0,'z':0.0}}}}])",
            handle.value()
        ));
    assert_eq!(
        host.document().get_entity(handle),
        Some(&expected_after_edit),
        "invalid geometry changed the line"
    );
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("finite coordinates"));
    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject duplicate', [{{'handle':{},'end':{{'x':8.0,'y':0.0,'z':0.0}}}},{{'handle':{},'end':{{'x':9.0,'y':0.0,'z':0.0}}}}])",
            handle.value(), handle.value()
        ));
    assert!(
        matches!(host.document().get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0),
        "rejected batch changed the line"
    );
    let dwg_bytes = document::DwgWriter::write_to_vec(host.document()).expect("write edited DWG");
    let dwg_doc = document::DwgReader::from_stream(std::io::Cursor::new(dwg_bytes))
        .read()
        .expect("reopen edited DWG");
    assert!(
        matches!(dwg_doc.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
    );
    let dxf_bytes = document::DxfWriter::new(host.document())
        .write_to_vec()
        .expect("write edited DXF");
    let dxf_doc = document::DxfReader::from_reader(std::io::Cursor::new(dxf_bytes))
        .expect("open edited DXF")
        .read()
        .expect("reopen edited DXF");
    assert!(
        matches!(dxf_doc.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(
        host.document().get_entity(handle).is_none(),
        "Python removal did not reach the host"
    );
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        3,
        "create, edit, and delete should each form one undo entry"
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 1.0)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_tolerance_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .expect("spawn staged Python plugin");
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    dispatch(
        &mut host,
        concat!(
            "PY_EVAL ocs.active_document.create_entity('Tolerance',",
            "insertion_point={'x':1.0,'y':2.0,'z':0.0},",
            "text='POSITION%%v0.1').handle"
        ),
    );
    let handle = host
        .document()
        .entities()
        .next()
        .expect("Python created a Tolerance")
        .common()
        .handle;
    let Some(EntityType::Tolerance(created)) = host.document().get_entity(handle) else {
        panic!("expected Tolerance");
    };
    assert_eq!(
        created.insertion_point,
        document::types::Vector3::new(1.0, 2.0, 0.0)
    );
    assert_eq!(created.dimension_style_name, "Standard");
    let created = created.clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].text",
            handle.value()
        ),
    );
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("POSITION%%v0.1"));

    let script_path = std::env::temp_dir().join(format!(
        "ocs_tolerance_document_model_{}.py",
        std::process::id()
    ));
    std::fs::write(
        &script_path,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "tolerance = doc.entities[{}]\n",
                "with doc.transaction('Edit tolerance'):\n",
                "    tolerance.insertion_point = (4.0, 5.0, 0.0)\n",
                "    tolerance.direction = (0.0, 1.0, 0.0)\n",
                "    tolerance.text = 'POSITION%%v0.2'\n",
                "doc.selection = [tolerance]\n"
            ),
            handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
    let _ = std::fs::remove_file(&script_path);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::Tolerance(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.insertion_point,
        document::types::Vector3::new(4.0, 5.0, 0.0)
    );
    assert_eq!(edited.direction, document::types::Vector3::UNIT_Y);
    assert_eq!(edited.text, "POSITION%%v0.2");
    assert_eq!(edited.common, created.common);
    assert_eq!(edited.normal, created.normal);
    assert_eq!(edited.dimension_style_name, created.dimension_style_name);
    assert_eq!(
        edited.dimension_style_handle,
        created.dimension_style_handle
    );
    assert_eq!(edited.text_height, created.text_height);
    assert_eq!(edited.dimension_gap, created.dimension_gap);
    assert_eq!(edited.dwg_unknown_short, created.dwg_unknown_short);
    assert_eq!(
        host.selection(),
        vec![handle],
        "Python selection did not reach the canvas"
    );

    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject direction', [{{'handle':{},'direction':{{'x':2.0,'y':0.0,'z':0.0}}}}])",
            handle.value()));
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("unit vector"));
    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject style', [{{'handle':{},'dimension_style_name':'Missing'}}])",
            handle.value()));
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("does not exist"));

    let dwg_bytes = document::DwgWriter::write_to_vec(host.document()).unwrap();
    let dwg_doc = document::DwgReader::from_stream(std::io::Cursor::new(dwg_bytes))
        .read()
        .unwrap();
    assert!(
        matches!(dwg_doc.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 4.0 && value.text == "POSITION%%v0.2")
    );
    let dxf_bytes = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    let dxf_doc = document::DxfReader::from_reader(std::io::Cursor::new(dxf_bytes))
        .unwrap()
        .read()
        .unwrap();
    assert!(
        matches!(dxf_doc.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 4.0 && value.text == "POSITION%%v0.2")
    );

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 1.0 && value.text == "POSITION%%v0.1")
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_shape_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());

    let shx_path = std::env::temp_dir().join(format!("ocs_host_shape_{}.shx", std::process::id()));
    let mut shx = b"AutoCAD-86 shapes 1.0\r\n\x1A".to_vec();
    for value in [1_u16, 1, 1, 1, 9] {
        shx.extend_from_slice(&value.to_le_bytes());
    }
    shx.extend_from_slice(b"ARROW\0\x10\x14\0");
    std::fs::write(&shx_path, shx).unwrap();

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let style_handle = host.document_mut().allocate_handle();
    let mut style = document::tables::TextStyle::new("TestShapes");
    style.handle = style_handle;
    style.is_shape_file = true;
    style.font_file = shx_path.to_string_lossy().into_owned();
    host.document_mut().text_styles.add(style).unwrap();

    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .expect("spawn staged Python plugin");
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    dispatch(
        &mut host,
        concat!(
            "PY_EVAL ocs.active_document.create_entity('Shape',",
            "insertion_point={'x':1.0,'y':2.0,'z':0.0},size=2.0,",
            "shape_name='ARROW',shape_number=1,style_name='TestShapes').handle"
        ),
    );
    let handle = host
        .document()
        .entities()
        .next()
        .expect("Python created a Shape")
        .common()
        .handle;
    let Some(EntityType::Shape(created)) = host.document().get_entity(handle) else {
        panic!("expected Shape");
    };
    assert_eq!(created.style_handle, Some(style_handle));
    let created = created.clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].shape_name",
            handle.value()
        ),
    );
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("ARROW"));

    let script_path = std::env::temp_dir().join(format!(
        "ocs_shape_document_model_{}.py",
        std::process::id()
    ));
    std::fs::write(
        &script_path,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "shape = doc.entities[{}]\n",
                "with doc.transaction('Edit shape'):\n",
                "    shape.insertion_point = (4.0, 5.0, 0.0)\n",
                "    shape.size = 3.0\n",
                "    shape.rotation = 0.5\n",
                "    shape.relative_x_scale = 1.5\n",
                "doc.selection = [shape]\n"
            ),
            handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
    let _ = std::fs::remove_file(&script_path);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::Shape(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.insertion_point,
        document::types::Vector3::new(4.0, 5.0, 0.0)
    );
    assert_eq!(edited.size, 3.0);
    assert_eq!(edited.rotation, 0.5);
    assert_eq!(edited.relative_x_scale, 1.5);
    assert_eq!(edited.common, created.common);
    assert_eq!(edited.shape_name, created.shape_name);
    assert_eq!(edited.shape_number, created.shape_number);
    assert_eq!(edited.style_name, created.style_name);
    assert_eq!(edited.style_handle, created.style_handle);
    assert_eq!(edited.normal, created.normal);
    assert_eq!(edited.thickness, created.thickness);
    assert_eq!(host.selection(), vec![handle]);

    let assert_real_glyph = |entity: &EntityType, document: &Document| {
        let EntityType::Shape(shape) = entity else {
            panic!("expected Shape");
        };
        let render = shape.to_render(document).expect("shape render");
        let crate::scene::convert::acad_to_render::RenderObject::Lines(points) = render.object
        else {
            panic!("expected shape linework");
        };
        assert_eq!(
            points.len(),
            3,
            "expected SHX glyph rather than diamond placeholder"
        );
        assert!(points.iter().flatten().all(|value| value.is_finite()));
    };
    assert_real_glyph(&expected, host.document());

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject size', [{{'handle':{},'size':0.0}}])",
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject style', [{{'handle':{},'style_name':'Missing'}}])",
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("does not exist"));

    let dwg_bytes = document::DwgWriter::write_to_vec(host.document()).unwrap();
    let dwg_doc = crate::io::load_bytes("shape.dwg", dwg_bytes).unwrap();
    let dwg_entity = dwg_doc.get_entity(handle).expect("Shape survives DWG");
    assert!(matches!(dwg_entity, EntityType::Shape(value)
            if value.shape_number == 1 && value.size == 3.0 && value.rotation == 0.5));
    assert_real_glyph(dwg_entity, &dwg_doc);
    let dxf_bytes = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    let dxf_doc = crate::io::load_bytes("shape.dxf", dxf_bytes).unwrap();
    let dxf_entity = dxf_doc.get_entity(handle).expect("Shape survives DXF");
    assert!(matches!(dxf_entity, EntityType::Shape(value)
            if value.shape_name == "ARROW" && value.size == 3.0 && (value.rotation - 0.5).abs() < 1e-12));
    assert_real_glyph(dxf_entity, &dxf_doc);

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    drop(process);
    drop(host);
    let _ = std::fs::remove_file(&shx_path);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Shape(value))
            if value.insertion_point.x == 1.0 && value.size == 2.0)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_attribute_definition_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let next = host.document().next_handle();
    let block_record_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let block_end_handle = Handle::new(next + 2);
    let mut block_record = document::tables::BlockRecord::new("TAGBLOCK");
    block_record.handle = block_record_handle;
    block_record.block_entity_handle = block_handle;
    block_record.block_end_handle = block_end_handle;
    host.document_mut().block_records.add(block_record).unwrap();
    let mut block = document::entities::Block::new("TAGBLOCK", document::types::Vector3::ZERO);
    block.common.handle = block_handle;
    block.common.owner_handle = block_record_handle;
    host.document_mut()
        .add_entity(EntityType::Block(block))
        .unwrap();
    let mut block_end = document::entities::BlockEnd::new();
    block_end.common.handle = block_end_handle;
    block_end.common.owner_handle = block_record_handle;
    host.document_mut()
        .add_entity(EntityType::BlockEnd(block_end))
        .unwrap();
    let insert =
        document::entities::Insert::new("TAGBLOCK", document::types::Vector3::new(10.0, 0.0, 0.0));
    let insert_handle = host
        .document_mut()
        .add_entity(EntityType::Insert(insert))
        .unwrap();

    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .expect("spawn staged Python plugin");
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.active_document.create_entity('AttributeDefinition',",
                "owner_handle={},tag='PART_NO',prompt='Part number',default_value='PN-001',",
                "insertion_point={{'x':1.0,'y':2.0,'z':0.0}},height=2.5).handle"
            ),
            block_record_handle.value()
        ),
    );
    let handle = host
        .document()
        .entities()
        .find(|entity| matches!(entity, EntityType::AttributeDefinition(_)))
        .expect("Python created an AttributeDefinition")
        .common()
        .handle;
    let Some(EntityType::AttributeDefinition(created)) = host.document().get_entity(handle) else {
        panic!("expected AttributeDefinition");
    };
    assert_eq!(created.common.owner_handle, block_record_handle);
    assert_eq!(created.tag, "PART_NO");
    assert_eq!(created.text_style, "Standard");
    assert!(host
        .document()
        .block_records
        .get("TAGBLOCK")
        .unwrap()
        .entity_handles
        .contains(&handle));
    let created = created.clone();
    dispatch(&mut host, &format!(
            "PY_EVAL (ocs.active_document.entities[{}].tag, ocs.active_document.entities[{}].owner_handle)",
            handle.value(), handle.value()));
    let readback = &host.app.command_line.history.last().unwrap().text;
    assert!(
        readback.contains("PART_NO") && readback.contains(&block_record_handle.value().to_string())
    );

    let script_path = std::env::temp_dir().join(format!(
        "ocs_attribute_definition_document_model_{}.py",
        std::process::id()
    ));
    std::fs::write(
        &script_path,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "definition = doc.entities[{}]\n",
                "with doc.transaction('Edit attribute definition'):\n",
                "    definition.insertion_point = (4.0, 5.0, 0.0)\n",
                "    definition.default_value = 'PN-002'\n",
                "    definition.rotation = 0.25\n",
                "    definition.width_factor = 1.25\n",
                "doc.selection = [definition]\n"
            ),
            handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
    let _ = std::fs::remove_file(&script_path);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::AttributeDefinition(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.insertion_point,
        document::types::Vector3::new(4.0, 5.0, 0.0)
    );
    assert_eq!(edited.default_value, "PN-002");
    assert_eq!(edited.rotation, 0.25);
    assert_eq!(edited.width_factor, 1.25);
    assert_eq!(edited.common, created.common);
    assert_eq!(edited.tag, created.tag);
    assert_eq!(edited.prompt, created.prompt);
    assert_eq!(edited.text_style, created.text_style);
    assert_eq!(edited.flags, created.flags);
    assert_eq!(edited.embedded_mtext, created.embedded_mtext);
    assert_eq!(host.selection(), vec![handle]);
    assert!(
        matches!(host.document().get_entity(insert_handle), Some(EntityType::Insert(value))
            if value.block_name == "TAGBLOCK")
    );

    let render = edited
        .to_render(host.document())
        .expect("attribute definition render");
    let crate::scene::convert::acad_to_render::RenderObject::Text(strokes) = render.object else {
        panic!("expected attribute definition text");
    };
    assert!(!strokes.is_empty());
    assert!(strokes.iter().any(|stroke| stroke
        .run
        .as_ref()
        .is_some_and(|run| run.text.contains("PART_NO"))));

    for (label, patch, message) in [
        ("Reject tag", "'tag':'BAD TAG'", "whitespace"),
        ("Reject height", "'height':0.0", "greater than zero"),
        ("Reject style", "'text_style':'Missing'", "does not exist"),
        ("Reject owner", "'owner_handle':999999", "read-only"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many({label:?}, [{{'handle':{}, {patch}}}])",
                handle.value()
            ),
        );
        assert_eq!(host.document().get_entity(handle), Some(&expected));
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains(message));
    }

    let assert_persisted = |label: &str, document: &Document, full_text_state: bool| {
        let actual = document.get_entity(handle);
        assert!(
            matches!(actual, Some(EntityType::AttributeDefinition(value))
                if value.common.owner_handle == block_record_handle
                    && value.tag == "PART_NO" && value.default_value == "PN-002"
                    && value.insertion_point == document::types::Vector3::new(4.0, 5.0, 0.0)
                    && (value.rotation - 0.25).abs() < 1e-12),
            "{label}: {actual:?}"
        );
        if full_text_state {
            assert!(
                matches!(actual, Some(EntityType::AttributeDefinition(value))
                    if value.width_factor == 1.25 && value.text_style == "Standard"
                        && value.prompt == "Part number"),
                "{label}: {actual:?}"
            );
        }
        assert!(document
            .block_records
            .get("TAGBLOCK")
            .unwrap()
            .entity_handles
            .contains(&handle));
        assert!(
            matches!(document.get_entity(insert_handle), Some(EntityType::Insert(value))
                if value.block_name == "TAGBLOCK")
        );
    };
    let dwg_bytes = document::DwgWriter::write_to_vec(host.document()).unwrap();
    let dwg_doc = crate::io::load_bytes("attribute-definition.dwg", dwg_bytes).unwrap();
    assert_persisted("DWG", &dwg_doc, true);
    let dxf_bytes = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    let dxf_doc = crate::io::load_bytes("attribute-definition.dxf", dxf_bytes).unwrap();
    // The pinned document DXF ATTDEF reader retains identity, placement,
    // height, value, prompt and rotation, but currently drops several
    // optional AcDbText fields such as width factor. Keep that external
    // codec gap explicit in the coverage ledger rather than claiming a
    // complete DXF property round trip here.
    assert_persisted("DXF", &dxf_doc, false);

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    // Document intentionally leaves the block membership slot in place
    // so delta undo can restore the shared entity without rewriting the
    // record. Writers must still omit the absent entity.
    let deleted_dwg = document::DwgWriter::write_to_vec(host.document()).unwrap();
    assert!(
        crate::io::load_bytes("attribute-definition-deleted.dwg", deleted_dwg)
            .unwrap()
            .get_entity(handle)
            .is_none()
    );
    let deleted_dxf = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    assert!(
        crate::io::load_bytes("attribute-definition-deleted.dxf", deleted_dxf)
            .unwrap()
            .get_entity(handle)
            .is_none()
    );
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert!(matches!(app.tabs[0].scene.document.get_entity(handle),
            Some(EntityType::AttributeDefinition(value))
                if value.default_value == "PN-001" && value.insertion_point.x == 1.0));
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_attribute_entity_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let next = host.document().next_handle();
    let record_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let end_handle = Handle::new(next + 2);
    let mut record = document::tables::BlockRecord::new("TAGBLOCK");
    record.handle = record_handle;
    record.block_entity_handle = block_handle;
    record.block_end_handle = end_handle;
    host.document_mut().block_records.add(record).unwrap();
    let mut block = document::entities::Block::new("TAGBLOCK", document::types::Vector3::ZERO);
    block.common.handle = block_handle;
    block.common.owner_handle = record_handle;
    host.document_mut()
        .add_entity(EntityType::Block(block))
        .unwrap();
    let mut end = document::entities::BlockEnd::new();
    end.common.handle = end_handle;
    end.common.owner_handle = record_handle;
    host.document_mut()
        .add_entity(EntityType::BlockEnd(end))
        .unwrap();
    let mut definition = document::entities::AttributeDefinition::new(
        "PART_NO".into(),
        "Part number".into(),
        "PN-001".into(),
    );
    definition.common.owner_handle = record_handle;
    definition.insertion_point = document::types::Vector3::new(0.0, 2.0, 0.0);
    let definition_handle = host
        .document_mut()
        .add_entity(EntityType::AttributeDefinition(definition))
        .unwrap();
    let insert_handle = host
        .document_mut()
        .add_entity(EntityType::Insert(document::entities::Insert::new(
            "TAGBLOCK",
            document::types::Vector3::new(10.0, 0.0, 0.0),
        )))
        .unwrap();

    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.active_document.create_entity('AttributeEntity',owner_handle={},",
                "tag='PART_NO',value='PN-101',",
                "insertion_point={{'x':10.0,'y':2.0,'z':0.0}},height=2.5).handle"
            ),
            insert_handle.value()
        ),
    );
    let created_insert = match host.document().get_entity(insert_handle).unwrap() {
        EntityType::Insert(insert) => insert,
        _ => unreachable!(),
    };
    assert_eq!(created_insert.attributes.len(), 1);
    let attribute_handle = created_insert.attributes[0].common.handle;
    assert_eq!(
        created_insert.attributes[0].attdef_handle,
        definition_handle
    );
    let created = created_insert.attributes[0].clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].value",
            attribute_handle.value()
        ),
    );
    let readback = &host.app.command_line.history.last().unwrap().text;
    assert!(readback.contains("PN-101"), "{readback}");

    let script =
        std::env::temp_dir().join(format!("ocs_attribute_entity_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "attribute = doc.entities[{}]\n",
                "with doc.transaction('Edit attribute'):\n",
                "    attribute.value = 'PN-102'\n",
                "    attribute.insertion_point = (12.0, 3.0, 0.0)\n",
                "    attribute.rotation = 0.25\n",
                "doc.selection = [doc.entities[{}]]\n"
            ),
            attribute_handle.value(),
            insert_handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.nested_attribute(attribute_handle).unwrap();
    let EntityType::AttributeEntity(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(edited.value, "PN-102");
    assert_eq!(
        edited.insertion_point,
        document::types::Vector3::new(12.0, 3.0, 0.0)
    );
    assert_eq!(edited.common, created.common);
    assert_eq!(edited.attdef_handle, definition_handle);
    assert_eq!(host.selection(), vec![insert_handle]);
    let render = edited.to_render(host.document()).unwrap();
    assert!(matches!(render.object,
            crate::scene::convert::acad_to_render::RenderObject::Text(ref strokes)
                if !strokes.is_empty()));

    for (patch, message) in [
        ("'tag':'MISSING'", "no definition"),
        ("'height':0.0", "greater than zero"),
        ("'owner_handle':999999", "read-only"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject attribute', [{{'handle':{}, {patch}}}])",
                attribute_handle.value()
            ),
        );
        assert_eq!(
            host.nested_attribute(attribute_handle),
            Some(expected.clone())
        );
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains(message));
    }

    let assert_saved = |document: &Document| {
        assert!(
            matches!(document.get_entity(insert_handle), Some(EntityType::Insert(insert))
                if insert.attributes.len() == 1
                    && insert.attributes[0].common.handle == attribute_handle
                    && insert.attributes[0].value == "PN-102"
                    && (insert.attributes[0].rotation - 0.25).abs() < 1e-12)
        );
    };
    assert_saved(
        &crate::io::load_bytes(
            "attribute.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        )
        .unwrap(),
    );
    assert_saved(
        &crate::io::load_bytes(
            "attribute.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        )
        .unwrap(),
    );

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            attribute_handle.value()
        ),
    );
    assert!(host.nested_attribute(attribute_handle).is_none());
    let assert_deleted = |document: &Document| {
        assert!(
            matches!(document.get_entity(insert_handle), Some(EntityType::Insert(insert))
                    if insert.attributes.is_empty())
        );
    };
    assert_deleted(
        &crate::io::load_bytes(
            "attribute-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        )
        .unwrap(),
    );
    assert_deleted(
        &crate::io::load_bytes(
            "attribute-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        )
        .unwrap(),
    );
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.len() == 1
                && insert.attributes[0].value == "PN-102")
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes[0].value == "PN-101")
    );
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
    );
    app.redo_steps(3);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
    );
}

#[test]
fn staged_python_hatch_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let script = std::env::temp_dir().join(format!("ocs_hatch_create_{}.py", std::process::id()));
    std::fs::write(&script, concat!(
            "def p(x, y): return {'x': x, 'y': y}\n",
            "def boundary(size):\n",
            "    pairs = [(0.0,0.0,size,0.0),(size,0.0,size,size),(size,size,0.0,size),(0.0,size,0.0,0.0)]\n",
            "    edges = [{'kind':'Line','value':{'start':p(a,b),'end':p(c,d)}} for a,b,c,d in pairs]\n",
            "    return {'flags':1,'edges':edges,'boundary_handles':[]}\n",
            "doc = ocs.active_document\n",
            "hatch = doc.create_entity('Hatch', paths=[boundary(10.0)])\n",
        )).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let hatch_handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(hatch.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "Python did not create Hatch: {:?}",
                host.app
                    .command_line
                    .history
                    .last()
                    .map(|entry| &entry.text)
            )
        });
    let created = host.document().get_entity(hatch_handle).unwrap().clone();
    assert!(host.app.tabs[0]
        .scene
        .hatches
        .get(&hatch_handle)
        .is_some_and(|model| model.boundary.len() >= 4 && model.name == "SOLID"));
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].pattern['name']",
            hatch_handle.value()
        ),
    );
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("SOLID"));

    let script = std::env::temp_dir().join(format!("ocs_hatch_edit_{}.py", std::process::id()));
    std::fs::write(&script, format!(concat!(
            "def p(x, y): return {{'x': x, 'y': y}}\n",
            "def boundary(size):\n",
            "    pairs = [(0.0,0.0,size,0.0),(size,0.0,size,size),(size,size,0.0,size),(0.0,size,0.0,0.0)]\n",
            "    edges = [{{'kind':'Line','value':{{'start':p(a,b),'end':p(c,d)}}}} for a,b,c,d in pairs]\n",
            "    return {{'flags':1,'edges':edges,'boundary_handles':[]}}\n",
            "doc = ocs.active_document\n",
            "hatch = doc.entities[{}]\n",
            "with doc.transaction('Edit hatch'):\n",
            "    hatch.paths = [boundary(12.0)]\n",
            "    hatch.is_solid = False\n",
            "    hatch.pattern = {{'name':'TEST','description':'test pattern','lines':[",
            "{{'angle':0.0,'base_point':p(0.0,0.0),'offset':p(0.0,2.0),'dash_lengths':[1.0,-1.0]}}]}}\n",
            "    hatch.pattern_scale = 2.0\n",
            "    hatch.pattern_angle = 0.25\n",
            "doc.selection = [hatch]\n"
        ), hatch_handle.value())).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.document().get_entity(hatch_handle).unwrap().clone();
    let EntityType::Hatch(edited) = &expected else {
        unreachable!()
    };
    assert!(!edited.is_solid);
    assert_eq!(edited.pattern.name, "TEST");
    assert_eq!(edited.pattern.lines.len(), 1);
    assert_eq!(edited.paths[0].edges.len(), 4);
    assert!((edited.pattern_scale - 2.0).abs() < 1e-12);
    assert_eq!(
        edited.gradient_color,
        match &created {
            EntityType::Hatch(value) => value.gradient_color.clone(),
            _ => unreachable!(),
        }
    );
    assert_eq!(host.selection(), vec![hatch_handle]);
    assert!(host.app.tabs[0]
        .scene
        .hatches
        .get(&hatch_handle)
        .is_some_and(|model| model.boundary.len() >= 4
            && model.name == "TEST"
            && matches!(&model.pattern,
                    crate::scene::model::hatch_model::HatchPattern::Pattern(lines)
                        if !lines.is_empty())));

    for (patch, message) in [
            ("'pattern_scale':0.0", "greater than zero"),
            ("'is_associative':True", "requires boundary handles"),
            ("'gradient_color':{}", "outside the editable schema"),
            ("'paths':[{'flags':1,'edges':[{'kind':'Line','value':{'start':{'x':0.0,'y':0.0},'end':{'x':1.0,'y':0.0}}}],'boundary_handles':[]}]", "not closed"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject hatch', [{{'handle':{}, {patch}}}])",
                hatch_handle.value()));
            assert_eq!(host.document().get_entity(hatch_handle), Some(&expected));
            assert!(host.app.command_line.history.last().unwrap().text.contains(message));
        }

    let assert_saved = |document: &Document| {
        assert!(
            matches!(document.get_entity(hatch_handle), Some(EntityType::Hatch(hatch))
                if !hatch.is_solid && hatch.pattern.name == "TEST"
                    && hatch.pattern.lines.len() == 1 && hatch.paths[0].edges.len() == 4
                    && (hatch.pattern_scale - 2.0).abs() < 1e-12)
        );
    };
    let reopened_dwg = crate::io::load_bytes(
        "hatch.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let reopened_dxf = crate::io::load_bytes(
        "hatch.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    assert_saved(&reopened_dwg);
    assert_saved(&reopened_dxf);
    for (name, mut document) in [
        ("hatch-reedit.dwg", reopened_dwg),
        ("hatch-reedit.dxf", reopened_dxf),
    ] {
        let before = document.get_entity(hatch_handle).unwrap().clone();
        let mut after = before.clone();
        let EntityType::Hatch(hatch) = &mut after else {
            unreachable!()
        };
        hatch.elevation = 3.0;
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        *document.get_entity_mut(hatch_handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        assert!(
            matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(hatch_handle),
                Some(EntityType::Hatch(hatch)) if (hatch.elevation - 3.0).abs() < 1e-12)
        );
    }
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            hatch_handle.value()
        ),
    );
    assert!(host.document().get_entity(hatch_handle).is_none());
    for (name, bytes) in [
        (
            "hatch-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "hatch-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        assert!(crate::io::load_bytes(name, bytes)
            .unwrap()
            .get_entity(hatch_handle)
            .is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(hatch_handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(hatch_handle),
        Some(&created)
    );
    app.undo_steps(1);
    assert!(app.tabs[0]
        .scene
        .document
        .get_entity(hatch_handle)
        .is_none());
    app.redo_steps(3);
    assert!(app.tabs[0]
        .scene
        .document
        .get_entity(hatch_handle)
        .is_none());
}

#[test]
fn staged_python_leader_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    dispatch(
        &mut host,
        concat!(
            "PY_EVAL ocs.active_document.create_entity('Text', text='NOTE', ",
            "insertion={'x':20.0,'y':10.0,'z':0.0}, height=2.5).handle"
        ),
    );
    let text_handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::Text(text) => Some(text.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Python did not create Text: {}", last_output(&host)));
    let script = std::env::temp_dir().join(format!("ocs_leader_create_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "doc.create_entity('Leader', vertices=[{{'x':0.0,'y':0.0,'z':0.0}},",
                "{{'x':10.0,'y':10.0,'z':0.0}},{{'x':20.0,'y':10.0,'z':0.0}}], ",
                "annotation_handle={}, hookline_enabled=True)\n"
            ),
            text_handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let leader_handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::Leader(leader) => Some(leader.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Python did not create Leader: {}", last_output(&host)));
    let created = host.document().get_entity(leader_handle).unwrap().clone();
    let EntityType::Leader(created_leader) = &created else {
        unreachable!()
    };
    assert_eq!(created_leader.annotation_handle, text_handle);
    assert_eq!(created_leader.vertices.len(), 3);

    let assert_geometry = |entity: &EntityType, document: &Document| {
        let EntityType::Leader(leader) = entity else {
            panic!("expected Leader")
        };
        let render = leader.to_render(document).expect("leader render");
        let crate::scene::convert::acad_to_render::RenderObject::Lines(points) = render.object
        else {
            panic!("expected leader linework");
        };
        assert!(points.len() >= leader.vertices.len());
        assert!(points
            .iter()
            .flatten()
            .all(|value| value.is_nan() || value.is_finite()));
    };
    assert_geometry(&created, host.document());
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].annotation_handle",
            leader_handle.value()
        ),
    );
    assert!(last_output(&host).contains(&text_handle.value().to_string()));

    // GUI picking groups a leader with its annotation, while scripted
    // selection stays exact.
    host.app.tabs[0].scene.select_entities(&[leader_handle]);
    let mut grouped = host.selection();
    grouped.sort_by_key(|handle| handle.value());
    assert_eq!(grouped, vec![text_handle, leader_handle]);
    host.app.tabs[0].scene.replace_selection_exact(&[]);

    let script = std::env::temp_dir().join(format!("ocs_leader_edit_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "leader = doc.entities[{}]\n",
                "with doc.transaction('Edit leader'):\n",
                "    leader.vertices = [{{'x':1.0,'y':1.0,'z':0.0}},{{'x':11.0,'y':12.0,'z':0.0}},",
                "{{'x':22.0,'y':12.0,'z':0.0}},{{'x':30.0,'y':12.0,'z':0.0}}]\n",
                "    leader.text_height = 4.0\n",
                "    leader.arrow_enabled = False\n",
                "    leader.annotation_offset = (1.0, 2.0, 0.0)\n",
                "    leader.override_color = {{'kind':'Rgb','value':{{'r':255,'g':0,'b':0}}}}\n",
                "doc.selection = [leader]\n"
            ),
            leader_handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.document().get_entity(leader_handle).unwrap().clone();
    let EntityType::Leader(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.vertices.len(),
        4,
        "edit failed: {}",
        last_output(&host)
    );
    assert_eq!(edited.text_height, 4.0);
    assert!(!edited.arrow_enabled);
    assert_eq!(
        edited.annotation_offset,
        document::types::Vector3::new(1.0, 2.0, 0.0)
    );
    assert_eq!(edited.annotation_handle, text_handle);
    assert_eq!(edited.common, created_leader.common);
    assert_eq!(edited.dimension_style, created_leader.dimension_style);
    assert_eq!(edited.hookline_enabled, created_leader.hookline_enabled);
    assert_eq!(edited.normal, created_leader.normal);
    assert_eq!(edited.origin, created_leader.origin);
    assert_eq!(
        edited.override_color,
        document::types::Color::Rgb { r: 255, g: 0, b: 0 }
    );
    assert_eq!(host.selection(), vec![leader_handle]);
    assert_geometry(&expected, host.document());

    for (patch, message) in [
        ("'annotation_handle':999999", "does not exist"),
        (
            "'creation_type':'WithTolerance'",
            "does not match creation_type",
        ),
        ("'creation_type':'NoAnnotation'", "no annotation"),
        ("'vertices':[{'x':0.0,'y':0.0,'z':0.0}]", "at least 2"),
        ("'text_height':0.0", "greater than zero"),
        ("'dimension_style':'Missing'", "dimension style"),
        ("'override_color':{'kind':'Index','value':300}", "Color"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject leader', [{{'handle':{}, {patch}}}])",
                leader_handle.value()
            ),
        );
        assert_eq!(
            host.document().get_entity(leader_handle),
            Some(&expected),
            "{patch}"
        );
        assert!(
            last_output(&host).contains(message),
            "{patch}: {}",
            last_output(&host)
        );
    }
    // A valid edit paired with an invalid one is atomic across entities.
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'text_width':5.0}},",
                "{{'handle':{},'text_height':0.0}}])"
            ),
            leader_handle.value(),
            text_handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(leader_handle), Some(&expected));

    // Both formats keep the path, annotation link, arrow flag and offset.
    // DWG R2010+ derives text_height/text_width/hookline_enabled instead of
    // storing them (document writes them for R13-R2007/R13-R14 only), so
    // only DXF must round-trip those three.
    let assert_saved = |document: &Document, is_dxf: bool| {
        let Some(EntityType::Leader(leader)) = document.get_entity(leader_handle) else {
            panic!("Leader missing after reopen");
        };
        assert_eq!(leader.vertices.len(), 4);
        assert_eq!(leader.annotation_handle, text_handle);
        assert!(!leader.arrow_enabled);
        // The script sets a true colour. DXF's LEADER group 77 holds an ACI
        // index only, so it cannot carry one, and the DWG writer does not
        // store `override_color` at all (BLOCKER, document): it reopens as
        // ByLayer in both formats.
        assert_eq!(
            leader.override_color,
            document::types::Color::ByLayer,
            "document now persists a true-colour Leader.override_color; drop the blocker"
        );
        assert_eq!(
            leader.annotation_offset,
            document::types::Vector3::new(1.0, 2.0, 0.0)
        );
        assert_eq!(leader.dimension_style, "Standard");
        if is_dxf {
            assert!((leader.text_height - 4.0).abs() < 1e-12);
            assert!(leader.hookline_enabled);
        } else {
            assert_eq!(
                leader.text_height, 2.5,
                "DWG R2010+ does not store text_height"
            );
            assert!(
                !leader.hookline_enabled,
                "DWG R2010+ does not store hookline_enabled"
            );
        }
        assert!(matches!(
            document.get_entity(text_handle),
            Some(EntityType::Text(_))
        ));
    };
    let reopened_dwg = crate::io::load_bytes(
        "leader.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let reopened_dxf = crate::io::load_bytes(
        "leader.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    assert_saved(&reopened_dwg, false);
    assert_saved(&reopened_dxf, true);
    for (name, mut document) in [
        ("leader-reedit.dwg", reopened_dwg),
        ("leader-reedit.dxf", reopened_dxf),
    ] {
        assert_geometry(document.get_entity(leader_handle).unwrap(), &document);
        let before = document.get_entity(leader_handle).unwrap().clone();
        let mut after = before.clone();
        let EntityType::Leader(leader) = &mut after else {
            unreachable!()
        };
        leader.vertices[3] = document::types::Vector3::new(35.0, 12.0, 0.0);
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        *document.get_entity_mut(leader_handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        assert!(
            matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(leader_handle),
                Some(EntityType::Leader(leader)) if (leader.vertices[3].x - 35.0).abs() < 1e-9)
        );
    }

    // Erasing a leader also erases its annotation (scene leader grouping).
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            leader_handle.value()
        ),
    );
    assert!(host.document().get_entity(leader_handle).is_none());
    assert!(host.document().get_entity(text_handle).is_none());
    for (name, bytes) in [
        (
            "leader-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "leader-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        let document = crate::io::load_bytes(name, bytes).unwrap();
        assert!(document.get_entity(leader_handle).is_none());
        assert!(document.get_entity(text_handle).is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    // Text create, Leader create, edit transaction, Leader delete.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 4);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(leader_handle),
        Some(&expected)
    );
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_some());
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(leader_handle),
        Some(&created)
    );
    app.undo_steps(1);
    assert!(app.tabs[0]
        .scene
        .document
        .get_entity(leader_handle)
        .is_none());
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_some());
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_none());
    app.redo_steps(4);
    assert!(app.tabs[0]
        .scene
        .document
        .get_entity(leader_handle)
        .is_none());
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_none());
}

#[test]
fn staged_python_mline_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let style_handle = host
        .document()
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            document::objects::ObjectType::MLineStyle(style) if style.name == "Standard" => {
                Some(*handle)
            }
            _ => None,
        })
        .expect("document carries the Standard MLineStyle");

    let script = std::env::temp_dir().join(format!("ocs_mline_create_{}.py", std::process::id()));
    std::fs::write(
        &script,
        concat!(
            "def p(x, y): return {'position': {'x': x, 'y': y, 'z': 0.0}}\n",
            "doc = ocs.active_document\n",
            "doc.create_entity('MLine', vertices=[p(0.0, 0.0), p(10.0, 0.0)])\n",
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::MLine(mline) => Some(mline.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Python did not create MLine: {}", last_output(&host)));
    let created = host.document().get_entity(handle).unwrap().clone();
    let EntityType::MLine(created_mline) = &created else {
        unreachable!()
    };
    assert_eq!(created_mline.style_handle, Some(style_handle));
    assert_eq!(created_mline.style_element_count, 2);
    assert_eq!(created_mline.start_point, document::types::Vector3::ZERO);
    let first_parameters = |mline: &document::entities::MLine, vertex: usize| -> Vec<f64> {
        mline.vertices[vertex]
            .segments
            .iter()
            .map(|segment| segment.parameters[0])
            .collect()
    };
    assert_eq!(first_parameters(created_mline, 0), vec![0.5, -0.5]);
    assert!(created_mline.vertices.iter().all(|vertex| {
        (vertex.direction.x - 1.0).abs() < 1e-9 && (vertex.miter.y.abs() - 1.0).abs() < 1e-9
    }));
    let assert_lines = |entity: &EntityType, document: &Document, expected_vertices: usize| {
        let EntityType::MLine(mline) = entity else {
            panic!("expected MLine")
        };
        assert_eq!(mline.vertices.len(), expected_vertices);
        let lines = crate::entities::mline::mline_lines(mline, document);
        assert!(lines.len() >= 2, "expected styled element lines");
        assert!(lines
            .iter()
            .flat_map(|line| &line.points)
            .flatten()
            .all(|v| v.is_finite()));
    };
    assert_lines(&created, host.document(), 2);

    let script = std::env::temp_dir().join(format!("ocs_mline_edit_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "def p(x, y): return {{'position': {{'x': x, 'y': y, 'z': 0.0}}}}\n",
                "doc = ocs.active_document\n",
                "mline = doc.entities[{}]\n",
                "with doc.transaction('Edit mline'):\n",
                "    mline.vertices = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]\n",
                "    mline.scale_factor = 2.0\n",
                "    mline.justification = 'Top'\n",
                "doc.selection = [mline]\n"
            ),
            handle.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::MLine(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.vertices.len(),
        3,
        "edit failed: {}",
        last_output(&host)
    );
    assert_eq!(edited.scale_factor, 2.0);
    assert_eq!(
        edited.justification,
        document::entities::MLineJustification::Top
    );
    assert_eq!(edited.style_handle, created_mline.style_handle);
    assert_eq!(edited.common, created_mline.common);
    assert_eq!(edited.normal, created_mline.normal);
    assert_eq!(edited.flags.bits() & !1, 0);
    // Top justification with scale 2: first element sits on the path,
    // the second 2.0 to the right of travel; the corner miter widens it.
    let end = first_parameters(edited, 0);
    assert!(
        end[0].abs() < 1e-9 && (end[1] + 2.0).abs() < 1e-9,
        "{end:?}"
    );
    let corner = first_parameters(edited, 1);
    assert!(
        corner[1].abs() > 2.0 && corner[1].is_finite(),
        "miter must widen at the corner: {corner:?}"
    );
    assert!((edited.vertices[1].direction.y - edited.vertices[1].direction.x).abs() > 1e-6);
    assert_lines(&expected, host.document(), 3);
    assert_eq!(host.selection(), vec![handle]);

    for (patch, message) in [
            ("'style_name':'Missing'", "does not exist"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
            ("'scale_factor':0.0", "nonzero"),
            ("'flags':64", "unknown bits"),
            ("'flags':3,'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "requires at least 3"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "coincide"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':float('nan'),'z':0.0}}]", "finite"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0},'segments':[{}]},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "segments but style"),
            ("'start_point':{'x':5.0,'y':5.0,'z':0.0}", "read-only"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject mline', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'scale_factor':3.0}},",
                "{{'handle':{},'scale_factor':0.0}}])"
            ),
            handle.value(),
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));

    let assert_saved = |document: &Document| {
        let Some(EntityType::MLine(mline)) = document.get_entity(handle) else {
            panic!("MLine missing after reopen");
        };
        assert_eq!(mline.vertices.len(), 3);
        assert_eq!(mline.scale_factor, 2.0);
        assert_eq!(
            mline.justification,
            document::entities::MLineJustification::Top
        );
        assert_eq!(mline.style_name, "Standard");
        assert_eq!(
            mline.vertices[2].position,
            document::types::Vector3::new(10.0, 10.0, 0.0)
        );
        let (a, b) = (first_parameters(mline, 1), first_parameters(edited, 1));
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9),
            "{a:?} vs {b:?}"
        );
        assert_lines(&document.get_entity(handle).unwrap().clone(), document, 3);
    };
    let reopened_dwg = crate::io::load_bytes(
        "mline.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let reopened_dxf = crate::io::load_bytes(
        "mline.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    assert_saved(&reopened_dwg);
    assert_saved(&reopened_dxf);
    for (name, mut document) in [
        ("mline-reedit.dwg", reopened_dwg),
        ("mline-reedit.dxf", reopened_dxf),
    ] {
        let before = document.get_entity(handle).unwrap().clone();
        let mut after = before.clone();
        let EntityType::MLine(mline) = &mut after else {
            unreachable!()
        };
        mline.vertices[2].position = document::types::Vector3::new(10.0, 12.0, 0.0);
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        let EntityType::MLine(mline) = &mut after else {
            unreachable!()
        };
        let old = match &before {
            EntityType::MLine(old) => old,
            _ => unreachable!(),
        };
        crate::entities::mline::normalize_scripted_mline(Some(old), mline, &document).unwrap();
        *document.get_entity_mut(handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        assert!(
            matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::MLine(mline)) if (mline.vertices[2].position.y - 12.0).abs() < 1e-9)
        );
    }

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    for (name, bytes) in [
        (
            "mline-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "mline-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        assert!(crate::io::load_bytes(name, bytes)
            .unwrap()
            .get_entity(handle)
            .is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&created)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_dimension_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    // (subtype, python fields, expected measurement)
    let fixtures: [(&str, &str, f64); 9] = [
            ("Linear", "definition_point=P(10,5), first_point=P(0,0), second_point=P(10,0)", 10.0),
            ("Aligned", "definition_point=P(1,5.5), first_point=P(0,0), second_point=P(3,4)", 5.0),
            ("Radius", "definition_point=P(5,0), angle_vertex=P(0,0)", 5.0),
            ("Diameter", "definition_point=P(10,0), angle_vertex=P(0,0)", 10.0),
            ("Angular3Pt", "definition_point=P(3,3), first_point=P(5,0), second_point=P(0,5), angle_vertex=P(0,0)", std::f64::consts::FRAC_PI_2),
            ("Angular2Ln", "definition_point=P(0,5), dimension_arc=P(3,3), first_point=P(0,0), second_point=P(5,0), angle_vertex=P(0,0)", std::f64::consts::FRAC_PI_2),
            ("Ordinate", "feature_location=P(5,3), leader_endpoint=P(5,8), is_ordinate_type_x=True", 5.0),
            ("Arc", "definition_point=P(3.5,3.5), first_extension_point=P(5,0), second_extension_point=P(0,5), center_point=P(0,0), arc_start_parameter=0.0, arc_end_parameter=1.5707963267948966", 5.0 * std::f64::consts::FRAC_PI_2),
            ("LargeRadial", "definition_point=P(10,0), chord_point=P(4,0)", 6.0),
        ];
    let mut script =
        String::from("def P(x, y): return {'x': x, 'y': y, 'z': 0.0}\ndoc = ocs.active_document\n");
    for (subtype, fields, _) in &fixtures {
        script.push_str(&format!(
            "doc.create_entity('Dimension', subtype='{subtype}', {fields})\n"
        ));
    }
    let path = std::env::temp_dir().join(format!("ocs_dimension_create_{}.py", std::process::id()));
    std::fs::write(&path, script).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", path.display()));
    let _ = std::fs::remove_file(&path);
    let subtype_of = |dimension: &document::entities::Dimension| -> &'static str {
        use document::entities::Dimension as D;
        match dimension {
            D::Aligned(_) => "Aligned",
            D::Linear(_) => "Linear",
            D::Radius(_) => "Radius",
            D::Diameter(_) => "Diameter",
            D::Angular2Ln(_) => "Angular2Ln",
            D::Angular3Pt(_) => "Angular3Pt",
            D::Ordinate(_) => "Ordinate",
            D::Arc(_) => "Arc",
            D::LargeRadial(_) => "LargeRadial",
        }
    };
    let handles_by_subtype = |document: &Document| {
        let mut map = std::collections::BTreeMap::new();
        for entity in document.entities() {
            if let EntityType::Dimension(dimension) = entity {
                map.insert(subtype_of(dimension), dimension.base().common.handle);
            }
        }
        map
    };
    let handles = handles_by_subtype(host.document());
    assert_eq!(
        handles.len(),
        9,
        "created: {handles:?}; {}",
        last_output(&host)
    );
    let measured = |document: &Document, handle: Handle| -> f64 {
        let Some(EntityType::Dimension(dimension)) = document.get_entity(handle) else {
            panic!("Dimension missing");
        };
        dimension.base().actual_measurement
    };
    for (subtype, _, expected) in &fixtures {
        let value = measured(host.document(), handles[subtype]);
        assert!(
            (value - expected).abs() < 1e-6,
            "{subtype}: measured {value}, expected {expected}"
        );
        let Some(EntityType::Dimension(dimension)) = host.document().get_entity(handles[subtype])
        else {
            unreachable!()
        };
        assert_eq!(
            dimension.stored_measurement(),
            value,
            "{subtype}: stored measurement is derived"
        );
        let wires = host.app.tabs[0].scene.wire_models_for(&[handles[subtype]]);
        assert!(
            !wires.is_empty(),
            "{subtype}: dimension produced no canvas geometry"
        );
    }
    let linear = handles["Linear"];
    let created_linear = host.document().get_entity(linear).unwrap().clone();

    // Edit: move a defining point (the measurement follows), change text
    // and style-independent placement; untouched fields survive.
    let script = std::env::temp_dir().join(format!("ocs_dimension_edit_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "linear = doc.entities[{}]\n",
                "radius = doc.entities[{}]\n",
                "with doc.transaction('Edit dimensions'):\n",
                "    linear.second_point = (20.0, 0.0, 0.0)\n",
                "    linear.text = '<> mm'\n",
                "    linear.attachment_point = 'TopCenter'\n",
                "    radius.definition_point = (8.0, 0.0, 0.0)\n",
                "doc.selection = [linear]\n"
            ),
            linear.value(),
            handles["Radius"].value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected_linear = host.document().get_entity(linear).unwrap().clone();
    let EntityType::Dimension(edited) = &expected_linear else {
        unreachable!()
    };
    let EntityType::Dimension(before) = &created_linear else {
        unreachable!()
    };
    assert!(
        (edited.base().actual_measurement - 20.0).abs() < 1e-9,
        "edit failed: {}",
        last_output(&host)
    );
    assert!((measured(host.document(), handles["Radius"]) - 8.0).abs() < 1e-9);
    assert_eq!(edited.base().text, "<> mm");
    assert_eq!(
        edited.base().attachment_point,
        document::entities::AttachmentPointType::TopCenter
    );
    assert_eq!(edited.base().common, before.base().common);
    assert_eq!(edited.base().style_name, before.base().style_name);
    assert_eq!(edited.base().normal, before.base().normal);
    assert_eq!(host.selection(), vec![linear]);
    let expected_radius = host
        .document()
        .get_entity(handles["Radius"])
        .unwrap()
        .clone();
    assert!(!host.app.tabs[0].scene.wire_models_for(&[linear]).is_empty());

    let reject_cases: [(&str, &str, &str); 9] = [
        (
            "Linear",
            "'leader_length':2.0",
            "does not apply to the Linear subtype",
        ),
        ("Linear", "'subtype':'Radius'", "read-only"),
        ("Linear", "'actual_measurement':5.0", "read-only"),
        ("Linear", "'style_name':'Missing'", "does not exist"),
        (
            "Linear",
            "'first_point':{'x':20.0,'y':0.0,'z':0.0}",
            "must not coincide",
        ),
        (
            "Linear",
            "'first_point':{'x':float('nan'),'y':0.0,'z':0.0}",
            "finite",
        ),
        ("Linear", "'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        (
            "Radius",
            "'angle_vertex':{'x':8.0,'y':0.0,'z':0.0}",
            "must not coincide",
        ),
        (
            "Linear",
            "'attachment_point':'Nowhere'",
            "unsupported AttachmentPointType",
        ),
    ];
    for (subtype, patch, message) in reject_cases {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject dimension', [{{'handle':{}, {patch}}}])",
                handles[subtype].value()
            ),
        );
        assert_eq!(
            host.document().get_entity(linear),
            Some(&expected_linear),
            "{patch}"
        );
        assert_eq!(
            host.document().get_entity(handles["Radius"]),
            Some(&expected_radius),
            "{patch}"
        );
        assert!(
            last_output(&host).contains(message),
            "{patch}: {}",
            last_output(&host)
        );
    }
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'text':'changed'}},",
                "{{'handle':{},'style_name':'Missing'}}])"
            ),
            linear.value(),
            handles["Radius"].value()
        ),
    );
    assert_eq!(host.document().get_entity(linear), Some(&expected_linear));
    for (patch, message) in [
            ("ocs.active_document.create_entity('Dimension', definition_point=P(0,0))", "requires subtype"),
            ("ocs.active_document.create_entity('Dimension', subtype='Cone')", "unsupported Dimension subtype"),
            ("ocs.active_document.create_entity('Dimension', subtype='Linear', first_point=P(0,0), second_point=P(1,0))", "requires definition_point"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL (lambda P: {patch})(lambda x, y: {{'x': x, 'y': y, 'z': 0.0}})"));
            assert_eq!(handles_by_subtype(host.document()).len(), 9, "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }

    // DWG and DXF: every subtype is recorded separately.
    let mut failures = Vec::new();
    let dwg = crate::io::load_bytes(
        "dimension.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "dimension.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        let reopened = handles_by_subtype(document);
        for (subtype, _, expected) in &fixtures {
            let expected = match *subtype {
                "Linear" => 20.0,
                "Radius" => 8.0,
                _ => *expected,
            };
            match reopened.get(subtype) {
                None => failures.push(format!("{format} {subtype}: missing")),
                Some(handle) => {
                    let value = measured(document, *handle);
                    if (value - expected).abs() > 1e-6 {
                        failures.push(format!(
                            "{format} {subtype}: measurement {value} != {expected}"
                        ));
                    }
                }
            }
        }
        let Some(EntityType::Dimension(dimension)) = document.get_entity(linear) else {
            failures.push(format!("{format} Linear handle lost"));
            continue;
        };
        if dimension.base().text != "<> mm" {
            failures.push(format!("{format} Linear text {:?}", dimension.base().text));
        }
    }
    assert!(failures.is_empty(), "persistence gaps: {failures:#?}");
    for (name, mut document) in [("dimension-reedit.dwg", dwg), ("dimension-reedit.dxf", dxf)] {
        let before = document.get_entity(linear).unwrap().clone();
        let mut after = before.clone();
        let EntityType::Dimension(dimension) = &mut after else {
            unreachable!()
        };
        if let document::entities::Dimension::Linear(value) = &mut **dimension {
            value.second_point.x = 30.0;
        }
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        let EntityType::Dimension(dimension) = &mut after else {
            unreachable!()
        };
        crate::entities::dimension::normalize_scripted_dimension(dimension);
        *document.get_entity_mut(linear).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        let reopened = crate::io::load_bytes(name, bytes).unwrap();
        assert!((measured(&reopened, linear) - 30.0).abs() < 1e-6, "{name}");
    }

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            linear.value()
        ),
    );
    assert!(host.document().get_entity(linear).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(linear),
        Some(&expected_linear)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(linear),
        Some(&created_linear)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(linear).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(linear).is_none());
}

#[test]
fn staged_python_multileader_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    let script = std::env::temp_dir().join(format!("ocs_mleader_create_{}.py", std::process::id()));
    std::fs::write(&script, concat!(
            "def P(x, y): return {'x': x, 'y': y, 'z': 0.0}\n",
            "doc = ocs.active_document\n",
            "doc.create_entity('MultiLeader', content_type='MText', context={\n",
            "    'has_text_contents': True, 'text_string': 'NOTE', 'text_location': P(20, 10),\n",
            "    'text_height': 2.5,\n",
            "    'leader_roots': [{'connection_point': P(15, 10), 'direction': P(1, 0), 'landing_distance': 2.0,\n",
            "        'lines': [{'points': [P(0, 0), P(10, 10), P(15, 10)]}]}]})\n",
        )).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::MultiLeader(value) => Some(value.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Python did not create MultiLeader: {}", last_output(&host)));
    let created = host.document().get_entity(handle).unwrap().clone();
    let EntityType::MultiLeader(created_ml) = &created else {
        unreachable!()
    };
    assert_eq!(created_ml.context.text_string, "NOTE");
    assert_eq!(created_ml.context.leader_roots[0].lines[0].points.len(), 3);
    assert!(
        !host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(),
        "no canvas geometry"
    );

    let script = std::env::temp_dir().join(format!("ocs_mleader_edit_{}.py", std::process::id()));
    std::fs::write(&script, format!(concat!(
            "def P(x, y): return {{'x': x, 'y': y, 'z': 0.0}}\n",
            "doc = ocs.active_document\n",
            "ml = doc.entities[{}]\n",
            "roots = ml.context['leader_roots']\n",
            "roots[0]['lines'][0]['points'] = [P(0, 0), P(12, 12), P(18, 12), P(22, 12)]\n",
            "with doc.transaction('Edit multileader'):\n",
            "    ml.context = {{'leader_roots': roots, 'text_string': 'EDITED', 'text_location': P(26, 12), 'text_height': 3.0}}\n",
            "    ml.dogleg_length = 3.0\n",
            "    ml.line_color = {{'kind': 'Rgb', 'value': {{'r': 0, 'g': 128, 'b': 255}}}}\n",
            "doc.selection = [ml]\n"
        ), handle.value())).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::MultiLeader(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(
        edited.context.text_string,
        "EDITED",
        "edit failed: {}",
        last_output(&host)
    );
    assert_eq!(edited.context.leader_roots[0].lines[0].points.len(), 4);
    assert_eq!(edited.dogleg_length, 3.0);
    assert_eq!(edited.context.text_height, 3.0);
    assert_eq!(edited.text_height, created_ml.text_height);
    assert_eq!(
        edited.line_color,
        document::types::Color::Rgb {
            r: 0,
            g: 128,
            b: 255
        }
    );
    // Untouched context fields and the entity identity survive a partial patch.
    assert_eq!(edited.common, created_ml.common);
    assert_eq!(edited.context.scale_factor, created_ml.context.scale_factor);
    assert_eq!(edited.content_type, created_ml.content_type);
    assert_eq!(
        edited.context.leader_roots[0].connection_point,
        created_ml.context.leader_roots[0].connection_point
    );
    assert_eq!(host.selection(), vec![handle]);
    assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());

    for (patch, message) in [
        ("'text_height':0.0", "read-only"),
        ("'scale_factor':-1.0", "greater than zero"),
        ("'context':{'text_height':float('inf')}", "finite"),
        ("'style_handle':999999", "does not exist"),
        ("'text_style_handle':999999", "does not exist"),
        ("'content_type':'None'", "cannot carry"),
        ("'context':{'nonsense':1}", "outside the editable schema"),
        (
            "'context':{'leader_roots':[{'lines':[{'points':[]}]}]}",
            "has no points",
        ),
        (
            "'context':{'text_location':{'x':float('nan'),'y':0.0,'z':0.0}}",
            "finite",
        ),
        ("'dwg_version':5", "read-only"),
        ("'line_color':{'kind':'Index','value':300}", "Color"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject multileader', [{{'handle':{}, {patch}}}])",
                handle.value()
            ),
        );
        assert_eq!(
            host.document().get_entity(handle),
            Some(&expected),
            "{patch}"
        );
        assert!(
            last_output(&host).contains(message),
            "{patch}: {}",
            last_output(&host)
        );
    }
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'dogleg_length':9.0}},",
                "{{'handle':{},'scale_factor':-1.0}}])"
            ),
            handle.value(),
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));

    let mut gaps = Vec::new();
    let dwg = crate::io::load_bytes(
        "mleader.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "mleader.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        let Some(EntityType::MultiLeader(value)) = document.get_entity(handle) else {
            gaps.push(format!("{format}: MultiLeader missing"));
            continue;
        };
        if value.context.text_string != "EDITED" {
            gaps.push(format!("{format}: text {:?}", value.context.text_string));
        }
        let points = value
            .context
            .leader_roots
            .first()
            .and_then(|r| r.lines.first())
            .map_or(0, |l| l.points.len());
        if points != 4 {
            gaps.push(format!("{format}: leader points {points}"));
        }
        if value.dogleg_length != 3.0 {
            gaps.push(format!("{format}: dogleg {}", value.dogleg_length));
        }
        if value.context.text_height != edited.context.text_height {
            gaps.push(format!(
                "{format}: context.text_height {}",
                value.context.text_height
            ));
        }
        if value.line_color != edited.line_color {
            gaps.push(format!("{format}: line_color {:?}", value.line_color));
        }
    }
    assert!(gaps.is_empty(), "persistence gaps: {gaps:#?}");
    for (name, mut document) in [("mleader-reedit.dwg", dwg), ("mleader-reedit.dxf", dxf)] {
        let before = document.get_entity(handle).unwrap().clone();
        let mut after = before.clone();
        let EntityType::MultiLeader(value) = &mut after else {
            unreachable!()
        };
        value.dogleg_length = 4.5;
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        *document.get_entity_mut(handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        assert!(
            matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::MultiLeader(value)) if value.dogleg_length == 4.5),
            "{name}"
        );
    }

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    for (name, bytes) in [
        (
            "mleader-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "mleader-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        assert!(crate::io::load_bytes(name, bytes)
            .unwrap()
            .get_entity(handle)
            .is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&created)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_table_lifecycle_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    dispatch(&mut host, "PY_EVAL ocs.active_document.create_entity('Table', insertion_point={'x':5.0,'y':5.0,'z':0.0}).handle");
    let handle = host
        .document()
        .entities()
        .find_map(|entity| match entity {
            EntityType::Table(value) => Some(value.common.handle),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Python did not create Table: {}", last_output(&host)));
    let created = host.document().get_entity(handle).unwrap().clone();
    let EntityType::Table(created_table) = &created else {
        unreachable!()
    };
    assert_eq!(
        (created_table.rows.len(), created_table.columns.len()),
        (3, 3)
    );
    assert_eq!(
        created_table.insertion_point,
        document::types::Vector3::new(5.0, 5.0, 0.0)
    );
    assert!(
        !host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(),
        "no canvas geometry"
    );

    let script = std::env::temp_dir().join(format!("ocs_table_edit_{}.py", std::process::id()));
    std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "t = doc.entities[{}]\n",
            "rows = t.rows\n",
            "cols = t.columns\n",
            "rows.append(dict(rows[0]))\n",
            "cols[0]['width'] = 30.0\n",
            "rows[1]['cells'][1]['contents'] = [{{'content_type': 'Value', 'scale': 1.0, 'text_height': 2.5,\n",
            "    'value': {{'value_type': 'String', 'raw_type_code': 4, 'text': 'Hello', 'formatted_value': 'Hello'}}}}]\n",
            "with doc.transaction('Edit table'):\n",
            "    t.rows = rows\n",
            "    t.columns = cols\n",
            "    t.merged_ranges = [{{'top_row': 0, 'left_col': 0, 'bottom_row': 0, 'right_col': 2}}]\n",
            "    t.break_spacing = 2.0\n",
            "doc.selection = [t]\n"
        ), handle.value())).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let expected = host.document().get_entity(handle).unwrap().clone();
    let EntityType::Table(edited) = &expected else {
        unreachable!()
    };
    assert_eq!(edited.rows.len(), 4, "edit failed: {}", last_output(&host));
    assert_eq!(edited.columns[0].width, 30.0);
    assert_eq!(edited.rows[1].cells[1].contents[0].value.text, "Hello");
    assert_eq!(edited.merged_ranges.len(), 1);
    assert_eq!(edited.break_spacing, 2.0);
    assert_eq!(edited.common, created_table.common);
    assert_eq!(edited.insertion_point, created_table.insertion_point);
    assert_eq!(edited.normal, created_table.normal);
    assert_eq!(edited.columns[1], created_table.columns[1]);
    assert_eq!(edited.rows[2], created_table.rows[2]);
    assert_eq!(edited.table_style_handle, created_table.table_style_handle);
    assert_eq!(host.selection(), vec![handle]);
    assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());

    for (patch, message) in [
            ("'columns':[{'name':'A','width':0.0},{'width':1.0},{'width':1.0}]", "greater than zero"),
            ("'columns':[{'width':5.0}]", "cells but the table has 1 columns"),
            ("'columns':[]", "at least one row and one column"),
            ("'merged_ranges':[{'top_row':0,'left_col':0,'bottom_row':9,'right_col':0}]", "outside the"),
            ("'merged_ranges':[{'top_row':1,'left_col':0,'bottom_row':2,'right_col':1},{'top_row':2,'left_col':1,'bottom_row':3,'right_col':2}]", "overlaps"),
            ("'table_style_handle':999999", "does not exist"),
            ("'block_name':'x'", "read-only"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("'break_spacing':-1.0", "non-negative"),
            ("'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject table', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
    dispatch(
        &mut host,
        &format!(
            concat!(
                "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'break_spacing':7.0}},",
                "{{'handle':{},'break_spacing':-1.0}}])"
            ),
            handle.value(),
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));

    let mut gaps = Vec::new();
    let dwg = crate::io::load_bytes(
        "table.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "table.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        let Some(EntityType::Table(value)) = document.get_entity(handle) else {
            gaps.push(format!("{format}: Table missing"));
            continue;
        };
        if value.rows.len() != 4 {
            gaps.push(format!("{format}: rows {}", value.rows.len()));
        }
        if value.columns.first().map(|c| c.width) != Some(30.0) {
            gaps.push(format!(
                "{format}: column width {:?}",
                value.columns.first().map(|c| c.width)
            ));
        }
        let text = value
            .rows
            .get(1)
            .and_then(|r| r.cells.get(1))
            .and_then(|c| c.contents.first())
            .map(|c| c.value.text.clone());
        if text.as_deref() != Some("Hello") {
            gaps.push(format!("{format}: cell text {text:?}"));
        }
        // DWG stores the ranges and DXF the origin cell's merge dimensions;
        // the DXF reader rebuilds the ranges from those, so both formats
        // come back with the merge in both forms.
        let origin = &value.rows[0].cells[0];
        if format == "DXF" && (origin.merge_width, origin.merge_height) != (3, 1) {
            gaps.push(format!(
                "DXF: merge dims {}x{}",
                origin.merge_width, origin.merge_height
            ));
        }
        if value.merged_ranges.len() != 1 {
            gaps.push(format!(
                "{format}: merged_ranges {}",
                value.merged_ranges.len()
            ));
        }
        if value.insertion_point != edited.insertion_point {
            gaps.push(format!("{format}: insertion {:?}", value.insertion_point));
        }
    }
    assert!(gaps.is_empty(), "persistence gaps: {gaps:#?}");
    for (name, mut document) in [("table-reedit.dwg", dwg), ("table-reedit.dxf", dxf)] {
        let before = document.get_entity(handle).unwrap().clone();
        let mut after = before.clone();
        let EntityType::Table(value) = &mut after else {
            unreachable!()
        };
        value.columns[1].width = 12.0;
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        *document.get_entity_mut(handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        assert!(
            matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::Table(value)) if value.columns[1].width == 12.0),
            "{name}"
        );
    }

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    for (name, bytes) in [
        (
            "table-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "table-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        assert!(crate::io::load_bytes(name, bytes)
            .unwrap()
            .get_entity(handle)
            .is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&created)
    );
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    app.redo_steps(3);
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

/// One scripted lifecycle shared by the mesh kinds: create, edit,
/// reject, save/reopen both formats, re-edit, delete, undo/redo.
struct MeshCase {
    create: &'static str,
    edit: &'static str,
    rejects: &'static [(&'static str, &'static str)],
    is_kind: fn(&EntityType) -> bool,
    digest: fn(&EntityType) -> String,
    reedit: fn(&mut EntityType),
    expect_created: &'static str,
    expect_edited: &'static str,
    expect_reedited: &'static str,
    /// Digest a DXF reopen yields when the engine loses a field (empty:
    /// same as DWG). A canary that fails once the engine round-trips it.
    expect_edited_dxf: &'static str,
    expect_reedited_dxf: &'static str,
    /// Same idea for DWG (empty: same as the in-session digest).
    expect_edited_dwg: &'static str,
    expect_reedited_dwg: &'static str,
}

fn run_mesh_lifecycle(case: &MeshCase) {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let run_script = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let path = std::env::temp_dir().join(format!("ocs_mesh_{tag}_{}.py", std::process::id()));
        std::fs::write(&path, format!("def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\ndoc = ocs.active_document\n{body}")).unwrap();
        dispatch(host, &format!("PY_RUN {}", path.display()));
        let _ = std::fs::remove_file(&path);
    };

    // Substituted into Python string literals: on Windows a backslash path such as
    // `C:\Users\...` is read as escape sequences (`\U` is a unicode escape), and Windows
    // accepts forward slashes, so hand the script a forward-slash path.
    let tmp = std::env::temp_dir().to_string_lossy().replace('\\', "/");
    let mut create_script = case.create.replace("TMPDIR", &tmp);
    // Insert is update-only: the host makes a block and one insert, and the
    // "create" script only has to exist.
    let make_polyline = create_script.contains("MAKEPOLYLINE");
    if make_polyline {
        // The legacy Polyline is update-only: the host makes one to edit.
        let points = [(0.0, 0.0, 0.0), (10.0, 0.0, 2.0), (10.0, 5.0, 4.0)]
            .map(|(x, y, z)| document::types::Vector3::new(x, y, z));
        host.add_entity(EntityType::Polyline(
            document::entities::Polyline::from_points(points.to_vec()),
        ));
    }
    let make_ole = create_script.contains("MAKEOLE");
    if make_ole {
        // An Ole2Frame is update-only: the host embeds a real picture.
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 120, 200, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let picture = crate::io::ole_embed::EmbeddedImage {
            bytes: png.into_inner(),
            pixel_width: 4,
            pixel_height: 3,
            name: "audit.png".into(),
            source: None,
        };
        crate::io::ole_embed::add_embedded_image(
            host.document_mut(),
            &picture,
            document::types::Vector3::new(10.0, 10.0, 0.0),
            20.0,
        )
        .unwrap();
    }
    let update_only = create_script.contains("MAKEINSERT") || make_polyline || make_ole;
    if update_only || create_script.contains("MAKEBLOCK") {
        let next = host.document().next_handle();
        let (record_handle, block_handle, end_handle) = (
            Handle::new(next),
            Handle::new(next + 1),
            Handle::new(next + 2),
        );
        let mut record = document::tables::BlockRecord::new("AUDITBLK");
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = end_handle;
        host.document_mut().block_records.add(record).unwrap();
        let mut block = document::entities::Block::new("AUDITBLK", document::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut end = document::entities::BlockEnd::new();
        end.common.handle = end_handle;
        end.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(end))
            .unwrap();
        if update_only {
            let insert = document::entities::Insert::new(
                "AUDITBLK",
                document::types::Vector3::new(1.0, 2.0, 0.0),
            );
            host.add_entity(EntityType::Insert(insert));
        }
        create_script = create_script.replace("BLOCKRECORD", &record_handle.value().to_string());
    }
    let paper = host
        .document()
        .block_records
        .iter()
        .find(|r| r.is_paper_space())
        .map(|r| r.handle.value());
    let layer0 = host
        .document()
        .layers
        .iter()
        .find(|l| l.name == "0")
        .map(|l| l.handle.value());
    if create_script.contains("VIEWPORTHANDLE") || create_script.contains("SCALEHANDLE") {
        // A view border ties to an existing viewport and scale.
        let owner = host
            .document()
            .block_records
            .iter()
            .find(|r| r.is_paper_space())
            .map(|r| r.handle)
            .unwrap();
        let mut viewport = document::entities::Viewport::new();
        viewport.common.owner_handle = owner;
        let viewport_handle = host.add_entity(EntityType::Viewport(Box::new(viewport)));
        let scale_handle = host.document_mut().allocate_handle();
        let mut scale = document::objects::Scale::new("1:1", 1.0, 1.0);
        scale.handle = scale_handle;
        host.document_mut()
            .objects
            .insert(scale_handle, document::objects::ObjectType::Scale(scale));
        create_script = create_script
            .replace("VIEWPORTHANDLE", &viewport_handle.value().to_string())
            .replace("SCALEHANDLE", &scale_handle.value().to_string());
    }
    create_script = create_script
        .replace("PAPERSPACE", &paper.unwrap_or_default().to_string())
        .replace("LAYER0", &layer0.unwrap_or_default().to_string());
    if create_script.contains("DEFHANDLE") {
        // Scripts reference an underlay definition that the drawing owns.
        let handle = host.document_mut().allocate_handle();
        let mut definition = document::objects::UnderlayDefinition::pdf("plan.pdf", "1");
        definition.handle = handle;
        host.document_mut().objects.insert(
            handle,
            document::objects::ObjectType::UnderlayDefinition(definition),
        );
        create_script = create_script.replace("DEFHANDLE", &handle.value().to_string());
    }
    run_script(&mut host, "create", &create_script);
    let handle = host
        .document()
        .entities()
        .find(|entity| (case.is_kind)(entity))
        .map(|entity| entity.common().handle)
        .unwrap_or_else(|| panic!("Python did not create the mesh: {}", last_output(&host)));
    let created = host.document().get_entity(handle).unwrap().clone();
    assert_eq!((case.digest)(&created), case.expect_created);
    // A paper-space viewport draws only on its layout tab, so the canvas
    // oracle first activates the layout that owns it.
    if matches!(created, EntityType::Viewport(_)) {
        let owner = created.common().owner_handle;
        let layout = host
            .document()
            .objects
            .values()
            .find_map(|object| match object {
                document::objects::ObjectType::Layout(l) if l.block_record == owner => {
                    Some(l.name.clone())
                }
                _ => None,
            })
            .expect("the viewport's owner is a layout block");
        host.app.tabs[0].scene.set_current_layout(layout);
    }
    let has_wires = true;
    assert!(
        !host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(),
        "no canvas geometry"
    );

    run_script(
        &mut host,
        "edit",
        &case
            .edit
            .replace("HANDLE", &handle.value().to_string())
            .replace("TMPDIR", &tmp)
            .replace("LAYER0", &layer0.unwrap_or_default().to_string()),
    );
    let expected = host.document().get_entity(handle).unwrap().clone();
    assert_eq!(
        (case.digest)(&expected),
        case.expect_edited,
        "edit failed: {}",
        last_output(&host)
    );
    assert_eq!(expected.common(), created.common());
    assert_eq!(host.selection(), vec![handle]);
    if has_wires {
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());
    }

    for (patch, message) in case.rejects {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject mesh', [{{'handle':{}, {patch}}}])",
                handle.value()
            ),
        );
        assert_eq!(
            host.document().get_entity(handle),
            Some(&expected),
            "{patch}"
        );
        assert!(
            last_output(&host).contains(message),
            "{patch}: {}",
            last_output(&host)
        );
    }
    let (first_patch, _) = case.rejects[0];
    dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{h}, 'layer':'Other'}}, {{'handle':{h}, {first_patch}}}])",
            h = handle.value()));
    assert_eq!(host.document().get_entity(handle), Some(&expected));

    let dwg = crate::io::load_bytes(
        "mesh.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "mesh.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    let mut gaps = Vec::new();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        let expected_digest = if format == "DXF" && !case.expect_edited_dxf.is_empty() {
            case.expect_edited_dxf
        } else if format == "DWG" && !case.expect_edited_dwg.is_empty() {
            case.expect_edited_dwg
        } else {
            case.expect_edited
        };
        match document.get_entity(handle) {
            Some(entity) if (case.digest)(entity) == expected_digest => {}
            Some(entity) => gaps.push(format!("{format}: {}", (case.digest)(entity))),
            None => gaps.push(format!("{format}: entity missing")),
        }
    }
    assert!(
        gaps.is_empty(),
        "persistence gaps (expected {} / DXF {:?}): {gaps:#?}",
        case.expect_edited,
        case.expect_edited_dxf
    );
    for (name, mut document) in [("mesh-reedit.dwg", dwg), ("mesh-reedit.dxf", dxf)] {
        let before = document.get_entity(handle).unwrap().clone();
        let mut after = before.clone();
        (case.reedit)(&mut after);
        ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
        ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after)
            .unwrap();
        *document.get_entity_mut(handle).unwrap() = after;
        let bytes = if name.ends_with("dwg") {
            document::DwgWriter::write_to_vec(&document).unwrap()
        } else {
            document::DxfWriter::new(&document).write_to_vec().unwrap()
        };
        let reopened = crate::io::load_bytes(name, bytes).unwrap();
        let expected_digest = if name.ends_with("dxf") && !case.expect_reedited_dxf.is_empty() {
            case.expect_reedited_dxf
        } else if name.ends_with("dwg") && !case.expect_reedited_dwg.is_empty() {
            case.expect_reedited_dwg
        } else {
            case.expect_reedited
        };
        assert_eq!(
            (case.digest)(reopened.get_entity(handle).unwrap()),
            expected_digest,
            "{name}"
        );
    }

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            handle.value()
        ),
    );
    assert!(host.document().get_entity(handle).is_none());
    for (name, bytes) in [
        (
            "mesh-deleted.dwg",
            document::DwgWriter::write_to_vec(host.document()).unwrap(),
        ),
        (
            "mesh-deleted.dxf",
            document::DxfWriter::new(host.document())
                .write_to_vec()
                .unwrap(),
        ),
    ] {
        assert!(crate::io::load_bytes(name, bytes)
            .unwrap()
            .get_entity(handle)
            .is_none());
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        if update_only { 2 } else { 3 }
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&expected)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(handle),
        Some(&created)
    );
    if !update_only {
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }
    app.redo_steps(if update_only { 2 } else { 3 });
    assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
}

#[test]
fn staged_python_polygon_mesh_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "verts = [{'location': P(i, j, 0.0)} for i in range(3) for j in range(3)]\n",
                "doc.create_entity('PolygonMesh', m_vertex_count=3, n_vertex_count=3, vertices=verts)\n"),
            edit: concat!(
                "mesh = doc.entities[HANDLE]\n",
                "verts = mesh.vertices\n",
                "verts[4]['location'] = P(1.0, 1.0, 2.5)\n",
                "with doc.transaction('Edit mesh'):\n",
                "    mesh.vertices = verts\n",
                "    mesh.m_smooth_density = 6\n",
                "    mesh.n_smooth_density = 6\n",
                "    mesh.smooth_type = 'Cubic'\n",
                "doc.selection = [mesh]\n"),
            rejects: &[
                ("'m_vertex_count':4", "requires 12"),
                ("'n_vertex_count':1", "at least 2"),
                ("'m_smooth_density':-1", "non-negative"),
                ("'elevation':float('nan')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}}]*9", "finite"),
                ("'smooth_type':'Spline'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::PolygonMesh(_)),
            digest: |entity| match entity {
                EntityType::PolygonMesh(m) => format!("{}x{} v{} z{} d{}/{} {:?}", m.m_vertex_count, m.n_vertex_count,
                    m.vertices.len(), m.vertices[4].location.z, m.m_smooth_density, m.n_smooth_density, m.smooth_type),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::PolygonMesh(m) = entity { m.vertices[0].location.z = 1.0; },
            expect_created: "3x3 v9 z0 d0/0 NoSmooth",
            expect_edited: "3x3 v9 z2.5 d6/6 Cubic",
            expect_reedited: "3x3 v9 z2.5 d6/6 Cubic",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn staged_python_polyface_mesh_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "verts = [{'location': P(0.0, 0.0, 0.0)}, {'location': P(4.0, 0.0, 0.0)},\n",
                "         {'location': P(4.0, 4.0, 0.0)}, {'location': P(0.0, 4.0, 0.0)}]\n",
                "faces = [{'index1': 1, 'index2': 2, 'index3': 3}, {'index1': 1, 'index2': 3, 'index3': 4}]\n",
                "doc.create_entity('PolyfaceMesh', vertices=verts, faces=faces)\n"),
            edit: concat!(
                "mesh = doc.entities[HANDLE]\n",
                "verts = mesh.vertices\n",
                "verts[2]['location'] = P(4.0, 4.0, 3.0)\n",
                "verts.append({'location': P(2.0, 6.0, 0.0)})\n",
                "faces = mesh.faces\n",
                "faces.append({'index1': 3, 'index2': -4, 'index3': 5})\n",
                "with doc.transaction('Edit mesh'):\n",
                "    mesh.vertices = verts\n",
                "    mesh.faces = faces\n",
                "doc.selection = [mesh]\n"),
            rejects: &[
                ("'faces':[{'index1':1,'index2':2,'index3':9}]", "references vertex 9"),
                ("'faces':[{'index1':1,'index2':2}]", "at least 3 vertex indices"),
                ("'faces':[]", "at least 1 face"),
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 3"),
                ("'thickness':float('inf')", "finite"),
                ("'seqend_handle':5", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::PolyfaceMesh(_)),
            digest: |entity| match entity {
                EntityType::PolyfaceMesh(m) => format!("v{} f{} z{} last {:?}", m.vertices.len(), m.faces.len(),
                    m.vertices[2].location.z,
                    m.faces.last().map(|f| (f.index1, f.index2, f.index3))),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::PolyfaceMesh(m) = entity { m.vertices[0].location.z = 1.0; },
            expect_created: "v4 f2 z0 last Some((1, 3, 4))",
            expect_edited: "v5 f3 z3 last Some((3, -4, 5))",
            expect_reedited: "v5 f3 z3 last Some((3, -4, 5))",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn staged_python_mesh_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "verts = [P(0.0, 0.0, 0.0), P(4.0, 0.0, 0.0), P(4.0, 4.0, 0.0), P(0.0, 4.0, 0.0)]\n",
            "doc.create_entity('Mesh', vertices=verts, faces=[{'vertices': [0, 1, 2, 3]}],\n",
            "    edges=[{'start': 0, 'end': 1, 'crease': 1.5}])\n"
        ),
        edit: concat!(
            "mesh = doc.entities[HANDLE]\n",
            "verts = mesh.vertices\n",
            "verts.append(P(2.0, 6.0, 0.0))\n",
            "faces = mesh.faces\n",
            "faces.append({'vertices': [3, 2, 4]})\n",
            "with doc.transaction('Edit mesh'):\n",
            "    mesh.vertices = verts\n",
            "    mesh.faces = faces\n",
            "    mesh.subdivision_level = 1\n",
            "doc.selection = [mesh]\n"
        ),
        rejects: &[
            ("'faces':[{'vertices':[0,1,9]}]", "references vertex 9"),
            ("'faces':[{'vertices':[0,1]}]", "at least 3 vertices"),
            ("'edges':[{'start':0,'end':9}]", "out of range"),
            ("'edges':[{'start':2,'end':2}]", "identical endpoints"),
            (
                "'edges':[{'start':0,'end':1,'crease':-1.0}]",
                "non-negative",
            ),
            ("'subdivision_level':-1", "non-negative"),
            ("'version':3", "read-only"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Mesh(_)),
        digest: |entity| match entity {
            EntityType::Mesh(m) => format!(
                "v{} f{} e{} sub{} crease {:?}",
                m.vertices.len(),
                m.faces.len(),
                m.edges.len(),
                m.subdivision_level,
                m.edges.first().and_then(|e| e.crease)
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Mesh(m) = entity {
                m.vertices[0].z = 1.0;
            }
        },
        expect_created: "v4 f1 e1 sub0 crease Some(1.5)",
        expect_edited: "v5 f2 e1 sub1 crease Some(1.5)",
        expect_reedited: "v5 f2 e1 sub1 crease Some(1.5)",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn staged_python_helix_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Helix', axis_base_point=P(0.0, 0.0, 0.0), axis_vector=P(0.0, 0.0, 1.0),\n",
                "    start_point=P(5.0, 0.0, 0.0), radius=5.0, turns=3.0, turn_height=2.0)\n"),
            edit: concat!(
                "helix = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit helix'):\n",
                "    helix.turns = 5.0\n",
                "    helix.handedness = False\n",
                "doc.selection = [helix]\n"),
            rejects: &[
                ("'radius':0.0", "greater than zero"),
                ("'turns':-1.0", "greater than zero"),
                ("'turn_height':float('nan')", "greater than zero"),
                ("'axis_vector':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'start_point':{'x':0.0,'y':0.0,'z':3.0}", "on the axis"),
                ("'spline':{}", "read-only"),
                ("'major_version':1", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Helix(_)),
            digest: |entity| match entity {
                EntityType::Helix(h) => format!("r{} t{} h{} endz{:.1} ccw{} cp{}", h.radius, h.turns, h.turn_height,
                    h.spline.control_points.last().map_or(f64::NAN, |p| p.z), h.handedness,
                    !h.spline.control_points.is_empty()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Helix(h) = entity { h.turns = 6.0; },
            expect_created: "r5 t3 h2 endz6.0 ccwtrue cptrue",
            expect_edited: "r5 t5 h2 endz10.0 ccwfalse cptrue",
            expect_reedited: "r5 t6 h2 endz10.0 ccwfalse cptrue",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn staged_python_raster_image_lifecycle_over_real_ipc() {
    image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 30, 30, 255]))
        .save(std::env::temp_dir().join("ocs_raster_test.png"))
        .unwrap();
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('RasterImage', file_path='TMPDIR/ocs_raster_test.png',\n",
                "    insertion_point=P(10.0, 20.0, 0.0), u_vector=P(0.5, 0.0, 0.0), v_vector=P(0.0, 0.5, 0.0))\n"),
            edit: concat!(
                "img = doc.entities[HANDLE]\n",
                "cb = img.clip_boundary\n",
                "cb['vertices'] = [{'x': 1.0, 'y': 1.0}, {'x': 7.0, 'y': 3.0}]\n",
                "with doc.transaction('Edit image'):\n",
                "    img.insertion_point = (12.0, 22.0, 0.0)\n",
                "    img.u_vector = (1.0, 0.0, 0.0)\n",
                "    img.brightness = 70\n",
                "    img.contrast = 40\n",
                "    img.fade = 10\n",
                "    img.clip_boundary = cb\n",
                "    img.clipping_enabled = True\n",
                "doc.selection = [img]\n"),
            rejects: &[
                ("'file_path':'other.png'", "fixed at creation"),
                ("'brightness':101", "within 0..=100"),
                ("'u_vector':{'x':0.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'v_vector':{'x':2.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'size':{'x':5.0,'y':5.0}", "read-only"),
                ("'definition_handle':5", "read-only"),
                ("'flags':64", "unknown or out-of-range bits"),
                ("'clip_boundary':{'vertices':[]}", "at least 2"),
            ],
            is_kind: |entity| matches!(entity, EntityType::RasterImage(_)),
            digest: |entity| match entity {
                EntityType::RasterImage(i) => format!("{} {:.1},{:.1} u{:.1} b{} c{} f{} clip{} n{} {}x{} def{}",
                    std::path::Path::new(&i.file_path).file_name().unwrap().to_string_lossy(),
                    i.insertion_point.x, i.insertion_point.y, i.u_vector.x, i.brightness, i.contrast, i.fade,
                    i.clipping_enabled, i.clip_boundary.vertices.len(), i.size.x, i.size.y, i.definition_handle.is_some()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::RasterImage(i) = entity { i.brightness = 80; },
            expect_created: "ocs_raster_test.png 10.0,20.0 u0.5 b50 c50 f0 clipfalse n2 8x4 deftrue",
            expect_edited: "ocs_raster_test.png 12.0,22.0 u1.0 b70 c40 f10 cliptrue n2 8x4 deftrue",
            expect_reedited: "ocs_raster_test.png 12.0,22.0 u1.0 b80 c40 f10 cliptrue n2 8x4 deftrue",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn staged_python_wipeout_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Wipeout', insertion_point=P(0.0, 0.0, 0.0),\n",
                "    u_vector=P(10.0, 0.0, 0.0), v_vector=P(0.0, 6.0, 0.0))\n"),
            edit: concat!(
                "w = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit wipeout'):\n",
                "    w.clip_type = 'Polygonal'\n",
                "    w.clip_boundary_vertices = [{'x': -0.5, 'y': -0.5}, {'x': 0.5, 'y': -0.5}, {'x': 0.0, 'y': 0.5}]\n",
                "    w.insertion_point = (2.0, 3.0, 0.0)\n",
                "    w.clip_mode = 'Inside'\n",
                "doc.selection = [w]\n"),
            rejects: &[
                ("'clip_boundary_vertices':[{'x':0.0,'y':0.0}]", "at least 3"),
                ("'clip_type':'Rectangular'", "exactly 2"),
                ("'u_vector':{'x':0.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'brightness':200", "within 0..=100"),
                ("'size':{'x':0.0,'y':1.0}", "greater than zero"),
                ("'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'definition_handle':5", "read-only"),
                ("'clip_type':'Circle'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Wipeout(_)),
            digest: |entity| match entity {
                EntityType::Wipeout(w) => format!("{:?} {:?} n{} at{:.1},{:.1} u{:.1} v{:.1}", w.clip_type, w.clip_mode,
                    w.clip_boundary_vertices.len(), w.insertion_point.x, w.insertion_point.y, w.u_vector.x, w.v_vector.y),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Wipeout(w) = entity { w.u_vector.x = 12.0; },
            expect_created: "Rectangular Outside n2 at0.0,0.0 u10.0 v6.0",
            expect_edited: "Polygonal Inside n3 at2.0,3.0 u10.0 v6.0",
            expect_reedited: "Polygonal Inside n3 at2.0,3.0 u12.0 v6.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            // DXF writes a polygonal boundary closed (the first vertex
            // again at the end), and reads it back that way.
            expect_edited_dxf: "Polygonal Inside n4 at2.0,3.0 u10.0 v6.0",
            expect_reedited_dxf: "Polygonal Inside n4 at2.0,3.0 u12.0 v6.0",
        });
}

#[test]
fn staged_python_underlay_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Underlay', underlay_type='Pdf', definition_handle=DEFHANDLE,\n",
            "    insertion_point=P(5.0, 5.0, 0.0), x_scale=2.0, y_scale=2.0)\n"
        ),
        edit: concat!(
            "u = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit underlay'):\n",
            "    u.insertion_point = (8.0, 9.0, 0.0)\n",
            "    u.x_scale = 3.0\n",
            "    u.y_scale = 3.0\n",
            "    u.rotation = 0.5\n",
            "    u.contrast = 70\n",
            "    u.fade = 20\n",
            "    u.clip_boundary_vertices = [{'x': 0.0, 'y': 0.0}, {'x': 4.0, 'y': 3.0}]\n",
            "doc.selection = [u]\n"
        ),
        rejects: &[
            ("'definition_handle':999999", "does not exist"),
            ("'underlay_type':'Dwf'", "does not match its definition"),
            ("'x_scale':0.0", "nonzero"),
            ("'rotation':float('nan')", "finite"),
            ("'contrast':101", "within 0..=100"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("'clip_boundary_vertices':[{'x':1.0,'y':1.0}]", "at least 2"),
            ("'flags':64", "unknown or out-of-range bits"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Underlay(_)),
        digest: |entity| match entity {
            EntityType::Underlay(u) => format!(
                "{:?} at{:.1},{:.1} s{:.1} r{:.1} c{} f{} clip{}",
                u.underlay_type,
                u.insertion_point.x,
                u.insertion_point.y,
                u.x_scale,
                u.rotation,
                u.contrast,
                u.fade,
                u.clip_boundary_vertices.len()
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Underlay(u) = entity {
                u.x_scale = 4.0;
            }
        },
        expect_created: "Pdf at5.0,5.0 s2.0 r0.0 c100 f0 clip0",
        expect_edited: "Pdf at8.0,9.0 s3.0 r0.5 c70 f20 clip2",
        expect_reedited: "Pdf at8.0,9.0 s4.0 r0.5 c70 f20 clip2",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn staged_python_viewport_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Viewport', owner_handle=PAPERSPACE, center=P(100.0, 100.0, 0.0),\n",
            "    width=80.0, height=60.0, view_center=P(0.0, 0.0, 0.0), view_height=50.0)\n"
        ),
        edit: concat!(
            "vp = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit viewport'):\n",
            "    vp.center = (120.0, 110.0, 0.0)\n",
            "    vp.width = 90.0\n",
            "    vp.view_height = 75.0\n",
            "    vp.twist_angle = 0.25\n",
            "    vp.frozen_layers = [LAYER0]\n",
            "doc.selection = [vp]\n"
        ),
        rejects: &[
            ("'width':0.0", "greater than zero"),
            ("'view_height':float('nan')", "greater than zero"),
            ("'view_direction':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("'frozen_layers':[999999]", "does not exist"),
            ("'circle_sides':2", "at least 3"),
            ("'id':7", "read-only"),
            ("'clip_boundary_handle':5", "read-only"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Viewport(_)),
        digest: |entity| match entity {
            EntityType::Viewport(v) => format!(
                "at{:.1},{:.1} {:.1}x{:.1} vh{:.1} tw{:.2} frozen{}",
                v.center.x,
                v.center.y,
                v.width,
                v.height,
                v.view_height,
                v.twist_angle,
                v.frozen_layers.len()
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Viewport(v) = entity {
                v.width = 95.0;
            }
        },
        expect_created: "at100.0,100.0 80.0x60.0 vh50.0 tw0.00 frozen0",
        expect_edited: "at120.0,110.0 90.0x60.0 vh75.0 tw0.25 frozen1",
        expect_reedited: "at120.0,110.0 95.0x60.0 vh75.0 tw0.25 frozen1",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn staged_python_view_border_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('ViewBorder', min=[0.0, 0.0], max=[40.0, 30.0], center=[20.0, 15.0], scale=1.0,\n",
                "    active_viewport=VIEWPORTHANDLE, scale_handle=SCALEHANDLE)\n"),
            edit: concat!(
                "vb = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit view border'):\n",
                "    vb.max = [60.0, 45.0]\n",
                "    vb.center = [30.0, 22.5]\n",
                "    vb.scale = 2.0\n",
                "    vb.rotation_angle = 0.3\n",
                "doc.selection = [vb]\n"),
            rejects: &[
                ("'max':[-1.0, 5.0]", "below max"),
                ("'scale':0.0", "greater than zero"),
                ("'rotation_angle':float('nan')", "finite"),
                ("'center':[float('inf'), 0.0]", "finite"),
                ("'active_viewport':999999", "does not exist"),
                ("'scale_handle':999999", "does not exist"),
                ("'version':3", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::ViewBorder(_)),
            digest: |entity| match entity {
                EntityType::ViewBorder(v) => format!("max{:.1},{:.1} c{:.1},{:.1} s{:.1} r{:.1}", v.max[0], v.max[1],
                    v.center[0], v.center[1], v.scale, v.rotation_angle),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::ViewBorder(v) = entity { v.scale = 3.0; },
            expect_created: "max40.0,30.0 c20.0,15.0 s1.0 r0.0",
            expect_edited: "max60.0,45.0 c30.0,22.5 s2.0 r0.3",
            expect_reedited: "max60.0,45.0 c30.0,22.5 s3.0 r0.3",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn staged_python_light_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Light', name='Key', light_type=3, position=P(10.0, 10.0, 10.0),\n",
            "    target=P(0.0, 0.0, 0.0), intensity=1.5, hotspot_angle=0.4, falloff_angle=0.8,\n",
            "    light_color={'kind': 'Rgb', 'value': {'r': 255, 'g': 240, 'b': 200}})\n"
        ),
        edit: concat!(
            "lt = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit light'):\n",
            "    lt.intensity = 3.0\n",
            "    lt.target = (5.0, 5.0, 0.0)\n",
            "    lt.position = (12.0, 8.0, 10.0)\n",
            "    lt.cast_shadows = True\n",
            "doc.selection = [lt]\n"
        ),
        rejects: &[
            ("'light_type':9", "must be 1 (distant)"),
            ("'intensity':-1.0", "non-negative"),
            (
                "'target':{'x':12.0,'y':8.0,'z':10.0}",
                "different from its position",
            ),
            ("'hotspot_angle':2.0", "must not exceed falloff_angle"),
            ("'position':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("'name':''", "empty"),
            ("'class_version':2", "read-only"),
            ("'light_color':{'kind':'Index','value':300}", "Color"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Light(_)),
        digest: |entity| match entity {
            EntityType::Light(l) => format!(
                "{} t{} i{:.1} pos{:.0},{:.0},{:.0} tgt{:.0},{:.0} sh{} {:?}",
                l.name,
                l.light_type,
                l.intensity,
                l.position.x,
                l.position.y,
                l.position.z,
                l.target.x,
                l.target.y,
                l.cast_shadows,
                l.light_color
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Light(l) = entity {
                l.intensity = 4.0;
            }
        },
        expect_created: "Key t3 i1.5 pos10,10,10 tgt0,0 shfalse Rgb { r: 255, g: 240, b: 200 }",
        expect_edited: "Key t3 i3.0 pos12,8,10 tgt5,5 shtrue Rgb { r: 255, g: 240, b: 200 }",
        expect_reedited: "Key t3 i4.0 pos12,8,10 tgt5,5 shtrue Rgb { r: 255, g: 240, b: 200 }",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_point_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Point', location=P(1.0, 2.0, 3.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.location = (4.0, 5.0, 6.0)\n",
            "    e.thickness = 2.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'location':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("'thickness':float('inf')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Point(_)),
        digest: |entity| match entity {
            EntityType::Point(v) => format!(
                "{:.1},{:.1},{:.1} th{:.1}",
                v.location.x, v.location.y, v.location.z, v.thickness
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Point(v) = entity {
                v.location.z = 9.0;
            }
        },
        expect_created: "1.0,2.0,3.0 th0.0",
        expect_edited: "4.0,5.0,6.0 th2.0",
        expect_reedited: "4.0,5.0,9.0 th2.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_line_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Line', start=P(0.0, 0.0, 0.0), end=P(10.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.end = (10.0, 5.0, 0.0)\n",
            "    e.thickness = 1.5\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'start':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("'thickness':float('inf')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Line(_)),
        digest: |entity| match entity {
            EntityType::Line(v) => {
                format!("{:.1},{:.1} th{:.1}", v.end.x, v.end.y, v.thickness)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Line(v) = entity {
                v.start.x = 1.0;
            }
        },
        expect_created: "10.0,0.0 th0.0",
        expect_edited: "10.0,5.0 th1.5",
        expect_reedited: "10.0,5.0 th1.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_circle_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Circle', center=P(0.0, 0.0, 0.0), radius=5.0)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.radius = 7.0\n",
            "    e.center = (1.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'radius':0.0", "radius"),
            ("'radius':-3.0", "radius"),
            ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Circle(_)),
        digest: |entity| match entity {
            EntityType::Circle(v) => {
                format!("{:.1},{:.1} r{:.1}", v.center.x, v.center.y, v.radius)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Circle(v) = entity {
                v.radius = 8.0;
            }
        },
        expect_created: "0.0,0.0 r5.0",
        expect_edited: "1.0,1.0 r7.0",
        expect_reedited: "1.0,1.0 r8.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_arc_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Arc', center=P(0.0, 0.0, 0.0), radius=5.0, start_angle=0.0, end_angle=1.5)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.end_angle = 2.0\n", "    e.radius = 6.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'radius':0.0", "radius"),
                ("'end_angle':float('nan')", "finite"),
                ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Arc(_)),
            digest: |entity| match entity {
                EntityType::Arc(v) => format!("r{:.1} {:.1}..{:.1}", v.radius, v.start_angle, v.end_angle),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Arc(v) = entity { v.start_angle = 0.5; },
            expect_created: "r5.0 0.0..1.5",
            expect_edited: "r6.0 0.0..2.0",
            expect_reedited: "r6.0 0.5..2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_ellipse_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Ellipse', center=P(0.0, 0.0, 0.0), major_axis=P(10.0, 0.0, 0.0), minor_axis_ratio=0.5)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.minor_axis_ratio = 0.25\n", "    e.end_parameter = 3.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'minor_axis_ratio':0.0", "ratio"),
                ("'minor_axis_ratio':2.0", "ratio"),
                ("'major_axis':{'x':0.0,'y':0.0,'z':0.0}", "axis"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Ellipse(_)),
            digest: |entity| match entity {
                EntityType::Ellipse(v) => format!("q{:.2} ..{:.1}", v.minor_axis_ratio, v.end_parameter),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Ellipse(v) = entity { v.minor_axis_ratio = 0.75; },
            expect_created: "q0.50 ..6.3",
            expect_edited: "q0.25 ..3.0",
            expect_reedited: "q0.75 ..3.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_ray_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Ray', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.base_point = (2.0, 2.0, 0.0)\n",
            "    e.direction = (0.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
            ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Ray(_)),
        digest: |entity| match entity {
            EntityType::Ray(v) => format!(
                "{:.1},{:.1} d{:.1},{:.1}",
                v.base_point.x, v.base_point.y, v.direction.x, v.direction.y
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Ray(v) = entity {
                v.base_point.x = 3.0;
            }
        },
        expect_created: "0.0,0.0 d1.0,0.0",
        expect_edited: "2.0,2.0 d0.0,1.0",
        expect_reedited: "3.0,2.0 d0.0,1.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_xline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create:
            "doc.create_entity('XLine', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.base_point = (2.0, 2.0, 0.0)\n",
            "    e.direction = (0.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
            ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::XLine(_)),
        digest: |entity| match entity {
            EntityType::XLine(v) => format!(
                "{:.1},{:.1} d{:.1},{:.1}",
                v.base_point.x, v.base_point.y, v.direction.x, v.direction.y
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::XLine(v) = entity {
                v.base_point.x = 3.0;
            }
        },
        expect_created: "0.0,0.0 d1.0,0.0",
        expect_edited: "2.0,2.0 d0.0,1.0",
        expect_reedited: "3.0,2.0 d0.0,1.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_solid_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Solid', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(0.0, 4.0, 0.0), fourth_corner=P(4.0, 4.0, 0.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.fourth_corner = (5.0, 5.0, 0.0)\n", "    e.thickness = 2.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'thickness':float('inf')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Solid(_)),
            digest: |entity| match entity {
                EntityType::Solid(v) => format!("{:.1},{:.1} th{:.1}", v.fourth_corner.x, v.fourth_corner.y, v.thickness),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Solid(v) = entity { v.first_corner.x = 1.0; },
            expect_created: "4.0,4.0 th0.0",
            expect_edited: "5.0,5.0 th2.0",
            expect_reedited: "5.0,5.0 th2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_face3d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Face3D', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(4.0, 4.0, 1.0), fourth_corner=P(0.0, 4.0, 1.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.third_corner = (4.0, 4.0, 2.0)\n", "    e.invisible_edges = 3\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'invisible_edges':64", "bits"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Face3D(_)),
            digest: |entity| match entity {
                EntityType::Face3D(v) => format!("z{:.1} inv{}", v.third_corner.z, v.invisible_edges.bits()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Face3D(v) = entity { v.first_corner.x = 1.0; },
            expect_created: "z1.0 inv0",
            expect_edited: "z2.0 inv3",
            expect_reedited: "z2.0 inv3",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_text_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Text', text='Hello', insertion=P(1.0, 2.0, 0.0), height=2.5)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.text = 'World'\n",
            "    e.rotation = 0.5\n",
            "    e.height = 3.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'height':0.0", "height"),
            ("'height':-1.0", "height"),
            ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Text(_)),
        digest: |entity| match entity {
            EntityType::Text(v) => format!("{} h{:.1} r{:.1}", v.value, v.height, v.rotation),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Text(v) = entity {
                v.height = 4.0;
            }
        },
        expect_created: "Hello h2.5 r0.0",
        expect_edited: "World h3.0 r0.5",
        expect_reedited: "World h4.0 r0.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_mtext_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('MText', text='Line one', insertion=P(1.0, 2.0, 0.0), height=2.5, rectangle_width=30.0)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.text = 'Line two'\n", "    e.rectangle_width = 40.0\n", "    e.rotation = 0.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'height':0.0", "height"),
                ("'rectangle_width':-1.0", "width"),
                ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::MText(_)),
            digest: |entity| match entity {
                EntityType::MText(v) => format!("{} w{:.1} r{:.1}", v.value, v.rectangle_width, v.rotation),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::MText(v) = entity { v.rectangle_width = 50.0; },
            expect_created: "Line one w30.0 r0.0",
            expect_edited: "Line two w40.0 r0.5",
            expect_reedited: "Line two w50.0 r0.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_lwpolyline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('LwPolyline', vertices=[{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 5.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 8.0}, 'bulge': 0.5}, {'location': {'x': 0.0, 'y': 8.0}}]\n", "    e.is_closed = True\n", "    e.constant_width = 0.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0}}]", "at least 2"),
                ("'constant_width':-1.0", "non-negative"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0}},{'location':{'x':1.0,'y':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::LwPolyline(_)),
            digest: |entity| match entity {
                EntityType::LwPolyline(v) => format!("n{} closed{} w{:.1} bulge{:.1}", v.vertices.len(), v.is_closed, v.constant_width, v.vertices.get(2).map_or(0.0, |x| x.bulge)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::LwPolyline(v) = entity { v.constant_width = 1.0; },
            expect_created: "n3 closedfalse w0.0 bulge0.0",
            expect_edited: "n4 closedtrue w0.5 bulge0.5",
            expect_reedited: "n4 closedtrue w1.0 bulge0.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_polyline2d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Polyline2D', vertices=[{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 5.0, 'z': 0.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 0.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 0.0}}]\n", "    e.closed = True\n", "    e.thickness = 1.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'thickness':float('inf')", "finite"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline2D(_)),
            digest: |entity| match entity {
                EntityType::Polyline2D(v) => format!("n{} closed{} th{:.1}", v.vertices.len(), v.flags.is_closed(), v.thickness),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::Polyline2D(v) = entity { v.thickness = 2.0; },
            expect_created: "n3 closedfalse th0.0",
            expect_edited: "n4 closedtrue th1.0",
            expect_reedited: "n4 closedtrue th2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "wrong kind LwPolyline",
            expect_reedited_dxf: "wrong kind LwPolyline",
        });
}

#[test]
fn audit_python_polyline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: "# MAKEPOLYLINE
",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n", "    e.closed = True\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline(_)),
            digest: |entity| match entity {
                EntityType::Polyline(v) => format!("n{} closed{} lastz{:.1}", v.vertices.len(), v.flags.is_closed(), v.vertices.last().map_or(0.0, |x| x.location.z)),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::Polyline(v) = entity { v.vertices[0].location.z = 1.0; },
            expect_created: "n3 closedfalse lastz4.0",
            expect_edited: "n4 closedtrue lastz6.0",
            expect_reedited: "n4 closedtrue lastz6.0",
            expect_edited_dwg: "wrong kind Polyline3D",
            expect_reedited_dwg: "wrong kind Polyline3D",
            expect_edited_dxf: "wrong kind Polyline3D",
            expect_reedited_dxf: "wrong kind Polyline3D",
        });
}

#[test]
fn audit_python_polyline3d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Polyline3D', vertices=[{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 5.0, 'z': 4.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'position': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n", "    e.elevation = 1.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'elevation':float('inf')", "finite"),
                ("'vertices':[{'position':{'x':float('nan'),'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline3D(_)),
            digest: |entity| match entity {
                EntityType::Polyline3D(v) => format!("n{} el{:.1} lastz{:.1}", v.vertices.len(), v.elevation, v.vertices.last().map_or(0.0, |x| x.position.z)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Polyline3D(v) = entity { v.elevation = 2.5; },
            expect_created: "n3 el0.0 lastz4.0",
            expect_edited: "n4 el1.5 lastz6.0",
            expect_reedited: "n4 el2.5 lastz6.0",
            expect_edited_dwg: "n4 el0.0 lastz6.0",
            expect_reedited_dwg: "n4 el0.0 lastz6.0",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_spline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Spline', degree=2, knots=[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], control_points=[{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 5.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.control_points = [{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 8.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}]\n", "    e.weights = [1.0, 2.0, 1.0]\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'degree':0", "at least 1"),
                ("'knots':[0.0,1.0]", "must hold"),
                ("'weights':[1.0,0.0,1.0]", "greater than zero"),
                ("'weights':[1.0]", "match"),
                ("'control_points':[{'x':float('nan'),'y':0.0,'z':0.0},{'x':1.0,'y':0.0,'z':0.0},{'x':2.0,'y':0.0,'z':0.0}]", "finite"),
                ("'knots':[0.0,0.0,1.0,0.5,1.0,1.0]", "nondecreasing"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Spline(_)),
            digest: |entity| match entity {
                EntityType::Spline(v) => format!("deg{} cp{} y{:.1} w{:?}", v.degree, v.control_points.len(), v.control_points[1].y, v.weights),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Spline(v) = entity { v.control_points[1].y = 9.0; },
            expect_created: "deg2 cp3 y5.0 w[]",
            expect_edited: "deg2 cp3 y8.0 w[1.0, 2.0, 1.0]",
            expect_reedited: "deg2 cp3 y9.0 w[1.0, 2.0, 1.0]",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
}

#[test]
fn audit_python_insert_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "# MAKEBLOCK\n",
            "doc.create_entity('Insert', block_name='AUDITBLK', insert_point=P(1.0, 2.0, 0.0))\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.insert_point = (4.0, 5.0, 0.0)\n",
            "    e.x_scale = 2.0\n",
            "    e.y_scale = 3.0\n",
            "    e.rotation = 0.5\n",
            "    e.column_count = 2\n",
            "    e.column_spacing = 10.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'x_scale':0.0", "nonzero"),
            ("'rotation':float('nan')", "finite"),
            ("'column_count':0", "greater than zero"),
            (
                "'insert_point':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'block_name':'OTHER'", "does not exist"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Insert(_)),
        digest: |entity| match entity {
            EntityType::Insert(v) => format!(
                "{} at{:.1},{:.1} s{:.1},{:.1} r{:.1} cols{}x{:.1}",
                v.block_name,
                v.insert_point.x,
                v.insert_point.y,
                v.x_scale(),
                v.y_scale(),
                v.rotation,
                v.column_count,
                v.column_spacing
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Insert(v) = entity {
                v.rotation = 1.0;
            }
        },
        expect_created: "AUDITBLK at1.0,2.0 s1.0,1.0 r0.0 cols1x0.0",
        expect_edited: "AUDITBLK at4.0,5.0 s2.0,3.0 r0.5 cols2x10.0",
        expect_reedited: "AUDITBLK at4.0,5.0 s2.0,3.0 r1.0 cols2x10.0",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
    });
}

#[test]
fn audit_python_attribute_definition_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "# MAKEBLOCK\n",
                "doc.create_entity('AttributeDefinition', owner_handle=BLOCKRECORD, tag='PART_NO', prompt='Part number',\n",
                "    default_value='PN-001', insertion_point=P(1.0, 2.0, 0.0), height=2.5, width_factor=1.25, oblique_angle=0.1,\n",
                "    rotation=0.25, horizontal_alignment='Center', vertical_alignment='Top', alignment_point=P(3.0, 4.0, 0.0),\n",
                "    flags={'invisible': True, 'constant': False, 'verify': True, 'preset': False, 'locked_position': False, 'annotative': False},\n",
                "    field_length=12, text_generation_flags=2, lock_position=True)\n"),
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.insertion_point = (4.0, 5.0, 0.0)\n",
                "    e.default_value = 'PN-002'\n",
                "    e.prompt = 'Serial'\n",
                "    e.height = 3.0\n",
                "    e.width_factor = 0.8\n",
                "    e.rotation = 0.5\n",
                "    e.oblique_angle = 0.2\n",
                "    e.horizontal_alignment = 'Right'\n",
                "    e.vertical_alignment = 'Middle'\n",
                "    e.flags = {'invisible': False, 'constant': True, 'verify': False, 'preset': True, 'locked_position': False, 'annotative': False}\n",
                "    e.field_length = 20\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'tag':'BAD TAG'", "whitespace"),
                ("'height':0.0", "greater than zero"),
                ("'width_factor':0.0", "width_factor"),
                ("'text_style':'Missing'", "does not exist"),
                ("'owner_handle':999999", "read-only"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'rotation':float('nan')", "finite"),
                ("'horizontal_alignment':'Nowhere'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::AttributeDefinition(_)),
            digest: |entity| match entity {
                EntityType::AttributeDefinition(v) => format!(
                    "{}|{}|{} ins{:.1},{:.1} al{:.1},{:.1} h{:.1} r{:.2} wf{:.2} ob{:.2} {:?}/{:?} f{}{}{}{} fl{} tg{} lock{}",
                    v.tag, v.prompt, v.default_value, v.insertion_point.x, v.insertion_point.y,
                    v.alignment_point.x, v.alignment_point.y, v.height, v.rotation, v.width_factor, v.oblique_angle,
                    v.horizontal_alignment, v.vertical_alignment,
                    u8::from(v.flags.invisible), u8::from(v.flags.constant), u8::from(v.flags.verify), u8::from(v.flags.preset),
                    v.field_length, v.text_generation_flags, u8::from(v.lock_position)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::AttributeDefinition(v) = entity { v.default_value = "PN-003".into(); },
            expect_created: "PART_NO|Part number|PN-001 ins1.0,2.0 al3.0,4.0 h2.5 r0.25 wf1.25 ob0.10 Center/Top f1010 fl12 tg2 lock1",
            expect_edited: "PART_NO|Serial|PN-002 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            expect_reedited: "PART_NO|Serial|PN-003 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            // Reading a DXF derives a justified attribute's insertion point
            // from its alignment point (#2728).
            expect_edited_dxf: "PART_NO|Serial|PN-002 ins-11.0,-5.4 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            expect_reedited_dxf: "PART_NO|Serial|PN-003 ins-11.0,-5.4 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
}

#[test]
fn audit_python_insert_creation_rules_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let mut records = std::collections::BTreeMap::new();
    for name in ["OUTER", "INNER"] {
        let next = host.document().next_handle();
        let (record_handle, block_handle, end_handle) = (
            Handle::new(next),
            Handle::new(next + 1),
            Handle::new(next + 2),
        );
        let mut record = document::tables::BlockRecord::new(name);
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = end_handle;
        host.document_mut().block_records.add(record).unwrap();
        let mut block = document::entities::Block::new(name, document::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut end = document::entities::BlockEnd::new();
        end.common.handle = end_handle;
        end.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(end))
            .unwrap();
        records.insert(name, record_handle);
    }
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let count = |host: &HostSession<'_>| {
        host.document()
            .entities()
            .filter(|e| matches!(e, EntityType::Insert(_)))
            .count()
    };
    let create = |name: &str, extra: &str| {
        format!(
            "PY_EVAL ocs.active_document.create_entity('Insert', block_name='{name}', insert_point={{'x':0.0,'y':0.0,'z':0.0}}{extra})")
    };

    // A valid insert of INNER into the drawing works.
    dispatch(&mut host, &create("INNER", ""));
    assert_eq!(count(&host), 1, "{}", last(&host));
    // Refused: unknown block, model space, bad scale, zero array count.
    for (command, message) in [
        (create("MISSING", ""), "does not exist"),
        (create("*Model_Space", ""), "model-space or paper-space"),
        (create("INNER", ", x_scale=0.0"), "nonzero"),
        (create("INNER", ", column_count=0"), "greater than zero"),
        (create("", ""), "empty"),
    ] {
        dispatch(&mut host, &command);
        assert_eq!(count(&host), 1, "{command}");
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    // A block cannot contain itself, directly or through another block.
    let owner = |name: &str| format!(", owner_handle={}", records[name].value());
    dispatch(&mut host, &create("OUTER", &owner("OUTER")));
    assert_eq!(count(&host), 1);
    assert!(last(&host).contains("contain itself"), "{}", last(&host));
    dispatch(&mut host, &create("INNER", &owner("OUTER")));
    assert_eq!(count(&host), 2, "{}", last(&host));
    dispatch(&mut host, &create("OUTER", &owner("INNER")));
    assert_eq!(count(&host), 2);
    assert!(last(&host).contains("contain itself"), "{}", last(&host));
    // The legacy Polyline cannot be created and says what to use instead.
    dispatch(
        &mut host,
        "PY_EVAL ocs.active_document.create_entity('Polyline', vertices=[])",
    );
    assert!(
        last(&host).contains("Polyline2D or Polyline3D"),
        "{}",
        last(&host)
    );
}

#[test]
fn audit_python_ole2frame_lifecycle_over_real_ipc() {
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 120, 200, 255]))
        .save(std::env::temp_dir().join("ocs_ole_audit.png"))
        .unwrap();
    run_mesh_lifecycle(&MeshCase {
        create: "doc.embed_picture('TMPDIR/ocs_ole_audit.png', origin=(10, 10, 0), width=20)\n",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.upper_left_corner = (12.0, 40.0, 0.0)\n",
            "    e.lower_right_corner = (42.0, 10.0, 0.0)\n",
            "    e.lock_aspect = 1\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'upper_left_corner':{'x':42.0,'y':40.0,'z':0.0}",
                "nonzero width and height",
            ),
            ("'lock_aspect':2", "0 or 1"),
            (
                "'lower_right_corner':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'version':3", "read-only"),
            ("'storage':{}", "outside the editable schema"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Ole2Frame(_)),
        digest: |entity| match entity {
            EntityType::Ole2Frame(v) => {
                let picture = match document::entities::extract_presentation(&v.encoded_payload()) {
                    Some(document::entities::OlePresentation::Raster(bytes)) => bytes.len(),
                    _ => 0,
                };
                format!(
                    "ul{:.1},{:.1} lr{:.1},{:.1} lock{} picture{}",
                    v.upper_left_corner.x,
                    v.upper_left_corner.y,
                    v.lower_right_corner.x,
                    v.lower_right_corner.y,
                    v.lock_aspect,
                    picture
                )
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Ole2Frame(v) = entity {
                v.lower_right_corner.x = 50.0;
            }
        },
        expect_created: "ul10.0,25.0 lr30.0,10.0 lock0 picture119",
        expect_edited: "ul12.0,40.0 lr42.0,10.0 lock1 picture119",
        expect_reedited: "ul12.0,40.0 lr50.0,10.0 lock1 picture119",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
    });
}

/// Evidence for docs/cadkernel-body-path.md: primitive, transform and
/// boolean bodies become Solid3D/Body payloads that lift back losslessly
/// and survive DWG and DXF. Slow in a debug build (booleans take ~23 s).
#[test]
#[ignore = "kernel spike; slow in debug builds"]
fn spike_kernel_body_round_trip() {
    use crate::scene::convert::solid3d_tess::{acis_record, solid_record};
    use crate::scene::model::solid_model as sm;
    let make = |body: &ocs_solid::model::Brep| -> document::entities::Solid3D {
        let sat = crate::scene::convert::acis_export::solid_to_sat(body).expect("solid_to_sat");
        let mut solid = document::entities::Solid3D::new();
        solid.wires = sm::edge_wires(body);
        solid.set_sat_document(&sat);
        solid
    };
    let report = |label: &str, solid: &document::entities::Solid3D| {
        let body = solid_record(solid);
        eprintln!(
            "SPIKE {label}: lift={} volume={:?} wires={} sat_bytes={} binary={}",
            body.is_some(),
            body.as_ref().map(sm::volume),
            solid.wires.len(),
            solid.acis_data.sat_data.len(),
            solid.acis_data.is_binary
        );
        body
    };
    let boxed = sm::box_solid([0.0, 0.0, 0.0], 10.0, 6.0, 4.0).unwrap();
    let mut doc = document::Document::new();
    let solid = make(&boxed);
    report("created box", &solid);
    let handle = doc.add_entity(EntityType::Solid3D(solid.clone())).unwrap();

    // Round trip both formats and compare payloads.
    let dwg =
        crate::io::load_bytes("s.dwg", document::DwgWriter::write_to_vec(&doc).unwrap()).unwrap();
    let dxf = crate::io::load_bytes(
        "s.dxf",
        document::DxfWriter::new(&doc).write_to_vec().unwrap(),
    )
    .unwrap();
    for (label, reopened) in [("DWG", &dwg), ("DXF", &dxf)] {
        let Some(EntityType::Solid3D(back)) = reopened.get_entity(handle) else {
            panic!("{label} lost the solid")
        };
        report(&format!("{label} reopen"), back);
        eprintln!(
            "SPIKE {label}: acis_data equal={} wires equal={} silhouettes {}",
            back.acis_data == solid.acis_data,
            back.wires.len() == solid.wires.len(),
            back.silhouettes.len()
        );
    }

    // Transforms: translate, rotate, mirror, matrix.
    let moved = sm::by_matrix(
        &boxed,
        [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 5., 5., 5., 1.,
        ],
    )
    .unwrap();
    let turned = sm::turned(&boxed, 2, std::f64::consts::FRAC_PI_4, [0.0, 0.0, 0.0]).unwrap();
    let mirrored = sm::mirrored(&boxed, 0, [0.0, 0.0, 0.0]).unwrap();
    for (label, body) in [
        ("moved", &moved),
        ("turned", &turned),
        ("mirrored", &mirrored),
    ] {
        let solid = make(body);
        let extent = sm::extent(body);
        eprintln!(
            "SPIKE {label}: volume={:.3} extent={:?} lifts_back={}",
            sm::volume(body),
            extent,
            solid_record(&solid).is_some()
        );
    }

    // Booleans and primitives.
    let cyl = sm::cylinder_solid([0.0, 0.0, -3.0], 2.0, 10.0).unwrap();
    for (label, op) in [
        ("union", sm::Bool::Union),
        ("subtract", sm::Bool::Subtract),
        ("intersect", sm::Bool::Intersect),
    ] {
        match sm::boolean_result(op, &boxed, &cyl) {
            Ok(None) => eprintln!("SPIKE box {label} cylinder: nothing left"),
            Ok(Some(result)) => {
                let solid = make(&result);
                eprintln!(
                    "SPIKE box {label} cylinder: volume={:.3} faces_wires={} lifts_back={}",
                    sm::volume(&result),
                    solid.wires.len(),
                    solid_record(&solid).is_some()
                );
            }
            Err(snag) => eprintln!("SPIKE box {label} cylinder: refused {snag:?}"),
        }
    }
    for (label, body) in [
        ("sphere", sm::sphere_solid([0.0; 3], 3.0)),
        ("torus", sm::torus_solid([0.0; 3], 5.0, 1.0)),
        ("wedge", sm::wedge_solid([0.0; 3], 4.0, 3.0, 2.0)),
        ("pyramid", sm::pyramid_solid([0.0; 3], 3.0, 4.0, 5)),
    ] {
        match body {
            Some(body) => {
                let solid = make(&body);
                eprintln!(
                    "SPIKE {label}: volume={:.3} lifts_back={}",
                    sm::volume(&body),
                    solid_record(&solid).is_some()
                );
            }
            None => eprintln!("SPIKE {label}: kernel returned None"),
        }
    }

    // The same payload inside a Body entity and a Region-like container.
    let mut body_entity = document::entities::Body::new();
    body_entity
        .set_sat_document(&crate::scene::convert::acis_export::solid_to_sat(&boxed).unwrap());
    let lifted = acis_record(&body_entity.acis_data);
    eprintln!(
        "SPIKE Body entity: lift={} volume={:?}",
        lifted.is_some(),
        lifted.as_ref().map(sm::volume)
    );
    let bh = doc.add_entity(EntityType::Body(body_entity)).unwrap();
    let dwg =
        crate::io::load_bytes("b.dwg", document::DwgWriter::write_to_vec(&doc).unwrap()).unwrap();
    let dxf = crate::io::load_bytes(
        "b.dxf",
        document::DxfWriter::new(&doc).write_to_vec().unwrap(),
    )
    .unwrap();
    for (label, reopened) in [("DWG", &dwg), ("DXF", &dxf)] {
        match reopened.get_entity(bh) {
            Some(EntityType::Body(back)) => eprintln!(
                "SPIKE Body {label}: lift={}",
                acis_record(&back.acis_data).is_some()
            ),
            other => eprintln!(
                "SPIKE Body {label}: came back as {:?}",
                other.map(|e| format!("{e:?}").chars().take(30).collect::<String>())
            ),
        }
    }
}

/// Per-operation kernel timings for docs/cadkernel-body-path.md.
#[test]
#[ignore = "kernel spike; slow in debug builds"]
fn spike_kernel_timings() {
    use crate::scene::model::solid_model as sm;
    use std::time::Instant;
    let time = |label: &str, f: &mut dyn FnMut() -> String| {
        let start = Instant::now();
        let note = f();
        eprintln!(
            "TIMING {label}: {:.2}s {note}",
            start.elapsed().as_secs_f64()
        );
    };
    let boxed = sm::box_solid([0.0; 3], 10.0, 6.0, 4.0).unwrap();
    let cyl = sm::cylinder_solid([0.0, 0.0, -3.0], 2.0, 10.0).unwrap();
    time("box primitive", &mut || {
        format!("{}", sm::box_solid([0.0; 3], 10.0, 6.0, 4.0).is_some())
    });
    time("sphere primitive", &mut || {
        format!("{}", sm::sphere_solid([0.0; 3], 3.0).is_some())
    });
    time("solid_to_sat(box)", &mut || {
        format!(
            "{}",
            crate::scene::convert::acis_export::solid_to_sat(&boxed).is_some()
        )
    });
    time("edge_wires(box)", &mut || {
        format!("{}", sm::edge_wires(&boxed).len())
    });
    time("transform (matrix)", &mut || {
        format!(
            "{}",
            sm::by_matrix(
                &boxed,
                [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 5., 5., 5., 1.]
            )
            .is_some()
        )
    });
    time("union", &mut || {
        format!(
            "{}",
            sm::boolean_result(sm::Bool::Union, &boxed, &cyl).is_ok()
        )
    });
    time("subtract", &mut || {
        format!(
            "{}",
            sm::boolean_result(sm::Bool::Subtract, &boxed, &cyl).is_ok()
        )
    });
    time("intersect", &mut || {
        format!(
            "{}",
            sm::boolean_result(sm::Bool::Intersect, &boxed, &cyl).is_ok()
        )
    });
    let result = sm::boolean_result(sm::Bool::Union, &boxed, &cyl)
        .unwrap()
        .unwrap();
    time("solid_to_sat(union)", &mut || {
        format!(
            "{}",
            crate::scene::convert::acis_export::solid_to_sat(&result).is_some()
        )
    });
    time("edge_wires(union)", &mut || {
        format!("{}", sm::edge_wires(&result).len())
    });
    time("volume(union)", &mut || {
        format!("{:.1}", sm::volume(&result))
    });
}

#[test]
fn audit_python_solid3d_lifecycle_over_real_ipc() {
    use crate::scene::convert::solid3d_tess::solid_record;
    use crate::scene::model::solid_model as sm;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let solids = |host: &HostSession<'_>| -> Vec<Handle> {
        let mut list: Vec<_> = host
            .document()
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(s) => Some(s.common.handle),
                _ => None,
            })
            .collect();
        list.sort_by_key(|h| h.value());
        list
    };
    let body_of = |host: &HostSession<'_>, handle: Handle| {
        let Some(EntityType::Solid3D(solid)) = host.document().get_entity(handle) else {
            panic!("no solid {handle:?}")
        };
        solid_record(solid).expect("payload lifts losslessly")
    };
    let volume = |host: &HostSession<'_>, handle: Handle| sm::volume(&body_of(host, handle));
    let extent =
        |host: &HostSession<'_>, handle: Handle| sm::extent(&body_of(host, handle)).unwrap();
    let close = |a: f64, b: f64, tolerance: f64| (a - b).abs() <= tolerance * b.abs().max(1.0);

    // C: every primitive, checked against its analytic volume.
    let script = std::env::temp_dir().join(format!("ocs_solid_create_{}.py", std::process::id()));
    std::fs::write(
        &script,
        concat!(
            "s = ocs.active_document.solids\n",
            "s.box(center=(0, 0, 0), size=(10, 6, 4))\n",
            "s.cylinder(center=(20, 0, 0), radius=2, height=5)\n",
            "s.sphere(center=(40, 0, 0), radius=3)\n",
            "s.torus(center=(60, 0, 0), major=5, minor=1)\n",
            "s.wedge(origin=(80, 0, 0), size=(4, 3, 2))\n",
            "s.pyramid(center=(100, 0, 0), radius=3, height=4, sides=5)\n",
            "s.box(center=(0, 20, 0), size=(2, 2, 2), layer='SOLIDS')\n",
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let list = solids(&host);
    assert_eq!(list.len(), 7, "{}", last(&host));
    let pi = std::f64::consts::PI;
    let five_gon = 0.5 * 5.0 * 3.0f64.powi(2) * (2.0 * pi / 5.0).sin();
    for (index, expected, tolerance) in [
        (0, 240.0, 1e-9),
        (1, pi * 4.0 * 5.0, 0.02),
        (2, 4.0 / 3.0 * pi * 27.0, 0.02),
        (3, 2.0 * pi * pi * 5.0 * 1.0, 0.02),
        (4, 12.0, 1e-9),
        (5, five_gon * 4.0 / 3.0, 0.02),
        (6, 8.0, 1e-9),
    ] {
        let actual = volume(&host, list[index]);
        assert!(
            close(actual, expected, tolerance),
            "solid {index}: volume {actual}, expected {expected}"
        );
    }
    let Some(EntityType::Solid3D(first)) = host.document().get_entity(list[0]) else {
        unreachable!()
    };
    assert_eq!((first.wires.len(), first.common.layer.as_str()), (12, "0"));
    assert!(
        matches!(host.document().get_entity(list[6]), Some(EntityType::Solid3D(s)) if s.common.layer == "SOLIDS")
    );
    assert!(
        !host.app.tabs[0]
            .scene
            .wire_models_for(&[list[0]])
            .is_empty(),
        "no canvas geometry"
    );
    let read = |host: &mut HostSession<'_>, expression: &str| {
        dispatch(host, &format!("PY_EVAL {expression}"));
        last(host)
    };
    assert!(read(
        &mut host,
        &format!("ocs.active_document.entities[{}].kind", list[0].value())
    )
    .contains("Solid3D"));

    // E: rigid moves keep the volume and move the extent exactly.
    let box_handle = list[0];
    let before = host.document().get_entity(box_handle).unwrap().clone();
    let undo_before = host.app.tabs[0].history.undo_stack.len();
    let (low, high) = extent(&host, box_handle);
    assert!(
        close(low[0], -5.0, 1e-6) && close(high[2], 2.0, 1e-6),
        "{low:?} {high:?}"
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (5, 5, 5)).handle",
            box_handle.value()
        ),
    );
    let (low, high) = extent(&host, box_handle);
    assert!(
        close(low[0], 0.0, 1e-6) && close(high[1], 8.0, 1e-6) && close(high[2], 7.0, 1e-6),
        "{low:?} {high:?}"
    );
    dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.rotate({}, 'z', 0.7853981633974483, (5, 5, 5)).handle", box_handle.value()));
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.mirror({}, 'x', (0, 0, 0)).handle",
            box_handle.value()
        ),
    );
    assert!(
        close(volume(&host, box_handle), 240.0, 1e-9),
        "moves preserve volume"
    );
    let Some(EntityType::Solid3D(moved)) = host.document().get_entity(box_handle) else {
        unreachable!()
    };
    assert_eq!(
        (moved.wires.len(), moved.history_handle, moved.common.handle),
        (12, None, box_handle)
    );
    assert_eq!(moved.common.layer, "0");
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len() - undo_before,
        3,
        "one undo step per move"
    );

    // V: refused operations change nothing and leave no undo step.
    let snapshot = host.document().get_entity(box_handle).unwrap().clone();
    let count_before = solids(&host).len();
    let undo_now = host.app.tabs[0].history.undo_stack.len();
    let empty_payload = document::entities::Solid3D::new();
    let unliftable = host.add_entity(EntityType::Solid3D(empty_payload.clone()));
    let unliftable_before = host.document().get_entity(unliftable).unwrap().clone();
    for (command, message) in [
        (
            "ocs.active_document.solids.box(size=(0, 1, 1))".to_string(),
            "greater than zero",
        ),
        (
            "ocs.active_document.solids.box(center=(float('nan'), 0, 0))".to_string(),
            "must be finite",
        ),
        (
            "ocs.active_document.solids.torus(major=1, minor=2)".to_string(),
            "smaller than the major",
        ),
        (
            "ocs.active_document.solids.pyramid(sides=2)".to_string(),
            "between 3 and 1024",
        ),
        (
            "ocs.active_document.solids.sphere(radius=-1)".to_string(),
            "greater than zero",
        ),
        (
            "ocs.active_document.solids.box(layer='')".to_string(),
            "layer name is empty",
        ),
        (
            "ocs.solid_create('blob', [1.0], None)".to_string(),
            "unknown primitive",
        ),
        (
            "ocs.solid_create('box', [1.0], None)".to_string(),
            "takes 6 numbers",
        ),
        (
            format!(
                "ocs.active_document.solids.transform({}, [2,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1])",
                box_handle.value()
            ),
            "rigid moves",
        ),
        (
            format!(
                "ocs.active_document.solids.transform({}, [1]*16)",
                box_handle.value()
            ),
            "rigid moves",
        ),
        (
            format!(
                "ocs.active_document.solids.translate({}, (float('nan'), 0, 0))",
                box_handle.value()
            ),
            "must be finite",
        ),
        (
            format!(
                "ocs.active_document.solids.translate({}, (1, 0, 0))",
                unliftable.value()
            ),
            "cannot be lifted losslessly",
        ),
        (
            "ocs.active_document.solids.translate(999999, (1, 0, 0))".to_string(),
            "does not exist",
        ),
    ] {
        dispatch(&mut host, &format!("PY_EVAL {command}"));
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    assert_eq!(host.document().get_entity(box_handle), Some(&snapshot));
    assert_eq!(
        host.document().get_entity(unliftable),
        Some(&unliftable_before)
    );
    assert_eq!(
        solids(&host).len(),
        count_before + 1,
        "only the fixture was added"
    );
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        undo_now,
        "refusals record no undo step"
    );
    // Transforming a non-solid is refused.
    let line = host.add_entity(EntityType::Line(document::entities::Line::from_points(
        document::types::Vector3::ZERO,
        document::types::Vector3::new(1.0, 0.0, 0.0),
    )));
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (1, 0, 0))",
            line.value()
        ),
    );
    assert!(
        last(&host).contains("outside the Python schema")
            || last(&host).contains("applies to Solid3D"),
        "{}",
        last(&host)
    );

    // W: both formats reopen the moved solid losslessly; the reopened
    // payload can be moved again.
    let moved_volume = volume(&host, box_handle);
    let moved_extent = extent(&host, box_handle);
    let dwg = crate::io::load_bytes(
        "solid.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "solid.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        let Some(EntityType::Solid3D(back)) = document.get_entity(box_handle) else {
            panic!("{format} lost the solid")
        };
        let body = solid_record(back).unwrap_or_else(|| panic!("{format} payload does not lift"));
        assert!(close(sm::volume(&body), moved_volume, 1e-6), "{format}");
        let reopened_extent = sm::extent(&body).unwrap();
        for axis in 0..3 {
            assert!(
                close(reopened_extent.0[axis], moved_extent.0[axis], 1e-6)
                    && close(reopened_extent.1[axis], moved_extent.1[axis], 1e-6),
                "{format} extent"
            );
        }
        let again = host.add_entity(EntityType::Solid3D(back.clone()));
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle",
                again.value()
            ),
        );
        let (low, _) = extent(&host, again);
        assert!(
            close(low[0], reopened_extent.0[0] + 100.0, 1e-6),
            "{format} re-edit"
        );
    }

    // D and U: delete, then undo and redo restore the moved state.
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            box_handle.value()
        ),
    );
    assert!(host.document().get_entity(box_handle).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    // Steps after creation: 3 moves, 2 re-edits of reopened copies, 1 delete.
    app.undo_steps(1);
    assert!(
        app.tabs[0].scene.document.get_entity(box_handle).is_some(),
        "undo restores the deleted solid"
    );
    app.undo_steps(5);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(box_handle),
        Some(&before),
        "undoing every move restores the original solid exactly"
    );
    app.redo_steps(6);
    assert!(
        app.tabs[0].scene.document.get_entity(box_handle).is_none(),
        "redo replays the moves and the delete"
    );
}

#[test]
fn audit_python_body_transform_over_real_ipc() {
    use crate::scene::model::solid_model as sm;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    // A Body entity carrying a kernel box; scripts cannot create one.
    let boxed = sm::box_solid([0.0; 3], 4.0, 4.0, 4.0).unwrap();
    let mut body = document::entities::Body::new();
    body.set_sat_document(&crate::scene::convert::acis_export::solid_to_sat(&boxed).unwrap());
    body.wires = sm::edge_wires(&boxed);
    let handle = host.add_entity(EntityType::Body(body));
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let extent = |host: &HostSession<'_>| {
        let Some(EntityType::Body(b)) = host.document().get_entity(handle) else {
            panic!("no body")
        };
        sm::extent(&crate::scene::convert::solid3d_tess::acis_record(&b.acis_data).unwrap())
            .unwrap()
    };
    assert!((extent(&host).0[0] + 2.0).abs() < 1e-6);
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].kind",
            handle.value()
        ),
    );
    assert!(host
        .app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("Body"));
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (10, 0, 0)).handle",
            handle.value()
        ),
    );
    assert!(
        (extent(&host).0[0] - 8.0).abs() < 1e-6,
        "{:?}",
        extent(&host)
    );
    let bytes = document::DwgWriter::write_to_vec(host.document()).unwrap();
    let reopened = crate::io::load_bytes("body.dwg", bytes).unwrap();
    let Some(EntityType::Body(back)) = reopened.get_entity(handle) else {
        panic!("DWG lost the body")
    };
    let lifted =
        crate::scene::convert::solid3d_tess::acis_record(&back.acis_data).expect("DWG body lifts");
    assert!((sm::volume(&lifted) - 64.0).abs() < 1e-6);
    // Scripts cannot create a Body and get the explanatory message.
    dispatch(
        &mut host,
        "PY_EVAL ocs.active_document.create_entity('Body')",
    );
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("use doc.solids"),
        "{}",
        host.app.command_line.history.last().unwrap().text
    );
}

#[test]
fn audit_python_region_lifecycle_over_real_ipc() {
    use crate::scene::convert::solid3d_tess::acis_record;
    use crate::scene::model::solid_model as sm;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let regions = |host: &HostSession<'_>| -> Vec<Handle> {
        let mut list: Vec<_> = host
            .document()
            .entities()
            .filter_map(|e| match e {
                EntityType::Region(r) => Some(r.common.handle),
                _ => None,
            })
            .collect();
        list.sort_by_key(|h| h.value());
        list
    };
    let extent_in = |document: &Document, handle: Handle| {
        let Some(EntityType::Region(region)) = document.get_entity(handle) else {
            panic!("no region {handle:?}")
        };
        let body = acis_record(&region.acis_data).expect("region payload lifts losslessly");
        sm::extent(&body).unwrap()
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;

    // C: regions from a circle (source kept), a closed rectangle (source
    // deleted) and a second circle on its own layer.
    let script = std::env::temp_dir().join(format!("ocs_region_create_{}.py", std::process::id()));
    std::fs::write(&script, concat!(
            "doc = ocs.active_document\n",
            "circle = doc.create_entity('Circle', center=(0, 0, 0), radius=5)\n",
            "rect = doc.create_entity('LwPolyline', is_closed=True, vertices=[{'location': {'x': 20.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 6.0}}, {'location': {'x': 20.0, 'y': 6.0}}])\n",
            "doc.solids.region(circle)\n",
            "doc.solids.region(rect, delete_source=True)\n",
            "other = doc.create_entity('Circle', center=(50, 0, 0), radius=2, layer='PROFILES')\n",
            "doc.solids.region(other, layer='REGIONS')\n",
        )).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let list = regions(&host);
    assert_eq!(list.len(), 3, "{}", last(&host));
    let (low, high) = extent_in(host.document(), list[0]);
    assert!(
        close(low[0], -5.0)
            && close(high[0], 5.0)
            && close(low[1], -5.0)
            && close(high[1], 5.0)
            && close(low[2], 0.0)
            && close(high[2], 0.0),
        "{low:?} {high:?}"
    );
    let (low, high) = extent_in(host.document(), list[1]);
    assert!(
        close(low[0], 20.0) && close(high[0], 30.0) && close(high[1], 6.0),
        "{low:?} {high:?}"
    );
    let layer_of = |host: &HostSession<'_>, handle: Handle| {
        host.document()
            .get_entity(handle)
            .unwrap()
            .common()
            .layer
            .clone()
    };
    assert_eq!(
        layer_of(&host, list[0]),
        "0",
        "defaults to the profile's layer"
    );
    assert_eq!(layer_of(&host, list[2]), "REGIONS");
    let circles = host
        .document()
        .entities()
        .filter(|e| matches!(e, EntityType::Circle(_)))
        .count();
    let polylines = host
        .document()
        .entities()
        .filter(|e| matches!(e, EntityType::LwPolyline(_)))
        .count();
    assert_eq!(
        (circles, polylines),
        (2, 0),
        "delete_source removed only the rectangle"
    );
    let Some(EntityType::Region(first)) = host.document().get_entity(list[0]) else {
        unreachable!()
    };
    assert_eq!(
        (first.wires.is_empty(), first.point_of_reference.z),
        (false, 0.0)
    );
    assert!(
        !host.app.tabs[0]
            .scene
            .wire_models_for(&[list[0]])
            .is_empty(),
        "no canvas geometry"
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].kind",
            list[0].value()
        ),
    );
    assert!(last(&host).contains("Region"));

    // V: refusals change nothing and leave no undo step.
    let open_line = host.add_entity(EntityType::Line(document::entities::Line::from_points(
        document::types::Vector3::ZERO,
        document::types::Vector3::new(5.0, 0.0, 0.0),
    )));
    let mut open_poly = document::entities::LwPolyline::new();
    open_poly.vertices = vec![
        document::entities::LwVertex::new(document::types::Vector2::new(0.0, 0.0)),
        document::entities::LwVertex::new(document::types::Vector2::new(4.0, 0.0)),
        document::entities::LwVertex::new(document::types::Vector2::new(4.0, 4.0)),
    ];
    let open_poly = host.add_entity(EntityType::LwPolyline(open_poly));
    let ray = host.add_entity(EntityType::Ray(document::entities::Ray::default()));
    let entity_count = host.document().entities().count();
    let undo_now = host.app.tabs[0].history.undo_stack.len();
    for (command, message) in [
        (
            format!("ocs.active_document.solids.region({})", open_line.value()),
            "must be closed",
        ),
        (
            format!("ocs.active_document.solids.region({})", open_poly.value()),
            "must be closed",
        ),
        (
            format!("ocs.active_document.solids.region({})", ray.value()),
            "planar profile",
        ),
        (
            format!("ocs.active_document.solids.region({})", list[0].value()),
            "already a region",
        ),
        (
            "ocs.active_document.solids.region(999999)".to_string(),
            "does not exist",
        ),
        (
            format!(
                "ocs.active_document.solids.region({}, layer='')",
                open_line.value()
            ),
            "layer name is empty",
        ),
    ] {
        dispatch(&mut host, &format!("PY_EVAL {command}"));
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    assert_eq!(host.document().entities().count(), entity_count);
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        undo_now,
        "refusals record no undo step"
    );

    // E: a region moves through the same kernel channel.
    let before = host.document().get_entity(list[0]).unwrap().clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (10, 5, 0)).handle",
            list[0].value()
        ),
    );
    let (low, high) = extent_in(host.document(), list[0]);
    assert!(
        close(low[0], 5.0) && close(high[0], 15.0) && close(low[1], 0.0) && close(high[1], 10.0),
        "{low:?} {high:?}"
    );

    // W: both formats reopen a region that lifts with the same extent, and
    // a reopened region moves again.
    let dwg = crate::io::load_bytes(
        "region.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "region.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    let moved = extent_in(host.document(), list[0]);
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        assert_eq!(regions_in(document).len(), 3, "{format}");
        let reopened = extent_in(document, list[0]);
        for axis in 0..3 {
            assert!(
                close(reopened.0[axis], moved.0[axis]) && close(reopened.1[axis], moved.1[axis]),
                "{format} extent {reopened:?} vs {moved:?}"
            );
        }
        let Some(EntityType::Region(back)) = document.get_entity(list[0]) else {
            unreachable!()
        };
        let again = host.add_entity(EntityType::Region(back.clone()));
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle",
                again.value()
            ),
        );
        assert!(
            close(
                extent_in(host.document(), again).0[0],
                reopened.0[0] + 100.0
            ),
            "{format} re-edit"
        );
    }
    fn regions_in(document: &Document) -> Vec<Handle> {
        document
            .entities()
            .filter_map(|e| match e {
                EntityType::Region(r) => Some(r.common.handle),
                _ => None,
            })
            .collect()
    }

    // D and U: delete, undo, and undo the creation steps exactly.
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            list[2].value()
        ),
    );
    assert!(host.document().get_entity(list[2]).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(1);
    assert!(
        app.tabs[0].scene.document.get_entity(list[2]).is_some(),
        "undo restores the deleted region"
    );
    // Two re-edits of reopened copies, then the move of the first region.
    app.undo_steps(3);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(list[0]),
        Some(&before),
        "undoing the move restores the region exactly"
    );
}

#[test]
fn audit_python_surface_lifecycle_over_real_ipc() {
    use crate::scene::convert::solid3d_tess::acis_record;
    use crate::scene::model::solid_model as sm;
    use document::entities::SurfaceKind;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let sorted = |mut list: Vec<Handle>| {
        list.sort_by_key(|h| h.value());
        list
    };
    let surfaces_in = |document: &Document| {
        sorted(
            document
                .entities()
                .filter_map(|e| match e {
                    EntityType::Surface(s) => Some(s.common.handle),
                    _ => None,
                })
                .collect(),
        )
    };
    let solids_in = |document: &Document| {
        sorted(
            document
                .entities()
                .filter_map(|e| match e {
                    EntityType::Solid3D(s) => Some(s.common.handle),
                    _ => None,
                })
                .collect(),
        )
    };
    let body_of = |document: &Document, handle: Handle| match document.get_entity(handle) {
        Some(EntityType::Surface(s)) => {
            acis_record(&s.acis_data).expect("surface payload lifts losslessly")
        }
        Some(EntityType::Solid3D(s)) => {
            acis_record(&s.acis_data).expect("solid payload lifts losslessly")
        }
        other => panic!("no surface or solid {handle:?}: {other:?}"),
    };
    let extent_in =
        |document: &Document, handle: Handle| sm::extent(&body_of(document, handle)).unwrap();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let kind_in = |document: &Document, handle: Handle| match document.get_entity(handle) {
        Some(EntityType::Surface(s)) => s.kind,
        other => panic!("no surface {handle:?}: {other:?}"),
    };

    // C: plane surfaces and extrusions from real profile entities.
    let script = std::env::temp_dir().join(format!("ocs_surface_create_{}.py", std::process::id()));
    std::fs::write(&script, concat!(
            "doc = ocs.active_document\n",
            "s = doc.solids\n",
            "circle = doc.create_entity('Circle', center=(0, 0, 0), radius=3)\n",
            "rect = doc.create_entity('LwPolyline', is_closed=True, vertices=[{'location': {'x': 20.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 6.0}}, {'location': {'x': 20.0, 'y': 6.0}}])\n",
            "open_path = doc.create_entity('LwPolyline', vertices=[{'location': {'x': 50.0, 'y': 0.0}}, {'location': {'x': 54.0, 'y': 0.0}}, {'location': {'x': 54.0, 'y': 4.0}}])\n",
            "temp = doc.create_entity('Circle', center=(70, 0, 0), radius=2)\n",
            "s.surface(circle)\n",
            "s.surface(rect, layer='SURFACES')\n",
            "s.extrude(rect, (0, 0, 4))\n",
            "s.extrude(circle, (0, 0, 5))\n",
            "s.extrude(open_path, (0, 0, 3))\n",
            "s.extrude(temp, (0, 0, 2), delete_source=True)\n",
        )).unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let surfaces = surfaces_in(host.document());
    let solids = solids_in(host.document());
    assert_eq!((surfaces.len(), solids.len()), (3, 3), "{}", last(&host));
    // surfaces[0] plane of the circle, [1] plane of the rectangle, [2] extruded open path.
    assert_eq!(
        (
            kind_in(host.document(), surfaces[0]),
            kind_in(host.document(), surfaces[1]),
            kind_in(host.document(), surfaces[2])
        ),
        (SurfaceKind::Plane, SurfaceKind::Plane, SurfaceKind::Generic)
    );
    let (low, high) = extent_in(host.document(), surfaces[0]);
    assert!(
        close(low[0], -3.0) && close(high[1], 3.0) && close(low[2], 0.0) && close(high[2], 0.0),
        "{low:?} {high:?}"
    );
    let (low, high) = extent_in(host.document(), surfaces[2]);
    assert!(
        close(low[0], 50.0) && close(high[0], 54.0) && close(low[2], 0.0) && close(high[2], 3.0),
        "open path extrusion {low:?} {high:?}"
    );
    assert_eq!(
        host.document()
            .get_entity(surfaces[1])
            .unwrap()
            .common()
            .layer,
        "SURFACES"
    );
    let pi = std::f64::consts::PI;
    for (index, expected, tolerance) in [
        (0, 240.0, 1e-9),
        (1, pi * 9.0 * 5.0, 0.02),
        (2, pi * 4.0 * 2.0, 0.02),
    ] {
        let volume = sm::volume(&body_of(host.document(), solids[index]));
        assert!(
            (volume - expected).abs() <= tolerance * expected,
            "solid {index}: {volume} vs {expected}"
        );
    }
    assert_eq!(
        host.document()
            .entities()
            .filter(|e| matches!(e, EntityType::Circle(_)))
            .count(),
        1,
        "delete_source removed only the extruded temporary circle"
    );
    assert!(
        !host.app.tabs[0]
            .scene
            .wire_models_for(&[surfaces[0]])
            .is_empty(),
        "no canvas geometry"
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].kind",
            surfaces[0].value()
        ),
    );
    assert!(last(&host).contains("Surface"));

    // V: refusals change nothing and leave no undo step.
    let ray = host.add_entity(EntityType::Ray(document::entities::Ray::default()));
    let circle = host
        .document()
        .entities()
        .find_map(|e| match e {
            EntityType::Circle(c) => Some(c.common.handle),
            _ => None,
        })
        .unwrap();
    let counts = (
        host.document().entities().count(),
        host.app.tabs[0].history.undo_stack.len(),
    );
    for (command, message) in [
        (
            format!(
                "ocs.active_document.solids.extrude({}, (0, 0, 0))",
                circle.value()
            ),
            "nonzero",
        ),
        (
            format!(
                "ocs.active_document.solids.extrude({}, (float('nan'), 0, 1))",
                circle.value()
            ),
            "must be finite",
        ),
        (
            format!(
                "ocs.active_document.solids.extrude({}, (0, 0, 1))",
                ray.value()
            ),
            "planar profile",
        ),
        (
            format!(
                "ocs.active_document.solids.extrude({}, (0, 0, 1))",
                solids[0].value()
            ),
            "curve profile",
        ),
        (
            format!(
                "ocs.active_document.solids.surface({})",
                surfaces[0].value()
            ),
            "curve profile",
        ),
        (
            format!("ocs.active_document.solids.surface({})", ray.value()),
            "planar profile",
        ),
        (
            format!(
                "ocs.active_document.solids.surface({}, layer='')",
                circle.value()
            ),
            "layer name is empty",
        ),
        (
            "ocs.active_document.solids.surface(999999)".to_string(),
            "does not exist",
        ),
        (
            "ocs.solid_extrude(1, [1.0], None, False)".to_string(),
            "3 numbers",
        ),
    ] {
        dispatch(&mut host, &format!("PY_EVAL {command}"));
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    assert_eq!(
        (
            host.document().entities().count(),
            host.app.tabs[0].history.undo_stack.len()
        ),
        counts,
        "refusals change nothing and record no undo step"
    );

    // E: isoline density is scriptable and validated; kind and payload are not.
    let plane = surfaces[0];
    let plane_before = host.document().get_entity(plane).unwrap().clone();
    let script = std::env::temp_dir().join(format!("ocs_surface_edit_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "e = doc.entities[{}]\n",
                "with doc.transaction('Edit surface'):\n",
                "    e.u_isolines = 8\n",
                "    e.v_isolines = 6\n",
            ),
            plane.value()
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let Some(EntityType::Surface(edited)) = host.document().get_entity(plane) else {
        unreachable!()
    };
    assert_eq!(
        (edited.u_isolines, edited.v_isolines, edited.kind),
        (8, 6, SurfaceKind::Plane),
        "{}",
        last(&host)
    );
    let edited_state = host.document().get_entity(plane).unwrap().clone();
    for (patch, message) in [
        ("'u_isolines':500", "between 0 and 200"),
        ("'surface_kind':'Extruded'", "read-only"),
        ("'surface_data':{}", "outside the editable schema"),
        ("'v_isolines':-1", "between 0 and 200"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject surface', [{{'handle':{}, {patch}}}])",
                plane.value()
            ),
        );
        assert_eq!(
            host.document().get_entity(plane),
            Some(&edited_state),
            "{patch}"
        );
        assert!(last(&host).contains(message), "{patch}: {}", last(&host));
    }
    // Moves keep a plane a plane and turn a swept surface generic.
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (10, 5, 0)).handle",
            plane.value()
        ),
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.translate({}, (0, 0, 7)).handle",
            surfaces[2].value()
        ),
    );
    assert_eq!(kind_in(host.document(), plane), SurfaceKind::Plane);
    let (low, high) = extent_in(host.document(), plane);
    assert!(
        close(low[0], 7.0) && close(high[0], 13.0) && close(low[1], 2.0) && close(high[1], 8.0),
        "{low:?} {high:?}"
    );
    let (low, _) = extent_in(host.document(), surfaces[2]);
    assert!(close(low[2], 7.0), "{low:?}");

    // W: both formats reopen surfaces that lift with the same extents, keep
    // the isolines, and a reopened surface moves again.
    let dwg = crate::io::load_bytes(
        "surface.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "surface.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    let moved = extent_in(host.document(), plane);
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        assert_eq!(
            (surfaces_in(document).len(), solids_in(document).len()),
            (3, 3),
            "{format}"
        );
        let reopened = extent_in(document, plane);
        for axis in 0..3 {
            assert!(
                close(reopened.0[axis], moved.0[axis]) && close(reopened.1[axis], moved.1[axis]),
                "{format} extent"
            );
        }
        let Some(EntityType::Surface(back)) = document.get_entity(plane) else {
            unreachable!()
        };
        assert_eq!(
            (back.u_isolines, back.v_isolines),
            (8, 6),
            "{format} isolines"
        );
        assert_eq!(
            back.kind,
            SurfaceKind::Plane,
            "{format} keeps the plane kind"
        );
        let again = host.add_entity(EntityType::Surface(back.clone()));
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle",
                again.value()
            ),
        );
        assert!(
            close(
                extent_in(host.document(), again).0[0],
                reopened.0[0] + 100.0
            ),
            "{format} re-edit"
        );
    }

    // D and U.
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            surfaces[1].value()
        ),
    );
    assert!(host.document().get_entity(surfaces[1]).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(1);
    assert!(
        app.tabs[0].scene.document.get_entity(surfaces[1]).is_some(),
        "undo restores the deleted surface"
    );
    // Two re-edits of reopened copies, two moves, and the isoline edit come before.
    app.undo_steps(2 + 2 + 1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(plane),
        Some(&plane_before),
        "undoing the edits restores the surface exactly"
    );
}

#[test]
fn audit_python_boolean_lifecycle_over_real_ipc() {
    use crate::scene::convert::solid3d_tess::solid_record;
    use crate::scene::model::solid_model as sm;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let solids_in = |document: &Document| {
        let mut list: Vec<Handle> = document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(s) => Some(s.common.handle),
                _ => None,
            })
            .collect();
        list.sort_by_key(|h| h.value());
        list
    };
    let body_in = |document: &Document, handle: Handle| {
        let Some(EntityType::Solid3D(solid)) = document.get_entity(handle) else {
            panic!("no solid {handle:?}")
        };
        solid_record(solid).expect("payload lifts losslessly")
    };
    let volume_in = |document: &Document, handle: Handle| sm::volume(&body_in(document, handle));
    let extent_in =
        |document: &Document, handle: Handle| sm::extent(&body_in(document, handle)).unwrap();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);

    // C: all three operations on two overlapping boxes, keeping the
    // operands. A: x 0..10, B: x 5..15, both 6 by 4; the overlap is 120.
    let script = std::env::temp_dir().join(format!("ocs_boolean_create_{}.py", std::process::id()));
    std::fs::write(
        &script,
        concat!(
            "s = ocs.active_document.solids\n",
            "a = s.box(center=(5, 3, 2), size=(10, 6, 4))\n",
            "b = s.box(center=(10, 3, 2), size=(10, 6, 4))\n",
            "s.union(a, b, keep_operands=True)\n",
            "s.subtract(a, b, keep_operands=True)\n",
            "s.intersect(a, b, keep_operands=True)\n",
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let list = solids_in(host.document());
    assert_eq!(
        list.len(),
        5,
        "operands kept and three results: {}",
        last(&host)
    );
    let (a, b) = (list[0], list[1]);
    for (index, expected) in [(2, 360.0), (3, 120.0), (4, 120.0)] {
        let actual = volume_in(host.document(), list[index]);
        assert!(
            close(actual, expected),
            "result {index}: volume {actual}, expected {expected}"
        );
    }
    let (low, high) = extent_in(host.document(), list[3]);
    assert!(
        close(low[0], 0.0) && close(high[0], 5.0),
        "subtract keeps x 0..5: {low:?} {high:?}"
    );
    let (low, high) = extent_in(host.document(), list[4]);
    assert!(
        close(low[0], 5.0) && close(high[0], 10.0),
        "intersect keeps x 5..10: {low:?} {high:?}"
    );
    let Some(EntityType::Solid3D(union)) = host.document().get_entity(list[2]) else {
        unreachable!()
    };
    assert_eq!(
        (union.wires.is_empty(), union.history_handle),
        (false, None)
    );
    assert!(
        !host.app.tabs[0]
            .scene
            .wire_models_for(&[list[2]])
            .is_empty(),
        "no canvas geometry"
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].kind",
            list[2].value()
        ),
    );
    assert!(last(&host).contains("Solid3D"));

    // Consuming operands: A minus B leaves only the result, on a new layer.
    let before_consume = (
        host.document().get_entity(a).unwrap().clone(),
        host.document().get_entity(b).unwrap().clone(),
    );
    let undo_before = host.app.tabs[0].history.undo_stack.len();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.subtract({}, {}, layer='CUT').handle",
            a.value(),
            b.value()
        ),
    );
    assert!(
        host.document().get_entity(a).is_none() && host.document().get_entity(b).is_none(),
        "operands are consumed"
    );
    let cut = *solids_in(host.document()).last().unwrap();
    assert!(close(volume_in(host.document(), cut), 120.0));
    assert_eq!(
        host.document().get_entity(cut).unwrap().common().layer,
        "CUT"
    );
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len() - undo_before,
        1,
        "one undo step per boolean"
    );

    // A second planar operation chains on the result: drill a through-hole.
    dispatch(
        &mut host,
        "PY_EVAL ocs.active_document.solids.box(center=(2.5, 3, 2), size=(2, 2, 6)).handle",
    );
    let hole = *solids_in(host.document()).last().unwrap();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.subtract({}, {}).handle",
            cut.value(),
            hole.value()
        ),
    );
    let drilled = *solids_in(host.document()).last().unwrap();
    assert!(
        close(volume_in(host.document(), drilled), 104.0),
        "120 minus a 2x2x4 hole is 104"
    );

    // V: refusals change nothing and leave no undo step.
    let line = host.add_entity(EntityType::Line(document::entities::Line::from_points(
        document::types::Vector3::ZERO,
        document::types::Vector3::new(1.0, 0.0, 0.0),
    )));
    let unliftable = host.add_entity(EntityType::Solid3D(document::entities::Solid3D::new()));
    let mut plane = document::entities::Surface::new(document::entities::SurfaceKind::Plane);
    plane.acis_data = document::entities::AcisData::new();
    let surface = host.add_entity(EntityType::Surface(Box::new(plane)));
    let far = {
        dispatch(
            &mut host,
            "PY_EVAL ocs.active_document.solids.box(center=(500, 500, 500), size=(1, 1, 1)).handle",
        );
        *solids_in(host.document()).last().unwrap()
    };
    let counts = (
        host.document().entities().count(),
        host.app.tabs[0].history.undo_stack.len(),
    );
    let z = drilled.value();
    for (command, message) in [
        (
            format!("ocs.active_document.solids.union({z}, {z})"),
            "cannot be combined with itself",
        ),
        (
            format!("ocs.active_document.solids.union({z}, {})", line.value()),
            "must be a Solid3D",
        ),
        (
            format!("ocs.active_document.solids.union({}, {z})", surface.value()),
            "must be a Solid3D",
        ),
        (
            format!("ocs.active_document.solids.union({z}, 999999)"),
            "does not exist",
        ),
        (
            format!(
                "ocs.active_document.solids.union({z}, {})",
                unliftable.value()
            ),
            "cannot be lifted losslessly",
        ),
        (
            format!(
                "ocs.active_document.solids.union({z}, {}, layer='')",
                far.value()
            ),
            "layer name is empty",
        ),
        (
            format!(
                "ocs.solid_boolean({z}, {}, 'xor', None, False)",
                far.value()
            ),
            "unknown operation",
        ),
        (
            format!("ocs.active_document.solids.intersect({z}, {})", far.value()),
            "changed",
        ),
    ] {
        dispatch(&mut host, &format!("PY_EVAL {command}"));
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    assert_eq!(
        (
            host.document().entities().count(),
            host.app.tabs[0].history.undo_stack.len()
        ),
        counts,
        "refusals change nothing and record no undo step"
    );

    // W: both formats reopen the drilled result losslessly; it moves again.
    let expected_volume = volume_in(host.document(), drilled);
    let expected_extent = extent_in(host.document(), drilled);
    let dwg = crate::io::load_bytes(
        "boolean.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "boolean.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
        assert!(
            close(volume_in(document, drilled), expected_volume),
            "{format} volume"
        );
        let reopened = extent_in(document, drilled);
        for axis in 0..3 {
            assert!(
                close(reopened.0[axis], expected_extent.0[axis])
                    && close(reopened.1[axis], expected_extent.1[axis]),
                "{format} extent"
            );
        }
        let Some(EntityType::Solid3D(back)) = document.get_entity(drilled) else {
            unreachable!()
        };
        let again = host.add_entity(EntityType::Solid3D(back.clone()));
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle",
                again.value()
            ),
        );
        assert!(
            close(
                extent_in(host.document(), again).0[0],
                reopened.0[0] + 100.0
            ),
            "{format} re-edit"
        );
    }

    // D and U: delete, then undo restores it, and undoing the consuming
    // boolean brings both operands back exactly.
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.delete_entity({})",
            drilled.value()
        ),
    );
    assert!(host.document().get_entity(drilled).is_none());
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(1);
    assert!(
        app.tabs[0].scene.document.get_entity(drilled).is_some(),
        "undo restores the deleted result"
    );
    // Two re-edits, the far box, the drilling subtract and the hole box, then the consuming subtract.
    app.undo_steps(2 + 1 + 1 + 1 + 1);
    assert_eq!(
        app.tabs[0].scene.document.get_entity(a),
        Some(&before_consume.0),
        "undo restores operand A exactly"
    );
    assert_eq!(
        app.tabs[0].scene.document.get_entity(b),
        Some(&before_consume.1),
        "undo restores operand B exactly"
    );
    assert!(
        app.tabs[0].scene.document.get_entity(cut).is_none(),
        "undo removes the boolean result"
    );
}

/// Release-only evidence for docs/cadkernel-body-path.md: the kernel's
/// refusals reach the script as messages. Curved booleans take about
/// 23 s in a debug build, so this is ignored by default.
#[test]
#[ignore = "curved boolean; slow in debug builds"]
fn audit_python_boolean_kernel_refusal_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let script =
        std::env::temp_dir().join(format!("ocs_boolean_refusal_{}.py", std::process::id()));
    std::fs::write(
        &script,
        concat!(
            "s = ocs.active_document.solids\n",
            "box = s.box(center=(0, 0, 0), size=(10, 6, 4))\n",
            "sphere = s.sphere(center=(0, 0, 0), radius=3)\n",
            "s.subtract(box, sphere)\n",
        ),
    )
    .unwrap();
    dispatch(&mut host, &format!("PY_RUN {}", script.display()));
    let _ = std::fs::remove_file(&script);
    let message = host.app.command_line.history.last().unwrap().text.clone();
    assert!(
        message.contains("Coincident") && message.contains("nothing was changed"),
        "{message}"
    );
}

#[test]
fn audit_python_section_symbol_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('SectionSymbol', symbol_scale=1.0, points=[\n",
                "    {'point': P(0.0, 0.0, 0.0), 'label': 'A', 'label_offset': P(0.0, 2.0, 0.0)},\n",
                "    {'point': P(40.0, 0.0, 0.0), 'label': 'A', 'label_offset': P(0.0, 2.0, 0.0)}])\n"),
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.points = [{'point': P(0.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, 3.0, 0.0)},\n",
                "        {'point': P(20.0, 10.0, 0.0), 'bulge': 0.0}, {'point': P(40.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, -3.0, 0.0)}]\n",
                "    e.symbol_scale = 2.0\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'points':[{'point':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2 points"),
                ("'points':[{'point':{'x':float('nan'),'y':0.0,'z':0.0}},{'point':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'symbol_scale':0.0", "greater than zero"),
                ("'style_handle':999999", "not an existing SectionViewStyle"),
                ("'view_rep_handle':999999", "not an existing ViewRep"),
                ("'raw_point_count_90':9", "read-only"),
                ("'end_a':[1.0, 1.0]", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::SectionSymbol(_)),
            digest: |entity| match entity {
                EntityType::SectionSymbol(v) => format!("n{} ends {:.1},{:.1}->{:.1},{:.1} label{} tick{:.1}/{:.1} scale{:.1} counts{}/{}",
                    v.points.len(), v.end_a[0], v.end_a[1], v.end_b[0], v.end_b[1], v.label, v.tick_a, v.tick_b,
                    v.symbol_scale, v.raw_point_count_90, v.raw_point_record_count),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::SectionSymbol(v) = entity { v.symbol_scale = 3.0; },
            expect_created: "n2 ends 0.0,0.0->40.0,0.0 labelA tick2.0/2.0 scale1.0 counts2/2",
            expect_edited: "n3 ends 0.0,0.0->40.0,0.0 labelB tick3.0/-3.0 scale2.0 counts3/3",
            expect_reedited: "n3 ends 0.0,0.0->40.0,0.0 labelB tick3.0/-3.0 scale3.0 counts3/3",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
}

#[test]
fn audit_python_section_symbol_references_over_real_ipc() {
    use document::objects::{ClassObject, ClassObjectData, ObjectType};
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let object = |host: &mut HostSession<'_>, data: ClassObjectData| {
        let handle = host.document_mut().allocate_handle();
        let mut class_object = ClassObject::new(data);
        class_object.handle = handle;
        host.document_mut()
            .objects
            .insert(handle, ObjectType::ClassObject(class_object));
        handle
    };
    let style = object(
        &mut host,
        ClassObjectData::SectionViewStyle(Default::default()),
    );
    let view_rep = object(&mut host, ClassObjectData::ViewRep(Default::default()));
    let scale = host.document_mut().allocate_handle();
    let mut scale_object = document::objects::Scale::new("1:1", 1.0, 1.0);
    scale_object.handle = scale;
    host.document_mut()
        .objects
        .insert(scale, ObjectType::Scale(scale_object));
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let count = |host: &HostSession<'_>| {
        host.document()
            .entities()
            .filter(|e| matches!(e, EntityType::SectionSymbol(_)))
            .count()
    };
    let create = |extra: &str| {
        format!(
            "PY_EVAL ocs.active_document.create_entity('SectionSymbol', points=[{{'point': {{'x': 0.0, 'y': 0.0, 'z': 0.0}}}}, {{'point': {{'x': 10.0, 'y': 0.0, 'z': 0.0}}}}]{extra}).handle")
    };
    // A symbol may reference the drawing's own style and view representation.
    dispatch(
        &mut host,
        &create(&format!(
            ", style_handle={}, view_rep_handle={}",
            style.value(),
            view_rep.value()
        )),
    );
    assert_eq!(count(&host), 1, "{}", last(&host));
    let Some(EntityType::SectionSymbol(symbol)) = host
        .document()
        .entities()
        .find(|e| matches!(e, EntityType::SectionSymbol(_)))
    else {
        unreachable!()
    };
    assert_eq!(
        (symbol.style_handle, symbol.view_rep_handle),
        (style, view_rep)
    );
    // A standalone symbol needs neither.
    dispatch(&mut host, &create(""));
    assert_eq!(count(&host), 2, "{}", last(&host));
    // Wrong object kinds are refused: a style slot cannot hold a view rep, a
    // scale object is neither, and a swapped pair fails.
    for (extra, message) in [
        (
            format!(", style_handle={}", view_rep.value()),
            "not an existing SectionViewStyle",
        ),
        (
            format!(", style_handle={}", scale.value()),
            "not an existing SectionViewStyle",
        ),
        (
            format!(", view_rep_handle={}", style.value()),
            "not an existing ViewRep",
        ),
        (
            ", view_rep_handle=424242".to_string(),
            "not an existing ViewRep",
        ),
    ] {
        dispatch(&mut host, &create(&extra));
        assert_eq!(count(&host), 2, "{extra}");
        assert!(last(&host).contains(message), "{extra}: {}", last(&host));
    }
}

#[test]
fn audit_python_ole2frame_creation_rules_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let dir = std::env::temp_dir();
    let png = dir.join("ocs_ole_rules.png");
    let jpeg = dir.join("ocs_ole_rules.jpg");
    let tiff = dir.join("ocs_ole_rules.tif");
    let text = dir.join("ocs_ole_rules.txt");
    let picture = image::RgbImage::from_pixel(8, 4, image::Rgb([200, 40, 40]));
    picture.save(&png).unwrap();
    picture.save(&jpeg).unwrap();
    picture.save(&tiff).unwrap();
    std::fs::write(&text, "not a picture").unwrap();
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let frames = |host: &HostSession<'_>| {
        host.document()
            .entities()
            .filter(|e| matches!(e, EntityType::Ole2Frame(_)))
            .count()
    };
    let picture_len = |host: &HostSession<'_>, index: usize| {
        let mut all: Vec<_> = host
            .document()
            .entities()
            .filter_map(|e| match e {
                EntityType::Ole2Frame(f) => Some(f.clone()),
                _ => None,
            })
            .collect();
        all.sort_by_key(|f| f.common.handle.value());
        match document::entities::extract_presentation(&all[index].encoded_payload()) {
            Some(document::entities::OlePresentation::Raster(bytes)) => bytes,
            other => panic!("no embedded raster: {other:?}"),
        }
    };
    let embed = |file: &std::path::Path, extra: &str| {
        format!(
            "PY_EVAL ocs.active_document.embed_picture({:?}, origin=(1, 2, 3), width=16{extra}).handle", file.display().to_string())
    };

    // C: PNG and JPEG are stored as read, a TIFF is re-encoded as PNG, and
    // the frame keeps the picture's 2:1 aspect and the requested layer.
    dispatch(&mut host, &embed(&png, ", layer='PICTURES'"));
    dispatch(&mut host, &embed(&jpeg, ""));
    dispatch(&mut host, &embed(&tiff, ""));
    assert_eq!(frames(&host), 3, "{}", last(&host));
    assert_eq!(
        picture_len(&host, 0),
        std::fs::read(&png).unwrap(),
        "PNG is stored as read"
    );
    assert_eq!(
        picture_len(&host, 1),
        std::fs::read(&jpeg).unwrap(),
        "JPEG is stored as read"
    );
    assert_eq!(
        &picture_len(&host, 2)[..4],
        b"\x89PNG",
        "TIFF is re-encoded as PNG"
    );
    let mut all: Vec<_> = host
        .document()
        .entities()
        .filter_map(|e| match e {
            EntityType::Ole2Frame(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    all.sort_by_key(|f| f.common.handle.value());
    assert_eq!(
        (all[0].common.layer.as_str(), all[1].common.layer.as_str()),
        ("PICTURES", "0")
    );
    let frame = &all[0];
    assert!(
        (frame.lower_right_corner.x - frame.upper_left_corner.x - 16.0).abs() < 1e-9,
        "width 16"
    );
    assert!(
        (frame.upper_left_corner.y - frame.lower_right_corner.y - 8.0).abs() < 1e-9,
        "height 8 keeps the 2:1 aspect"
    );
    assert!(
        (frame.lower_right_corner.y - 2.0).abs() < 1e-9
            && (frame.upper_left_corner.x - 1.0).abs() < 1e-9,
        "origin is the bottom-left corner"
    );

    // V: refusals change nothing and record no undo step.
    let undo = host.app.tabs[0].history.undo_stack.len();
    let missing = dir.join("ocs_ole_rules_missing.png");
    for (command, message) in [
            (embed(&missing, ""), "cannot read the picture"),
            (embed(&text, ""), "cannot read the picture"),
            (embed(&png, ", layer=''").replace("width=16", "width=16"), "layer name is empty"),
            (embed(&png, "").replace("width=16", "width=0"), "must be finite and greater than zero"),
            (embed(&png, "").replace("origin=(1, 2, 3)", "origin=(float('nan'), 0, 0)"), "must be finite"),
            ("ocs.embed_picture('x.png', [1.0], 5.0, None)".to_string(), "origin needs 3 numbers"),
            ("ocs.active_document.create_entity('Ole2Frame', upper_left_corner=(0, 0, 0), lower_right_corner=(1, 1, 0))".to_string(), "doc.embed_picture"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {}", command.trim_start_matches("PY_EVAL ")));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
    assert_eq!(frames(&host), 3);
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        undo,
        "refusals record no undo step"
    );
    for file in [&png, &jpeg, &tiff, &text] {
        let _ = std::fs::remove_file(file);
    }
}

#[test]
fn audit_python_viewport_ids_and_scale_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let paper = host
        .document()
        .block_records
        .iter()
        .find(|r| r.is_paper_space())
        .map(|r| r.handle)
        .unwrap();
    let layout = host
        .document()
        .objects
        .values()
        .find_map(|o| match o {
            document::objects::ObjectType::Layout(l) if l.block_record == paper => {
                Some(l.name.clone())
            }
            _ => None,
        })
        .unwrap();
    host.app.tabs[0].scene.set_current_layout(layout);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(process
            .dispatch(host, command, &mut |_| {})
            .expect("Python dispatch"));
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let viewports = |host: &HostSession<'_>| {
        let mut list: Vec<_> = host
            .document()
            .entities()
            .filter_map(|e| match e {
                EntityType::Viewport(v) => Some((v.common.handle, v.id)),
                _ => None,
            })
            .collect();
        list.sort_by_key(|(h, _)| h.value());
        list
    };
    let create = |width: f64, height: f64, view_height: f64| {
        format!(
            "PY_EVAL ocs.active_document.create_entity('Viewport', owner_handle={}, center=(50, 50, 0), width={width}, height={height}, view_height={view_height}).handle", paper.value())
    };
    dispatch(&mut host, &create(80.0, 60.0, 50.0));
    dispatch(&mut host, &create(40.0, 30.0, 10.0));
    dispatch(&mut host, &create(20.0, 20.0, 20.0));
    let list = viewports(&host);
    assert_eq!(list.len(), 3, "{}", last(&host));
    let ids: Vec<i16> = list.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        ids,
        vec![2, 3, 4],
        "scripted viewports get unique ids of at least 2: {ids:?}"
    );
    for (handle, expected) in [
        (list[0].0, 60.0 / 50.0),
        (list[1].0, 30.0 / 10.0),
        (list[2].0, 1.0),
    ] {
        let Some(EntityType::Viewport(v)) = host.document().get_entity(handle) else {
            unreachable!()
        };
        assert!(
            (v.custom_scale - expected).abs() < 1e-12,
            "custom_scale {} vs {expected}",
            v.custom_scale
        );
        assert!(
            !host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(),
            "viewport {handle:?} draws a frame"
        );
    }
    // The scale follows the geometry; it cannot be set directly.
    let second = list[0].0;
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Zoom', [{{'handle':{}, 'view_height':25.0}}])",
            second.value()
        ),
    );
    let Some(EntityType::Viewport(zoomed)) = host.document().get_entity(second) else {
        unreachable!()
    };
    assert!(
        (zoomed.custom_scale - 60.0 / 25.0).abs() < 1e-12,
        "{}",
        last(&host)
    );
    assert_eq!(zoomed.id, 2, "an edit keeps the id");
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Resize', [{{'handle':{}, 'height':90.0}}])",
            second.value()
        ),
    );
    let Some(EntityType::Viewport(resized)) = host.document().get_entity(second) else {
        unreachable!()
    };
    assert!((resized.custom_scale - 90.0 / 25.0).abs() < 1e-12);
    let before = host.document().get_entity(second).unwrap().clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Scale', [{{'handle':{}, 'custom_scale':3.0}}])",
            second.value()
        ),
    );
    assert!(last(&host).contains("read-only"), "{}", last(&host));
    assert_eq!(host.document().get_entity(second), Some(&before));
    // Control: the engine's default id 0 is not invisible here (there is no
    // sheet viewport to confuse it with), so the derived id matters for
    // uniqueness, not for visibility.
    let mut bare = document::entities::Viewport::new();
    bare.common.owner_handle = paper;
    bare.width = 40.0;
    bare.height = 30.0;
    let bare = host
        .document_mut()
        .add_entity(EntityType::Viewport(Box::new(bare)))
        .unwrap();
    assert_eq!(
        host.document().get_entity(bare).map(|e| match e {
            EntityType::Viewport(v) => v.id,
            _ => -1,
        }),
        Some(0)
    );
    assert!(!host.app.tabs[0].scene.wire_models_for(&[bare]).is_empty());
}

/// Evidence for the RasterImage ledger row: what a save does with a bare
/// image (OCS's native IMAGE command) and with a definition-linked one.
#[test]
#[ignore = "exploratory evidence"]
fn spike_raster_image_definition_persistence() {
    let dir = std::env::temp_dir();
    let path = dir
        .join("ocs_spike_image.png")
        .to_string_lossy()
        .into_owned();
    image::RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255]))
        .save(&path)
        .unwrap();
    for linked in [false, true] {
        let mut doc = document::Document::new();
        let mut img = document::entities::RasterImage::with_size(
            &path,
            document::types::Vector3::ZERO,
            8.0,
            4.0,
            16.0,
            8.0,
        );
        if linked {
            let h = doc.allocate_handle();
            let mut def = document::objects::ImageDefinition::with_dimensions(path.clone(), 8, 4);
            def.handle = h;
            def.is_loaded = true;
            doc.objects
                .insert(h, document::objects::ObjectType::ImageDefinition(def));
            img.definition_handle = Some(h);
        }
        let handle = doc.add_entity(EntityType::RasterImage(img)).unwrap();
        for (label, document) in [
            (
                "DWG",
                crate::io::load_bytes("i.dwg", document::DwgWriter::write_to_vec(&doc).unwrap())
                    .unwrap(),
            ),
            (
                "DXF",
                crate::io::load_bytes(
                    "i.dxf",
                    document::DxfWriter::new(&doc).write_to_vec().unwrap(),
                )
                .unwrap(),
            ),
        ] {
            let entity = document.get_entity(handle);
            let defs = document
                .objects
                .values()
                .filter(|o| matches!(o, document::objects::ObjectType::ImageDefinition(_)))
                .count();
            let reactors = document
                .objects
                .values()
                .filter(|o| matches!(o, document::objects::ObjectType::ImageDefinitionReactor(_)))
                .count();
            let dicts = document.objects.values().filter(|o| matches!(o, document::objects::ObjectType::Dictionary(d) if d.entries.iter().any(|(k, _)| k.contains("IMAGE")))).count();
            match entity {
                    Some(EntityType::RasterImage(i)) => eprintln!("SPIKE linked={linked} {label}: file_path_ok={} definition={:?} reactor={:?} defs={defs} reactors={reactors} image_dicts={dicts}",
                        i.file_path == path, i.definition_handle.map(|h| h.value()), i.definition_reactor_handle.map(|h| h.value())),
                    other => eprintln!("SPIKE linked={linked} {label}: came back as {:?}", other.map(|e| format!("{e:?}").chars().take(24).collect::<String>())),
                }
        }
    }
}

/// Layer table through Python over the real runner: create, modify, rename,
/// delete and make current, with refusals that leave the drawing untouched,
/// one undo step per change, and DWG/DXF persistence of every property.
#[test]
fn audit_python_layer_table_over_real_ipc() {
    use document::types::{Color, LineWeight};
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_layers_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + ' '.join(refused) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (Vec<String>, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
        let words = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        (words(refused), words(accepted))
    };

    // Build: two layers with full properties, a modify, an entity, a rename, and the refusals.
    run(
        &mut host,
        "build",
        r#"
L.create('Walls', color=1, lineweight=50, description='load bearing', transparency=30)
L.create('Grid', color=(10, 200, 30), linetype='Continuous', off=True, plottable=False)
doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), layer='Walls')
L.modify('Walls', color=5, locked=True)
L.rename('Walls', 'Structure')
check('dup', lambda: L.create('structure'))
check('badname', lambda: L.create('a<b'))
check('empty', lambda: L.create('  '))
check('badcolor0', lambda: L.create('C1', color=0))
check('badcolorlayer', lambda: L.create('C1', color={'kind': 'ByLayer'}))
check('badltype', lambda: L.create('C2', linetype='Nope'))
check('badweight', lambda: L.create('C3', lineweight=999))
check('badtransp', lambda: L.create('C4', transparency=95))
check('unknownkey', lambda: L.create('C5', colour=1))
check('rename0', lambda: L.rename('0', 'X'))
check('renameclash', lambda: L.rename('Grid', 'STRUCTURE'))
check('renamemissing', lambda: L.rename('Nope', 'X2'))
check('del0', lambda: L.delete('0'))
check('delcurrent', lambda: L.delete(L.current['name']))
check('delused', lambda: L.delete('Structure'))
check('freezecurrent', lambda: L.modify(L.current['name'], frozen=True))
check('modnone', lambda: L.modify('Grid'))
check('modmissing', lambda: L.modify('Nope', off=True))
check('currentmissing', lambda: L.set_current('Nope'))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused.len(), 19, "{refused:?}");

    let check_state = |label: &str, document: &Document| {
        let names: Vec<_> = document.layers.iter().map(|l| l.name.as_str()).collect();
        assert!(
            !names.iter().any(|n| n.eq_ignore_ascii_case("Walls")),
            "{label}: old name gone: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with('C') && n.len() == 2),
            "{label}: a refused request left a layer: {names:?}"
        );
        let structure = document
            .layers
            .get("Structure")
            .unwrap_or_else(|| panic!("{label}: Structure missing"));
        assert_eq!(structure.color, Color::Index(5), "{label}");
        assert_eq!(structure.line_weight, LineWeight::Value(50), "{label}");
        assert!(structure.flags.locked, "{label}");
        assert_eq!(structure.description, "load bearing", "{label}");
        assert_ne!(
            structure.transparency,
            document::types::Transparency::ByLayer,
            "{label}: transparency"
        );
        let grid = document
            .layers
            .get("Grid")
            .unwrap_or_else(|| panic!("{label}: Grid missing"));
        assert_eq!(
            grid.color,
            Color::Rgb {
                r: 10,
                g: 200,
                b: 30
            },
            "{label}"
        );
        assert!(grid.flags.off && !grid.is_plottable, "{label}");
        assert_eq!(grid.line_type.to_uppercase(), "CONTINUOUS", "{label}");
        let line = document
            .entities()
            .find_map(|e| match e {
                EntityType::Line(l) => Some(l.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label}: line missing"));
        assert_eq!(
            line.common.layer, "Structure",
            "{label}: entity followed the rename"
        );
    };
    check_state("live", host.document());
    let dwg = crate::io::load_bytes(
        "layers.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "layers.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check_state("DWG", &dwg);
    check_state("DXF", &dxf);

    // Destroy: current layer, delete with and without objects.
    run(
        &mut host,
        "destroy",
        r#"
L.set_current('Grid')
check('delcurrent2', lambda: L.delete('Grid'))
check('freezecurrent2', lambda: L.modify('Grid', frozen=True))
L.set_current('0')
L.delete('Grid')
check('delused2', lambda: L.delete('Structure'))
L.delete('Structure', erase_objects=True)
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(accepted.is_empty(), "{accepted:?}");
    assert_eq!(refused, vec!["delcurrent2", "freezecurrent2", "delused2"]);
    let document = host.document();
    assert!(document.layers.get("Grid").is_none() && document.layers.get("Structure").is_none());
    assert_eq!(document.header.current_layer_name, "0");
    assert!(
        !document
            .entities()
            .any(|e| matches!(e, EntityType::Line(_))),
        "erase_objects erased the line"
    );

    // U: the last two steps are the report layer and the erasing delete;
    // undoing them restores the deleted layer and its object together.
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    let before = app.tabs[0].history.undo_stack.len();
    app.undo_steps(2);
    assert_eq!(app.tabs[0].history.undo_stack.len(), before - 2);
    let document = &app.tabs[0].scene.document;
    assert!(
        document.layers.get("Structure").is_some(),
        "undo restored the layer"
    );
    assert!(
        document
            .entities()
            .any(|e| matches!(e, EntityType::Line(_))),
        "undo restored the line"
    );
}

/// Text and dimension styles through Python over the real runner: create,
/// modify, copy, rename (references follow), delete and make current, with
/// refusals that change nothing, DWG/DXF persistence and one-step undo.
#[test]
fn audit_python_text_and_dim_styles_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_styles_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "doc = ocs.active_document\nS = doc.text_styles\nD = doc.dim_styles\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (usize, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
        (
            refused.trim().parse().unwrap(),
            accepted.split_whitespace().map(str::to_owned).collect(),
        )
    };

    run(
        &mut host,
        "build",
        r#"
S.create('Title', height=5, width_factor=0.8, oblique=15, font='romans', big_font='bigfont', backward=True, annotative=True)
S.create('Notes', font='arial.ttf')
S.modify('Title', height=6, upside_down=True)
D.create('Metric', dimscale=2, dimtxt=3.5, dimasz=2.5, dimtxsty='Title')
D.create('Metric2', copy_from='Metric', dimtxt=4)
D.modify('Metric', dimscale=3)
S.rename('Notes', 'Remarks')
S.rename('Title', 'Heading')
check('t_dup', lambda: S.create('heading'))
check('t_badname', lambda: S.create('a|b'))
check('t_height', lambda: S.create('T1', height=-1))
check('t_width0', lambda: S.create('T2', width_factor=0))
check('t_oblique', lambda: S.create('T3', oblique=89))
check('t_nofont', lambda: S.create('T4', font=''))
check('t_unknown', lambda: S.create('T5', fnt='x'))
check('t_modmissing', lambda: S.modify('Nope', height=1))
check('t_modnone', lambda: S.modify('Heading'))
check('t_renameStandard', lambda: S.rename('Standard', 'Std2'))
check('t_rename_case', lambda: S.rename('Heading', 'HEADING'))
check('t_rename_clash', lambda: S.rename('Remarks', 'heading'))
check('t_delStandard', lambda: S.delete('Standard'))
check('t_delInUse', lambda: S.delete('Heading'))
check('t_current_missing', lambda: S.set_current('Nope'))
check('d_dup', lambda: D.create('metric'))
check('d_unknown', lambda: D.create('D1', dimbogus=1))
check('d_handle', lambda: D.create('D2', dimtxsty_handle=5))
check('d_name', lambda: D.modify('Metric', name='X'))
check('d_type', lambda: D.create('D3', dimscale='big'))
check('d_scale0', lambda: D.create('D4', dimscale=0))
check('d_txt', lambda: D.create('D5', dimtxt=-1))
check('d_txsty', lambda: D.create('D6', dimtxsty='Nope'))
check('d_copy_missing', lambda: D.create('D7', copy_from='Nope'))
check('d_delStandard', lambda: D.delete('Standard'))
check('d_modmissing', lambda: D.modify('Nope', dimscale=2))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused, 26);

    let check_state = |label: &str, document: &Document| {
        assert!(
            document.text_styles.get("Title").is_none()
                && document.text_styles.get("Notes").is_none(),
            "{label}: old names gone"
        );
        let title = document
            .text_styles
            .get("Heading")
            .unwrap_or_else(|| panic!("{label}: Heading missing"));
        assert_eq!(title.height, 6.0, "{label}");
        assert!((title.width_factor - 0.8).abs() < 1e-9, "{label}");
        assert!(
            (title.oblique_angle - 15f64.to_radians()).abs() < 1e-6,
            "{label}: oblique {}",
            title.oblique_angle
        );
        assert_eq!(title.font_file.to_lowercase(), "romans", "{label}");
        assert_eq!(title.big_font_file.to_lowercase(), "bigfont", "{label}");
        assert!(title.flags.backward && title.flags.upside_down, "{label}");
        let remarks = document
            .text_styles
            .get("Remarks")
            .unwrap_or_else(|| panic!("{label}: Remarks missing"));
        assert_eq!(
            remarks.font_file.to_lowercase(),
            "arial.ttf",
            "{label}: {remarks:?}"
        );
        let metric = document
            .dim_styles
            .get("Metric")
            .unwrap_or_else(|| panic!("{label}: Metric missing"));
        assert_eq!(
            (metric.dimscale, metric.dimtxt, metric.dimasz),
            (3.0, 3.5, 2.5),
            "{label}"
        );
        assert_eq!(
            metric.dimtxsty, "Heading",
            "{label}: dimtxsty followed the rename"
        );
        if label == "DXF" {
            assert_eq!(
                metric.dimtxsty_handle, title.handle,
                "{label}: text style link by handle"
            );
        }
        let copy = document
            .dim_styles
            .get("Metric2")
            .unwrap_or_else(|| panic!("{label}: Metric2 missing"));
        assert_eq!(
            (copy.dimscale, copy.dimtxt),
            (2.0, 4.0),
            "{label}: copied before Metric changed"
        );
    };
    check_state("live", host.document());
    let live_title = host.document().text_styles.get("Heading").unwrap().clone();
    assert!(
        live_title.annotative,
        "annotative is kept in the live style"
    );
    assert_eq!(
        host.document()
            .dim_styles
            .get("Metric")
            .unwrap()
            .dimtxsty_handle,
        live_title.handle,
        "dim style links the text style handle"
    );
    let dwg = crate::io::load_bytes(
        "styles.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "styles.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check_state("DWG", &dwg);
    check_state("DXF", &dxf);

    run(
        &mut host,
        "destroy",
        r#"
S.set_current('Remarks')
D.set_current('Metric2')
check('t_delcurrent', lambda: S.delete('Remarks'))
check('d_delcurrent', lambda: D.delete('Metric2'))
check('d_delInUseByCurrentOnly', lambda: D.delete('Metric2'))
S.set_current('Standard')
D.set_current('Standard')
S.delete('Remarks')
D.delete('Metric2')
D.delete('Metric')
S.delete('Heading')
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(accepted.is_empty(), "{accepted:?}");
    assert_eq!(refused, 3);
    let document = host.document();
    for name in ["Heading", "Remarks"] {
        assert!(document.text_styles.get(name).is_none(), "{name} deleted");
    }
    for name in ["Metric", "Metric2"] {
        assert!(document.dim_styles.get(name).is_none(), "{name} deleted");
    }
    assert_eq!(document.header.current_text_style_name, "Standard");
    assert_eq!(document.header.current_dimstyle_name, "Standard");

    // U: the final report layer and the four deletes are five steps; undoing
    // them all brings the styles back.
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(5);
    let document = &app.tabs[0].scene.document;
    assert!(
        document.text_styles.get("Heading").is_some()
            && document.text_styles.get("Remarks").is_some(),
        "undo restored the text styles"
    );
    assert!(
        document.dim_styles.get("Metric").is_some() && document.dim_styles.get("Metric2").is_some(),
        "undo restored the dimension styles"
    );
}

/// Block definitions through Python over the real runner: create from
/// entities (with and without erasing them), modify, rename with inserts
/// following, guarded delete, DWG/DXF persistence and undo.
#[test]
fn audit_python_blocks_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_blocks_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nB = doc.blocks\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (usize, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
        (
            refused.trim().parse().unwrap(),
            accepted.split_whitespace().map(str::to_owned).collect(),
        )
    };

    run(
        &mut host,
        "build",
        r#"
line = doc.create_entity('Line', start=P(0, 0, 0), end=P(4, 0, 0))
circle = doc.create_entity('Circle', center=P(2, 2, 0), radius=1)
B.create('Widget', [line, circle], base_point=(1, 1, 0), description='a widget')
B.create('Widget2', [line.handle], erase_originals=True)
doc.create_entity('Insert', block_name='Widget', insert_point=P(10, 0, 0))
doc.create_entity('Insert', block_name='widget', insert_point=P(20, 0, 0))
B.modify('Widget', explodable=False, description='a gadget')
B.rename('Widget', 'Gadget')
member = B['Gadget']['entities'][0]['handle']
check('dup', lambda: B.create('gadget', [circle]))
check('empty', lambda: B.create('E1', []))
check('badname', lambda: B.create('a<b', [circle]))
check('star', lambda: B.create('*U9', [circle]))
check('missing_entity', lambda: B.create('E2', [999999]))
check('twice', lambda: B.create('E3', [circle, circle]))
check('nonfinite_base', lambda: B.create('E4', [circle], base_point=(1e999, 0, 0)))
check('in_block', lambda: B.create('E5', [member]))
check('unknown_key', lambda: ocs.block_operation('create', 'E6', {'entities': [circle.handle], 'origin': [0, 0, 0]}))
check('rename_missing', lambda: B.rename('Nope', 'X'))
check('rename_clash', lambda: B.rename('Gadget', 'WIDGET2'))
check('rename_case', lambda: B.rename('Gadget', 'GADGET'))
check('rename_star', lambda: B.rename('Gadget', '*Y'))
check('delete_missing', lambda: B.delete('Nope'))
check('delete_inserted', lambda: B.delete('Gadget'))
check('delete_layout', lambda: B.delete('*Model_Space'))
check('modify_none', lambda: B.modify('Gadget'))
check('modify_missing', lambda: B.modify('Nope', description='x'))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused, 18);

    let check_state = |label: &str, document: &Document| {
        assert!(
            document.block_records.get("Widget").is_none(),
            "{label}: old block name gone"
        );
        let members = |name: &str| -> Vec<EntityType> {
            let record = document
                .block_records
                .get(name)
                .unwrap_or_else(|| panic!("{label}: block {name} missing"));
            document
                .entities()
                .filter(|e| {
                    e.common().owner_handle == record.handle
                        && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
                })
                .cloned()
                .collect()
        };
        let gadget = members("Gadget");
        assert_eq!(gadget.len(), 2, "{label}: Gadget members");
        let line = gadget
            .iter()
            .find_map(|e| match e {
                EntityType::Line(l) => Some(l.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label}: line"));
        assert_eq!(
            (line.start.x, line.start.y, line.end.x, line.end.y),
            (-1.0, -1.0, 3.0, -1.0),
            "{label}: shifted by -base"
        );
        let circle = gadget
            .iter()
            .find_map(|e| match e {
                EntityType::Circle(c) => Some(c.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label}: circle"));
        assert_eq!(
            (circle.center.x, circle.center.y, circle.radius),
            (1.0, 1.0, 1.0),
            "{label}"
        );
        let record = document.block_records.get("Gadget").unwrap();
        assert_eq!(
            (record.base_point.x, record.base_point.y),
            (1.0, 1.0),
            "{label}: base point"
        );
        assert!(!record.explodable, "{label}: explodable");
        assert_eq!(record.description, "a gadget", "{label}: description");
        assert_eq!(members("Widget2").len(), 1, "{label}: Widget2 members");
        let inserts: Vec<_> = document
            .entities()
            .filter_map(|e| match e {
                EntityType::Insert(i) => Some(i.block_name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            inserts.iter().filter(|n| n.as_str() == "Gadget").count(),
            2,
            "{label}: inserts follow the rename: {inserts:?}"
        );
        // originals: Widget kept the circle and line in model space, Widget2 erased its line copy's source
        let canvas_kinds: Vec<_> = document
            .entities()
            .filter(|e| {
                document
                    .block_records
                    .iter()
                    .any(|r| r.handle == e.common().owner_handle && r.is_layout())
            })
            .map(|e| format!("{:?}", e).chars().take(6).collect::<String>())
            .collect();
        assert_eq!(
            canvas_kinds
                .iter()
                .filter(|k| k.starts_with("Circle"))
                .count(),
            1,
            "{label}: circle original kept: {canvas_kinds:?}"
        );
        assert_eq!(
            canvas_kinds
                .iter()
                .filter(|k| k.starts_with("Line("))
                .count(),
            0,
            "{label}: line original erased: {canvas_kinds:?}"
        );
    };
    check_state("live", host.document());
    assert_eq!(
        host.document()
            .block_records
            .get("Gadget")
            .unwrap()
            .description,
        "a gadget"
    );
    let dwg = crate::io::load_bytes(
        "blocks.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "blocks.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check_state("DWG", &dwg);
    check_state("DXF", &dxf);

    run(
        &mut host,
        "destroy",
        r#"
for entity in list(doc.entities):
    if entity.kind == 'Insert':
        doc.delete_entity(entity)
B.delete('Gadget')
B.delete('Widget2')
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(accepted.is_empty(), "{accepted:?}");
    assert_eq!(refused, 0);
    let document = host.document();
    assert!(
        document.block_records.get("Gadget").is_none()
            && document.block_records.get("Widget2").is_none()
    );
    assert!(
        !document
            .entities()
            .any(|e| matches!(e, EntityType::Line(_))),
        "block members were removed with their definitions"
    );
    assert_eq!(
        document
            .entities()
            .filter(|e| matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)))
            .count(),
        0
    );

    // U: the report layer and both block deletes; undoing them restores the definitions.
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(3);
    let document = &app.tabs[0].scene.document;
    let members = |name: &str| {
        document
            .block_records
            .get(name)
            .map(|r| r.entity_handles.len())
    };
    assert_eq!(
        members("Gadget"),
        Some(2),
        "undo restored Gadget with its members"
    );
    assert_eq!(members("Widget2"), Some(1), "undo restored Widget2");
}

/// Block contents through Python over the real runner: add entities
/// (including an attribute definition) to a definition, edit and delete them
/// with the ordinary entity API, refuse bad requests, persist through
/// DWG/DXF and undo.
#[test]
fn audit_python_block_contents_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_bcontents_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nB = doc.blocks\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (usize, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
        (
            refused.trim().parse().unwrap(),
            accepted.split_whitespace().map(str::to_owned).collect(),
        )
    };

    run(
        &mut host,
        "build",
        r#"
seed = doc.create_entity('Circle', center=P(0, 0, 0), radius=1)
B.create('Part', [seed], erase_originals=True)
line = doc.create_entity('Line', start=P(0, 0, 0), end=P(5, 0, 0), block='Part')
arc = doc.create_entity('Arc', center=P(0, 0, 0), radius=5.0, start_angle=0.0, end_angle=1.5, block='Part')
tag = doc.create_entity('AttributeDefinition', block='Part', tag='PART_NO', prompt='Part number', default_value='PN-001', insertion_point=P(1, 2, 0), height=2.5)
with doc.transaction('Lengthen'):
    line.end = (8, 0, 0)
doc.delete_entity(arc)
B.create('Outer', [doc.create_entity('Circle', center=P(0, 0, 0), radius=1)], erase_originals=True)
doc.create_entity('Insert', block_name='Part', insert_point=P(0, 0, 0), block='Outer')
check('missing_block', lambda: doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), block='Nope'))
check('layout_block', lambda: doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), block='*Model_Space'))
check('zero_circle', lambda: doc.create_entity('Circle', center=P(1, 1, 0), radius=0, block='Part'))
check('viewport', lambda: doc.create_entity('Viewport', block='Part'))
check('bad_tag', lambda: doc.create_entity('AttributeDefinition', block='Part', tag='  ', prompt='p', default_value='', insertion_point=P(0, 0, 0), height=1))
check('bad_height', lambda: doc.create_entity('AttributeDefinition', block='Part', tag='T', prompt='p', default_value='', insertion_point=P(0, 0, 0), height=0))
check('self_insert', lambda: doc.create_entity('Insert', block_name='Part', insert_point=P(0, 0, 0), block='Part'))
check('cycle', lambda: doc.create_entity('Insert', block_name='Outer', insert_point=P(0, 0, 0), block='Part'))
check('unknown_insert_block', lambda: doc.create_entity('Insert', block_name='Ghost', insert_point=P(0, 0, 0), block='Part'))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused, 9);

    let check_state = |label: &str, document: &Document| {
        let record = document
            .block_records
            .get("Part")
            .unwrap_or_else(|| panic!("{label}: Part missing"));
        let members: Vec<EntityType> = document
            .entities()
            .filter(|e| {
                e.common().owner_handle == record.handle
                    && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
            })
            .cloned()
            .collect();
        assert_eq!(
            members.len(),
            3,
            "{label}: circle, line, attribute definition (the arc was deleted): {:?}",
            members
                .iter()
                .map(|e| format!("{e:?}").chars().take(10).collect::<String>())
                .collect::<Vec<_>>()
        );
        let line = members
            .iter()
            .find_map(|e| match e {
                EntityType::Line(l) => Some(l.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label}: line"));
        assert_eq!(
            (line.end.x, line.end.y),
            (8.0, 0.0),
            "{label}: edited inside the block"
        );
        assert!(
            members.iter().any(|e| matches!(e, EntityType::Circle(_))),
            "{label}: circle from the seed"
        );
        let attdef = members
            .iter()
            .find_map(|e| match e {
                EntityType::AttributeDefinition(a) => Some(a.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label}: attdef"));
        assert_eq!(
            (
                attdef.tag.as_str(),
                attdef.prompt.as_str(),
                attdef.default_value.as_str()
            ),
            ("PART_NO", "Part number", "PN-001"),
            "{label}"
        );
        assert!(
            !members.iter().any(|e| matches!(e, EntityType::Arc(_))),
            "{label}: arc deleted"
        );
        let outer = document
            .block_records
            .get("Outer")
            .unwrap_or_else(|| panic!("{label}: Outer missing"));
        let nested = document.entities().any(|e| matches!(e, EntityType::Insert(i) if i.block_name == "Part" && i.common.owner_handle == outer.handle));
        assert!(nested, "{label}: Part is inserted inside Outer");
    };
    check_state("live", host.document());
    let dwg = crate::io::load_bytes(
        "bc.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "bc.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check_state("DWG", &dwg);
    check_state("DXF", &dxf);

    // U: one more add, then the report layer; undoing both leaves the block as before.
    run(
        &mut host,
        "extra",
        "doc.create_entity('Circle', center=P(9, 9, 0), radius=2, block='Part')",
    );
    assert_eq!(report(host.document()).1.len(), 0);
    {
        let document = host.document();
        let record = document.block_records.get("Part").unwrap();
        let live: Vec<_> = record
            .entity_handles
            .iter()
            .map(|h| (h.value(), document.get_entity(*h).is_some()))
            .collect();
        assert_eq!(
            record.entity_handles.len(),
            4,
            "record entity_handles (handle, exists): {live:?}"
        );
    }
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(2);
    // Membership bookkeeping is intrinsic to an add and stays listed across an entity-delta
    // undo (see `Scene::add_entity_internal`), so count the entities that really exist.
    let document = &app.tabs[0].scene.document;
    let part = document.block_records.get("Part").unwrap();
    let existing = part
        .entity_handles
        .iter()
        .filter(|h| document.get_entity(**h).is_some())
        .count();
    assert_eq!(existing, 3, "undo removed the added member");
}

/// Linetypes and layouts through Python over the real runner: create,
/// modify, rename (layers follow), guarded delete, page setup, entities
/// placed on a layout, DWG/DXF persistence and undo.
#[test]
fn audit_python_linetypes_and_layouts_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_ltlay_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nLT = doc.linetypes\nLY = doc.layouts\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (usize, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
        (
            refused.trim().parse().unwrap(),
            accepted.split_whitespace().map(str::to_owned).collect(),
        )
    };

    run(
        &mut host,
        "build",
        r#"
LT.create('Bracket', [12, -3, 2, -3], 'Phantom test')
LT.create('Dots', [0, -2], 'dots')
LT.modify('Dots', pattern=[0, -4], description='sparse dots')
LT.create('Temp', [1, -1])
L.create('Hidden2', linetype='Bracket')
L.create('Dotted2', linetype='Dots')
LT.rename('Dots', 'Sparse')
LY.create('Sheet1')
LY.create('Sheet2')
LY.set_page('Sheet1', paper_size=(420, 297), rotation=0, scale=(1, 50))
LY.rename('Sheet2', 'Details')
LY.set_current('Sheet1')
doc.create_entity('Circle', center=P(50, 50, 0), radius=5)
LY.set_current('Model')
check('lt_dup', lambda: LT.create('bracket', [1, -1]))
check('lt_standard_dup', lambda: LT.create('Dashed', [1, -1]))
check('lt_badname', lambda: LT.create('a|b', [1, -1]))
check('lt_empty', lambda: LT.create('X1', []))
check('lt_single', lambda: LT.create('X2', [1]))
check('lt_nogap', lambda: LT.create('X3', [1, 2]))
check('lt_nodash', lambda: LT.create('X4', [-1, -2]))
check('lt_nonfinite', lambda: LT.create('X5', [1e999, -1]))
check('lt_toomany', lambda: LT.create('X6', [1, -1] * 7))
check('lt_modnone', lambda: LT.modify('Bracket'))
check('lt_modmissing', lambda: LT.modify('Nope', description='x'))
check('lt_modcontinuous', lambda: LT.modify('Continuous', description='x'))
check('lt_renamecontinuous', lambda: LT.rename('Continuous', 'Solid2'))
check('lt_renameclash', lambda: LT.rename('Temp', 'BRACKET'))
check('lt_renamecase', lambda: LT.rename('Temp', 'TEMP'))
check('lt_delcontinuous', lambda: LT.delete('ByLayer'))
check('lt_delinuse', lambda: LT.delete('Bracket'))
check('lt_delmissing', lambda: LT.delete('Nope'))
check('ly_dup', lambda: LY.create('sheet1'))
check('ly_model', lambda: LY.create('Model'))
check('ly_badname', lambda: LY.create('a<b'))
check('ly_renamemodel', lambda: LY.rename('Model', 'X'))
check('ly_renamemissing', lambda: LY.rename('Nope', 'X'))
check('ly_renameclash', lambda: LY.rename('Details', 'SHEET1'))
check('ly_renamecase', lambda: LY.rename('Details', 'DETAILS'))
check('ly_deletemodel', lambda: LY.delete('Model'))
check('ly_deletemissing', lambda: LY.delete('Nope'))
check('ly_currentmissing', lambda: LY.set_current('Nope'))
check('ly_pagemissing', lambda: LY.set_page('Nope', rotation=0))
check('ly_pagemodel', lambda: LY.set_page('Model', rotation=0))
check('ly_pagenone', lambda: LY.set_page('Sheet1'))
check('ly_badsize', lambda: LY.set_page('Sheet1', paper_size=(0, 10)))
check('ly_badrotation', lambda: LY.set_page('Sheet1', rotation=45))
check('ly_badscale', lambda: LY.set_page('Sheet1', scale=(0, 1)))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused, 34);

    let check_state = |label: &str, document: &Document| {
        assert!(
            document.line_types.get("Dots").is_none(),
            "{label}: old linetype name gone"
        );
        let phantom = document
            .line_types
            .get("Bracket")
            .unwrap_or_else(|| panic!("{label}: Bracket missing"));
        let lengths: Vec<f64> = phantom.elements.iter().map(|e| e.length).collect();
        assert_eq!(lengths, vec![12.0, -3.0, 2.0, -3.0], "{label}: pattern");
        assert_eq!(phantom.pattern_length, 20.0, "{label}");
        let sparse = document
            .line_types
            .get("Sparse")
            .unwrap_or_else(|| panic!("{label}: Sparse missing"));
        assert_eq!(
            sparse.elements.iter().map(|e| e.length).collect::<Vec<_>>(),
            vec![0.0, -4.0],
            "{label}: modified pattern"
        );
        assert_eq!(
            document.layers.get("Dotted2").unwrap().line_type,
            "Sparse",
            "{label}: layer followed the rename"
        );
        assert_eq!(
            document.layers.get("Hidden2").unwrap().line_type,
            "Bracket",
            "{label}"
        );
        let layouts = |name: &str| {
            document.objects.values().find_map(|o| match o {
                document::objects::ObjectType::Layout(l)
                    if l.name == name && !l.block_record.is_null() =>
                {
                    Some(l.clone())
                }
                _ => None,
            })
        };
        assert!(layouts("Sheet2").is_none(), "{label}: old layout name gone");
        let sheet = layouts("Sheet1").unwrap_or_else(|| panic!("{label}: Sheet1 missing"));
        assert_eq!(
            (sheet.paper_width, sheet.paper_height),
            (420.0, 297.0),
            "{label}: paper size"
        );
        assert_eq!(sheet.plot_rotation, 0, "{label}: rotation");
        assert_eq!(
            (sheet.plot_scale_numerator, sheet.plot_scale_denominator),
            (1.0, 50.0),
            "{label}: scale"
        );
        assert!(layouts("Details").is_some(), "{label}: Details missing");
        let on_sheet = document
            .entities()
            .filter(|e| {
                e.common().owner_handle == sheet.block_record && matches!(e, EntityType::Circle(_))
            })
            .count();
        assert_eq!(on_sheet, 1, "{label}: the circle was created on the sheet");
        assert!(
            !document
                .entities()
                .any(|e| matches!(e, EntityType::Circle(_))
                    && document
                        .block_records
                        .iter()
                        .any(|r| r.name == "*Model_Space" && r.handle == e.common().owner_handle)),
            "{label}: no circle in model space"
        );
    };
    check_state("live", host.document());
    assert_eq!(
        host.app.tabs[0].scene.current_layout, "Model",
        "the script switched back to Model"
    );
    let dwg = crate::io::load_bytes(
        "ltl.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "ltl.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check_state("DWG", &dwg);
    check_state("DXF", &dxf);

    run(
        &mut host,
        "order",
        r#"
LY.create('Sheet3')
order = LY.set_tab_order(['Sheet3', 'details', 'layout1', ' Sheet1 '])
if order != ['Model', 'Sheet3', 'Details', 'Layout1', 'Sheet1']:
    accepted.append('BADORDER_' + '_'.join(order))
check('to_missing', lambda: LY.set_tab_order(['Sheet3', 'Details', 'Layout1']))
check('to_unknown', lambda: LY.set_tab_order(['Sheet3', 'Details', 'Layout1', 'Sheet1', 'Nope']))
check('to_twice', lambda: LY.set_tab_order(['Sheet3', 'Details', 'Layout1', 'Sheet1', 'sheet1']))
check('to_model', lambda: LY.set_tab_order(['Model', 'Sheet3', 'Details', 'Layout1', 'Sheet1']))
check('to_string', lambda: LY.set_tab_order('Sheet1'))
check('to_nonames', lambda: ocs.layout_operation('set_tab_order', '', None))
check('to_badkey', lambda: ocs.layout_operation('set_tab_order', '', {'names': ['Sheet3', 'Details', 'Layout1', 'Sheet1'], 'x': 1}))
check('to_notlist', lambda: ocs.layout_operation('set_tab_order', '', {'names': 5}))
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(
        accepted.is_empty(),
        "accepted invalid requests: {accepted:?}"
    );
    assert_eq!(refused, 8);
    let paper = |names: Vec<String>| -> Vec<String> {
        names.into_iter().filter(|n| n != "Model").collect()
    };
    assert_eq!(
        paper(host.app.tabs[0].scene.layout_names()),
        ["Sheet3", "Details", "Layout1", "Sheet1"],
        "the tabs follow the script's order"
    );
    let dwg = crate::io::load_bytes(
        "order.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let mut saved: Vec<(i16, String)> = dwg
        .objects
        .values()
        .filter_map(|o| match o {
            document::objects::ObjectType::Layout(l)
                if l.name != "Model" && !l.block_record.is_null() =>
            {
                Some((l.tab_order, l.name.clone()))
            }
            _ => None,
        })
        .collect();
    saved.sort();
    assert_eq!(
        saved.into_iter().map(|(_, n)| n).collect::<Vec<_>>(),
        ["Sheet3", "Details", "Layout1", "Sheet1"],
        "the saved DWG keeps the order"
    );

    run(
        &mut host,
        "destroy",
        r#"
LY.set_current('Details')
LY.delete('Details')
LY.delete('Sheet1')
LT.delete('Temp')
"#,
    );
    let (refused, accepted) = report(host.document());
    assert!(accepted.is_empty(), "{accepted:?}");
    assert_eq!(refused, 0);
    let document = host.document();
    assert!(document.line_types.get("Temp").is_none());
    assert!(!document.objects.values().any(|o| matches!(o, document::objects::ObjectType::Layout(l) if l.name == "Sheet1" || l.name == "Details")));
    assert!(
        !document
            .entities()
            .any(|e| matches!(e, EntityType::Circle(_))),
        "the sheet's circle went with its layout"
    );
    assert_eq!(
        host.app.tabs[0].scene.current_layout, "Model",
        "deleting the current layout fell back to Model"
    );

    // U: the report layer and the three deletes; undoing them restores the layouts and the linetype.
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(4);
    let document = &app.tabs[0].scene.document;
    assert!(
        document.line_types.get("Temp").is_some(),
        "undo restored the linetype"
    );
    let names = app.tabs[0].scene.layout_names();
    assert!(
        names.contains(&"Sheet1".to_owned()) && names.contains(&"Details".to_owned()),
        "undo restored the layouts: {names:?}"
    );
    assert!(
        document
            .entities()
            .any(|e| matches!(e, EntityType::Circle(_))),
        "undo restored the sheet's circle"
    );
}

/// The modify wrappers through Python over the real runner: each drives
/// the real OCS command and the resulting geometry is checked exactly.
#[test]
fn audit_python_modify_wrappers_over_real_ipc() {
    // PEDIT's line-to-polyline conversion runs deep inside a nested command step; a debug
    // build needs more than the 2 MiB a test thread gets (the application's main thread has 8 MiB).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(modify_wrappers_audit_body)
        .unwrap()
        .join()
        .unwrap();
}

fn modify_wrappers_audit_body() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    // The array assertions below check loose copies; ARRAYRECT/ARRAYPOLAR
    // build associative arrays by default (thread-local, this thread only).
    crate::modules::draw::defaults::set_array_assoc(0.0);
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dir = std::env::temp_dir();
    let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
        let script = dir.join(format!("ocs_modify_{tag}_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
                "def P(x, y, z=0.0): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nM = doc.modify\nL = doc.layers\n",
                "refused, failed = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        failed.append('ACCEPTED_' + tag)\n",
                "def step(tag, fn):\n",
                "    try:\n        return fn()\n    except Exception as error:\n",
                "        failed.append(tag + '_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:45])\n",
                "{body}\n",
                "L.create(('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(failed))[:250])\n",
            ), body = body)).unwrap();
        assert!(process
            .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
            .unwrap());
        let _ = std::fs::remove_file(&script);
    };
    let report = |document: &Document| -> (usize, Vec<String>) {
        let name = document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n.starts_with("REPORT"))
            .last()
            .expect("script report layer");
        let (refused, failed) = name["REPORT".len()..].split_once('~').unwrap();
        (
            refused.trim().parse().unwrap(),
            failed.split_whitespace().map(str::to_owned).collect(),
        )
    };

    run(
        &mut host,
        "build",
        r#"
def line(a, b): return doc.create_entity('Line', start=P(*a), end=P(*b))
def circle(c, r): return doc.create_entity('Circle', center=P(*c), radius=r)
a = line((0, 0), (10, 0))
step('offset', lambda: M.offset(a, 2, (5, 5)))
h = line((100, 0), (110, 0)); v = line((105, -5), (105, 5))
step('trim', lambda: M.trim(h, (108, 0)))
h2 = line((200, 0), (209, 0)); w = line((215, -5), (215, 5))
step('extend', lambda: M.extend(h2, (208, 0)))
f1 = line((300, 0), (310, 0)); f2 = line((300, 0), (300, 10))
step('fillet', lambda: M.fillet(f1, f2, 2, (308, 0), (300, 8)))
m = circle((400, 0), 1)
step('move', lambda: M.move([m], (0, 0, 0), (5, 5, 0)))
c = circle((500, 0), 1)
step('copy', lambda: M.copy([c], (0, 0, 0), (0, 10, 0)))
r = line((600, 0), (610, 0))
step('rotate', lambda: M.rotate([r], (600, 0, 0), 90))
sc = line((700, 0), (710, 0))
step('scale', lambda: M.scale([sc], (700, 0, 0), 2))
mi = line((800, 0), (810, 0))
step('mirror', lambda: M.mirror([mi], (0, 5, 0), (1, 5, 0)))
e = line((900, 0), (910, 0))
step('erase', lambda: M.erase([e]))
ca = line((1300, 0), (1310, 0)); cb = line((1300, 0), (1300, 10))
step('chamfer', lambda: M.chamfer(ca, cb, 2, 3, (1308, 0), (1300, 8)))
ar = line((1400, 0), (1401, 0))
step('array_rect', lambda: M.array_rect([ar], 2, 3, 10, 20))
ap = line((1500, 0), (1501, 0))
step('array_polar', lambda: M.array_polar([ap], (1500, 10, 0), 4, 360))
pl = doc.create_entity('LwPolyline', vertices=[{'location': P(1600, 0)}, {'location': P(1610, 0)}, {'location': P(1610, 10)}])
step('explode', lambda: M.explode([pl]))
j1 = line((1700, 0), (1705, 0)); j2 = line((1705, 0), (1710, 0))
step('join', lambda: M.join([j1, j2]))
bk = line((1800, 0), (1810, 0))
step('break', lambda: M.break_entity(bk, (1802, 0, 0), (1805, 0, 0)))
st = line((1900, 0), (1910, 0))
step('stretch', lambda: M.stretch((1908, -2, 0), (1912, 2, 0), (1910, 0, 0), (1915, 0, 0)))
ln = line((2000, 0), (2010, 0))
step('lengthen', lambda: M.lengthen(ln, 5, (2009, 0, 0)))
def lwp(pts, closed=False): return doc.create_entity('LwPolyline', is_closed=closed, vertices=[{'location': P(*p)} for p in pts])
pc = lwp([(2100, 0), (2110, 0), (2110, 10)])
step('polyline_close', lambda: M.polyline_close(pc))
po = lwp([(2150, 0), (2160, 0), (2160, 10)], True)
step('polyline_open', lambda: M.polyline_open(po))
pw = lwp([(2200, 0), (2210, 0), (2210, 10)])
step('polyline_width', lambda: M.polyline_width(pw, 0.5))
pr = lwp([(2300, 0), (2310, 0), (2310, 10)])
step('polyline_reverse', lambda: M.polyline_reverse(pr))
pj = lwp([(2400, 0), (2410, 0)]); pjl = line((2410, 0), (2420, 0))
step('polyline_join', lambda: M.polyline_join(pj, [pjl]))
pline = line((2500, 0), (2510, 0))
step('polyline_from_line', lambda: M.polyline_width(pline, 1))
pb = line((2600, 0), (2601, 0)); pp = line((2600, 50), (2650, 50))
step('array_path', lambda: M.array_path([pb], pp, 5))
a3 = line((2700, 0), (2701, 0))
step('array_3d', lambda: M.array_3d([a3], 2, 2, 2, 10, 20, 30))
step('run_line', lambda: doc.command('LINE 1100,0 1110,10'))
def interactive():
    with doc.start_command('CIRCLE') as c:
        c.point((1200, 0, 0))
        c.text(3)
        if c.waiting:
            raise RuntimeError('circle still waiting')
step('interactive', interactive)
check('quit', lambda: doc.start_command('QUIT'))
check('newdoc', lambda: doc.command('NEW'))
check('py', lambda: doc.command('PY_RUN nothing'))
check('unknown', lambda: doc.start_command('NOSUCHCOMMAND'))
check('incomplete', lambda: doc.command('OFFSET'))
check('input_without_command', lambda: ocs.command_step('point', {'point': [0, 0, 0]}))
open_command = doc.start_command('LINE')
check('second_command', lambda: doc.start_command('CIRCLE'))
check('run_while_active', lambda: doc.command('POINT 1,1'))
open_command.cancel()
check('bad_point', lambda: ocs.command_step('point', {'point': [0, 0, 0]}))
"#,
    );
    let (refused, failed) = report(host.document());
    assert!(failed.is_empty(), "wrapper failures: {failed:?}");
    assert_eq!(refused, 9);

    let lines: Vec<document::entities::Line> = host
        .document()
        .entities()
        .filter_map(|e| match e {
            EntityType::Line(l) => Some(l.clone()),
            _ => None,
        })
        .collect();
    let circles: Vec<document::entities::Circle> = host
        .document()
        .entities()
        .filter_map(|e| match e {
            EntityType::Circle(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let arcs: Vec<document::entities::Arc> = host
        .document()
        .entities()
        .filter_map(|e| match e {
            EntityType::Arc(a) => Some(a.clone()),
            _ => None,
        })
        .collect();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let has_line = |x1: f64, y1: f64, x2: f64, y2: f64| {
        lines.iter().any(|l| {
            (near(l.start.x, x1) && near(l.start.y, y1) && near(l.end.x, x2) && near(l.end.y, y2))
                || (near(l.start.x, x2)
                    && near(l.start.y, y2)
                    && near(l.end.x, x1)
                    && near(l.end.y, y1))
        })
    };
    let describe = || {
        lines
            .iter()
            .map(|l| format!("({},{})-({},{})", l.start.x, l.start.y, l.end.x, l.end.y))
            .collect::<Vec<_>>()
    };
    assert!(
        has_line(0.0, 0.0, 10.0, 0.0) && has_line(0.0, 2.0, 10.0, 2.0),
        "offset: {:?}",
        describe()
    );
    assert!(has_line(100.0, 0.0, 105.0, 0.0), "trim: {:?}", describe());
    assert!(has_line(200.0, 0.0, 215.0, 0.0), "extend: {:?}", describe());
    assert!(
        arcs.iter()
            .any(|a| near(a.radius, 2.0) && near(a.center.x, 302.0) && near(a.center.y, 2.0)),
        "fillet arc: {arcs:?}"
    );
    assert!(
        circles
            .iter()
            .any(|c| near(c.center.x, 405.0) && near(c.center.y, 5.0)),
        "move: {circles:?}"
    );
    assert!(
        !circles.iter().any(|c| near(c.center.x, 400.0)),
        "move left the original: {circles:?}"
    );
    assert!(
        circles
            .iter()
            .any(|c| near(c.center.x, 500.0) && near(c.center.y, 0.0))
            && circles
                .iter()
                .any(|c| near(c.center.x, 500.0) && near(c.center.y, 10.0)),
        "copy: {circles:?}"
    );
    assert!(
        has_line(600.0, 0.0, 600.0, 10.0),
        "rotate: {:?}",
        describe()
    );
    assert!(has_line(700.0, 0.0, 720.0, 0.0), "scale: {:?}", describe());
    assert!(
        has_line(800.0, 0.0, 810.0, 0.0) && has_line(800.0, 10.0, 810.0, 10.0),
        "mirror keeps the source and adds the reflection: {:?}",
        describe()
    );
    assert!(!has_line(900.0, 0.0, 910.0, 0.0), "erase: {:?}", describe());
    assert!(
        has_line(1100.0, 0.0, 1110.0, 10.0),
        "doc.command run: {:?}",
        describe()
    );
    assert!(
        has_line(1302.0, 0.0, 1300.0, 3.0),
        "chamfer: {:?}",
        describe()
    );
    for (x, y) in [
        (1400.0, 0.0),
        (1420.0, 0.0),
        (1440.0, 0.0),
        (1400.0, 10.0),
        (1420.0, 10.0),
        (1440.0, 10.0),
    ] {
        assert!(
            has_line(x, y, x + 1.0, y),
            "rectangular array member at ({x}, {y}): {:?}",
            describe()
        );
    }
    assert!(
        has_line(1500.0, 0.0, 1501.0, 0.0) && has_line(1500.0, 20.0, 1499.0, 20.0),
        "polar array (start and half turn): {:?}",
        describe()
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.start.x > 1450.0 && l.start.x < 1520.0
                || l.end.x > 1450.0 && l.end.x < 1520.0)
            .count(),
        4,
        "polar array has four members"
    );
    assert!(
        has_line(1600.0, 0.0, 1610.0, 0.0) && has_line(1610.0, 0.0, 1610.0, 10.0),
        "explode: {:?}",
        describe()
    );
    assert!(!host.document().entities().any(|e| matches!(e, EntityType::LwPolyline(p) if p.vertices.first().is_some_and(|v| near(v.location.x, 1600.0)))), "explode removed the polyline");
    assert!(
        has_line(1700.0, 0.0, 1710.0, 0.0) && !has_line(1700.0, 0.0, 1705.0, 0.0),
        "join: {:?}",
        describe()
    );
    assert!(
        has_line(1800.0, 0.0, 1802.0, 0.0)
            && has_line(1805.0, 0.0, 1810.0, 0.0)
            && !has_line(1800.0, 0.0, 1810.0, 0.0),
        "break: {:?}",
        describe()
    );
    assert!(
        has_line(1900.0, 0.0, 1915.0, 0.0),
        "stretch: {:?}",
        describe()
    );
    assert!(
        has_line(2000.0, 0.0, 2015.0, 0.0),
        "lengthen: {:?}",
        describe()
    );
    let polylines: Vec<document::entities::LwPolyline> = host
        .document()
        .entities()
        .filter_map(|e| match e {
            EntityType::LwPolyline(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    let at = |x: f64| {
        polylines
            .iter()
            .find(|p| p.vertices.iter().any(|v| near(v.location.x, x)))
    };
    let pc = at(2100.0).expect("closed polyline");
    assert!(pc.is_closed, "polyline_close");
    assert!(
        !at(2150.0).expect("opened polyline").is_closed,
        "polyline_open"
    );
    let pw = at(2200.0).expect("widened polyline");
    assert!(
        pw.constant_width == 0.5
            || pw
                .vertices
                .iter()
                .all(|v| v.start_width == 0.5 && v.end_width == 0.5),
        "polyline_width: {pw:?}"
    );
    let pr = at(2300.0).expect("reversed polyline");
    assert_eq!(
        (pr.vertices[0].location.x, pr.vertices[0].location.y),
        (2310.0, 10.0),
        "polyline_reverse: {pr:?}"
    );
    let pj = polylines
        .iter()
        .find(|p| p.vertices.iter().any(|v| near(v.location.x, 2420.0)))
        .expect("joined polyline");
    assert_eq!(pj.vertices.len(), 3, "polyline_join: {pj:?}");
    assert!(
        !has_line(2410.0, 0.0, 2420.0, 0.0),
        "polyline_join consumed the line: {:?}",
        describe()
    );
    let from_line = at(2500.0).expect("a line turned into a polyline");
    assert!(
        from_line.constant_width == 1.0 || from_line.vertices.iter().all(|v| v.start_width == 1.0),
        "polyline_from_line: {from_line:?}"
    );
    assert!(
        !has_line(2500.0, 0.0, 2510.0, 0.0),
        "the line became the polyline: {:?}",
        describe()
    );
    let path_copies: Vec<(f64, f64)> = lines
        .iter()
        .filter(|l| l.start.x > 2599.0 && l.start.x < 2660.0)
        .map(|l| (l.start.x, l.start.y))
        .collect();
    for x in [2600.0, 2612.5, 2625.0, 2637.5, 2650.0] {
        assert!(
            path_copies.iter().any(|p| near(p.0, x) && near(p.1, 0.0)),
            "array_path item at x={x} (five evenly spaced along the 50-long path): {path_copies:?}"
        );
    }
    let three_d: Vec<(f64, f64, f64)> = lines
        .iter()
        .filter(|l| l.start.x > 2699.0 && l.start.x < 2725.0)
        .map(|l| (l.start.x, l.start.y, l.start.z))
        .collect();
    for x in [2700.0, 2720.0] {
        for y in [0.0, 10.0] {
            for z in [0.0, 30.0] {
                assert!(
                    three_d
                        .iter()
                        .any(|p| near(p.0, x) && near(p.1, y) && near(p.2, z)),
                    "array_3d member ({x},{y},{z}): {three_d:?}"
                );
            }
        }
    }
    assert!(
        circles
            .iter()
            .any(|c| near(c.center.x, 1200.0) && near(c.radius, 3.0)),
        "start_command session: {circles:?}"
    );
    assert!(
        host.app.tabs[0].active_cmd.is_none(),
        "no command is left running"
    );

    // U: a wrapper's change is one undoable step (the report layer is the other).
    run(
        &mut host,
        "undo",
        r#"
u = doc.create_entity('Circle', center=P(1000, 0), radius=1)
step('undo_move', lambda: M.move([u], (0, 0, 0), (7, 0, 0)))
"#,
    );
    assert!(report(host.document()).1.is_empty());
    let moved = |document: &Document, x: f64| {
        document.entities().any(
            |e| matches!(e, EntityType::Circle(c) if near(c.center.x, x) && near(c.center.y, 0.0)),
        )
    };
    assert!(moved(host.document(), 1007.0));
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(2);
    assert!(
        moved(&app.tabs[0].scene.document, 1000.0),
        "undo put the circle back"
    );
    assert!(!moved(&app.tabs[0].scene.document, 1007.0));
}

/// A scripted dict is checked all the way down: a nested record (a polyline
/// vertex, a mesh vertex) may not carry an unknown key or omit the field that
/// defines its geometry, so a typo never turns into a silent zero coordinate.
#[test]
fn audit_python_nested_input_is_validated_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let script = std::env::temp_dir().join(format!("ocs_nested_{}.py", std::process::id()));
    std::fs::write(&script, concat!(
            "def P(x, y, z=0.0): return {'x': x, 'y': y, 'z': z}\n",
            "doc = ocs.active_document\nL = doc.layers\n",
            "refused, problems = [], []\n",
            "def check(tag, fn, expect):\n",
            "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError) as error:\n",
            "        if expect in str(error):\n            refused.append(tag)\n",
            "        else:\n            problems.append('WRONGMESSAGE_' + tag)\n",
            "    else:\n        problems.append('ACCEPTED_' + tag)\n",
            "def lw(vertices): return lambda: doc.create_entity('LwPolyline', vertices=vertices)\n",
            "ok = {'location': P(0, 0)}\n",
            "check('lw_xy_keys', lw([ok, {'x': 5, 'y': 5}]), 'no property')\n",
            "check('lw_empty_vertex', lw([ok, {}]), 'needs location')\n",
            "check('lw_null_location', lw([ok, {'location': None}]), 'needs location')\n",
            "check('lw_typo', lw([ok, {'location': P(1, 1), 'bulgee': 0.5}]), 'no property')\n",
            "check('lw_message_lists_keys', lw([ok, {'lokation': P(1, 1)}]), 'location')\n",
            "check('poly2d_missing_location', lambda: doc.create_entity('Polyline2D', vertices=[{'flags': 0}, {'location': P(1, 1)}]), 'needs location')\n",
            "check('poly2d_typo', lambda: doc.create_entity('Polyline2D', vertices=[{'location': P(0, 0)}, {'loc': P(1, 1)}]), 'no property')\n",
            "check('mesh_missing_location', lambda: doc.create_entity('PolygonMesh', vertices=[{}]), 'needs location')\n",
            "good = doc.create_entity('LwPolyline', vertices=[ok, {'location': P(10, 0), 'bulge': 0.5}, {'location': P(10, 10)}])\n",
            "good2 = doc.create_entity('Polyline2D', vertices=[{'location': P(0, 0)}, {'location': P(5, 5)}])\n",
            "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(problems))\n",
        )).unwrap();
    assert!(process
        .dispatch(
            &mut host,
            &format!("PY_RUN {}", script.display()),
            &mut |_| {}
        )
        .unwrap());
    let _ = std::fs::remove_file(&script);
    let name = host
        .document()
        .layers
        .iter()
        .map(|l| l.name.clone())
        .filter(|n| n.starts_with("REPORT"))
        .last()
        .expect("script report layer");
    let (refused, problems) = name["REPORT".len()..].split_once('~').unwrap();
    assert_eq!(problems.trim(), "", "problems: {problems}");
    assert_eq!(refused.trim(), "8");
    // Nothing invalid reached the drawing; the two valid polylines did.
    let document = host.document();
    let lw: Vec<_> = document
        .entities()
        .filter_map(|e| match e {
            EntityType::LwPolyline(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(lw.len(), 1, "only the valid LwPolyline was created");
    assert_eq!(lw[0].vertices.len(), 3);
    assert_eq!(lw[0].vertices[1].bulge, 0.5);
    assert_eq!(
        document
            .entities()
            .filter(|e| matches!(e, EntityType::Polyline2D(_)))
            .count(),
        1
    );
    assert!(!document
        .entities()
        .any(|e| matches!(e, EntityType::PolygonMesh(_))));
    drop(process);
}

/// The user's SCRIPTCOMMANDS setting gates the command runner, and a script cannot change it.
#[test]
fn script_commands_setting_gates_the_runner() {
    use ocs_plugin_api::host::CommandRequest as R;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    assert!(app.script_commands, "scripts may run commands by default");
    let run = |app: &mut OpenCADStudio, line: &str| {
        HostSession::new(app, 0).run_command(R::Run { line: line.into() })
    };
    assert!(run(&mut app, "LINE 0,0 10,0").is_ok());

    // A script cannot touch the setting, on or off.
    let refused = run(&mut app, "SCRIPTCOMMANDS 0").unwrap_err();
    assert!(refused.contains("cannot be run from a script"), "{refused}");
    assert!(app.script_commands);

    // The user turns it off at the command line.
    let reply = app.automation_op(r#"{"op":"run","cmd":"SCRIPTCOMMANDS 0"}"#);
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(!app.script_commands);
    let refused = run(&mut app, "LINE 0,0 20,0").unwrap_err();
    assert!(refused.contains("turned off"), "{refused}");
    let refused = HostSession::new(&mut app, 0)
        .run_command(R::Start {
            name: "LINE".into(),
        })
        .unwrap_err();
    assert!(refused.contains("turned off"), "{refused}");
    assert_eq!(
        app.tabs[0].scene.document.entities().count(),
        1,
        "nothing ran while it was off"
    );
    assert!(
        HostSession::new(&mut app, 0).run_command(R::Cancel).is_ok(),
        "Cancel is still allowed while it is off"
    );

    // And back on.
    app.automation_op(r#"{"op":"run","cmd":"SCRIPTCOMMANDS 1"}"#);
    assert!(app.script_commands);
    assert!(run(&mut app, "LINE 0,0 20,0").is_ok());
    assert_eq!(app.tabs[0].scene.document.entities().count(), 2);
}

/// Exploration harness (not a gate): prints how the real commands answer
/// each step so the wrappers are written against observed prompts.
#[test]
#[ignore]
#[allow(unused_must_use)]
fn spike_command_runner_prompts() {
    use ocs_plugin_api::host::CommandRequest as R;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let show = |label: &str, r: &Result<ocs_plugin_api::host::CommandOutcome, String>| match r {
        Ok(o) => eprintln!(
            "SPIKE {label}: {} prompt={:?} accepts={:?} opts={:?} added={} err={:?}",
            o.status, o.prompt, o.accepts, o.options, o.added, o.error
        ),
        Err(e) => eprintln!("SPIKE {label}: ERR {e}"),
    };
    let run = |host: &mut HostSession<'_>, label: &str, r: R| {
        let out = host.run_command(r);
        show(label, &out);
        out
    };
    let mk = |host: &mut HostSession<'_>, lines: &[&str]| -> Vec<Handle> {
        for l in lines {
            host.run_command(R::Run { line: (*l).into() }).unwrap();
        }
        host.document()
            .entities()
            .map(|e| e.common().handle)
            .collect()
    };
    let hs = mk(&mut host, &["PLINE 2400,0 2410,0", "LINE 2410,0 2420,0"]);
    run(
        &mut host,
        "PEDIT start",
        R::Start {
            name: "PEDIT".into(),
        },
    );
    run(
        &mut host,
        "pick",
        R::Entity {
            handle: hs[0],
            point: [2400.0, 0.0, 0.0],
        },
    );
    run(&mut host, "J", R::Token { text: "J".into() });
    host.app.tabs[0].scene.replace_selection_exact(&[hs[1]]);
    run(&mut host, "selection", R::Selection);
    run(&mut host, "enter", R::Enter);
    run(&mut host, "X", R::Token { text: "X".into() });
    eprintln!(
        "SPIKE final entity count {}",
        host.document().entities().count()
    );
}

#[test]
fn audit_python_raster_image_definition_linkage_over_real_ipc() {
    use document::objects::ObjectType;
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let dir = std::env::temp_dir();
    let (a, b) = (dir.join("ocs_link_a.png"), dir.join("ocs_link_b.png"));
    image::RgbaImage::from_pixel(8, 4, image::Rgba([9, 9, 9, 255]))
        .save(&a)
        .unwrap();
    image::RgbaImage::from_pixel(6, 6, image::Rgba([90, 9, 9, 255]))
        .save(&b)
        .unwrap();
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let script = dir.join(format!("ocs_link_{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            concat!(
                "doc = ocs.active_document\n",
                "doc.create_entity('RasterImage', file_path={a:?}, insertion_point=(0, 0, 0))\n",
                "doc.create_entity('RasterImage', file_path={a:?}, insertion_point=(20, 0, 0))\n",
                "doc.create_entity('RasterImage', file_path={b:?}, insertion_point=(40, 0, 0))\n",
            ),
            a = a.display().to_string(),
            b = b.display().to_string()
        ),
    )
    .unwrap();
    assert!(process
        .dispatch(
            &mut host,
            &format!("PY_RUN {}", script.display()),
            &mut |_| {}
        )
        .unwrap());
    let _ = std::fs::remove_file(&script);

    // The structure: one definition per file, registered and owned by ACAD_IMAGE_DICT.
    let structure = |document: &Document| {
        let root = document.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root_dict)) = document.objects.get(&root) else {
            panic!("no root dictionary")
        };
        let dict_handle = root_dict.get("ACAD_IMAGE_DICT");
        let entries = dict_handle
            .and_then(|h| match document.objects.get(&h) {
                Some(ObjectType::Dictionary(d)) => Some(d.entries.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let defs: Vec<_> = document
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                ObjectType::ImageDefinition(d) => Some((*h, d.owner, d.file_name.clone())),
                _ => None,
            })
            .collect();
        let mut images: Vec<_> = document
            .entities()
            .filter_map(|e| match e {
                EntityType::RasterImage(i) => {
                    Some((i.common.handle, i.definition_handle, i.file_path.clone()))
                }
                _ => None,
            })
            .collect();
        images.sort_by_key(|(h, _, _)| h.value());
        (dict_handle, entries, defs, images)
    };
    let check = |label: &str, document: &Document| {
        let (dict, entries, defs, images) = structure(document);
        let dict = dict.unwrap_or_else(|| panic!("{label}: no ACAD_IMAGE_DICT"));
        assert_eq!(defs.len(), 2, "{label}: one definition per file");
        let mut keys: Vec<_> = entries.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["ocs_link_a", "ocs_link_b"], "{label}");
        for (key, handle) in &entries {
            let (_, owner, file) = defs
                .iter()
                .find(|(h, _, _)| h == handle)
                .unwrap_or_else(|| panic!("{label}: dangling entry {key}"));
            assert_eq!(*owner, dict, "{label}: {key} is owned by ACAD_IMAGE_DICT");
            assert!(file.contains(key.as_str()), "{label}: {key} -> {file}");
        }
        assert_eq!(images.len(), 3, "{label}");
        assert_eq!(
            images[0].1, images[1].1,
            "{label}: images of one file share a definition"
        );
        assert_ne!(images[0].1, images[2].1, "{label}");
        for (_, definition, path) in &images {
            assert!(
                defs.iter().any(|(h, _, _)| Some(*h) == *definition),
                "{label}: definition resolves"
            );
            assert!(
                path.contains("ocs_link_"),
                "{label}: file path kept: {path}"
            );
        }
    };
    check("live", host.document());
    let dwg = crate::io::load_bytes(
        "link.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let dxf = crate::io::load_bytes(
        "link.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    check("DWG", &dwg);
    check("DXF", &dxf);

    // U: undoing the last image leaves no dangling dictionary entry.
    drop(process);
    drop(host);
    app.finish_pending_history(0);
    app.undo_steps(1);
    let (_, entries, defs, images) = structure(&app.tabs[0].scene.document);
    assert!(images.len() < 3);
    for (key, handle) in &entries {
        assert!(
            defs.iter().any(|(h, _, _)| h == handle),
            "undo left a dangling entry {key}"
        );
    }
    for file in [&a, &b] {
        let _ = std::fs::remove_file(file);
    }
}

#[test]
fn staged_python_point_pick_and_cancel_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let process = {
        let mut host = HostSession::new(&mut app, 0);
        std::sync::Arc::new(
            ocs_plugin_api::process::PluginProcess::spawn(
                std::path::Path::new(&plugin_path),
                &mut host,
                crate::plugin::v4_support::notification_handler(),
                None,
            )
            .expect("spawn staged Python plugin"),
        )
    };
    let request = |app: &mut OpenCADStudio, prompt: &str, entity: bool| {
        let mut started = None;
        {
            let mut host = HostSession::new(app, 0);
            let method = if entity {
                "request_entity"
            } else {
                "request_point"
            };
            let cmd = format!("PY_EVAL ocs.active_document.{method}('{prompt}')");
            assert!(process
                .dispatch(&mut host, &cmd, &mut |id| started = Some(id))
                .unwrap());
        }
        let output = app.command_line.history.last().unwrap().text.clone();
        let token = output
            .split_whitespace()
            .find_map(|part| part.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("Python pick token missing from {output:?}"));
        let id = started.expect("Python request started an interactive command");
        app.set_active_command(
            0,
            Box::new(PluginProcessInteractiveAdapter::new(
                std::sync::Arc::clone(&process),
                id,
            )),
        );
        token
    };

    let picked_token = request(&mut app, "Pick a point", false);
    let result = app.tabs[0]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(glam::DVec3::new(3.0, 4.0, 0.0));
    let _ = app.apply_cmd_result(result);
    assert!(app.tabs[0].active_cmd.is_none());
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({picked_token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("point"));

    let target = {
        let mut host = HostSession::new(&mut app, 0);
        host.add_entity(EntityType::Line(Line::from_points(
            document::types::Vector3::new(6.0, 7.0, 0.0),
            document::types::Vector3::new(9.0, 7.0, 0.0),
        )))
    };
    let entity_token = request(&mut app, "Pick an entity", true);
    assert!(app.tabs[0].active_cmd.as_ref().unwrap().needs_entity_pick());
    let result = app.tabs[0]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_entity_pick(target, glam::DVec3::new(6.0, 7.0, 0.0));
    let _ = app.apply_cmd_result(result);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({entity_token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("entity"));
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains(&target.value().to_string()));

    let cancelled_token = request(&mut app, "Cancel a point", false);
    let result = app.tabs[0].active_cmd.as_mut().unwrap().on_enter();
    let _ = app.apply_cmd_result(result);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({cancelled_token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("cancelled"));
}

/// A script started from a typed line (`PY_RUN <file>`) asks for a point. The
/// file name was the plugin's argument, so it is not an answer to the prompt
/// the script opened, and the line's closing Enter does not cancel it.
#[test]
fn staged_python_typed_line_leaves_the_requested_point_prompt_open() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let _serial = crate::plugin::external::PROCESS_TESTS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let script = std::env::temp_dir().join(format!("ocs_py_typed_{}.py", std::process::id()));
    std::fs::write(
        &script,
        "ocs.active_document.request_point(\"Pick the corner\")\n",
    )
    .unwrap();
    let script_path = script.to_string_lossy().replace('\\', "/");
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let id = {
        let mut host = HostSession::new(&mut app, 0);
        crate::plugin::external::spawn_into_manager_for_tests(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .expect("spawn staged Python plugin")
        .0
    };
    let _ = app.run_command_line(&format!("PY_RUN {script_path}"));
    let prompt = app.tabs[0].active_cmd.as_ref().map(|command| command.prompt());
    if let Some(command) = app.tabs[0].active_cmd.as_mut() {
        let _ = command.on_enter();
    }
    app.tabs[0].active_cmd = None;
    crate::plugin::external::remove_plugin_for_tests(&id);
    let _ = std::fs::remove_file(&script);
    assert_eq!(
        prompt.as_deref(),
        Some("Pick the corner"),
        "history: {:?}",
        app.command_line
            .history
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn staged_python_tabs_isolate_tokens_and_notifications() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(2));
    app.tabs[1].is_start = false;
    let tab0 = app.tabs[0].id;
    let tab1 = app.tabs[1].id;
    assert_ne!(tab0, tab1);
    let process = {
        let mut host = HostSession::new(&mut app, 0);
        std::sync::Arc::new(
            ocs_plugin_api::process::PluginProcess::spawn(
                std::path::Path::new(&plugin_path),
                &mut host,
                crate::plugin::v4_support::notification_handler(),
                None,
            )
            .expect("spawn staged Python plugin"),
        )
    };
    let mut started = None;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                "PY_EVAL ocs.active_document.request_point('Tab zero')",
                &mut |id| started = Some(id)
            )
            .unwrap());
    }
    let token = app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .split_whitespace()
        .find_map(|part| part.parse::<u64>().ok())
        .expect("pick token");
    app.set_active_command(
        0,
        Box::new(PluginProcessInteractiveAdapter::new(
            std::sync::Arc::clone(&process),
            started.unwrap(),
        )),
    );
    {
        let mut host = HostSession::new(&mut app, 1);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("None"),
        "other tab must not consume the pending token"
    );
    let result = app.tabs[0]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(glam::DVec3::new(2.0, 3.0, 0.0));
    let _ = app.apply_cmd_result(result);
    {
        let mut host = HostSession::new(&mut app, 1);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("None"));
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(app
        .command_line
        .history
        .last()
        .unwrap()
        .text
        .contains("point"));

    use ocs_plugin_api::host::HostNotification;
    process
        .notify_plugin(
            None,
            HostNotification::DrawingChanged {
                tab_id: tab0,
                epoch: 101,
            },
        )
        .unwrap();
    process
        .notify_plugin(
            None,
            HostNotification::DrawingChanged {
                tab_id: tab1,
                epoch: 202,
            },
        )
        .unwrap();
    // Notifications are best-effort and consumed by the runner's reader
    // thread before its next Dispatch. Give that thread a bounded handoff.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let poll_events_until = |app: &mut OpenCADStudio, tab: usize, epoch: &str| {
        let mut output = String::new();
        for _ in 0..10 {
            {
                let mut host = HostSession::new(app, tab);
                assert!(process
                    .dispatch(
                        &mut host,
                        "PY_EVAL ocs.active_document.poll_events()",
                        &mut |_| {}
                    )
                    .unwrap());
            }
            output = app.command_line.history.last().unwrap().text.clone();
            if output.contains(epoch) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        output
    };
    let other_events = poll_events_until(&mut app, 1, "202");
    assert!(
        other_events.contains("202") && !other_events.contains("101"),
        "{other_events}"
    );
    let first_events = poll_events_until(&mut app, 0, "101");
    assert!(
        first_events.contains("101") && !first_events.contains("202"),
        "{first_events}"
    );

    for epoch in 0..257 {
        process
            .notify_plugin(
                None,
                HostNotification::DrawingChanged {
                    tab_id: tab0,
                    epoch,
                },
            )
            .unwrap();
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                "PY_EVAL ocs.active_document.poll_events()",
                &mut |_| {}
            )
            .unwrap());
    }
    let overflow = &app.command_line.history.last().unwrap().text;
    assert!(
        overflow.contains("overflow") && overflow.contains("dropped"),
        "{overflow}"
    );

    process
        .notify_plugin(
            None,
            HostNotification::DrawingChanged {
                tab_id: tab0,
                epoch: 303,
            },
        )
        .unwrap();
    process
        .notify_plugin(None, HostNotification::DocumentTabClosed { tab_id: tab0 })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process
            .dispatch(
                &mut host,
                "PY_EVAL ocs.active_document.poll_events()",
                &mut |_| {}
            )
            .unwrap());
    }
    assert!(
        app.command_line.history.last().unwrap().text.contains("[]"),
        "closing a tab must discard its queued events"
    );
}

#[test]
fn unmapped_canvas_kind_layer_change_undoes() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle;
    {
        let mut host = HostSession::new(&mut app, 0);
        handle = host.add_entity(EntityType::MLine(document::entities::MLine::default()));
        let original = host.document().get_entity(handle).unwrap();
        let changed =
            ocs_plugin_api::entity_coverage::patch_canvas_layer(original, "HATCHES").unwrap();
        host.update_entities_transaction("Move hatch layer", vec![changed])
            .unwrap();
        assert_eq!(
            host.document().get_entity(handle).unwrap().common().layer,
            "HATCHES"
        );
    }
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    assert_eq!(
        app.tabs[0]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .layer,
        "0"
    );
}

#[test]
fn insert_transform_transaction_undoes_without_changing_block_identity() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle;
    {
        let mut host = HostSession::new(&mut app, 0);
        let insert =
            document::entities::Insert::new("DOOR", document::types::Vector3::new(1.0, 2.0, 0.0));
        handle = host.add_entity(EntityType::Insert(insert));
        let mut changed = host.document().get_entity(handle).unwrap().clone();
        if let EntityType::Insert(insert) = &mut changed {
            insert.insert_point.x = 4.0;
            insert.set_x_scale(2.0);
        }
        host.update_entities_transaction("Move block", vec![changed])
            .unwrap();
        assert!(
            matches!(host.document().get_entity(handle), Some(EntityType::Insert(insert))
                if insert.insert_point.x == 4.0 && insert.x_scale() == 2.0 && insert.block_name == "DOOR")
        );
    }
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    assert!(
        matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Insert(insert))
            if insert.insert_point.x == 1.0 && insert.x_scale() == 1.0 && insert.block_name == "DOOR")
    );
}

#[test]
fn system_variables_change_without_command_reentry() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    assert_eq!(
        host.system_variable("clayer"),
        Some(HostSettingValue::Text("0".into()))
    );
    assert_eq!(
        host.system_variable("SNAPANG"),
        Some(HostSettingValue::Number(0.0))
    );
    assert!(host
        .set_system_variable("CLAYER", HostSettingValue::Text("Missing".into()))
        .is_err());
    assert_eq!(
        host.system_variable("CLAYER"),
        Some(HostSettingValue::Text("0".into()))
    );

    let mut point = Point::new();
    point.common.layer = "Annotations".into();
    host.add_entity(EntityType::Point(point));
    assert_eq!(
        host.set_system_variable("clayer", HostSettingValue::Text("Annotations".into())),
        Ok(HostSettingValue::Text("Annotations".into()))
    );
    assert_eq!(
        host.system_variable("CLAYER"),
        Some(HostSettingValue::Text("Annotations".into()))
    );
    assert_eq!(host.app.tabs[0].active_layer, "Annotations");
    assert_eq!(host.app.tabs[0].layers.current_layer, "Annotations");
    assert_eq!(host.app.ribbon.active_layer, "Annotations");
    assert!(host.app.tabs[0].dirty);

    assert_eq!(
        host.set_system_variable("snapang", HostSettingValue::Number(450.0)),
        Ok(HostSettingValue::Number(90.0))
    );
    assert_eq!(
        host.system_variable("SNAPANG"),
        Some(HostSettingValue::Number(90.0))
    );
    assert!(host
        .set_system_variable("SNAPANG", HostSettingValue::Number(f64::INFINITY))
        .is_err());
    assert!(host
        .set_system_variable("SNAPANG", HostSettingValue::Number(f64::MAX))
        .is_err());
    assert_eq!(
        host.system_variable("SNAPANG"),
        Some(HostSettingValue::Number(90.0))
    );
}

#[test]
fn xdata_record_round_trips_and_registers_appid() {
    let mut app = OpenCADStudio::new_for_test();
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::new()));

    let mut rec = ExtendedDataRecord::new("DEMO_SURVEY");
    rec.add_value(XDataValue::String("PNT-1".to_string()));
    rec.add_value(XDataValue::Integer32(42));
    assert!(host.write_record(h, rec));

    let got = host.read_record(h, "DEMO_SURVEY").expect("record missing");
    assert_eq!(got.values.len(), 2);
    // APPID registered so the XDATA survives a DWG/DXF round-trip.
    assert!(host.document().app_ids.contains("DEMO_SURVEY"));

    // A second write replaces rather than duplicates the record.
    let mut rec2 = ExtendedDataRecord::new("DEMO_SURVEY");
    rec2.add_value(XDataValue::String("PNT-2".to_string()));
    assert!(host.write_record(h, rec2));
    let got = host.read_record(h, "DEMO_SURVEY").unwrap();
    assert_eq!(got.values.len(), 1);

    // Removal reports whether anything was dropped.
    assert!(host.remove_record(h, "DEMO_SURVEY"));
    assert!(host.read_record(h, "DEMO_SURVEY").is_none());
    assert!(!host.remove_record(h, "DEMO_SURVEY"));
}

#[test]
fn plugin_state_round_trips_through_hostapi_trait() {
    use ocs_plugin_api::host::{self, HostApi};
    let mut app = OpenCADStudio::new_for_test();
    let mut session = HostSession::new(&mut app, 0);
    let host: &mut dyn HostApi = &mut session;

    // Absent before first use.
    assert!(host::plugin_state::<u32>(&*host, "opencad.demo").is_none());
    // Insert via ensure, then mutate.
    *host::ensure_plugin_state(host, "opencad.demo", || 7u32).unwrap() += 1;
    assert_eq!(
        *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
        8
    );
    *host::plugin_state_mut::<u32>(host, "opencad.demo").unwrap() = 100;
    assert_eq!(
        *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
        100
    );
}

#[test]
fn ensure_plugin_state_type_mismatch_does_not_panic() {
    use ocs_plugin_api::host::{self, HostApi};
    let mut app = OpenCADStudio::new_for_test();
    let mut session = HostSession::new(&mut app, 0);
    let host: &mut dyn HostApi = &mut session;
    let _ = host::ensure_plugin_state(host, "opencad.demo", || 7u32);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = host::ensure_plugin_state::<String>(&mut *host, "opencad.demo", || "x".to_string());
    }));
    assert!(
        outcome.is_ok(),
        "type mismatch in ensure_plugin_state must degrade, not panic: {:?}",
        outcome.err()
    );
    assert!(
        host::ensure_plugin_state::<String>(host, "opencad.demo", || "x".to_string()).is_none()
    );
    assert_eq!(
        *host::ensure_plugin_state::<u32>(host, "opencad.demo", || 0).unwrap(),
        7
    );
}

#[test]
fn add_entities_batch_assigns_handles_and_publishes_once() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    let pts: Vec<EntityType> = (0..5)
        .map(|i| {
            EntityType::Point(Point::at(document::types::Vector3::new(
                i as f64, i as f64, 0.0,
            )))
        })
        .collect();
    let handles = host.add_entities(pts);

    assert_eq!(handles.len(), 5);
    assert!(handles.iter().all(|h| !h.is_null()));
    // Each handle is unique.
    let mut set = std::collections::HashSet::new();
    for h in &handles {
        assert!(set.insert(h.value()));
    }
    // All entities are in the document.
    for h in &handles {
        assert!(host.document().get_entity(*h).is_some());
    }
}

#[test]
fn update_entity_replaces_in_place_preserving_handle() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        1.0, 1.0, 0.0,
    ))));
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;

    // Edit a snapshot copy (as a plugin would) and commit it.
    let mut edited = host.document().get_entity(h).unwrap().clone();
    edited.common_mut().layer = "PLUGIN_EDIT".to_string();
    assert!(host.update_entity(edited));

    // Same handle, edit applied, geometry re-tessellated.
    let got = host
        .document()
        .get_entity(h)
        .expect("entity kept its handle");
    assert_eq!(got.common().layer, "PLUGIN_EDIT");
    assert_ne!(
        host.app.tabs[0].scene.geometry_epoch, epoch_before,
        "update should bump geometry"
    );

    // Updating an unknown handle fails and changes nothing.
    let mut ghost = Point::new();
    ghost.common.handle = Handle::new(999_999);
    assert!(!host.update_entity(EntityType::Point(ghost)));
}

fn reference_test_line(end_x: f64, y: f64, handle: Handle) -> EntityType {
    let mut line = document::entities::Line::from_points(
        document::types::Vector3::new(0.0, y, 0.0),
        document::types::Vector3::new(end_x, y, 0.0),
    );
    line.common.handle = handle;
    EntityType::Line(line)
}

#[test]
fn reference_only_refusals_record_no_undo_step() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    // Marking an absent entity refuses.
    assert!(host.set_reference_only(Handle::new(999_999), true).is_err());
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    // A locked layer is as unwritable for the designation as for every
    // other plugin write.
    host.app.tabs[0]
        .scene
        .document
        .layers
        .get_mut("0")
        .map(|layer| layer.lock());
    assert!(host.set_reference_only(handle, true).is_err());
    host.app.tabs[0]
        .scene
        .document
        .layers
        .get_mut("0")
        .map(|layer| layer.unlock());
    assert!(
        !host.app.tabs[0].scene.is_reference_only(handle),
        "refusals must not partially apply the designation"
    );
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        0,
        "refusals record no undo step"
    );
    // An idempotent re-mark succeeds without recording another step.
    host.set_reference_only(handle, true).unwrap();
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 1);
    host.set_reference_only(handle, true).unwrap();
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        1,
        "an idempotent re-mark records no undo step"
    );
}

#[test]
fn reference_only_designation_composes_undo_like_entity_writes() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    // A scope-less designation is one undo step of its own.
    host.set_reference_only(handle, true).unwrap();
    host.set_reference_only(handle, false).unwrap();
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 2);

    // Undo reverts the unmark, redo re-applies it.
    drop(host);
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(host.app.tabs[0].scene.is_reference_only(handle));
    drop(host);
    app.redo_steps(1);
    let mut host = HostSession::new(&mut app, 0);
    assert!(!host.app.tabs[0].scene.is_reference_only(handle));

    // Inside a plugin-opened push_undo scope the designation joins the
    // scope: an entity write plus two markings is ONE step under the
    // plugin's label.
    host.push_undo("PLUGIN: refresh");
    let preview = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        3.0, 0.0, 0.0,
    ))));
    host.set_reference_only(preview, true).unwrap();
    host.set_reference_only(handle, true).unwrap();
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    assert_eq!(app.tabs[0].history.undo_stack[2].label(), "PLUGIN: refresh");

    // Undoing the plugin operation reverts it whole.
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(
        host.app.tabs[0]
            .scene
            .document
            .get_entity(preview)
            .is_none(),
        "undoing the scope removed the entity created inside it"
    );
    assert!(
        !host.app.tabs[0].scene.is_reference_only(handle),
        "undoing the scope restored the designation's prior state"
    );
}

#[test]
fn reference_only_designation_is_not_written_to_dwg_or_dxf() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    // The designation lives in the scene's in-memory set only.
    assert!(host.app.tabs[0].scene.is_reference_only(handle));

    // The host writes nothing of its own into the document: no APPID
    // registration, no marker XDATA on the designated entity.
    assert!(host.document().app_ids.get("OCS").is_none());
    assert!(host
        .document()
        .get_entity(handle)
        .expect("designated entity")
        .common()
        .extended_data
        .get_record("OCS")
        .is_none());

    let contains_marker =
        |bytes: &[u8]| bytes.windows(15).any(|window| window == b"REFERENCE_ONLY");

    // DWG: the marker string appears nowhere in the saved bytes, and a
    // reopened file carries neither the host APPID nor the marker record.
    let dwg = document::DwgWriter::write_to_vec(host.document()).unwrap();
    assert!(
        !contains_marker(&dwg),
        "DWG save must not contain the designation marker"
    );
    let reloaded = crate::io::load_bytes("reference-only.dwg", dwg).unwrap();
    assert!(reloaded.app_ids.get("OCS").is_none());
    assert!(reloaded
        .get_entity(handle)
        .expect("the line survives the round trip")
        .common()
        .extended_data
        .get_record("OCS")
        .is_none());
    assert!(
        !contains_marker(&document::DwgWriter::write_to_vec(&reloaded).unwrap()),
        "a re-save of the reopened file stays marker-free"
    );

    // DXF: same contract for the text format.
    let dxf = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    assert!(
        !contains_marker(&dxf),
        "DXF save must not contain the designation marker"
    );
    let reloaded = crate::io::load_bytes("reference-only.dxf", dxf).unwrap();
    assert!(reloaded.app_ids.get("OCS").is_none());
    assert!(reloaded
        .get_entity(handle)
        .expect("the line survives the round trip")
        .common()
        .extended_data
        .get_record("OCS")
        .is_none());
}

#[test]
fn erasing_a_designated_entity_undoes_designation_with_it() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();

    // The plugin erase path removes a designated entity inside a scope:
    // entity and set entry leave together.
    host.push_undo("PLUGIN: replace the underlying model");
    host.remove_entity(handle);
    drop(host);
    app.finish_pending_history(0);
    let host = HostSession::new(&mut app, 0);
    assert!(host.app.tabs[0].scene.document.get_entity(handle).is_none());
    assert!(!host.app.tabs[0].scene.is_reference_only(handle));
    drop(host);

    // Undoing the erase restores the entity AND its designation: the set
    // mutation rode the scope as an explicit designation channel.
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_some(),
        "undo restored the erased entity"
    );
    assert!(
        host.app.tabs[0].scene.is_reference_only(handle),
        "undo restored the designation together with its entity"
    );
}

#[test]
fn user_paths_refuse_and_plugin_paths_write_reference_only_entities() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    let plain = host.add_entity(reference_test_line(10.0, 5.0, Handle::NULL));

    // Grips: the reference-only entity contributes none; the editable one
    // keeps its own.
    host.set_selection(&[handle]).unwrap();
    host.app.refresh_selected_grips();
    assert!(
        host.app.tabs[0].selected_grips.is_empty(),
        "reference-only entities contribute no grips"
    );
    host.set_selection(&[plain]).unwrap();
    host.app.refresh_selected_grips();
    assert!(!host.app.tabs[0].selected_grips.is_empty());

    // The user in-place update refuses; the user batch skips only the
    // reference-only member; the plugin-owned variants write through.
    assert!(!host.app.tabs[0]
        .scene
        .update_entity(reference_test_line(20.0, 0.0, handle)));
    let applied = host.app.tabs[0].scene.update_entities(vec![
        reference_test_line(20.0, 0.0, handle),
        reference_test_line(20.0, 5.0, plain),
    ]);
    assert_eq!(
        applied, 1,
        "the user batch applies only the editable entity"
    );
    let end_x = |scene: &crate::scene::Scene, h: Handle| match scene.document.get_entity(h) {
        Some(EntityType::Line(line)) => line.end.x,
        _ => panic!("entity {h:?} must be a line"),
    };
    let scene = &host.app.tabs[0].scene;
    assert_eq!(
        end_x(scene, handle),
        10.0,
        "the user paths refused the write"
    );
    assert_eq!(end_x(scene, plain), 20.0);
    // The plugin-owned variant writes through. A real plugin refresh
    // clones the live entity (designation included), edits it, and
    // replaces it — so model that flow instead of a bare synthetic.
    let mut moved = match host.app.tabs[0].scene.document.get_entity(handle) {
        Some(entity) => entity.clone(),
        _ => unreachable!("the entity exists"),
    };
    if let EntityType::Line(line) = &mut moved {
        line.end.x = 30.0;
    }
    assert!(host.app.tabs[0].scene.update_entity_plugin(moved));
    let scene = &host.app.tabs[0].scene;
    assert_eq!(
        end_x(scene, handle),
        30.0,
        "the plugin-owned update writes through"
    );
    assert!(
        scene.is_reference_only(handle),
        "a refresh-style plugin write preserves the designation"
    );

    // User erase skips reference-only entities; the plugin erase path
    // removes them.
    host.app.tabs[0].scene.erase_entities(&[handle]);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_some(),
        "user erase skips reference-only entities"
    );
    host.app.tabs[0].scene.erase_entities_plugin(&[handle]);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_none(),
        "the plugin erase path removes reference-only entities"
    );
    assert!(host.app.tabs[0].scene.document.get_entity(plain).is_some());
}

#[test]
fn select_all_visible_skips_reference_only_entities() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    let plain = host.add_entity(reference_test_line(10.0, 5.0, Handle::NULL));

    assert_eq!(host.app.tabs[0].scene.select_all_visible(), 1);
    let selected = host.app.tabs[0].scene.selected_handles_in_order();
    assert_eq!(selected, vec![plain]);
}

#[test]
fn add_and_update_auto_register_novel_layers_with_real_handles() {
    // A plugin adds/edits an entity naming a layer no LAYER command ever
    // created. The layer must gain a real table entry (non-null handle) so
    // it survives a DWG save instead of collapsing to layer 0 (#252, #67).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    assert!(!host.document().layers.contains("PLUGIN-LAYER"));
    let mut pt = Point::at(document::types::Vector3::new(3.0, 3.0, 0.0));
    pt.common.layer = "PLUGIN-LAYER".to_string();
    host.add_entity(EntityType::Point(pt));

    let layer = host
        .document()
        .layers
        .get("PLUGIN-LAYER")
        .expect("novel layer auto-registered on add_entity");
    assert!(
        !layer.handle.is_null(),
        "auto-registered layer must carry a real handle (#67)"
    );

    // Adding again on the same (now-existing) layer must not duplicate or
    // error — the always-present default "0" is likewise never re-created.
    let count_before = host.document().layers.len();
    host.add_entity(EntityType::Point(Point::new())); // default layer "0"
    let mut pt2 = Point::new();
    pt2.common.layer = "PLUGIN-LAYER".to_string();
    host.add_entity(EntityType::Point(pt2));
    assert_eq!(host.document().layers.len(), count_before);

    // Retargeting an entity to a novel layer via update_entity registers it.
    let h = host.add_entity(EntityType::Point(Point::new()));
    let mut edited = host.document().get_entity(h).unwrap().clone();
    edited.common_mut().layer = "EDIT-LAYER".to_string();
    assert!(host.update_entity(edited));
    assert!(host.document().layers.contains("EDIT-LAYER"));
}

#[test]
fn remove_entity_deletes_and_clears_caches() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        2.0, 2.0, 0.0,
    ))));
    assert!(host.document().get_entity(h).is_some());

    assert!(host.remove_entity(h));
    assert!(host.document().get_entity(h).is_none());
    assert!(!host.app.tabs[0].scene.hatches.contains_key(&h));
    assert!(!host.app.tabs[0].scene.meshes.contains_key(&h));

    // Removing an already-gone handle reports false.
    assert!(!host.remove_entity(h));
}

#[test]
fn update_entity_refreshes_selected_grips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    // A real selection event rebuilt the grip cache the way the app's
    // own paths do.
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(1.0, 1.0, 0.0),
            glam::DVec3::new(3.0, 1.0, 0.0),
            glam::DVec3::new(2.0, 1.0, 0.0),
        ]
    );
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(101.0, 1.0, 0.0);
            line.end = document::types::Vector3::new(103.0, 1.0, 0.0);
        }
        assert!(host.update_entity(edited));
    }
    // The cached grip markers must sit on the refreshed geometry, not at
    // their pre-edit positions.
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(101.0, 1.0, 0.0),
            glam::DVec3::new(103.0, 1.0, 0.0),
            glam::DVec3::new(102.0, 1.0, 0.0),
        ]
    );
}

#[test]
fn update_entities_transaction_refreshes_selected_grips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(0.0, 0.0, 0.0);
        line.end = document::types::Vector3::new(2.0, 0.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(50.0, 0.0, 0.0);
            line.end = document::types::Vector3::new(52.0, 0.0, 0.0);
        }
        host.update_entities_transaction("Move line", vec![edited])
            .unwrap();
    }
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(50.0, 0.0, 0.0),
            glam::DVec3::new(52.0, 0.0, 0.0),
            glam::DVec3::new(51.0, 0.0, 0.0),
        ]
    );
}

#[test]
fn remove_entity_clears_selected_grips_of_removed_entity() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.remove_entity(handle));
    }
    // Erase deselects the handle; the grip cache must drop the markers
    // of the removed entity instead of keeping them until the next
    // selection event.
    assert!(app.tabs[0].selected_grips.is_empty());
}

#[test]
fn update_entity_without_selection_keeps_grip_cache_empty() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        host.add_entity(EntityType::Line(line))
    };
    app.refresh_selected_grips();
    assert!(app.tabs[0].selected_grips.is_empty());
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(10.0, 1.0, 0.0);
        }
        // Still succeeds; nothing was selected, so no panic, no phantom
        // grips.
        assert!(host.update_entity(edited));
    }
    assert!(app.tabs[0].selected_grips.is_empty());
}

#[test]
fn background_tab_update_leaves_active_grips_untouched() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let _handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    // Mark one cached grip so an accidental refresh (which would rebuild
    // it from the active tab's unchanged scene) is observable.
    app.tabs[0].selected_grips[0].world = glam::DVec3::new(999.0, 999.0, 0.0);
    // A second document tab is background while tab 0 stays active.
    app.push_test_document();
    assert_eq!(app.active_tab, 0);
    {
        let mut host = HostSession::new(&mut app, 1);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(100.0, 100.0, 0.0);
        line.end = document::types::Vector3::new(102.0, 100.0, 0.0);
        let background = host.add_entity(EntityType::Line(line));
        host.set_selection(&[background]).unwrap();
        let mut edited = host.document().get_entity(background).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(200.0, 100.0, 0.0);
        }
        assert!(host.update_entity(edited));
    }
    // The background-tab session neither refreshed nor disturbed the
    // active tab's cached grips.
    assert_eq!(
        app.tabs[0].selected_grips[0].world,
        glam::DVec3::new(999.0, 999.0, 0.0)
    );
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
}

/// A plugin command: second point commits a Point and ends.
struct PlacePoint {
    got_first: bool,
}
impl ocs_plugin_api::host::InteractiveCommand for PlacePoint {
    fn prompt(&self) -> String {
        crate::t!("Pick a point").into_owned()
    }
    fn on_point(&mut self, pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        use ocs_plugin_api::host::CommandStep;
        if self.got_first {
            let p =
                document::entities::Point::at(document::types::Vector3::new(pt[0], pt[1], pt[2]));
            CommandStep::CommitAndEnd(document::EntityType::Point(p))
        } else {
            self.got_first = true;
            CommandStep::NeedPoint
        }
    }
}

#[test]
fn plugin_interactive_command_drives_host_flow() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PlacePoint { got_first: false }));
    }
    assert!(app.tabs[0].active_cmd.is_some());
    for pt in [
        glam::DVec3::new(0.0, 0.0, 0.0),
        glam::DVec3::new(5.0, 5.0, 0.0),
    ] {
        let r = app.tabs[0].active_cmd.as_mut().unwrap().on_point(pt);
        let _ = app.apply_cmd_result(r);
    }
    assert_eq!(app.tabs[0].scene.document.entities().count(), 1);
    assert!(
        app.tabs[0].active_cmd.is_none(),
        "command should have ended"
    );
}

/// A plugin command that collects points; with `steps_back` it takes
/// Ctrl+Z itself and drops its last point.
struct CollectPoints {
    points: std::sync::Arc<std::sync::Mutex<Vec<[f64; 3]>>>,
    steps_back: bool,
}
impl ocs_plugin_api::host::InteractiveCommand for CollectPoints {
    fn prompt(&self) -> String {
        format!("Point {}", self.points.lock().unwrap().len() + 1)
    }
    fn on_point(&mut self, pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        self.points.lock().unwrap().push(pt);
        ocs_plugin_api::host::CommandStep::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<ocs_plugin_api::host::CommandStep> {
        if !self.steps_back {
            return None;
        }
        self.points
            .lock()
            .unwrap()
            .pop()
            .map(|_| ocs_plugin_api::host::CommandStep::NeedPoint)
    }
}

/// A drawing with one committed point (one undo step), then a running
/// `CollectPoints` command that has taken two points.
fn app_collecting_two_points(
    steps_back: bool,
) -> (
    OpenCADStudio,
    std::sync::Arc<std::sync::Mutex<Vec<[f64; 3]>>>,
) {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PlacePoint { got_first: false }));
    }
    for pt in [
        glam::DVec3::new(0.0, 0.0, 0.0),
        glam::DVec3::new(5.0, 5.0, 0.0),
    ] {
        let r = app.tabs[0].active_cmd.as_mut().unwrap().on_point(pt);
        let _ = app.apply_cmd_result(r);
    }
    assert_eq!(app.tabs[0].scene.document.entities().count(), 1);
    let points = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(CollectPoints {
            points: points.clone(),
            steps_back,
        }));
    }
    for pt in [
        glam::DVec3::new(1.0, 0.0, 0.0),
        glam::DVec3::new(2.0, 0.0, 0.0),
    ] {
        let r = app.tabs[0].active_cmd.as_mut().unwrap().on_point(pt);
        let _ = app.apply_cmd_result(r);
    }
    assert_eq!(points.lock().unwrap().len(), 2);
    (app, points)
}

/// Ctrl+Z inside a plugin command that collects points drops its last
/// point, the way PLINE pops a vertex, and leaves the drawing alone.
#[test]
fn ctrl_z_steps_a_plugin_command_back_without_undoing_the_drawing() {
    let (mut app, points) = app_collecting_two_points(true);
    let _ = app.update(crate::app::Message::Undo);
    assert_eq!(*points.lock().unwrap(), vec![[1.0, 0.0, 0.0]]);
    assert_eq!(
        app.tabs[0].scene.document.entities().count(),
        1,
        "the committed point survives"
    );
    let cmd = app.tabs[0].active_cmd.as_ref().expect("still running");
    assert_eq!(cmd.prompt(), "Point 2");
}

/// A plugin command that does not take the step back keeps the document
/// undo it gets today.
#[test]
fn ctrl_z_runs_the_document_undo_when_the_plugin_command_declines() {
    let (mut app, points) = app_collecting_two_points(false);
    let _ = app.update(crate::app::Message::Undo);
    assert_eq!(points.lock().unwrap().len(), 2);
    assert_eq!(app.tabs[0].scene.document.entities().count(), 0);
}

/// A plugin command that previews a rubber-band line to the cursor.
struct PreviewLine {
    start: [f64; 3],
    color: Option<[f32; 4]>,
}
impl ocs_plugin_api::host::InteractiveCommand for PreviewLine {
    fn prompt(&self) -> String {
        "Pick endpoint".to_string()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        ocs_plugin_api::host::CommandStep::Done
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<ocs_plugin_api::host::PreviewWire> {
        vec![ocs_plugin_api::host::PreviewWire::line(
            self.start, pt, self.color,
        )]
    }
}

/// A plugin command that previews an analytical circle.
struct PreviewCircle {
    center: [f64; 3],
    color: Option<[f32; 4]>,
}
impl ocs_plugin_api::host::InteractiveCommand for PreviewCircle {
    fn prompt(&self) -> String {
        "Pick radius".to_string()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        ocs_plugin_api::host::CommandStep::Done
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<ocs_plugin_api::host::PreviewWire> {
        let dx = pt[0] - self.center[0];
        let dy = pt[1] - self.center[1];
        let r = (dx * dx + dy * dy).sqrt();
        vec![ocs_plugin_api::host::PreviewWire::circle(
            self.center,
            r,
            self.color,
        )]
    }
}

/// A plugin command that previews an analytical arc.
struct PreviewArc {
    center: [f64; 3],
    color: Option<[f32; 4]>,
}
impl ocs_plugin_api::host::InteractiveCommand for PreviewArc {
    fn prompt(&self) -> String {
        "Pick endpoint".to_string()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        ocs_plugin_api::host::CommandStep::Done
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<ocs_plugin_api::host::PreviewWire> {
        let dx = pt[0] - self.center[0];
        let dy = pt[1] - self.center[1];
        let r = (dx * dx + dy * dy).sqrt();
        vec![ocs_plugin_api::host::PreviewWire::arc(
            self.center,
            r,
            0.0,
            std::f64::consts::PI,
            self.color,
        )]
    }
}

#[test]
fn plugin_cursor_move_preview_wires() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;

    // 1. Polyline default color (None -> CYAN)
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PreviewLine {
            start: [0.0, 0.0, 0.0],
            color: None,
        }));
    }
    assert!(app.tabs[0].active_cmd.is_some());
    let cmd = app.tabs[0].active_cmd.as_mut().unwrap();
    let wires = cmd.on_preview_wires(glam::DVec3::new(10.0, 20.0, 0.0));
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].name, "rubber_band");
    assert_eq!(
        wires[0].color,
        crate::scene::model::wire_model::WireModel::CYAN
    );

    // 2. Polyline custom color ([1.0, 0.0, 0.0, 1.0])
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PreviewLine {
            start: [0.0, 0.0, 0.0],
            color: Some([1.0, 0.0, 0.0, 1.0]),
        }));
    }
    let cmd2 = app.tabs[0].active_cmd.as_mut().unwrap();
    let wires2 = cmd2.on_preview_wires(glam::DVec3::new(10.0, 20.0, 0.0));
    assert_eq!(wires2.len(), 1);
    assert_eq!(wires2[0].color, [1.0, 0.0, 0.0, 1.0]);

    // 3. Analytical Circle with custom color
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PreviewCircle {
            center: [5.0, 5.0, 0.0],
            color: Some([0.0, 1.0, 0.0, 1.0]),
        }));
    }
    let cmd3 = app.tabs[0].active_cmd.as_mut().unwrap();
    let circle_wires = cmd3.on_preview_wires(glam::DVec3::new(15.0, 5.0, 0.0));
    assert_eq!(circle_wires.len(), 1);
    assert_eq!(circle_wires[0].points.len(), 2);
    assert_eq!(circle_wires[0].color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(circle_wires[0].tangent_geoms.len(), 1);
    assert!(matches!(
        circle_wires[0].tangent_geoms[0],
        crate::scene::model::wire_model::TangentGeom::PlanarCircle { radius, .. } if (radius - 10.0).abs() < 1e-6
    ));
    let instances =
        crate::scene::pipeline::circle_gpu::extract_circle_instances(&circle_wires[0], 0.0);
    assert!(instances.is_some());
    assert_eq!(instances.unwrap().len(), 1);

    // 4. Analytical Arc
    {
        let mut host = HostSession::new(&mut app, 0);
        host.start_interactive(Box::new(PreviewArc {
            center: [0.0, 0.0, 0.0],
            color: None,
        }));
    }
    let cmd4 = app.tabs[0].active_cmd.as_mut().unwrap();
    let arc_wires = cmd4.on_preview_wires(glam::DVec3::new(10.0, 0.0, 0.0));
    assert_eq!(arc_wires.len(), 1);
    assert_eq!(arc_wires[0].points.len(), 2);
    assert_eq!(arc_wires[0].tangent_geoms.len(), 1);
    assert!(matches!(
        arc_wires[0].tangent_geoms[0],
        crate::scene::model::wire_model::TangentGeom::Arc { radius, .. } if (radius - 10.0).abs() < 1e-6
    ));
    let arc_instances =
        crate::scene::pipeline::circle_gpu::extract_circle_instances(&arc_wires[0], 0.0);
    assert!(arc_instances.is_some());
    assert_eq!(arc_instances.unwrap().len(), 1);
}

/// A plugin command that picks an existing object, then marks it.
struct PickThenMark;
impl ocs_plugin_api::host::InteractiveCommand for PickThenMark {
    fn prompt(&self) -> String {
        crate::t!("Pick an object").into_owned()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
        ocs_plugin_api::host::CommandStep::Cancel
    }
    fn needs_object_pick(&self) -> bool {
        true
    }
    fn on_object_pick(
        &mut self,
        _handle: document::Handle,
        pt: [f64; 3],
    ) -> ocs_plugin_api::host::CommandStep {
        let p = document::entities::Point::at(document::types::Vector3::new(pt[0], pt[1], pt[2]));
        ocs_plugin_api::host::CommandStep::CommitAndEnd(document::EntityType::Point(p))
    }
}

#[test]
fn plugin_object_pick_routes_to_command() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let target = {
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(document::EntityType::Point(document::entities::Point::at(
            document::types::Vector3::new(3.0, 4.0, 0.0),
        )));
        host.start_interactive(Box::new(PickThenMark));
        h
    };
    // The command requested an entity pick, not a free point.
    assert!(app.tabs[0].active_cmd.as_ref().unwrap().needs_entity_pick());
    let r = app.tabs[0]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_entity_pick(target, glam::DVec3::new(3.0, 4.0, 0.0));
    let _ = app.apply_cmd_result(r);
    // Original point + the mark the command committed.
    assert_eq!(app.tabs[0].scene.document.entities().count(), 2);
}

#[test]
fn host_document_reader_sees_entities() {
    use ocs_plugin_api::host::ReaderEntityKind;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    host.add_entity(document::EntityType::Point(document::entities::Point::at(
        document::types::Vector3::new(7.0, 8.0, 0.0),
    )));
    let reader = host.document_reader();
    assert_eq!(reader.entity_count(), 1);
    let mut kinds = Vec::new();
    reader.for_each_entity(&mut |e| kinds.push(e.kind));
    assert_eq!(kinds, vec![ReaderEntityKind::Point]);
}

#[test]
fn host_document_view_publish_and_read_shared() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let info = host.document_view().unwrap();
    let reader =
        ocs_plugin_api::shm::SharedDocumentReader::<ocs_plugin_api::shm::DocumentViewData>::open(
            std::path::Path::new(&info.path),
        )
        .unwrap();
    assert_eq!(reader.entity_count(), 0);

    host.add_entity(document::EntityType::Point(document::entities::Point::at(
        document::types::Vector3::new(1.0, 2.0, 0.0),
    )));

    assert_eq!(reader.entity_count(), 1);
}

/// Read an entity handle from the live document, write XDATA for that
/// handle, read it back, and remove it.
#[test]
fn document_reader_to_xdata_roundtrip() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        7.0, 8.0, 0.0,
    ))));

    {
        let reader = host.document_reader();
        assert_eq!(reader.entity_count(), 1);
        let mut handles = Vec::new();
        reader.for_each_entity(&mut |e| handles.push(e.handle));
        assert_eq!(handles, vec![h]);
    }

    let mut rec = ExtendedDataRecord::new("ROUNDTRIP");
    rec.add_value(XDataValue::String("from-reader".to_string()));
    assert!(host.write_record(h, rec));

    let got = host.read_record(h, "ROUNDTRIP").expect("record missing");
    assert_eq!(got.values.len(), 1);
    assert!(matches!(got.values[0], XDataValue::String(ref s) if s == "from-reader"));

    assert!(host.remove_record(h, "ROUNDTRIP"));
    assert!(host.read_record(h, "ROUNDTRIP").is_none());
}

/// Publish a shared document view, read the entity handle from shared
/// memory, then write and read-back XDATA through the normal HostApi RPCs.
#[test]
fn shared_document_view_read_then_write_xdata_roundtrip() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let info = host.document_view().unwrap();
    let reader =
        ocs_plugin_api::shm::SharedDocumentReader::<ocs_plugin_api::shm::DocumentViewData>::open(
            std::path::Path::new(&info.path),
        )
        .unwrap();

    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        1.0, 2.0, 0.0,
    ))));
    assert_eq!(reader.entity_count(), 1);

    let mut handles = Vec::new();
    reader.for_each_entity(&mut |e| handles.push(e.handle));
    assert_eq!(handles, vec![h]);

    let mut rec = ExtendedDataRecord::new("SHM_ROUNDTRIP");
    rec.add_value(XDataValue::Integer32(123));
    assert!(host.write_record(h, rec));

    let got = host
        .read_record(h, "SHM_ROUNDTRIP")
        .expect("record missing");
    assert_eq!(got.values.len(), 1);
    assert!(matches!(got.values[0], XDataValue::Integer32(123)));
}

#[test]
fn test_plugin_add_layer_with_defaults_and_modify() {
    use ocs_plugin_api::host::LayerConfig;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    // 1. Add layer with minimal config (only name specified)
    let config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        ..Default::default()
    };
    let handle = host.add_layer(config).expect("should create layer");
    assert_ne!(handle, document::Handle::NULL);

    // Verify defaults were applied
    let layer = host
        .document()
        .layers
        .get("ELECTRICAL")
        .expect("layer should exist");
    assert_eq!(layer.color, document::types::Color::Index(7));
    assert_eq!(layer.line_type, "Continuous");
    assert_eq!(layer.line_weight, document::types::LineWeight::ByLayer);
    assert!(!layer.flags.off);
    assert!(!layer.flags.frozen);
    assert!(!layer.flags.locked);
    assert!(layer.is_plottable);

    // 2. Duplicate add_layer should be rejected (return None)
    let dup_config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        color: Some(document::types::Color::Index(1)),
        ..Default::default()
    };
    assert!(
        host.add_layer(dup_config).is_none(),
        "duplicate layer should return None"
    );

    // 3. Modify only color and locked; other properties should remain untouched
    let mod_config = LayerConfig {
        name: "ELECTRICAL".to_string(),
        color: Some(document::types::Color::Index(1)),
        locked: Some(true),
        ..Default::default()
    };
    assert!(host.modify_layer(mod_config));

    let updated = host
        .document()
        .layers
        .get("ELECTRICAL")
        .expect("layer should exist");
    assert_eq!(updated.color, document::types::Color::Index(1)); // Modified to red
    assert!(updated.flags.locked); // Modified to locked
    assert_eq!(updated.line_type, "Continuous"); // Kept as-is
    assert_eq!(updated.line_weight, document::types::LineWeight::ByLayer); // Kept as-is
    assert!(!updated.flags.off); // Kept as-is

    // 4. Modify nonexistent layer returns false
    let non_existent = LayerConfig {
        name: "DOES_NOT_EXIST".to_string(),
        color: Some(document::types::Color::Index(2)),
        ..Default::default()
    };
    assert!(!host.modify_layer(non_existent));
}

/// The catalog linetypes a new drawing leaves out: the ones that draw
/// shapes only this application defines.
const LEFT_OUT_OF_A_NEW_DRAWING: [&str; 5] = ["POINT1", "POINT2", "PLANT", "DRAINAGE", "DRAINAGE2"];

fn layer_on(name: &str, linetype: &str) -> ocs_plugin_api::host::LayerConfig {
    ocs_plugin_api::host::LayerConfig {
        name: name.to_string(),
        linetype: Some(linetype.to_string()),
        ..Default::default()
    }
}

/// A plugin that names a catalog linetype the drawing lacks for a new
/// layer gets it loaded, as a pick in the layer list does.
#[test]
fn a_plugin_layer_is_created_on_a_catalog_linetype_the_drawing_lacks() {
    for linetype in LEFT_OUT_OF_A_NEW_DRAWING {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            host.document().line_types.get(linetype).is_none(),
            "{linetype}: a new drawing leaves it out"
        );

        let created = host.add_layer(layer_on("SITE", &linetype.to_lowercase()));

        assert!(created.is_some(), "{linetype}: the layer is created");
        assert_eq!(
            host.document().layers.get("SITE").unwrap().line_type,
            linetype
        );
        assert!(
            host.document()
                .line_types
                .get(linetype)
                .is_some_and(|loaded| loaded.is_complex()),
            "{linetype}: loaded with its shapes"
        );
    }
}

#[test]
fn a_plugin_moves_a_layer_to_a_catalog_linetype_the_drawing_lacks() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    host.add_layer(ocs_plugin_api::host::LayerConfig {
        name: "SITE".to_string(),
        ..Default::default()
    })
    .expect("a layer on Continuous");

    assert!(host.modify_layer(layer_on("SITE", "POINT2")));

    assert_eq!(
        host.document().layers.get("SITE").unwrap().line_type,
        "POINT2"
    );
    assert!(host.document().line_types.get("POINT2").is_some());
}

#[test]
fn the_layer_table_route_takes_a_catalog_linetype_the_drawing_lacks() {
    use ocs_plugin_api::host::TableOperation;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    let created = host.table_operation(TableOperation::LayerCreate {
        config: layer_on("DRAINS", "DRAINAGE"),
    });
    assert!(created.is_ok(), "create: {created:?}");
    assert_eq!(
        host.document().layers.get("DRAINS").unwrap().line_type,
        "DRAINAGE"
    );

    let modified = host.table_operation(TableOperation::LayerModify {
        config: layer_on("DRAINS", "PLANT"),
    });
    assert!(modified.is_ok(), "modify: {modified:?}");
    assert_eq!(
        host.document().layers.get("DRAINS").unwrap().line_type,
        "PLANT"
    );
}

/// Naming a linetype for a layer loads that one alone: the other catalog
/// linetypes the drawing lacks stay out of it.
#[test]
fn a_plugin_layer_loads_only_the_linetype_it_names() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    for purged in ["DASHED", "CENTER"] {
        assert!(host.document_mut().line_types.remove(purged).is_some());
    }
    let before = host.document().line_types.iter().count();

    assert!(host.add_layer(layer_on("AXES", "Center")).is_some());

    assert_eq!(
        host.document().layers.get("AXES").unwrap().line_type,
        "CENTER"
    );
    assert!(
        host.document().line_types.get("DASHED").is_none(),
        "a purged linetype nobody named stays purged"
    );
    assert_eq!(host.document().line_types.iter().count(), before + 1);
}

#[test]
fn a_plugin_layer_is_refused_a_linetype_nobody_defines() {
    use ocs_plugin_api::host::TableOperation;
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let before = host.document().line_types.iter().count();

    assert!(host.add_layer(layer_on("SITE", "NO_SUCH")).is_none());
    let refusal = host
        .table_operation(TableOperation::LayerCreate {
            config: layer_on("SITE", "NO_SUCH"),
        })
        .unwrap_err();

    assert!(refusal.contains("does not exist"), "{refusal}");
    assert!(host.document().layers.get("SITE").is_none());
    assert_eq!(host.document().line_types.iter().count(), before);
}

#[test]
fn test_entity_creation_with_novel_layer_refreshes_gui() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;

    // Verify DYNAMIC_PLUGIN_LINES does not exist initially
    assert!(!app.tabs[0]
        .scene
        .document
        .layers
        .contains("DYNAMIC_PLUGIN_LINES"));
    assert!(!app
        .ribbon
        .layer_infos
        .iter()
        .any(|l| l.name == "DYNAMIC_PLUGIN_LINES"));

    // 1. Add entity via HostSession (simulating plugin add_entity / add_line)
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::from_coords(0.0, 0.0, 0.0, 10.0, 10.0, 0.0);
        line.common.layer = "DYNAMIC_PLUGIN_LINES".to_string();
        let handle = host.add_entity(document::EntityType::Line(line));
        assert_ne!(handle, document::Handle::NULL);
    }

    // Verify layer is created in document AND refreshed in ribbon + layer panel
    assert!(app.tabs[0]
        .scene
        .document
        .layers
        .contains("DYNAMIC_PLUGIN_LINES"));
    assert!(
        app.ribbon
            .layer_infos
            .iter()
            .any(|l| l.name == "DYNAMIC_PLUGIN_LINES"),
        "Ribbon layers should contain DYNAMIC_PLUGIN_LINES after add_entity"
    );
    assert!(
        app.tabs[0]
            .layers
            .layers
            .iter()
            .any(|l| l.name == "DYNAMIC_PLUGIN_LINES"),
        "Layer panel should contain DYNAMIC_PLUGIN_LINES after add_entity"
    );

    // 2. Commit entity via command driver (simulating interactive tool / palette commit)
    let mut line2 = document::entities::Line::from_coords(10.0, 10.0, 0.0, 20.0, 20.0, 0.0);
    line2.common.layer = "SURVEY_TRAVERSE".to_string();
    let _ = app.commit_entity_handle_preserve_layer(document::EntityType::Line(line2));

    // Verify SURVEY_TRAVERSE layer is created and refreshed in ribbon + layer panel
    assert!(app.tabs[0]
        .scene
        .document
        .layers
        .contains("SURVEY_TRAVERSE"));
    assert!(
        app.ribbon
            .layer_infos
            .iter()
            .any(|l| l.name == "SURVEY_TRAVERSE"),
        "Ribbon layers should contain SURVEY_TRAVERSE after commit_entity"
    );
    assert!(
        app.tabs[0]
            .layers
            .layers
            .iter()
            .any(|l| l.name == "SURVEY_TRAVERSE"),
        "Layer panel should contain SURVEY_TRAVERSE after commit_entity"
    );
}

#[test]
fn test_plugin_execute_command() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    // 1. Single-line command with newline finishes command
    assert!(host.execute_command("LINE 0,0 10,10\n"));
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert_eq!(host.document().entities().count(), 1);

    // 2. Streamed command without newline leaves tool active
    assert!(host.execute_command("LINE 10,10"));
    assert!(host.app.tabs[0].active_cmd.is_some());
    // Feed next point without newline
    assert!(host.execute_command("20,20"));
    assert!(host.app.tabs[0].active_cmd.is_some());
    // Send explicit ENTER
    assert!(host.execute_command("ENTER"));
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert_eq!(host.document().entities().count(), 2);

    // 3. Command with PAUSE buffers remaining tokens until point is provided
    assert!(host.execute_command("LINE 20,20 PAUSE ENTER"));
    assert!(host.app.tabs[0].active_cmd.is_some());
    assert_eq!(
        host.app.tabs[0].pending_pause_tokens,
        Some(vec!["ENTER".to_string()])
    );
    // User clicks second point in viewport (which calls on_point + apply_cmd_result)
    let r = host.app.tabs[0]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(glam::DVec3::new(30.0, 30.0, 0.0));
    let _ = host.app.apply_cmd_result(r);
    // Draining in apply_cmd_result should have executed ENTER and completed LINE
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert_eq!(host.document().entities().count(), 3);

    // 4. Command with PAUSE and trailing \n
    assert!(host.execute_command("LINE 30,30 PAUSE\n"));
    assert!(host.app.tabs[0].active_cmd.is_some());
    assert_eq!(
        host.app.tabs[0].pending_pause_tokens,
        Some(vec!["ENTER".to_string()])
    );
    // Escape cancels paused command cleanly
    assert!(host.execute_command("ESC"));
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert_eq!(host.app.tabs[0].pending_pause_tokens, None);

    // 5. CLAYER command
    host.add_layer(ocs_plugin_api::host::LayerConfig {
        name: "TEST".to_string(),
        ..Default::default()
    });
    assert!(host.execute_command("CLAYER TEST\n"));
    assert_eq!(host.document().header.current_layer_name, "TEST");

    // 6. Non-typical command: POINT
    assert!(host.execute_command("POINT 50,50\n"));
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert_eq!(host.document().entities().count(), 4);

    // 7. Non-typical command: DONUT
    assert!(host.execute_command("DONUT 10 30 100,100 \n"));
    assert!(host.app.tabs[0].active_cmd.is_none());
    assert!(host.document().entities().count() >= 5);

    // 8. Non-typical command: ELLIPSE
    assert!(host.execute_command("ELLIPSE 0,0 80,0 30\n"));
    assert!(host.app.tabs[0].active_cmd.is_none());

    // 9. Non-typical inline commands: UCS
    assert!(host.execute_command("UCS ORIGIN 50,50,0\n"));
    assert!(host.app.tabs[0].active_ucs.is_some());
    assert!(host.execute_command("UCS W\n"));
    assert!(host.app.tabs[0].active_ucs.is_none());

    // 10. Non-typical inline commands: SETVAR
    assert!(host.execute_command("SETVAR PDMODE 35\n"));

    // 11. Viewport visual style via drive_headless_task
    assert!(host.execute_command("VSCURRENT FLATSHADED\n"));
    assert_eq!(
        host.app.tabs[0].render_mode,
        ocs_plugin_api::host::document::entities::ViewportRenderMode::FlatShaded
    );

    // 12. Drafting aids toggle via drive_headless_task
    let initial_grid = host.app.show_grid;
    assert!(host.execute_command("GRID\n"));
    assert_eq!(host.app.show_grid, !initial_grid);
}

/// The session tests' profile chain: one ramp-declared parabola
/// (slope 0 -> 0.1 over the run), expressed against the API v14 wire
/// types (rebuilt here so the session tests stay independent of the
/// engine module's own fixtures).
fn staging_profile_chain() -> ocs_plugin_api::host::ParametricChain {
    ocs_plugin_api::host::ParametricChain {
        plane: ocs_plugin_api::host::ParametricPlane::Profile,
        segments: vec![ocs_plugin_api::host::ParametricSegment {
            def: ocs_plugin_api::host::CurveSegmentDef {
                length: 80.0,
                ramp: ocs_plugin_api::host::Ramp::Polynomial {
                    degree: 1,
                    coeffs: [0.0, 0.1, 0.0, 0.0],
                },
                cross_slope: None,
            },
            start_point: [0.0, 100.0],
            start_direction: 0.0,
        }],
        layer: "PLUGIN_PROFILE".to_string(),
    }
}

fn staging_plan_chain() -> ocs_plugin_api::host::ParametricChain {
    ocs_plugin_api::host::ParametricChain {
        plane: ocs_plugin_api::host::ParametricPlane::Plan,
        segments: vec![ocs_plugin_api::host::ParametricSegment {
            def: ocs_plugin_api::host::CurveSegmentDef {
                // Curvature ramp 0 -> -1/300 over the arc length (the
                // record's straight-to-circle spiral restated as data).
                length: 60.0,
                ramp: ocs_plugin_api::host::Ramp::Polynomial {
                    degree: 1,
                    coeffs: [0.0, -1.0 / 300.0, 0.0, 0.0],
                },
                cross_slope: None,
            },
            start_point: [0.0, 0.0],
            start_direction: 0.0,
        }],
        layer: "PLUGIN_DISPLAY".to_string(),
    }
}

#[test]
fn parametric_staging_is_reference_only_in_one_undo_step() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let chain = staging_plan_chain();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        handles = host
            .stage_parametric_chain("align-1", &chain, 1e-3)
            .expect("the demo plan chain stages");
        assert!(!handles.is_empty());
        for handle in &handles {
            assert!(
                host.app.tabs[0].scene.is_reference_only(*handle),
                "staged display data must be reference-only"
            );
            assert!(host.document().get_entity(*handle).is_some());
        }
    }
    app.finish_pending_history(0);
    // The whole staging (add + designations) = ONE undo step.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    for handle in &handles {
        assert!(
            app.tabs[0].scene.document.get_entity(*handle).is_none(),
            "undo removed the staged display set (byte-exact restore)"
        );
        assert!(
            !app.tabs[0].scene.is_reference_only(*handle),
            "the designation rolled back with the entities"
        );
    }
    // Redo re-applies the whole step.
    app.redo_steps(1);
    for handle in &handles {
        assert!(app.tabs[0].scene.document.get_entity(*handle).is_some());
        assert!(app.tabs[0].scene.is_reference_only(*handle));
    }
}

#[test]
fn parametric_staging_refusals_change_nothing_and_record_no_steps() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut broken = staging_plan_chain();
    let mut host = HostSession::new(&mut app, 0);
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;
    // Broken tolerances refuse before anything changes.
    assert!(host.stage_parametric_chain("bad", &broken, 0.0).is_err());
    assert!(host.stage_parametric_chain("bad", &broken, -1.0).is_err());
    // Broken declared data refuses too.
    broken.segments[0].def.length = -1.0;
    assert!(host.stage_parametric_chain("bad", &broken, 1e-3).is_err());
    assert_eq!(host.document().entities().count(), 0);
    assert_eq!(host.app.tabs[0].scene.geometry_epoch, epoch_before);
}

// ── API v15: quad cursor menu panels ─────────────────────────────────────

fn quad_panel(id: &str, controls: &[&str]) -> ocs_plugin_api::dialog::QuadPanelDef {
    use ocs_plugin_api::dialog::{QuadActionDef, QuadItemDef};
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    let mut def = ocs_plugin_api::dialog::QuadPanelDef::new(id, id.to_ascii_uppercase());
    for control in controls {
        def = def.with_item(QuadItemDef {
            control_id: (*control).to_string(),
            label: control.to_string(),
            icon: OwnedIconKind::Glyph(control.to_string()),
            action: QuadActionDef::Callback,
        });
    }
    def
}

#[test]
fn quad_panels_register_replace_per_owner_and_reject_foreign() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    // The sink marks the plugin loaded+alive for the visibility predicate
    // (the class-owner tests' idiom; thread-local).
    crate::plugin::grip_points::install_in_process_sink("PLUGIN", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_RIVAL", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });

    // Register (owner PLUGIN) and map to the quad model feed.
    let results = {
        let mut host = HostSession::new(&mut app, 0);
        host.register_quad_panels("PLUGIN", &[quad_panel("qtest.align", &["flip"])])
    };
    assert_eq!(results, vec![true]);
    {
        // The feed probe is a read-only snapshot: read it before the
        // session borrows `app` mutably (the store is process-global).
        let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
        let mut host = HostSession::new(&mut app, 0);
        let mine = feed
            .iter()
            .find(|panel| panel.id == "plugin:PLUGIN:qtest.align")
            .expect("the registered panel joins the model feed");
        assert_eq!(mine.label, "QTEST.ALIGN");
        assert_eq!(mine.items[0].command, "quadplugin:PLUGIN:qtest.align:flip");

        // Same owner: per-panel replace keeps the owner's slot (the new
        // declaration's item count wins).
        let results =
            host.register_quad_panels("PLUGIN", &[quad_panel("qtest.align", &["flip", "other"])]);
        assert_eq!(results, vec![true]);
        drop(host);
        let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
        assert_eq!(
            feed.iter()
                .find(|panel| panel.id == "plugin:PLUGIN:qtest.align")
                .expect("still registered")
                .items
                .len(),
            2
        );

        // A foreign owner cannot replace another plugin's panel id.
        let mut host = HostSession::new(&mut app, 0);
        let results =
            host.register_quad_panels("PLUGIN_RIVAL", &[quad_panel("qtest.align", &["x"])]);
        assert_eq!(results, vec![false]);

        // Unregister: only the owner's ids are removed (the ownership
        // law runs both ways — the rival's own id refuses the owner).
        assert_eq!(
            host.unregister_quad_panels("PLUGIN", &["qtest.align"]),
            vec![true]
        );
        assert_eq!(
            host.unregister_quad_panels("PLUGIN", &["qtest.align"]),
            vec![false]
        );
    }
    let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
    assert!(
        feed.iter()
            .all(|panel| panel.id != "plugin:PLUGIN:qtest.align"),
        "the unregistered panel drops out of the feed"
    );
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN");
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN_RIVAL");
}

#[test]
fn quad_panels_icon_budget_degrades_to_glyph() {
    use crate::ui::popup::quad_menu::QuadIcon;
    use ocs_plugin_api::dialog::QuadItemDef;
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    // The §4 conversion: a good svg keeps its bytes in the UI model; an
    // oversized one degrades to the glyph tile without refusing the
    // panel; a declared glyph passes through.
    let good = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        .as_bytes()
        .to_vec();
    let oversized = vec![b'x'; crate::plugin::quad::MAX_QUAD_ICON_BYTES + 1];
    let def = ocs_plugin_api::dialog::QuadPanelDef::new("qtest.icons", "Icons")
        .with_item(QuadItemDef {
            control_id: "good".to_string(),
            label: "Good".to_string(),
            icon: OwnedIconKind::Svg(good),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        })
        .with_item(QuadItemDef {
            control_id: "big".to_string(),
            label: "Big".to_string(),
            icon: OwnedIconKind::Svg(oversized),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        })
        .with_item(QuadItemDef {
            control_id: "glyph".to_string(),
            label: "Glyph".to_string(),
            icon: OwnedIconKind::Glyph("G".to_string()),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        });
    // The conversion is a pure mapping — exercise it directly.
    let icons: Vec<QuadIcon> = def
        .items
        .into_iter()
        .map(|item| quad_item_icon(item.label, item.icon))
        .collect();
    assert!(matches!(icons[0], QuadIcon::SvgOwned(_)));
    assert!(matches!(&icons[1], QuadIcon::Glyph(text) if text == "B"));
    assert!(matches!(&icons[2], QuadIcon::Glyph(text) if text == "G"));
}

#[test]
fn quad_feed_has_no_v14_auto_tab_per_plugin() {
    // User decision: the v14 auto "Plugins" tab is gone. A visible plugin
    // with plain commands contributes no quad panel at all — only
    // explicitly registered quad panels do (its commands stay reachable
    // through the command line / auto-complete).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    // The sink marks the plugin loaded+alive for the visibility predicate
    // (the register tests' idiom): with the removed synthesis this is
    // exactly the case that used to grow an auto tab.
    // An id of its own: the quad store is process-wide, and other tests
    // register panels for "PLUGIN" while this one runs.
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_NO_QUAD", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
    assert_eq!(
        feed.iter()
            .filter(|panel| panel.id.ends_with(":auto"))
            .count(),
        0,
        "no auto tab is synthesized for a plain-command plugin"
    );
    assert_eq!(
        feed.len(),
        0,
        "no registration, no contribution: the feed stays empty"
    );
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN_NO_QUAD");
}

// ── API v14: plugin class objects ──────────────────────────────────────

/// Becomes the owner plugin in these tests: the in-process sink installs
/// the thread-local liveness the visibility predicate consults in tests.
fn install_class_owner() {
    crate::plugin::grip_points::install_in_process_sink("PLUGIN", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
}

fn traverse_class() -> ocs_plugin_api::host::PluginClassDeclaration {
    ocs_plugin_api::host::PluginClassDeclaration {
        class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
        schema_version: 4,
        grip_schema: vec![ocs_plugin_api::host::PluginGripSchemaEntry {
            grip_id: 1,
            param_name: "peak_gradient".to_string(),
            drives: None,
        }],
        parameters: vec![],
    }
}

fn traverse_instance() -> ocs_plugin_api::host::PluginClassInstance {
    ocs_plugin_api::host::PluginClassInstance {
        class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
        schema_version: 4,
        design: ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
            staging_profile_chain(),
        ),
        tolerance: 1e-3,
        layer: "PLUGIN_PROFILE".to_string(),
        design_record: "PLUGIN_ALIGN_DISPLAY".to_string(),
        selectable_custom: false,
    }
}

#[test]
fn boot_time_declaration_is_attributed_to_the_declared_package_id() {
    // HOST-FIXES-TODO 1: the spawn bootstrap registers entity classes and
    // quad panels while GetManifest is still in flight — the registry
    // entry does not exist yet, and the manifest window used to serve
    // with an empty identity ("plugin \"\" is not a loaded, alive
    // plugin"). The declaration law now treats the IN-FLIGHT BOOT as the
    // live, authenticated transport: the package's declared id rides the
    // connection before the manifest and the manifest must confirm it,
    // while the empty identity still refuses.
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    crate::plugin::external::with_fresh_installed_manager(|| {
        // An empty identity is never attributable, boot window or not.
        let empty_err = host
            .register_plugin_class("", &traverse_class())
            .unwrap_err();
        assert!(
            empty_err.contains("not a loaded, alive plugin"),
            "got: {empty_err}"
        );

        // The declared package id in its boot window passes the law.
        crate::plugin::external::with_manager(|m| m.begin_boot("opencad.example"));
        assert!(host
            .register_plugin_class("opencad.example", &traverse_class())
            .is_ok());

        // After the boot window closes (and without a registry entry at
        // that id) the law refuses again: a different class key under the
        // same owner must not register.
        crate::plugin::external::with_manager(|m| m.end_boot("opencad.example"));
        let mut unrelated = traverse_class();
        unrelated.class_id = "OTHER_KEY".to_string();
        let closed_err = host
            .register_plugin_class("opencad.example", &unrelated)
            .unwrap_err();
        assert!(
            closed_err.contains("not a loaded, alive plugin"),
            "got: {closed_err}"
        );

        // The empty identity law survives the whole exercise.
        let still_empty = host
            .register_plugin_class("", &traverse_class())
            .unwrap_err();
        assert!(still_empty.contains("not a loaded, alive plugin"));
    });
}

#[test]
fn plugin_class_registration_is_owned_idempotent_and_refusing() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let not_owner = host
        .register_plugin_class("ghost", &traverse_class())
        .unwrap_err();
    assert!(
        not_owner.contains("not a loaded, alive plugin"),
        "got: {not_owner}"
    );
    install_class_owner();
    // Register, then re-register identical: idempotent, still one entry.
    assert!(host
        .register_plugin_class("PLUGIN", &traverse_class())
        .is_ok());
    assert!(host
        .register_plugin_class("PLUGIN", &traverse_class())
        .is_ok());
    assert_eq!(host.app.tabs[0].plugin_classes.total(), 1);
    // A different schema refuses without touching the live class.
    let mut bumped = traverse_class();
    bumped.schema_version = 5;
    let err = host.register_plugin_class("PLUGIN", &bumped).unwrap_err();
    assert!(err.contains("different schema"), "got: {err}");
    assert_eq!(
        host.app.tabs[0]
            .plugin_classes
            .find("PLUGIN", "PLUGIN_ALIGN_DISPLAY")
            .expect("live entry")
            .schema_version,
        4
    );
    // Malformed declarations refuse: empty identity, unnamed grip
    // parameter, duplicate grip ids.
    let mut empty = traverse_class();
    empty.class_id = String::new();
    assert!(host.register_plugin_class("PLUGIN", &empty).is_err());
    let mut unnamed = traverse_class();
    unnamed.grip_schema[0].param_name = String::new();
    assert!(host.register_plugin_class("PLUGIN", &unnamed).is_err());
    let mut dup = traverse_class();
    dup.grip_schema
        .push(ocs_plugin_api::host::PluginGripSchemaEntry {
            grip_id: 1,
            param_name: "other".to_string(),
            drives: None,
        });
    assert!(host.register_plugin_class("PLUGIN", &dup).is_err());
    assert_eq!(host.app.tabs[0].plugin_classes.total(), 1);
}

#[test]
fn plugin_class_registry_survives_across_request_sessions() {
    // The registry is TAB state, not session state: HostSession lives one
    // request; the class declaration must outlive the dropping session
    // (this is the regression for the per-session-registry defect).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host
            .register_plugin_class("PLUGIN", &traverse_class())
            .is_ok());
    }
    // Session B: a DIFFERENT HostSession over the same tab (the IPC
    // server's per-request reality) stages the instance.
    let mut host = HostSession::new(&mut app, 0);
    let handles = host
        .stage_plugin_class_instance("PLUGIN", &traverse_instance())
        .expect("the class outlives its declaring session");
    assert!(!handles.is_empty());
    assert!(host.app.tabs[0].scene.is_reference_only(handles[0]));
    assert!(!host.app.tabs[0].scene.native_edit_disabled(handles[0]));
    let stamp = host
        .read_record(handles[0], "PLUGIN_ALIGN_DISPLAY")
        .expect("legacy owner stamp");
    assert_eq!(stamp.values.len(), 2, "unflagged staging is unchanged");
}

#[test]
fn selectable_custom_instances_are_selectable_but_native_edit_disabled() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(host
        .register_plugin_class("PLUGIN", &traverse_class())
        .is_ok());
    let mut instance = traverse_instance();
    instance.selectable_custom = true;
    instance.design_record.clear();
    let handles = host
        .stage_plugin_class_instance("PLUGIN", &instance)
        .expect("the selectable custom instance stages");
    assert!(!handles.is_empty());
    for &handle in &handles {
        assert!(!host.app.tabs[0].scene.is_reference_only(handle));
        assert!(host.app.tabs[0].scene.native_edit_disabled(handle));
        let stamp = host
            .read_record(handle, "PLUGIN_ALIGN_DISPLAY")
            .expect("selectable custom owner stamp");
        assert_eq!(stamp.values.len(), 3);
        assert!(matches!(
            &stamp.values[1],
            document::xdata::XDataValue::String(value) if value.is_empty()
        ));
        assert!(matches!(
            stamp.values[2],
            document::xdata::XDataValue::Integer32(1)
        ));
        assert!(host.entity_grips(handle).expect("grip read").is_empty());

        let original = host.document().get_entity(handle).unwrap().clone();
        assert!(!host.app.tabs[0].scene.update_entity(original.clone()));
        host.app.tabs[0].scene.erase_entities(&[handle]);
        assert!(host.document().get_entity(handle).is_some());
    }

    let handle = handles[0];
    host.app.tabs[0].scene.select_entity(handle, true);
    assert!(host.app.tabs[0].scene.selected.contains(&handle));
    assert!(host.app.property_target_handles(0).is_empty());
    let original = host.document().get_entity(handle).unwrap().clone();
    assert!(
        host.update_entity(original),
        "plugin updates remain allowed"
    );
    assert!(host.remove_entity(handle), "plugin erases remain allowed");
    assert!(host.document().get_entity(handle).is_none());
}

#[test]
fn plugin_class_instances_refuse_foreign_owners() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(host
        .register_plugin_class("PLUGIN", &traverse_class())
        .is_ok());
    // Another alive plugin sees the class but cannot stage it.
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_RIVAL", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let err = host
        .stage_plugin_class_instance("PLUGIN_RIVAL", &traverse_instance())
        .unwrap_err();
    assert!(err.contains("owned by plugin"), "got: {err}");
    assert!(err.contains("PLUGIN"), "got: {err}");
    assert_eq!(host.document().entities().count(), 0);
}

#[test]
fn plugin_class_instance_stages_reference_only_with_identity_stamp() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let instance = traverse_instance();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host
            .register_plugin_class("PLUGIN", &traverse_class())
            .is_ok());
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the class instance stages");
        assert!(!handles.is_empty());
        for handle in &handles {
            // The identity stamp: class key as the XDATA application, the
            // schema version + the record linkage as its values, the
            // APPID auto-registered so the stamp survives the file.
            let stamped = host
                .read_record(*handle, "PLUGIN_ALIGN_DISPLAY")
                .expect("the class identity stamp");
            assert_eq!(stamped.values.len(), 2);
            let ocs_plugin_api::host::document::xdata::XDataValue::Integer32(version) =
                stamped.values[0]
            else {
                panic!("the stamp carries the schema version as an integer");
            };
            assert_eq!(version, 4);
            let ocs_plugin_api::host::document::xdata::XDataValue::String(record) =
                stamped.values[1].clone()
            else {
                panic!("the stamp carries the design record linkage");
            };
            assert_eq!(record, "PLUGIN_ALIGN_DISPLAY");
            assert!(host.document().app_ids.contains("PLUGIN_ALIGN_DISPLAY"));
            assert!(
                host.app.tabs[0].scene.is_reference_only(*handle),
                "staged class display must be reference-only"
            );
        }
    }
    app.finish_pending_history(0);
    // Stamp + designation + add: ONE undo step, the entity-write contract.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    for handle in &handles {
        assert!(app.tabs[0].scene.document.get_entity(*handle).is_none());
    }
}

#[test]
fn class_stamp_and_display_set_survive_dxf_and_dwg_round_trips() {
    // The stamp's file-level law: the identity stamp (the XDATA record
    // under the auto-registered APPID) and the staged display set
    // survive BOTH file formats with empty loss rows; the stamp values
    // (schema version + record linkage + selectable-custom flags) stay
    // byte-identical.
    use document::{Dwg, Dxf, Format};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host
            .register_plugin_class("PLUGIN", &traverse_class())
            .is_ok());
        let mut instance = traverse_instance();
        instance.selectable_custom = true;
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the selectable custom class instance stages");
    }
    let read_stamp = |document: &document::Document, handle: document::Handle| {
        let entity = document
            .get_entity(handle)
            .unwrap_or_else(|| panic!("staged entity {handle:?} lost"));
        entity
            .common()
            .extended_data
            .get_record("PLUGIN_ALIGN_DISPLAY")
            .unwrap_or_else(|| panic!("the class identity stamp was lost"))
            .values
            .clone()
    };
    fn round_trip<F: Format>(
        format: &F,
        document: &document::Document,
        handles: &[document::Handle],
        stamp: &dyn Fn(&document::Document, document::Handle) -> Vec<document::xdata::XDataValue>,
        name: &str,
    ) {
        let (bytes, losses) = format.write(document).unwrap();
        assert!(
            losses.is_empty(),
            "{name} write must not lose anything: {losses:?}"
        );
        let (back, read_losses) = format.read(&bytes).unwrap();
        assert!(
            read_losses.is_empty(),
            "{name} read must not lose anything: {read_losses:?}"
        );
        for handle in handles {
            let expected = stamp(document, *handle);
            assert_eq!(
                stamp(&back, *handle),
                expected,
                "{name}: the class stamp must be byte-identical"
            );
            assert!(
                crate::plugin::entity_labels::native_edit_disabled(
                    back.get_entity(*handle).expect("flagged entity survives"),
                ),
                "{name}: the selectable-custom flag survives"
            );
        }
    }
    round_trip(
        &Dxf,
        &app.tabs[0].scene.document,
        &handles,
        &read_stamp,
        "DXF",
    );
    round_trip(
        &Dwg,
        &app.tabs[0].scene.document,
        &handles,
        &read_stamp,
        "DWG",
    );
}

#[test]
fn plugin_class_instance_refusals_change_nothing_and_record_no_steps() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(host
        .register_plugin_class("PLUGIN", &traverse_class())
        .is_ok());
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;
    // Unregistered class.
    let mut ghost = traverse_instance();
    ghost.class_id = "GHOST".to_owned();
    assert!(host.stage_plugin_class_instance("PLUGIN", &ghost).is_err());
    // Schema version mismatch.
    let mut mismatch = traverse_instance();
    mismatch.schema_version = 5;
    assert!(host
        .stage_plugin_class_instance("PLUGIN", &mismatch)
        .is_err());
    // Broken design parameter (the chain audit refuses at the contract).
    let mut broken = traverse_instance();
    let ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(chain) = &mut broken.design;
    chain.segments[0].start_direction = f64::NAN;
    broken.tolerance = 1e-3;
    assert!(host.stage_plugin_class_instance("PLUGIN", &broken).is_err());
    // Broken tolerance.
    assert!(host
        .stage_plugin_class_instance("PLUGIN", &traverse_instance_for(0.0))
        .is_err());
    assert_eq!(host.document().entities().count(), 0);
    assert_eq!(host.app.tabs[0].scene.geometry_epoch, epoch_before);
}

fn traverse_instance_for(tolerance: f64) -> ocs_plugin_api::host::PluginClassInstance {
    let mut instance = traverse_instance();
    instance.tolerance = tolerance;
    instance
}

#[test]
fn parametric_sample_is_a_pure_read() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let chain = staging_profile_chain();
    {
        let host = HostSession::new(&mut app, 0);
        let epoch_before = host.app.tabs[0].scene.geometry_epoch;
        // The profile parabola h(s) = 100 + s^2/(2*800): the midpoint value.
        let mid = host.sample_parametric(&chain, 40.0).unwrap().unwrap();
        assert!((mid.point[1] - 101.0).abs() < 1e-9);
        assert!((mid.point[0] - 40.0).abs() < 1e-9);
        // The tangent stays (1, g)/|..| with g = s/800.
        assert!((mid.direction[1] / mid.direction[0] - 0.05).abs() < 1e-9);
        assert!(host.sample_parametric(&chain, -1.0).unwrap().is_none());
        assert!(host.sample_parametric(&chain, 141.0).unwrap().is_none());
        assert_eq!(host.app.tabs[0].scene.geometry_epoch, epoch_before);
    }
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        0,
        "the sample records nothing"
    );
}

#[test]
fn parametric_validation_is_pure_and_rejects_a_hard_corner() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let chain = staging_profile_chain();
    let mut hard_corner = chain.clone();
    hard_corner
        .segments
        .push(ocs_plugin_api::host::ParametricSegment {
            def: ocs_plugin_api::host::CurveSegmentDef {
                length: 20.0,
                ramp: ocs_plugin_api::host::Ramp::Polynomial {
                    degree: 0,
                    coeffs: [0.25, 0.0, 0.0, 0.0],
                },
                cross_slope: None,
            },
            // The first segment ends at (80, 104) with gradient 0.1: the
            // point seam is welded, but the 0.25 gradient is a hard corner.
            start_point: [80.0, 104.0],
            start_direction: 0.25f64.atan(),
        });

    let host = HostSession::new(&mut app, 0);
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;
    let document_before = host.document().clone();
    let undo_before = host.app.tabs[0].history.undo_stack.len();
    let dirty_before = host.app.tabs[0].dirty;
    let published_before = host.app.last_plugin_document;
    let had_document_view = host.doc_store.is_some();

    HostApi::validate_parametric_chain(&host, &chain, 1e-3)
        .expect("the valid chain passes the complete audit");
    let err = HostApi::validate_parametric_chain(&host, &hard_corner, 1e-3).unwrap_err();
    assert!(err.contains("seam segment 0 to 1"), "{err}");
    assert!(err.contains("declared start gradient"), "{err}");
    let empty = ocs_plugin_api::host::ParametricChain {
        segments: Vec::new(),
        ..chain.clone()
    };
    assert!(HostApi::validate_parametric_chain(&host, &empty, 1e-3)
        .unwrap_err()
        .contains("at least one segment"));

    assert_eq!(host.app.tabs[0].scene.geometry_epoch, epoch_before);
    assert_eq!(host.document(), &document_before);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo_before);
    assert_eq!(host.app.tabs[0].dirty, dirty_before);
    assert_eq!(host.app.last_plugin_document, published_before);
    assert_eq!(host.doc_store.is_some(), had_document_view);
}

#[test]
fn parametric_staging_joins_a_plugin_scope() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let plan = staging_plan_chain();
    let profile = staging_profile_chain();
    {
        let mut host = HostSession::new(&mut app, 0);
        host.push_undo("PLUGIN: refresh alignment display");
        let _ = host.stage_parametric_chain("plan", &plan, 1e-3).unwrap();
        let _ = host
            .stage_parametric_chain("profile", &profile, 1e-3)
            .unwrap();
    }
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        1,
        "the plugin's scope carried both stagings"
    );
}

/// The grip-edit restage fixture (the domain crest as ramp data):
/// [constant slope 0.01 over 100 -> slope-linear 0.01 to 0.06 over
/// 200 -> constant slope 0.06 over 80 at the exact end (300, 8.0)],
/// entering at (0, 0). Every segment stages the count-stable exact
/// carrier (line, degree-2 spline, line).
fn crest_profile_chain() -> ocs_plugin_api::host::ParametricChain {
    ocs_plugin_api::host::ParametricChain {
        plane: ocs_plugin_api::host::ParametricPlane::Profile,
        segments: vec![
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    length: 100.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 0,
                        coeffs: [0.01, 0.0, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [0.0, 0.0],
                start_direction: 0.01f64.atan(),
            },
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    // Slope-linear ramp 0.01 -> 0.06 (the declared
                    // crest restated as data).
                    length: 200.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 1,
                        coeffs: [0.01, 0.05, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [100.0, 1.0],
                start_direction: 0.01f64.atan(),
            },
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    length: 80.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 0,
                        coeffs: [0.06, 0.0, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [300.0, 8.0],
                start_direction: 0.06f64.atan(),
            },
        ],
        layer: "PLUGIN_PROFILE".to_string(),
    }
}

#[test]
fn grip_edit_restage_updates_the_display_set_in_place() {
    // The grip-edit restage arc (session level): a drag edits the
    // declared crest parameter, the lane re-derives the edited design's
    // entity set, and the honest restage applies it over the SAME
    // handles through `update_entities_transaction`. Read semantics
    // (this test pins them): the transaction preserves the reference-
    // only designation by construction (it is scene state keyed by
    // handle and the write never touches it), and the XDATA identity
    // stamp survives because the honest lane bases the replacement on
    // the stamped clone and patches only geometry — no re-stamping.
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host
            .register_plugin_class("PLUGIN", &traverse_class())
            .is_ok());
        let mut instance = traverse_instance();
        instance.design =
            ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(crest_profile_chain());
        instance.tolerance = 1e-3;
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the crest instance stages");
        assert_eq!(
            handles.len(),
            3,
            "three exact carriers (line, spline, line)"
        );
        let EntityType::Spline(spline) = host.document().get_entity(handles[1]).expect("live")
        else {
            panic!("the crest stages a degree-2 spline");
        };
        assert_eq!(spline.degree, 2);
        assert!(
            (spline.control_points[2].y - 8.0).abs() < 1e-9,
            "the declared crest height"
        );
    }
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        1,
        "staging stays one step"
    );

    {
        let mut host = HostSession::new(&mut app, 0);
        // THE DRAG (host-local, the grip-edit policy): the crest's end
        // gradient 0.06 -> 0.09 — the ramp's end value revised; the
        // joint re-derived from the engine's sampling and the honest
        // upstream re-declared.
        let mut edited = crest_profile_chain();
        edited.segments[1].def.ramp = ocs_plugin_api::host::Ramp::Polynomial {
            degree: 1,
            coeffs: [0.01, 0.08, 0.0, 0.0],
        };
        let front = ocs_plugin_api::host::ParametricChain {
            plane: ocs_plugin_api::host::ParametricPlane::Profile,
            segments: edited.segments[..2].to_vec(),
            layer: String::new(),
        };
        let joint = host
            .sample_parametric(&front, 300.0)
            .expect("the front samples")
            .expect("the joint sits at the front chain's end");
        assert!(
            (joint.point[1] - 11.0).abs() < 1e-9,
            "the crest moved to 11.0"
        );
        let joint_gradient = joint.direction[1] / joint.direction[0];
        edited.segments[2].start_point = joint.point;
        edited.segments[2].start_direction = joint_gradient.atan();
        // The tail's slope law restates the propagated joint slope (the
        // ramp data in place of the old gradient pair).
        edited.segments[2].def.ramp = ocs_plugin_api::host::Ramp::Polynomial {
            degree: 0,
            coeffs: [joint_gradient, 0.0, 0.0, 0.0],
        };

        // Re-derive the edited design's entity set: same carriers, same
        // count, in the same order (the staging builds per segment).
        let (new_entities, carriers) = crate::parametric::build_display_entities(&edited, 1e-3)
            .expect("the edited design derives");
        assert_eq!(
            new_entities.len(),
            handles.len(),
            "the edited set is count-stable"
        );
        assert!(matches!(
            carriers[1],
            crate::parametric::StagedCarrier::Exact
        ));
        let EntityType::Spline(fresh) = &new_entities[1] else {
            panic!("the edited crest still carries the exact spline carrier");
        };
        assert!((fresh.control_points[2].y - 11.0).abs() < 1e-9);

        // The honest lane: clone the live entities (identity, layer and
        // the XDATA stamp ride along) and patch ONLY the geometry from
        // the re-derived carriers.
        let mut restaged: Vec<EntityType> = handles
            .iter()
            .map(|h| {
                host.document()
                    .get_entity(*h)
                    .expect("the staged entity is live")
                    .clone()
            })
            .collect();
        for (slot, fresh) in restaged.iter_mut().zip(new_entities.iter()) {
            match (slot, fresh) {
                (EntityType::Line(old), EntityType::Line(newer)) => {
                    old.start = newer.start;
                    old.end = newer.end;
                }
                (EntityType::Spline(old), EntityType::Spline(newer)) => {
                    old.degree = newer.degree;
                    old.control_points = newer.control_points.clone();
                    old.knots = newer.knots.clone();
                    old.weights = newer.weights.clone();
                    old.begin_tangent = newer.begin_tangent;
                    old.end_tangent = newer.end_tangent;
                }
                _ => panic!("the re-derived set keeps the staged carrier shape"),
            }
        }
        host.update_entities_transaction("PLUGIN: grip edit", restaged)
            .expect("the restage applies over the same handles");
    }
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0].history.undo_stack.len(),
        2,
        "the restage composes as exactly one more undo step"
    );

    {
        let host = HostSession::new(&mut app, 0);
        assert_eq!(
            host.document().entities().count(),
            3,
            "the restage updates the display set in place"
        );
        // The changed carrier vertices are the document's state now...
        let EntityType::Spline(spline) = host
            .document()
            .get_entity(handles[1])
            .expect("the spline keeps its handle")
        else {
            panic!("the carrier keeps its kind over the restage");
        };
        assert!(
            (spline.control_points[2].y - 11.0).abs() < 1e-9,
            "the edited crest rides"
        );
        let EntityType::Line(tail) = host.document().get_entity(handles[2]).expect("live") else {
            panic!("the tail keeps its kind");
        };
        assert!(
            (tail.end.y - (11.0 + 0.09 * 80.0)).abs() < 1e-9,
            "the tail line follows the moved joint"
        );
        for handle in &handles {
            // ... and the designations and the identity stamps are
            // intact (the in-place policy: no re-stamp, no re-designate).
            assert!(
                host.app.tabs[0].scene.is_reference_only(*handle),
                "the designated display set stays reference-only"
            );
            let stamped = host
                .read_record(*handle, "PLUGIN_ALIGN_DISPLAY")
                .expect("the stamp");
            assert_eq!(
                stamped.values.len(),
                2,
                "the identity stamp survives the restage"
            );
        }
    }

    // ONE undo step round-trips the original display set.
    app.undo_steps(1);
    {
        let host = HostSession::new(&mut app, 0);
        assert_eq!(
            host.document().entities().count(),
            3,
            "nothing added or removed"
        );
        let EntityType::Spline(spline) = host
            .document()
            .get_entity(handles[1])
            .expect("the spline keeps its handle")
        else {
            panic!("the carrier keeps its kind over the undo");
        };
        assert!(
            (spline.control_points[2].y - 8.0).abs() < 1e-9,
            "the original crest is back after the one undo step"
        );
        for handle in &handles {
            assert!(
                host.app.tabs[0].scene.is_reference_only(*handle),
                "the designations survive the undo round trip"
            );
            assert!(
                host.read_record(*handle, "PLUGIN_ALIGN_DISPLAY").is_some(),
                "the identity stamp survives the undo round trip"
            );
        }
    }
}

#[test]
fn plugin_class_parameters_read_write_and_refuse() {
    // The session pair over a host-staged instance: the declared
    // schema renders rows straight from the design record; an editable
    // parameter writes through the record's (name, value) pair in ONE
    // undo step; unknown names, read-only rows, and unparseable values
    // refuse before any change.
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut declaration = traverse_class();
        declaration.parameters = vec![
            ocs_plugin_api::host::PluginClassParameter {
                name: "peak_gradient".to_string(),
                value_kind: ocs_plugin_api::host::PluginClassParameterKind::Real,
                editable: true,
            },
            ocs_plugin_api::host::PluginClassParameter {
                name: "segments".to_string(),
                value_kind: ocs_plugin_api::host::PluginClassParameterKind::Integer,
                editable: true,
            },
            ocs_plugin_api::host::PluginClassParameter {
                name: "locked_note".to_string(),
                value_kind: ocs_plugin_api::host::PluginClassParameterKind::Text,
                editable: false,
            },
        ];
        host.register_plugin_class("PLUGIN", &declaration)
            .expect("the parameter schema registers");
        let mut instance = traverse_instance();
        instance.design =
            ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(crest_profile_chain());
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the instance stages");
        // The declared (name, value) pair layout: alternating String
        // name / value entries — the layout the declaration owns.
        host.create_xrecord(
            &["PLUGIN"],
            "PLUGIN_ALIGN_DISPLAY",
            vec![
                document::objects::XRecordEntry::string(1, "peak_gradient"),
                document::objects::XRecordEntry::new(
                    140,
                    document::objects::XRecordValue::Double(0.06),
                ),
                document::objects::XRecordEntry::string(1, "segments"),
                document::objects::XRecordEntry::new(90, document::objects::XRecordValue::Int32(3)),
                document::objects::XRecordEntry::string(1, "locked_note"),
                document::objects::XRecordEntry::string(1, "per alignment sheet"),
            ],
        )
        .expect("the design record seeds its namespace");
    }
    app.finish_pending_history(0);
    let undo_before = app.tabs[0].history.undo_stack.len();
    assert_eq!(
        undo_before, 2,
        "staging + record seeding compose as two steps"
    );
    {
        let mut host = HostSession::new(&mut app, 0);
        host.update_plugin_class_instance_parameter(handles[0], "peak_gradient", "0.09")
            .expect("the editable row writes");
    }
    app.finish_pending_history(0);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert_eq!(
            host.app.tabs[0].history.undo_stack.len(),
            undo_before + 1,
            "the parameter write composes as exactly one more undo step"
        );
        let rows = host
            .plugin_class_instance_properties(handles[0])
            .expect("the rows re-read");
        assert_eq!(
            rows[0].value.as_deref(),
            Some("0.09"),
            "the written pair is the read truth"
        );
        // The record's OTHER pairs (and the plugin's own layout) stayed
        // exactly as the plugin laid them.
        let entries = host
            .read_xrecord(&["PLUGIN"], "PLUGIN_ALIGN_DISPLAY")
            .expect("the record");
        assert_eq!(entries.len(), 6);
        assert_eq!(format!("{:?}", entries[3].value), "Int32(3)");
        assert_eq!(
            format!("{:?}", entries[5].value),
            "String(\"per alignment sheet\")"
        );

        // REFUSALS: unknown name, read-only row, unparseable and
        // non-finite values — each before any change, each recording
        // nothing.
        let steps_before = host.app.tabs[0].history.undo_stack.len();
        assert!(
            host.update_plugin_class_instance_parameter(handles[0], "not_a_parameter", "2.0")
                .unwrap_err()
                .contains("has no parameter named"),
            "an unknown parameter refuses honestly"
        );
        assert!(
            host.update_plugin_class_instance_parameter(handles[0], "locked_note", "edited")
                .unwrap_err()
                .contains("read-only"),
            "a read-only row never writes"
        );
        assert!(
            host.update_plugin_class_instance_parameter(handles[0], "peak_gradient", "abc")
                .unwrap_err()
                .contains("does not parse"),
            "a non-numerical value refuses honestly"
        );
        assert!(
            host.update_plugin_class_instance_parameter(handles[0], "peak_gradient", "inf")
                .unwrap_err()
                .contains("finite"),
            "a non-finite value refuses honestly"
        );
        assert_eq!(
            host.app.tabs[0].history.undo_stack.len(),
            steps_before,
            "refusals record nothing"
        );
        let entries_after = host
            .read_xrecord(&["PLUGIN"], "PLUGIN_ALIGN_DISPLAY")
            .expect("the record");
        assert_eq!(
            entries_after.len(),
            entries.len(),
            "refusals changed nothing"
        );
        for (before, after) in entries.iter().zip(entries_after.iter()) {
            assert_eq!(before.code, after.code, "refusals changed nothing");
            assert_eq!(format!("{:?}", before.value), format!("{:?}", after.value));
        }
    }
}

#[test]
fn plugin_class_parameters_panel_binds_and_commits() {
    // The panel surface (API v14): a stamped selectable entity (the
    // v10/v12 lane's own anchor) rides its class's declared rows, and
    // the commit route writes the record row through the session pair.
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let handle;
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut declaration = traverse_class();
        declaration.parameters = vec![ocs_plugin_api::host::PluginClassParameter {
            name: "peak_gradient".to_string(),
            value_kind: ocs_plugin_api::host::PluginClassParameterKind::Real,
            editable: true,
        }];
        host.register_plugin_class("PLUGIN", &declaration)
            .expect("the parameter schema registers");
        handle = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
            12.0, 3.0, 0.0,
        ))));
        let mut stamp = document::xdata::ExtendedDataRecord::new("PLUGIN_ALIGN_DISPLAY");
        stamp.add_value(document::xdata::XDataValue::Integer32(4));
        stamp.add_value(document::xdata::XDataValue::String(
            "PLUGIN_ALIGN_DISPLAY".to_string(),
        ));
        assert!(host.write_record(handle, stamp), "the stamp rides");
        host.create_xrecord(
            &["PLUGIN"],
            "PLUGIN_ALIGN_DISPLAY",
            vec![
                document::objects::XRecordEntry::string(1, "peak_gradient"),
                document::objects::XRecordEntry::new(
                    140,
                    document::objects::XRecordValue::Double(0.06),
                ),
            ],
        )
        .expect("the design record seeds its namespace");
    }
    app.finish_pending_history(0);

    // Select the anchor entity; the panel builds the section.
    app.tabs[0].scene.selected.insert(handle);
    app.refresh_properties();
    {
        let panel = &app.tabs[0].properties;
        let section = panel
            .sections
            .iter()
            .find(|section| section.title == crate::t!("Plugin class parameters").into_owned())
            .expect("the stamped selection rides its class's rows");
        assert_eq!(section.props.len(), 2, "the source line + the declared row");
        assert_eq!(section.props[0].label, "Source");
        assert!(
            matches!(
                &section.props[0].value,
                crate::scene::model::object::PropValue::ReadOnly(source)
                    if source.contains("PLUGIN_ALIGN_DISPLAY")
            ),
            "the derived-view line points at the owning record row"
        );
        let row = &section.props[1];
        assert_eq!(row.label, "peak_gradient");
        assert!(
            matches!(
                &row.value,
                crate::scene::model::object::PropValue::PluginParam {
                    name,
                    value,
                    editable: true,
                } if name == "peak_gradient" && value == "0.06"
            ),
            "a selectable lane entity rides editable rows"
        );
    }

    // The commit route: typing + Enter === the record row writes.
    let _ = app.update(crate::app::Message::PropPluginParamInput {
        name: "peak_gradient".to_string(),
        value: "0.09".to_string(),
    });
    let _ = app.update(crate::app::Message::PropPluginParamCommit(
        "peak_gradient".to_string(),
    ));
    {
        let host = HostSession::new(&mut app, 0);
        let rows = host
            .plugin_class_instance_properties(handle)
            .expect("the rows record the commit");
        assert_eq!(
            rows[0].value.as_deref(),
            Some("0.09"),
            "the committed value landed on the record row"
        );
    }

    // A selection without a registered class stamp renders no section.
    let mut plain = OpenCADStudio::new_for_test();
    plain.tabs[0].is_start = false;
    {
        let mut host = HostSession::new(&mut plain, 0);
        let unstamped = host.add_entity(EntityType::Point(Point::at(
            document::types::Vector3::new(1.0, 1.0, 0.0),
        )));
        plain.tabs[0].scene.selected.insert(unstamped);
    }
    plain.refresh_properties();
    assert!(
        plain.tabs[0]
            .properties
            .sections
            .iter()
            .all(|section| section.title != crate::t!("Plugin class parameters").into_owned()),
        "an unstamped selection rides no class rows"
    );
}

fn api16_text_panel() -> ocs_plugin_api::dialog::PanelDef {
    use ocs_plugin_api::dialog::{DialogControl, PanelDef};

    PanelDef::new("panel", "Panel")
        .with_control(DialogControl::Text {
            id: "length".into(),
            label: "Length".into(),
            default: "1.0".into(),
        })
        .with_control(DialogControl::Button {
            id: "submit".into(),
            label: "Submit".into(),
            action: Some("PANEL_SUBMIT".into()),
            align_h: None,
            width: None,
        })
}

fn api16_number_panel() -> ocs_plugin_api::dialog::PanelDef {
    let mut panel = api16_text_panel();
    panel.controls[0] = ocs_plugin_api::dialog::DialogControl::Number {
        id: "length".into(),
        label: "Length".into(),
        default: 1.0,
        min: None,
        max: None,
        step: None,
    };
    panel
}

#[test]
fn api16_panel_registration_is_idempotent_and_owner_authenticated() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let tab_id = app.tabs[0].id;
    let mut host = HostSession::new(&mut app, 0);

    assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("2.5".to_string()),
        )]),
    ));
    assert!(
        host.register_panel_for_plugin("OWNER", api16_text_panel()),
        "unchanged re-registration must not reset the live draft"
    );
    let before = host.app.plugin_panels["panel"].clone();

    let mut foreign_panel = api16_text_panel();
    foreign_panel.title = "Foreign replacement".into();
    assert!(!host.register_panel_for_plugin("OTHER", foreign_panel));
    assert!(!host.update_panel_for_plugin(
        "OTHER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("99.0".to_string()),
        )]),
    ));
    assert!(!host.toggle_panel_for_plugin("OTHER", "panel", Some(true)));
    assert!(host.panel_snapshot_for_plugin("OTHER", "panel").is_err());
    assert_eq!(host.app.plugin_panels.get("panel"), Some(&before));

    let snapshot = host.panel_snapshot_for_plugin("OWNER", "panel").unwrap();
    assert_eq!(snapshot.plugin_id, "OWNER");
    assert_eq!(snapshot.panel_id, "panel");
    assert_eq!(snapshot.tab_id, tab_id);
    assert_eq!(snapshot.values.len(), 1);
    assert_eq!(
        snapshot.values.get("length"),
        Some(&DialogValue::Text("2.5".to_string()))
    );
    assert_eq!(snapshot.raw_text.len(), 1);
    assert_eq!(
        snapshot.raw_text.get("length").map(String::as_str),
        Some("2.5")
    );
    assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    let direct = host
        .panel_snapshot("PANEL")
        .expect("ordinary HostApi command read-back works");
    assert_eq!(direct.tab_id, tab_id);
    assert_eq!(direct.values.get("length"), snapshot.values.get("length"));
    assert!(!host.app.plugin_panels["panel"].visible);
    assert_eq!(host.app.plugin_panels.get("panel"), Some(&before));
}

#[test]
fn api16_panel_snapshot_uses_the_real_session_tab_without_document_writes() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let first_tab_id = app.tabs[0].id;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    }
    let second_tab_id = app.push_test_document();
    assert_eq!(second_tab_id, app.tabs[1].id);
    assert_ne!(first_tab_id, second_tab_id);
    assert_eq!(app.active_tab, 0);

    let first_document = app.tabs[0].scene.document.clone();
    let second_document = app.tabs[1].scene.document.clone();
    let first = HostSession::new(&mut app, 0)
        .panel_snapshot_for_plugin("OWNER", "panel")
        .unwrap();
    let second = HostSession::new(&mut app, 1)
        .panel_snapshot_for_plugin("OWNER", "panel")
        .unwrap();

    assert_eq!(first.tab_id, first_tab_id);
    assert_eq!(second.tab_id, second_tab_id);
    assert_ne!(first.tab_id, second.tab_id);
    for snapshot in [&first, &second] {
        assert_eq!(snapshot.plugin_id, "OWNER");
        assert_eq!(snapshot.panel_id, "panel");
        assert_eq!(snapshot.values.len(), 1);
        assert_eq!(
            snapshot.values.get("length"),
            Some(&DialogValue::Text("1.0".to_string()))
        );
        assert_eq!(snapshot.raw_text.len(), 1);
        assert_eq!(
            snapshot.raw_text.get("length").map(String::as_str),
            Some("1.0")
        );
        assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    }
    assert_eq!(app.tabs[0].scene.document, first_document);
    assert_eq!(app.tabs[1].scene.document, second_document);
    assert_eq!(app.active_tab, 0);
    assert!(app.tabs[0].history.pending.is_none());
    assert!(app.tabs[1].history.pending.is_none());
}

#[test]
fn api16_panel_snapshot_rejects_malformed_and_missing_panels_without_writes() {
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let document_before = app.tabs[0].scene.document.clone();
    let undo_before = app.tabs[0].history.undo_stack.len();
    let mut host = HostSession::new(&mut app, 0);
    assert!(host.register_panel_for_plugin("OWNER", api16_number_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "panel",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("-".to_string()),
        )]),
    ));
    let snapshot = host.panel_snapshot_for_plugin("OWNER", "panel").unwrap();

    assert_eq!(
        snapshot.raw_text.get("length").map(String::as_str),
        Some("-")
    );
    assert!(!snapshot.values.contains_key("length"));
    assert!(
        snapshot.errors.iter().any(|error| {
            error.contains("invalid finite number")
                && error.contains("'length'")
                && error.contains("'-'")
        }),
        "{:?}",
        snapshot.errors
    );

    let error = host
        .panel_snapshot_for_plugin("OWNER", "missing")
        .unwrap_err();
    assert!(error.contains("not registered"), "{error}");
    assert_eq!(host.document(), &document_before);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo_before);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn api17_panel_reset_preserves_owner_and_aliases_remain_authenticated() {
    use crate::app::Message;
    use ocs_plugin_api::dialog::DialogValue;

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
        assert!(host.update_panel_for_plugin(
            "OWNER",
            "panel",
            std::collections::HashMap::from([(
                "length".to_string(),
                DialogValue::Text("2.5".to_string()),
            )]),
        ));
    }

    // The built-in reset path must retain the authenticated owner and
    // restore the declaration's defaults, not brick subsequent access.
    let _ = app.update(Message::PluginPanelButtonClick(
        "panel".to_string(),
        "reset".to_string(),
        Some("reset".to_string()),
    ));
    let mut host = HostSession::new(&mut app, 0);
    let snapshot = host
        .panel_snapshot_for_plugin("OWNER", "panel")
        .expect("owner remains able to read after reset");
    assert_eq!(
        snapshot.values.get("length"),
        Some(&DialogValue::Text("1.0".into()))
    );
    assert!(host.register_panel_for_plugin("OWNER", api16_text_panel()));
    assert!(host.update_panel_for_plugin(
        "OWNER",
        "PANEL",
        std::collections::HashMap::from([(
            "length".to_string(),
            DialogValue::Text("3.0".to_string()),
        )]),
    ));
    assert!(host.toggle_panel_for_plugin("OWNER", "Panel", Some(true)));
    assert!(!host.update_panel_for_plugin("OTHER", "panel", std::collections::HashMap::new(),));
    assert!(host.panel_snapshot_for_plugin("OTHER", "Panel").is_err());
}

#[test]
fn api17_explicit_plugin_panel_reopens_in_clean_screen_without_document_writes() {
    use ocs_plugin_api::dialog::{DialogControl, PanelDef};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    app.clean_screen = true;
    let mut host = HostSession::new(&mut app, 0);
    for id in ["cvifc.ifc_h_properties", "cvifc.ifc_v_properties"] {
        let panel = PanelDef::new(id, "IFC Alignment Segment")
            .with_command(format!(
                "CVI_IFC_{}_PANEL",
                if id.ends_with("h_properties") {
                    "H"
                } else {
                    "V"
                }
            ))
            .with_control(DialogControl::Text {
                id: "source_record".into(),
                label: "Source record".into(),
                default: "Alignment-Axis-h-seg-001".into(),
            })
            .with_control(DialogControl::Button {
                id: "apply".into(),
                label: "Apply".into(),
                action: Some("CVI_IFC_PANEL_APPLY".into()),
                align_h: None,
                width: None,
            });
        assert!(host.register_panel_for_plugin("opencad.civil_ifc", panel));
    }
    let panel_id = crate::ui::dock::PanelId::Plugin(1);
    let other_id = crate::ui::dock::PanelId::Plugin(2);
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(host.toggle_panel_for_plugin(
        "opencad.civil_ifc",
        "cvifc.ifc_h_properties",
        Some(true),
    ));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(
        matches!(
            host.app.dock.location(panel_id),
            Some((crate::app::config::DockSide::Right, _))
        ),
        "an explicit show places a new panel into the default dock"
    );
    assert!(host.app.dock_panel_on_screen(panel_id));
    assert!(!host.app.dock_panel_visible(other_id));
    assert!(host.app.plugin_panels["cvifc.ifc_h_properties"].visible);
    assert!(!host.app.plugin_panels["cvifc.ifc_v_properties"].visible);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn plugin_panels_toggle_like_graphic_attributes_including_clean_screen() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    app.clean_screen = true;
    let graphic_id = crate::ui::dock::PanelId::GraphicAttributes;
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(!app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));
    let _ = app.update(crate::app::Message::ToggleGraphicAttributes);
    assert!(app.show_graphic_attributes);
    assert!(!app.dock_panel_visible(graphic_id));

    let mut host = HostSession::new(&mut app, 0);
    let panel_id = crate::ui::dock::PanelId::Plugin(1);
    assert!(host.register_panel_for_plugin("opencad.civil_ifc", api16_text_panel()));
    // Like Graphic Attributes, the panel stays hidden in Clean Screen;
    // opening it still binds its values without a document edit.
    app.clean_screen = false;
    let mut host = HostSession::new(&mut app, 0);
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(host.app.dock_panel_on_screen(panel_id));
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(!host.app.dock_panel_visible(panel_id));
    assert!(!host.app.plugin_panels["panel"].visible);
    assert!(host.toggle_panel_for_plugin("opencad.civil_ifc", "panel", None));
    assert!(host.app.dock_panel_visible(panel_id));
    assert!(host.app.plugin_panels["panel"].visible);
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
    assert!(host.app.tabs[0].history.pending.is_none());
}

#[test]
fn api17_update_panel_for_plugin_allows_owned_modal_dialog() {
    use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogValue};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let dlg = DialogDef::new("Test Dialog").with_control(DialogControl::Text {
        id: "field1".into(),
        label: "Field 1".into(),
        default: "initial".into(),
    });
    app.open_plugin_dialog_on_tab(0, dlg, Some("MY_PLUGIN".to_string()));

    let mut host = HostSession::new(&mut app, 0);

    // Foreign plugin cannot update the dialog
    assert!(!host.update_panel_for_plugin(
        "OTHER_PLUGIN",
        "Test Dialog",
        std::collections::HashMap::from([("field1".into(), DialogValue::Text("hacked".into()))]),
    ));

    // Owner plugin can update by title
    assert!(host.update_panel_for_plugin(
        "MY_PLUGIN",
        "Test Dialog",
        std::collections::HashMap::from([("field1".into(), DialogValue::Text("updated".into()))]),
    ));
    let field1 = |host: &HostSession| {
        host.app
            .plugin_dialog_state
            .as_ref()
            .unwrap()
            .text_values
            .get("field1")
            .cloned()
    };
    assert_eq!(field1(&host).as_deref(), Some("updated"));

    // Owner plugin can also update with empty panel_id (active dialog convention)
    assert!(host.update_panel_for_plugin(
        "MY_PLUGIN",
        "",
        std::collections::HashMap::from([(
            "field1".into(),
            DialogValue::Text("empty_id_update".into())
        )]),
    ));
    assert_eq!(field1(&host).as_deref(), Some("empty_id_update"));
}
