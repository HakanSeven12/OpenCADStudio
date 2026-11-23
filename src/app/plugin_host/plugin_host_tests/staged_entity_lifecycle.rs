use super::*;

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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Move line', [{{'handle':{},'end':{{'x':5.0,'y':0.0,'z':0.0}}}}])",
            handle.value()
        ),
    );
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
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject NaN', [{{'handle':{},'start':{{'x':float('nan'),'y':0.0,'z':0.0}}}}])",
            handle.value()
        ),
    );
    assert_eq!(
        host.document().get_entity(handle),
        Some(&expected_after_edit),
        "invalid geometry changed the line"
    );
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("finite coordinates")
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject duplicate', [{{'handle':{},'end':{{'x':8.0,'y':0.0,'z':0.0}}}},{{'handle':{},'end':{{'x':9.0,'y':0.0,'z':0.0}}}}])",
            handle.value(),
            handle.value()
        ),
    );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("POSITION%%v0.1")
    );

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

    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject direction', [{{'handle':{},'direction':{{'x':2.0,'y':0.0,'z':0.0}}}}])",
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("unit vector")
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.update_many('Reject style', [{{'handle':{},'dimension_style_name':'Missing'}}])",
            handle.value()
        ),
    );
    assert_eq!(host.document().get_entity(handle), Some(&expected));
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("does not exist")
    );

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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("ARROW")
    );

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
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("does not exist")
    );

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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        host.document()
            .block_records
            .get("TAGBLOCK")
            .unwrap()
            .entity_handles
            .contains(&handle)
    );
    let created = created.clone();
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL (ocs.active_document.entities[{}].tag, ocs.active_document.entities[{}].owner_handle)",
            handle.value(),
            handle.value()
        ),
    );
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
    assert!(strokes.iter().any(|stroke| {
        stroke
            .run
            .as_ref()
            .is_some_and(|run| run.text.contains("PART_NO"))
    }));

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
        assert!(
            host.app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(message)
        );
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
        assert!(
            document
                .block_records
                .get("TAGBLOCK")
                .unwrap()
                .entity_handles
                .contains(&handle)
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            host.app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(message)
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        host.app.tabs[0]
            .scene
            .hatches
            .get(&hatch_handle)
            .is_some_and(|model| model.boundary.len() >= 4 && model.name == "SOLID")
    );
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.entities[{}].pattern['name']",
            hatch_handle.value()
        ),
    );
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("SOLID")
    );

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
    assert!(
        host.app.tabs[0]
            .scene
            .hatches
            .get(&hatch_handle)
            .is_some_and(|model| model.boundary.len() >= 4
                && model.name == "TEST"
                && matches!(&model.pattern,
                    crate::scene::model::hatch_model::HatchPattern::Pattern(lines)
                        if !lines.is_empty()))
    );

    for (patch, message) in [
        ("'pattern_scale':0.0", "greater than zero"),
        ("'is_associative':True", "requires boundary handles"),
        ("'gradient_color':{}", "outside the editable schema"),
        (
            "'paths':[{'flags':1,'edges':[{'kind':'Line','value':{'start':{'x':0.0,'y':0.0},'end':{'x':1.0,'y':0.0}}}],'boundary_handles':[]}]",
            "not closed",
        ),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject hatch', [{{'handle':{}, {patch}}}])",
                hatch_handle.value()
            ),
        );
        assert_eq!(host.document().get_entity(hatch_handle), Some(&expected));
        assert!(
            host.app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(message)
        );
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
        assert!(
            crate::io::load_bytes(name, bytes)
                .unwrap()
                .get_entity(hatch_handle)
                .is_none()
        );
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
    assert!(
        app.tabs[0]
            .scene
            .document
            .get_entity(hatch_handle)
            .is_none()
    );
    app.redo_steps(3);
    assert!(
        app.tabs[0]
            .scene
            .document
            .get_entity(hatch_handle)
            .is_none()
    );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            points
                .iter()
                .flatten()
                .all(|value| value.is_nan() || value.is_finite())
        );
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
    assert!(
        app.tabs[0]
            .scene
            .document
            .get_entity(leader_handle)
            .is_none()
    );
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_some());
    app.undo_steps(1);
    assert!(app.tabs[0].scene.document.get_entity(text_handle).is_none());
    app.redo_steps(4);
    assert!(
        app.tabs[0]
            .scene
            .document
            .get_entity(leader_handle)
            .is_none()
    );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.points)
                .flatten()
                .all(|v| v.is_finite())
        );
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
        (
            "'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]",
            "at least 2",
        ),
        ("'scale_factor':0.0", "nonzero"),
        ("'flags':64", "unknown bits"),
        (
            "'flags':3,'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]",
            "requires at least 3",
        ),
        (
            "'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':0.0,'y':0.0,'z':0.0}}]",
            "coincide",
        ),
        (
            "'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':float('nan'),'z':0.0}}]",
            "finite",
        ),
        (
            "'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0},'segments':[{}]},{'position':{'x':1.0,'y':0.0,'z':0.0}}]",
            "segments but style",
        ),
        ("'start_point':{'x':5.0,'y':5.0,'z':0.0}", "read-only"),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject mline', [{{'handle':{}, {patch}}}])",
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    // (subtype, python fields, expected measurement)
    let fixtures: [(&str, &str, f64); 9] = [
        (
            "Linear",
            "definition_point=P(10,5), first_point=P(0,0), second_point=P(10,0)",
            10.0,
        ),
        (
            "Aligned",
            "definition_point=P(1,5.5), first_point=P(0,0), second_point=P(3,4)",
            5.0,
        ),
        (
            "Radius",
            "definition_point=P(5,0), angle_vertex=P(0,0)",
            5.0,
        ),
        (
            "Diameter",
            "definition_point=P(10,0), angle_vertex=P(0,0)",
            10.0,
        ),
        (
            "Angular3Pt",
            "definition_point=P(3,3), first_point=P(5,0), second_point=P(0,5), angle_vertex=P(0,0)",
            std::f64::consts::FRAC_PI_2,
        ),
        (
            "Angular2Ln",
            "definition_point=P(0,5), dimension_arc=P(3,3), first_point=P(0,0), second_point=P(5,0), angle_vertex=P(0,0)",
            std::f64::consts::FRAC_PI_2,
        ),
        (
            "Ordinate",
            "feature_location=P(5,3), leader_endpoint=P(5,8), is_ordinate_type_x=True",
            5.0,
        ),
        (
            "Arc",
            "definition_point=P(3.5,3.5), first_extension_point=P(5,0), second_extension_point=P(0,5), center_point=P(0,0), arc_start_parameter=0.0, arc_end_parameter=1.5707963267948966",
            5.0 * std::f64::consts::FRAC_PI_2,
        ),
        (
            "LargeRadial",
            "definition_point=P(10,0), chord_point=P(4,0)",
            6.0,
        ),
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
        (
            "ocs.active_document.create_entity('Dimension', definition_point=P(0,0))",
            "requires subtype",
        ),
        (
            "ocs.active_document.create_entity('Dimension', subtype='Cone')",
            "unsupported Dimension subtype",
        ),
        (
            "ocs.active_document.create_entity('Dimension', subtype='Linear', first_point=P(0,0), second_point=P(1,0))",
            "requires definition_point",
        ),
    ] {
        dispatch(
            &mut host,
            &format!("PY_EVAL (lambda P: {patch})(lambda x, y: {{'x': x, 'y': y, 'z': 0.0}})"),
        );
        assert_eq!(handles_by_subtype(host.document()).len(), 9, "{patch}");
        assert!(
            last_output(&host).contains(message),
            "{patch}: {}",
            last_output(&host)
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
    };
    let last_output =
        |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

    dispatch(
        &mut host,
        "PY_EVAL ocs.active_document.create_entity('Table', insertion_point={'x':5.0,'y':5.0,'z':0.0}).handle",
    );
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
        (
            "'columns':[{'name':'A','width':0.0},{'width':1.0},{'width':1.0}]",
            "greater than zero",
        ),
        (
            "'columns':[{'width':5.0}]",
            "cells but the table has 1 columns",
        ),
        ("'columns':[]", "at least one row and one column"),
        (
            "'merged_ranges':[{'top_row':0,'left_col':0,'bottom_row':9,'right_col':0}]",
            "outside the",
        ),
        (
            "'merged_ranges':[{'top_row':1,'left_col':0,'bottom_row':2,'right_col':1},{'top_row':2,'left_col':1,'bottom_row':3,'right_col':2}]",
            "overlaps",
        ),
        ("'table_style_handle':999999", "does not exist"),
        ("'block_name':'x'", "read-only"),
        ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ("'break_spacing':-1.0", "non-negative"),
        (
            "'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}",
            "finite",
        ),
    ] {
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.update_many('Reject table', [{{'handle':{}, {patch}}}])",
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
