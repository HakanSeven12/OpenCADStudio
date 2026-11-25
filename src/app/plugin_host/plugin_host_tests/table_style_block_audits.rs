use super::*;

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
        assert!(
            process
                .dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {})
                .unwrap()
        );
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
