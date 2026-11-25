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

    assert!(
        host.linetype_operation(create("Gap First", &[-1.0, 1.0]))
            .is_err()
    );
    assert!(
        host.linetype_operation(create("Comma,Name", &[1.0, -1.0]))
            .is_err()
    );
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
        assert!(
            view.entities
                .iter()
                .any(|entity| entity.handle == attribute_handle.value())
        );

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
        assert!(
            host.update_entities_transaction("Move points", vec![a.clone(), invalid])
                .is_err()
        );
        let mut invalid_geometry = b.clone();
        if let EntityType::Point(p) = &mut invalid_geometry {
            p.location.x = f64::NAN;
        }
        assert!(
            host.update_entities_transaction("Move points", vec![a.clone(), invalid_geometry])
                .unwrap_err()
                .contains("Point.location")
        );
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

mod xrecord;

mod staged_entity_lifecycle;

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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{h}, 'layer':'Other'}}, {{'handle':{h}, {first_patch}}}])",
            h = handle.value()
        ),
    );
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
        assert!(
            crate::io::load_bytes(name, bytes)
                .unwrap()
                .get_entity(handle)
                .is_none()
        );
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
            "doc.create_entity('PolygonMesh', m_vertex_count=3, n_vertex_count=3, vertices=verts)\n"
        ),
        edit: concat!(
            "mesh = doc.entities[HANDLE]\n",
            "verts = mesh.vertices\n",
            "verts[4]['location'] = P(1.0, 1.0, 2.5)\n",
            "with doc.transaction('Edit mesh'):\n",
            "    mesh.vertices = verts\n",
            "    mesh.m_smooth_density = 6\n",
            "    mesh.n_smooth_density = 6\n",
            "    mesh.smooth_type = 'Cubic'\n",
            "doc.selection = [mesh]\n"
        ),
        rejects: &[
            ("'m_vertex_count':4", "requires 12"),
            ("'n_vertex_count':1", "at least 2"),
            ("'m_smooth_density':-1", "non-negative"),
            ("'elevation':float('nan')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            (
                "'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}}]*9",
                "finite",
            ),
            ("'smooth_type':'Spline'", "unsupported"),
        ],
        is_kind: |entity| matches!(entity, EntityType::PolygonMesh(_)),
        digest: |entity| match entity {
            EntityType::PolygonMesh(m) => format!(
                "{}x{} v{} z{} d{}/{} {:?}",
                m.m_vertex_count,
                m.n_vertex_count,
                m.vertices.len(),
                m.vertices[4].location.z,
                m.m_smooth_density,
                m.n_smooth_density,
                m.smooth_type
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::PolygonMesh(m) = entity {
                m.vertices[0].location.z = 1.0;
            }
        },
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
            "doc.create_entity('PolyfaceMesh', vertices=verts, faces=faces)\n"
        ),
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
            "doc.selection = [mesh]\n"
        ),
        rejects: &[
            (
                "'faces':[{'index1':1,'index2':2,'index3':9}]",
                "references vertex 9",
            ),
            (
                "'faces':[{'index1':1,'index2':2}]",
                "at least 3 vertex indices",
            ),
            ("'faces':[]", "at least 1 face"),
            (
                "'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]",
                "at least 3",
            ),
            ("'thickness':float('inf')", "finite"),
            ("'seqend_handle':5", "read-only"),
        ],
        is_kind: |entity| matches!(entity, EntityType::PolyfaceMesh(_)),
        digest: |entity| match entity {
            EntityType::PolyfaceMesh(m) => format!(
                "v{} f{} z{} last {:?}",
                m.vertices.len(),
                m.faces.len(),
                m.vertices[2].location.z,
                m.faces.last().map(|f| (f.index1, f.index2, f.index3))
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::PolyfaceMesh(m) = entity {
                m.vertices[0].location.z = 1.0;
            }
        },
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
            "    start_point=P(5.0, 0.0, 0.0), radius=5.0, turns=3.0, turn_height=2.0)\n"
        ),
        edit: concat!(
            "helix = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit helix'):\n",
            "    helix.turns = 5.0\n",
            "    helix.handedness = False\n",
            "doc.selection = [helix]\n"
        ),
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
            EntityType::Helix(h) => format!(
                "r{} t{} h{} endz{:.1} ccw{} cp{}",
                h.radius,
                h.turns,
                h.turn_height,
                h.spline.control_points.last().map_or(f64::NAN, |p| p.z),
                h.handedness,
                !h.spline.control_points.is_empty()
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Helix(h) = entity {
                h.turns = 6.0;
            }
        },
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
            "    insertion_point=P(10.0, 20.0, 0.0), u_vector=P(0.5, 0.0, 0.0), v_vector=P(0.0, 0.5, 0.0))\n"
        ),
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
            "doc.selection = [img]\n"
        ),
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
            EntityType::RasterImage(i) => format!(
                "{} {:.1},{:.1} u{:.1} b{} c{} f{} clip{} n{} {}x{} def{}",
                std::path::Path::new(&i.file_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
                i.insertion_point.x,
                i.insertion_point.y,
                i.u_vector.x,
                i.brightness,
                i.contrast,
                i.fade,
                i.clipping_enabled,
                i.clip_boundary.vertices.len(),
                i.size.x,
                i.size.y,
                i.definition_handle.is_some()
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::RasterImage(i) = entity {
                i.brightness = 80;
            }
        },
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
            "    u_vector=P(10.0, 0.0, 0.0), v_vector=P(0.0, 6.0, 0.0))\n"
        ),
        edit: concat!(
            "w = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit wipeout'):\n",
            "    w.clip_type = 'Polygonal'\n",
            "    w.clip_boundary_vertices = [{'x': -0.5, 'y': -0.5}, {'x': 0.5, 'y': -0.5}, {'x': 0.0, 'y': 0.5}]\n",
            "    w.insertion_point = (2.0, 3.0, 0.0)\n",
            "    w.clip_mode = 'Inside'\n",
            "doc.selection = [w]\n"
        ),
        rejects: &[
            ("'clip_boundary_vertices':[{'x':0.0,'y':0.0}]", "at least 3"),
            ("'clip_type':'Rectangular'", "exactly 2"),
            ("'u_vector':{'x':0.0,'y':0.0,'z':0.0}", "not parallel"),
            ("'brightness':200", "within 0..=100"),
            ("'size':{'x':0.0,'y':1.0}", "greater than zero"),
            (
                "'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'definition_handle':5", "read-only"),
            ("'clip_type':'Circle'", "unsupported"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Wipeout(_)),
        digest: |entity| match entity {
            EntityType::Wipeout(w) => format!(
                "{:?} {:?} n{} at{:.1},{:.1} u{:.1} v{:.1}",
                w.clip_type,
                w.clip_mode,
                w.clip_boundary_vertices.len(),
                w.insertion_point.x,
                w.insertion_point.y,
                w.u_vector.x,
                w.v_vector.y
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Wipeout(w) = entity {
                w.u_vector.x = 12.0;
            }
        },
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
            "    active_viewport=VIEWPORTHANDLE, scale_handle=SCALEHANDLE)\n"
        ),
        edit: concat!(
            "vb = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit view border'):\n",
            "    vb.max = [60.0, 45.0]\n",
            "    vb.center = [30.0, 22.5]\n",
            "    vb.scale = 2.0\n",
            "    vb.rotation_angle = 0.3\n",
            "doc.selection = [vb]\n"
        ),
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
            EntityType::ViewBorder(v) => format!(
                "max{:.1},{:.1} c{:.1},{:.1} s{:.1} r{:.1}",
                v.max[0], v.max[1], v.center[0], v.center[1], v.scale, v.rotation_angle
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::ViewBorder(v) = entity {
                v.scale = 3.0;
            }
        },
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

mod audit_entity_lifecycle;

mod kernel_boolean_audits;

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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
            "PY_EVAL ocs.active_document.embed_picture({:?}, origin=(1, 2, 3), width=16{extra}).handle",
            file.display().to_string()
        )
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
            "PY_EVAL ocs.active_document.create_entity('Viewport', owner_handle={}, center=(50, 50, 0), width={width}, height={height}, view_height={view_height}).handle",
            paper.value()
        )
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
                Some(EntityType::RasterImage(i)) => eprintln!(
                    "SPIKE linked={linked} {label}: file_path_ok={} definition={:?} reactor={:?} defs={defs} reactors={reactors} image_dicts={dicts}",
                    i.file_path == path,
                    i.definition_handle.map(|h| h.value()),
                    i.definition_reactor_handle.map(|h| h.value())
                ),
                other => eprintln!(
                    "SPIKE linked={linked} {label}: came back as {:?}",
                    other.map(|e| format!("{e:?}").chars().take(24).collect::<String>())
                ),
            }
        }
    }
}

mod table_style_block_audits;

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
        assert!(
            process
                .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
                .unwrap()
        );
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
    assert!(
        process
            .dispatch(
                &mut host,
                &format!("PY_RUN {}", script.display()),
                &mut |_| {}
            )
            .unwrap()
    );
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
    assert!(
        !document
            .entities()
            .any(|e| matches!(e, EntityType::PolygonMesh(_)))
    );
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
    assert!(
        process
            .dispatch(
                &mut host,
                &format!("PY_RUN {}", script.display()),
                &mut |_| {}
            )
            .unwrap()
    );
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
            assert!(
                process
                    .dispatch(&mut host, &cmd, &mut |id| started = Some(id))
                    .unwrap()
            );
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({picked_token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("point")
    );

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
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({entity_token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("entity")
    );
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains(&target.value().to_string())
    );

    let cancelled_token = request(&mut app, "Cancel a point", false);
    let result = app.tabs[0].active_cmd.as_mut().unwrap().on_enter();
    let _ = app.apply_cmd_result(result);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({cancelled_token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("cancelled")
    );
}

/// A script asks for an array drag: the plugin previews the copies while the
/// cursor moves, and the last point runs the script again with the result.
#[test]
fn staged_python_drag_previews_and_resumes_the_script_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
        .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
    assert!(std::path::Path::new(&runner_path).is_file());
    let script = std::env::temp_dir().join(format!("ocs_py_drag_{}.py", std::process::id()));
    std::fs::write(
        &script,
        r#"doc = ocs.active_document
result = doc.resume()
if result is None:
    try:
        doc.request_drag(["Base", "Vector"], [], "array")
    except ValueError as error:
        print("refused:", error)
    doc.request_drag(["Base", "Vector", "End"], [[(0, 0), (1, 0), (1, 1)]], "array", "seven")
else:
    print("resumed", result["status"], result["count"], result["state"], result["points"][2])
"#,
    )
    .unwrap();
    let script_path = script.to_string_lossy().replace('\\', "/");
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
    let history = |app: &OpenCADStudio| {
        app.command_line
            .history
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut started = None;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            process
                .dispatch(&mut host, &format!("PY_RUN {script_path}"), &mut |id| {
                    started = Some(id)
                })
                .unwrap()
        );
    }
    let text = history(&app);
    assert!(
        text.contains("refused: request_drag: an array takes 3 prompts"),
        "{text}"
    );
    let id = started.unwrap_or_else(|| panic!("no drag started: {text}"));
    app.set_active_command(
        0,
        Box::new(PluginProcessInteractiveAdapter::new(
            std::sync::Arc::clone(&process),
            id,
        )),
    );
    let command = app.tabs[0].active_cmd.as_mut().unwrap();
    assert_eq!(command.prompt(), "Base");
    assert!(
        command
            .on_preview_wires(glam::DVec3::new(9.0, 9.0, 0.0))
            .is_empty()
    );
    for point in [
        glam::DVec3::new(10.0, 0.0, 0.0),
        glam::DVec3::new(12.0, 0.0, 0.0),
    ] {
        assert!(matches!(
            command.on_point(point),
            crate::command::CmdResult::NeedPoint
        ));
    }
    assert_eq!(command.prompt(), "End");
    // 6.5 units along a 2-unit vector: the rubber line and three copies
    assert_eq!(
        command
            .on_preview_wires(glam::DVec3::new(16.5, 3.0, 0.0))
            .len(),
        4
    );
    let result = command.on_point(glam::DVec3::new(16.5, 3.0, 0.0));
    let crate::command::CmdResult::Script(line) = result else {
        panic!("the last point runs the script again");
    };
    assert!(line.starts_with("PY_RESUME "), "{line}");
    assert!(line.ends_with(&script_path), "{line}");
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process.dispatch(&mut host, &line, &mut |_| {}).unwrap());
    }
    let text = history(&app);
    assert!(
        text.contains("resumed drag 3 seven [16.5, 3.0, 0.0]"),
        "{text}"
    );
    // the result is read once: the same line again is a first run
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(process.dispatch(&mut host, &line, &mut |_| {}).unwrap());
    }
    let text = history(&app);
    assert_eq!(text.matches("refused: request_drag").count(), 2, "{text}");
    app.tabs[0].active_cmd = None;
    let _ = std::fs::remove_file(&script);
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
    let prompt = app.tabs[0]
        .active_cmd
        .as_ref()
        .map(|command| command.prompt());
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.request_point('Tab zero')",
                    &mut |id| started = Some(id)
                )
                .unwrap()
        );
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("None")
    );
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap()
        );
    }
    assert!(
        app.command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("point")
    );

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
                assert!(
                    process
                        .dispatch(
                            &mut host,
                            "PY_EVAL ocs.active_document.poll_events()",
                            &mut |_| {}
                        )
                        .unwrap()
                );
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.poll_events()",
                    &mut |_| {}
                )
                .unwrap()
        );
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
        assert!(
            process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.poll_events()",
                    &mut |_| {}
                )
                .unwrap()
        );
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
    assert!(
        host.set_system_variable("CLAYER", HostSettingValue::Text("Missing".into()))
            .is_err()
    );
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
    assert!(
        host.set_system_variable("SNAPANG", HostSettingValue::Number(f64::INFINITY))
            .is_err()
    );
    assert!(
        host.set_system_variable("SNAPANG", HostSettingValue::Number(f64::MAX))
            .is_err()
    );
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

mod host_entity_writes;

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

mod layer_management;

#[test]
fn entity_creation_with_novel_layer_refreshes_gui() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;

    // Verify DYNAMIC_PLUGIN_LINES does not exist initially
    assert!(
        !app.tabs[0]
            .scene
            .document
            .layers
            .contains("DYNAMIC_PLUGIN_LINES")
    );
    assert!(
        !app.ribbon
            .layer_infos
            .iter()
            .any(|l| l.name == "DYNAMIC_PLUGIN_LINES")
    );

    // 1. Add entity via HostSession (simulating plugin add_entity / add_line)
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::from_coords(0.0, 0.0, 0.0, 10.0, 10.0, 0.0);
        line.common.layer = "DYNAMIC_PLUGIN_LINES".to_string();
        let handle = host.add_entity(document::EntityType::Line(line));
        assert_ne!(handle, document::Handle::NULL);
    }

    // Verify layer is created in document AND refreshed in ribbon + layer panel
    assert!(
        app.tabs[0]
            .scene
            .document
            .layers
            .contains("DYNAMIC_PLUGIN_LINES")
    );
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
    assert!(
        app.tabs[0]
            .scene
            .document
            .layers
            .contains("SURVEY_TRAVERSE")
    );
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
fn plugin_execute_command() {
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

mod quad_panels;

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

mod class_registration;

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
    assert!(
        HostApi::validate_parametric_chain(&host, &empty, 1e-3)
            .unwrap_err()
            .contains("at least one segment")
    );

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
        assert!(
            host.register_plugin_class("PLUGIN", &traverse_class())
                .is_ok()
        );
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

mod plugin_panels;
