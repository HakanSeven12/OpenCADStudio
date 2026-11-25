use super::*;

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
                [
                    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 5., 5., 5., 1.
                ]
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        read(
            &mut host,
            &format!("ocs.active_document.entities[{}].kind", list[0].value())
        )
        .contains("Solid3D")
    );

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
    dispatch(
        &mut host,
        &format!(
            "PY_EVAL ocs.active_document.solids.rotate({}, 'z', 0.7853981633974483, (5, 5, 5)).handle",
            box_handle.value()
        ),
    );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
    assert!(
        host.app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("Body")
    );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
            "    {'point': P(40.0, 0.0, 0.0), 'label': 'A', 'label_offset': P(0.0, 2.0, 0.0)}])\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.points = [{'point': P(0.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, 3.0, 0.0)},\n",
            "        {'point': P(20.0, 10.0, 0.0), 'bulge': 0.0}, {'point': P(40.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, -3.0, 0.0)}]\n",
            "    e.symbol_scale = 2.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'points':[{'point':{'x':0.0,'y':0.0,'z':0.0}}]",
                "at least 2 points",
            ),
            (
                "'points':[{'point':{'x':float('nan'),'y':0.0,'z':0.0}},{'point':{'x':1.0,'y':0.0,'z':0.0}}]",
                "finite",
            ),
            ("'symbol_scale':0.0", "greater than zero"),
            ("'style_handle':999999", "not an existing SectionViewStyle"),
            ("'view_rep_handle':999999", "not an existing ViewRep"),
            ("'raw_point_count_90':9", "read-only"),
            ("'end_a':[1.0, 1.0]", "read-only"),
        ],
        is_kind: |entity| matches!(entity, EntityType::SectionSymbol(_)),
        digest: |entity| match entity {
            EntityType::SectionSymbol(v) => format!(
                "n{} ends {:.1},{:.1}->{:.1},{:.1} label{} tick{:.1}/{:.1} scale{:.1} counts{}/{}",
                v.points.len(),
                v.end_a[0],
                v.end_a[1],
                v.end_b[0],
                v.end_b[1],
                v.label,
                v.tick_a,
                v.tick_b,
                v.symbol_scale,
                v.raw_point_count_90,
                v.raw_point_record_count
            ),
            other => format!(
                "wrong kind {}",
                format!("{other:?}").split('(').next().unwrap()
            ),
        },
        reedit: |entity| {
            if let EntityType::SectionSymbol(v) = entity {
                v.symbol_scale = 3.0;
            }
        },
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
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
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
            "PY_EVAL ocs.active_document.create_entity('SectionSymbol', points=[{{'point': {{'x': 0.0, 'y': 0.0, 'z': 0.0}}}}, {{'point': {{'x': 10.0, 'y': 0.0, 'z': 0.0}}}}]{extra}).handle"
        )
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
