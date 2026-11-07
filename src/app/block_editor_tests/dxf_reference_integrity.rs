use super::*;
use document::{
    objects::{
        BlockEvaluationGraph, Dictionary, DictionaryWithDefault, DynamicBlockData, ObjectType,
        PlaceHolder, ProxyObject, ProxyObjectReference, ProxyPayload, ProxyReferenceKind,
        RegisteredClassObject, SemanticProperty, SemanticPropertyValue, SortEntitiesTable,
        XRecord,
    },
    tables::AppId,
    xdata::{ExtendedData, ExtendedDataRecord, XDataValue},
    DxfVersion, Document,
};

struct DimContainerFixture {
    app: OpenCADStudio,
    _block_def: Handle,
    param_handle: Handle,
    dim_handles: [Handle; 3],
    dummy_handle: Handle,
    dummy_ext_dict_handle: Handle,
    surviving_xrecord: Handle,
}

fn encode_synthetic_dimcontainer2(
    version: DxfVersion,
    handle: Handle,
    owner: Handle,
    class_number: i16,
) -> (Vec<u8>, i64) {
    use document::io::dwg::dwg_stream_writers::DwgMergedWriter;
    use document::io::dwg::{DwgReferenceType, DwgVersion};

    let dwg_version = DwgVersion::from_dxf_version(version).unwrap();
    let mut writer = DwgMergedWriter::new(dwg_version, version);
    writer.write_object_type(class_number);
    if version < DxfVersion::AC1024 {
        writer.save_position_for_size();
    }
    writer.main_mut().write_handle_undefined(handle.value());
    // EED size = 0
    writer.write_bit_short(0);
    // Owner handle
    writer.write_handle(DwgReferenceType::SoftPointer, owner.value());
    // Reactors count = 0
    writer.write_bit_long(0);
    // XDictionary
    if version >= DxfVersion::AC1018 {
        writer.write_bit(true); // no xdic
    } else {
        writer.write_handle(DwgReferenceType::HardOwnership, 0);
    }
    // DS data flag (R2013+)
    if version >= DxfVersion::AC1027 {
        writer.write_bit(false);
    }
    let raw = writer.merge();
    let handle_bits = raw.len() as i64 * 8 - writer.handle_start_bits();
    (raw, handle_bits)
}

fn synthetic_dimcontainer2_dynamic_block_fixture() -> DimContainerFixture {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    begin(&mut app, reference);
    let block_def = app.tabs[i]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let target_line = members(&app)[0];

    let param_handle = crate::scene::block_distance_parameter::create(
        &mut app.tabs[i].scene.document,
        block_def,
        crate::scene::block_distance_parameter::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::UNIT_Y,
    )
    .expect("create linear parameter");
    crate::scene::block_actions::create(
        &mut app.tabs[i].scene.document,
        block_def,
        crate::scene::block_actions::Kind::Move,
        param_handle,
        &[target_line],
        glam::DVec3::ZERO,
    )
    .expect("create move action");
    app.close_block_edit(i, false);

    let evaluated = crate::scene::block_actions::instance::evaluate(
        &app.tabs[i].scene.document,
        reference,
        param_handle,
        glam::DVec3::new(15.0, 0.0, 0.0),
    )
    .expect("evaluate dynamic block")
    .expect("evaluated document");
    app.tabs[i].scene.document = evaluated;

    let doc = &mut app.tabs[i].scene.document;
    doc.version = DxfVersion::AC1032;
    doc.dwg_source_version = Some(DxfVersion::AC1032);

    let mut app_id = AppId::new("SYNTH_MECH");
    app_id.handle = doc.allocate_handle();
    let _ = doc.app_ids.add(app_id);

    let mut dim_class = document::classes::DxfClass::new("DIMCONTAINER2", "AcDbDimContainer2");
    dim_class.class_number = 500;
    doc.classes.push_preserving(dim_class);

    let root_dict = doc.header.named_objects_dict_handle;
    let dim_handles = [
        Handle::new(0x3F5A),
        Handle::new(0x3F6B),
        Handle::new(0x3F7C),
    ];
    for handle in dim_handles {
        while doc.header.handle_seed <= handle.value() {
            let _ = doc.allocate_handle();
        }
    }

    // 1. Subdictionary in NOD pointing to two DIMCONTAINER2 records and one surviving XRecord.
    let subdict_handle = doc.allocate_handle();
    let mut subdict = Dictionary::new();
    subdict.handle = subdict_handle;
    subdict.owner = root_dict;
    subdict.add_entry("DIM_A", dim_handles[0]);
    subdict.add_entry("DIM_B", dim_handles[1]);
    subdict.set_entry_hard_owner("DIM_A", true);

    let surviving_xrecord = doc.allocate_handle();
    let mut xrec = XRecord::new();
    xrec.handle = surviving_xrecord;
    xrec.owner = subdict_handle;
    xrec.reactors.push(dim_handles[0]);
    xrec.add_string(1, "SURVIVING_DATA");
    xrec.add_handle(340, dim_handles[1]);
    xrec.object_references.push(ProxyObjectReference {
        kind: ProxyReferenceKind::SoftPointer,
        handle: dim_handles[2],
    });
    subdict.add_entry("KEEP_XREC", surviving_xrecord);

    doc.objects
        .insert(subdict_handle, ObjectType::Dictionary(subdict));
    doc.objects
        .insert(surviving_xrecord, ObjectType::XRecord(xrec));
    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_dict) {
        root.add_entry("SYNTH_DIM_CONTAINERS", subdict_handle);
    }

    // 2. Three DWG-only DIMCONTAINER2 Unknown objects (no raw_dxf_codes) with valid DWG records.
    for (idx, handle) in dim_handles.into_iter().enumerate() {
        let owner = if idx < 2 { subdict_handle } else { root_dict };
        let (raw, handle_bits) =
            encode_synthetic_dimcontainer2(DxfVersion::AC1032, handle, owner, 500);
        doc.objects.insert(
            handle,
            ObjectType::Unknown {
                type_name: "DIMCONTAINER2".to_string(),
                handle,
                owner,
                raw_dxf_codes: None,
                raw_dwg_data: Some(raw),
                raw_dwg_handle_bits: handle_bits,
                raw_dwg_version: Some(DxfVersion::AC1032),
            },
        );
    }

    // Attach object_xdata to an omitted DIMCONTAINER2 so write_objects must not
    // leak trailing XDATA onto a preceding object in OBJECTS.
    let mut omitted_xdata = ExtendedData::new();
    let mut omitted_rec = ExtendedDataRecord::new("SYNTH_MECH");
    omitted_rec.add_value(XDataValue::String("OMITTED".into()));
    omitted_xdata.add_record(omitted_rec);
    doc.object_xdata.insert(dim_handles[0], omitted_xdata);

    // 3. Cascading child objects owned by DIMCONTAINER2 (extension dictionary + child XRecord).
    let child_ext_dict_handle = doc.allocate_handle();
    let mut child_ext_dict = Dictionary::new();
    child_ext_dict.handle = child_ext_dict_handle;
    child_ext_dict.owner = dim_handles[0];
    let grand_child_xrec_handle = doc.allocate_handle();
    let mut grand_child_xrec = XRecord::new();
    grand_child_xrec.handle = grand_child_xrec_handle;
    grand_child_xrec.owner = child_ext_dict_handle;
    grand_child_xrec.add_string(1, "OWNED_BY_OMITTED");
    child_ext_dict.add_entry("CHILD", grand_child_xrec_handle);
    doc.objects
        .insert(child_ext_dict_handle, ObjectType::Dictionary(child_ext_dict));
    doc.objects.insert(
        grand_child_xrec_handle,
        ObjectType::XRecord(grand_child_xrec),
    );

    let direct_child_xrec_handle = doc.allocate_handle();
    let mut direct_child_xrec = XRecord::new();
    direct_child_xrec.handle = direct_child_xrec_handle;
    direct_child_xrec.owner = dim_handles[1];
    direct_child_xrec.add_string(1, "DIRECT_CHILD_OF_DIMCONTAINER2");
    doc.objects.insert(
        direct_child_xrec_handle,
        ObjectType::XRecord(direct_child_xrec),
    );

    // 4. Standalone entity whose extension dictionary only contains DIMCONTAINER2[2].
    let dummy_handle = doc.allocate_handle();
    let mut dummy_line = document::entities::Line::new();
    dummy_line.start = document::types::Vector3::new(0.0, 0.0, 0.0);
    dummy_line.end = document::types::Vector3::new(10.0, 10.0, 0.0);
    dummy_line.common.handle = dummy_handle;
    dummy_line.common.owner_handle = doc.header.model_space_block_handle;
    let dummy_ext_dict_handle = doc.allocate_handle();
    let mut dummy_ext_dict = Dictionary::new();
    dummy_ext_dict.handle = dummy_ext_dict_handle;
    dummy_ext_dict.owner = dummy_handle;
    dummy_ext_dict.add_entry("ONLY_OMITTED", dim_handles[2]);
    doc.objects.insert(
        dummy_ext_dict_handle,
        ObjectType::Dictionary(dummy_ext_dict),
    );
    dummy_line.common.xdictionary_handle = Some(dummy_ext_dict_handle);
    doc.add_entity(EntityType::Line(dummy_line))
        .expect("add dummy entity");

    // 5. Reactors and XDATA handle on the dynamic block reference INSERT and parameter.
    if let Some(insert_entity) = doc.get_entity_mut(reference) {
        let common = insert_entity.common_mut();
        common.reactors.push(dim_handles[0]);
        let mut rec = ExtendedDataRecord::new("SYNTH_MECH");
        rec.add_value(XDataValue::Handle(dim_handles[0]));
        common.extended_data.add_record(rec);
    }

    if let Some(ObjectType::DynamicBlock(param_obj)) = doc.objects.get_mut(&param_handle) {
        param_obj.reactors.push(dim_handles[1]);
    }

    // 6. DictionaryWithDefault, ProxyObject, RegisteredClassObject, and SortEntitiesTable references.
    let placeholder_handle = doc.allocate_handle();
    let dict_default_handle = doc.allocate_handle();
    let mut placeholder = PlaceHolder::default();
    placeholder.handle = placeholder_handle;
    placeholder.owner = dict_default_handle;
    let mut dict_default = DictionaryWithDefault::default();
    dict_default.handle = dict_default_handle;
    dict_default.owner = root_dict;
    dict_default.default_handle = dim_handles[0];
    dict_default
        .entries
        .push(("OMITTED_STYLE".into(), dim_handles[0]));
    dict_default
        .entries
        .push(("NORMAL_STYLE".into(), placeholder_handle));
    doc.objects
        .insert(placeholder_handle, ObjectType::PlaceHolder(placeholder));
    doc.objects.insert(
        dict_default_handle,
        ObjectType::DictionaryWithDefault(dict_default),
    );
    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_dict) {
        root.add_entry("SYNTH_DEFAULT_DICT", dict_default_handle);
    }

    let proxy_handle = doc.allocate_handle();
    let proxy = ProxyObject {
        handle: proxy_handle,
        owner: subdict_handle,
        reactors: vec![dim_handles[2]],
        xdictionary_handle: None,
        proxy_id: 499,
        class_id: 499,
        dxf_subclass: "AcDbProxyObject".into(),
        version: 0,
        dwg_version: 21,
        maintenance_version: 0,
        from_dxf: false,
        payload: ProxyPayload::from_bytes(&[1, 2, 3, 4]),
        text_payload: ProxyPayload::default(),
        object_ids: vec![
            ProxyObjectReference {
                kind: ProxyReferenceKind::SoftPointer,
                handle: dim_handles[0],
            },
            ProxyObjectReference {
                kind: ProxyReferenceKind::HardPointer,
                handle: surviving_xrecord,
            },
        ],
        dxf_data: Vec::new(),
    };
    doc.objects
        .insert(proxy_handle, ObjectType::ProxyObject(proxy));

    let reg_handle = doc.allocate_handle();
    let reg = RegisteredClassObject {
        dxf_name: "MCDBCONTAINER2".into(),
        cpp_class_name: "McDbContainer2".into(),
        handle: reg_handle,
        owner: subdict_handle,
        reactors: vec![dim_handles[1]],
        xdictionary_handle: None,
        properties: vec![SemanticProperty {
            subclass: "McDbContainer2".into(),
            code: 330,
            value: SemanticPropertyValue::Handle(dim_handles[2]),
        }],
        payload: ProxyPayload::default(),
        object_ids: vec![ProxyObjectReference {
            kind: ProxyReferenceKind::SoftPointer,
            handle: dim_handles[1],
        }],
        raw_dwg_data: None,
        raw_dwg_handle_bits: 0,
        raw_dwg_version: None,
    };
    doc.objects
        .insert(reg_handle, ObjectType::RegisteredClass(reg));

    let sort_handle = doc.allocate_handle();
    let mut sort_table = SortEntitiesTable::for_block(doc.header.model_space_block_handle);
    sort_table.handle = sort_handle;
    sort_table.owner_handle = subdict_handle;
    sort_table.add_entry(reference, Handle::new(1));
    sort_table.add_entry(dim_handles[2], Handle::new(2));
    doc.objects
        .insert(sort_handle, ObjectType::SortEntitiesTable(sort_table));

    if let Some(ObjectType::Dictionary(sub)) = doc.objects.get_mut(&subdict_handle) {
        sub.add_entry("KEEP_PROXY", proxy_handle);
        sub.add_entry("KEEP_REG", reg_handle);
        sub.add_entry("KEEP_SORT", sort_handle);
    }

    DimContainerFixture {
        app,
        _block_def: block_def,
        param_handle,
        dim_handles,
        dummy_handle,
        dummy_ext_dict_handle,
        surviving_xrecord,
    }
}

fn temp_test_path(stem: &str, ext: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "ocs_{stem}_{}_{id}.{ext}",
        std::process::id()
    ))
}

fn evaluation_graph_handle(doc: &Document) -> Handle {
    doc.objects
        .values()
        .find_map(|object| match object {
            ObjectType::DynamicBlock(object) => matches!(
                object.data,
                DynamicBlockData::EvaluationGraph(_)
            )
            .then_some(object.handle),
            _ => None,
        })
        .expect("the authored definition carries an evaluation graph")
}

#[test]
fn same_version_dwg_preserves_dimcontainer2_records_and_dynamic_block_graph() {
    let mut fx = synthetic_dimcontainer2_dynamic_block_fixture();
    let i = fx.app.active_tab;
    assert_eq!(
        crate::io::dropped_on_save_count(
            &fx.app.tabs[i].scene.document,
            DxfVersion::AC1032,
            false
        ),
        0
    );

    let dwg_path = temp_test_path("dimcontainer2_same_ver", "dwg");
    let req = serde_json::json!({
        "path": dwg_path.to_string_lossy(),
        "target_version": "2018",
        "allow_lossy": false,
    });
    let res = fx.app.save_verified_request(&req).expect("save_verified DWG");
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["verified"], true, "{res}");
    assert_eq!(res["dropped_on_save"], 0, "{res}");

    let reopened = crate::io::load_file(&dwg_path).expect("reopen saved DWG");
    let _ = std::fs::remove_file(&dwg_path);

    for handle in fx.dim_handles {
        assert!(
            matches!(
                reopened.objects.get(&handle),
                Some(ObjectType::Unknown { type_name, .. }) if type_name == "DIMCONTAINER2"
            ),
            "same-version DWG must preserve DIMCONTAINER2 {handle:?}"
        );
    }
    let graph_handle = evaluation_graph_handle(&reopened);
    let ObjectType::DynamicBlock(eval_obj) = reopened
        .objects
        .get(&graph_handle)
        .expect("evaluation graph object")
    else {
        panic!("expected DynamicBlock object");
    };
    let DynamicBlockData::EvaluationGraph(BlockEvaluationGraph { nodes, .. }) = &eval_obj.data
    else {
        panic!("expected EvaluationGraph data");
    };
    assert!(
        nodes.iter().any(|n| n.expression == fx.param_handle),
        "same-version DWG must preserve dynamic block parameter expression in graph"
    );
}

#[test]
fn dxf_export_warns_when_unacknowledged_and_prunes_dangling_references_when_acknowledged() {
    let mut fx = synthetic_dimcontainer2_dynamic_block_fixture();
    let i = fx.app.active_tab;
    assert_eq!(
        crate::io::dropped_on_save_count(
            &fx.app.tabs[i].scene.document,
            DxfVersion::AC1032,
            true
        ),
        3
    );

    let dxf_path = temp_test_path("dimcontainer2_lossy", "dxf");
    let audit_req = serde_json::json!({
        "op": "audit",
        "path": dxf_path.to_string_lossy(),
        "target_version": "2018",
    });
    let audit = fx.app.automation_op(&audit_req.to_string());
    assert_eq!(audit["ok"], true, "{audit}");
    assert_eq!(audit["status"], "warning", "{audit}");
    assert_eq!(audit["target"]["dropped_on_save"], 3, "{audit}");

    // Unacknowledged lossy DXF conversion must be refused.
    let refused_req = serde_json::json!({
        "path": dxf_path.to_string_lossy(),
        "target_version": "2018",
        "allow_lossy": false,
    });
    let refused = fx.app.save_verified_request(&refused_req).unwrap_err();
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["code"], "lossy_conversion_not_acknowledged",
        "{refused}"
    );
    assert_eq!(refused["dropped_on_save"], 3, "{refused}");
    assert!(!dxf_path.exists(), "refused save must not write output file");

    // Acknowledged lossy DXF conversion must prune references to omitted records
    // and pass structural DXF verification without dangling or orphaned handles.
    let allowed_req = serde_json::json!({
        "path": dxf_path.to_string_lossy(),
        "target_version": "2018",
        "allow_lossy": true,
    });
    let saved = fx.app.save_verified_request(&allowed_req).expect("acknowledged lossy save");
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["verified"], true, "{saved}");
    assert_eq!(saved["dropped_on_save"], 3, "{saved}");
    assert_eq!(saved["dxf_structure"]["ok"], true, "{saved}");
    assert_eq!(
        saved["dxf_structure"]["dangling_handles"],
        serde_json::json!([]),
        "{saved}"
    );
    assert_eq!(
        saved["dxf_structure"]["orphaned_objects"],
        serde_json::json!([]),
        "{saved}"
    );
    assert_eq!(
        saved["dxf_structure"]["duplicate_handles"],
        serde_json::json!([]),
        "{saved}"
    );

    let reopened = crate::io::load_file(&dxf_path).expect("reopen saved DXF");
    let _ = std::fs::remove_file(&dxf_path);

    for handle in fx.dim_handles {
        assert!(
            !reopened.objects.contains_key(&handle),
            "omitted DIMCONTAINER2 {handle:?} must not appear in reopened DXF"
        );
    }

    // Surviving XRecord keeps its string entry and nulls the omitted handle slot.
    let Some(ObjectType::XRecord(reopened_xrec)) = reopened.objects.get(&fx.surviving_xrecord)
    else {
        panic!("surviving XRecord missing after DXF export");
    };
    assert_eq!(reopened_xrec.get_string(1), Some("SURVIVING_DATA"));
    assert_eq!(
        reopened_xrec
            .get_first_by_code(340)
            .and_then(|e| e.value.as_handle()),
        Some(Handle::NULL)
    );

    // Standalone entity's extension dictionary (which only pointed to an omitted object) was pruned.
    let dummy_entity = reopened
        .get_entity(fx.dummy_handle)
        .expect("dummy entity in reopened DXF");
    assert!(
        dummy_entity.common().xdictionary_handle.is_none(),
        "extension dictionary pointing solely to omitted record must be pruned"
    );
    assert!(
        !reopened.objects.contains_key(&fx.dummy_ext_dict_handle),
        "pruned extension dictionary must not be written to DXF"
    );

    // Supported dynamic block graph remains intact and evaluable after DXF round-trip.
    let graph_handle = evaluation_graph_handle(&reopened);
    let ObjectType::DynamicBlock(eval_obj) = reopened
        .objects
        .get(&graph_handle)
        .expect("evaluation graph object in reopened DXF")
    else {
        panic!("expected DynamicBlock object in reopened DXF");
    };
    let DynamicBlockData::EvaluationGraph(BlockEvaluationGraph { nodes, .. }) = &eval_obj.data
    else {
        panic!("expected EvaluationGraph data in reopened DXF");
    };
    assert!(
        nodes.iter().any(|n| n.expression == fx.param_handle),
        "linear parameter preserved in dynamic block graph"
    );

    let insert_handle = reopened
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(ins) => Some(ins.common.handle),
            _ => None,
        })
        .expect("insert entity in reopened DXF");
    let evaluated = crate::scene::block_actions::instance::evaluate(
        &reopened,
        insert_handle,
        fx.param_handle,
        glam::DVec3::new(20.0, 0.0, 0.0),
    )
    .expect("evaluate dynamic block after reopened DXF")
    .expect("evaluated document after reopened DXF");
    assert!(
        evaluated.entities().count() > 0,
        "dynamic block remains evaluable after lossy DXF round-trip"
    );
}

#[test]
fn dxf_verification_still_rejects_broken_essential_dynamic_block_references() {
    let mut fx = synthetic_dimcontainer2_dynamic_block_fixture();
    let i = fx.app.active_tab;

    // Corrupt an essential expression handle inside the surviving EvaluationGraph
    // to a non-existent handle (0xDEAD) that is NOT an omitted unsupported record.
    let eval_graph_handle = evaluation_graph_handle(&fx.app.tabs[i].scene.document);
    let Some(ObjectType::DynamicBlock(eval_obj)) = fx.app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&eval_graph_handle)
    else {
        panic!("evaluation graph object missing");
    };
    let DynamicBlockData::EvaluationGraph(BlockEvaluationGraph { nodes, .. }) = &mut eval_obj.data
    else {
        panic!("expected EvaluationGraph data");
    };
    nodes[0].expression = Handle::new(0xDEAD);

    let dxf_path = temp_test_path("dimcontainer2_broken_essential", "dxf");
    let req = serde_json::json!({
        "path": dxf_path.to_string_lossy(),
        "target_version": "2018",
        "allow_lossy": true,
    });
    let res = fx.app.save_verified_request(&req).unwrap_err();
    let _ = std::fs::remove_file(&dxf_path);

    assert_eq!(res["ok"], false, "{res}");
    assert_eq!(res["code"], "invalid_dxf_handle_graph", "{res}");
    let dangling = res["dxf_structure"]["dangling_handles"]
        .as_array()
        .expect("dangling_handles array");
    assert!(
        dangling.iter().any(|v| v.as_str() == Some("DEAD")),
        "broken essential expression handle 0xDEAD must still be caught: {res}"
    );
}
