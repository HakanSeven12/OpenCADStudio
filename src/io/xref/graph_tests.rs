use super::{graph::*, *};
use document::entities::{Insert, Line};
use document::tables::BlockRecord;

fn source(children: &[(&str, bool)]) -> Arc<ParsedXrefSource> {
    let mut doc = Document::new();
    doc.add_entity(EntityType::Line(Line::from_coords(0., 0., 0., 1., 0., 0.)))
        .unwrap();
    for (name, overlay) in children {
        let mut br = BlockRecord::new(*name);
        br.handle = doc.allocate_handle();
        br.block_entity_handle = doc.allocate_handle();
        br.block_end_handle = doc.allocate_handle();
        br.flags.is_xref = true;
        br.flags.is_xref_overlay = *overlay;
        br.xref_path = name.to_string();
        doc.block_records.add(br).unwrap();
        doc.add_entity(EntityType::Insert(Insert::new(*name, Vector3::ZERO)))
            .unwrap();
    }
    Arc::new(ParsedXrefSource {
        document: doc,
        context: None,
        content: None,
        path: PathBuf::from("/project/source.dwg"),
        unavailable: false,
    })
}

#[derive(Default)]
struct Access {
    sources: HashMap<XrefResourceId, Arc<ParsedXrefSource>>,
    reads: usize,
}
fn id(name: &str) -> XrefResourceId {
    XrefResourceId::Desktop(name.into())
}
impl XrefResourceAccess for Access {
    fn resolve(
        &mut self,
        _: &XrefResourceId,
        _: &ParsedXrefSource,
        edge: &XrefEdge,
    ) -> Result<XrefResourceId, String> {
        let id = id(&edge.locator);
        if self.sources.contains_key(&id) {
            Ok(id)
        } else {
            Err("Source not found".into())
        }
    }
    fn read(
        &mut self,
        id: &XrefResourceId,
        _: &ParsedXrefSource,
        _: &XrefEdge,
    ) -> Result<Arc<ParsedXrefSource>, String> {
        self.reads += 1;
        self.sources
            .get(id)
            .cloned()
            .ok_or("Source not found".into())
    }
}

#[test]
fn diamond_and_repeated_placements_are_not_cycles() {
    let root = source(&[("B", false), ("C", false)]);
    let mut access = Access::default();
    access.sources.insert(id("B"), source(&[("D", false)]));
    access.sources.insert(id("C"), source(&[("D", false)]));
    access.sources.insert(id("D"), source(&[]));
    let graph = prepare_xref_closure(id("A"), root, 10, &[], &mut access).unwrap();
    assert_eq!(graph.rows.len(), 4);
    assert_eq!(
        graph
            .rows
            .iter()
            .filter(|r| r.name == "D" && r.status == "Loaded")
            .count(),
        2
    );
    assert!(graph.diagnostics.is_empty());
    assert_eq!(access.reads, 3);
    assert_eq!(
        graph
            .document
            .entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        5
    );
}

#[test]
fn self_root_and_internal_cycles_cut_only_closing_edge() {
    let root = source(&[("B", false), ("D", false)]);
    let mut access = Access::default();
    access.sources.insert(id("A"), Arc::clone(&root));
    access.sources.insert(id("B"), source(&[("C", false)]));
    access.sources.insert(id("C"), source(&[("B", false)]));
    access.sources.insert(id("D"), source(&[]));
    let graph = prepare_xref_closure(id("A"), root, 10, &[], &mut access).unwrap();
    assert_eq!(
        graph.rows.iter().filter(|r| r.status == "Circular").count(),
        1
    );
    assert!(graph
        .rows
        .iter()
        .any(|r| r.name == "D" && r.status == "Loaded"));
    assert_eq!(access.reads, 3);
    let graph = prepare_xref_closure(id("A"), source(&[]), 10, &[id("A")], &mut access).unwrap();
    assert!(!graph.diagnostics.is_empty());
    assert_eq!(
        graph
            .document
            .entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        0
    );
}

#[test]
fn overlay_propagation_matches_cad() {
    let mut access = Access::default();
    access.sources.insert(id("C"), source(&[]));
    let graph = prepare_xref_closure(
        id("B"),
        source(&[("A", true), ("C", false)]),
        10,
        &[id("A")],
        &mut access,
    )
    .unwrap();
    assert_eq!(graph.rows.len(), 1);
    assert_eq!(graph.rows[0].name, "C");
    assert_eq!(access.reads, 1);
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn missing_child_does_not_hide_parent_or_sibling() {
    let mut access = Access::default();
    access.sources.insert(id("C"), source(&[]));
    let graph = prepare_xref_closure(
        id("B"),
        source(&[("missing", false), ("C", false)]),
        10,
        &[],
        &mut access,
    )
    .unwrap();
    assert!(graph
        .rows
        .iter()
        .any(|r| r.name == "missing" && r.status == "NotFound"));
    assert!(graph
        .rows
        .iter()
        .any(|r| r.name == "C" && r.status == "Loaded"));
    assert_eq!(
        graph
            .document
            .entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        2
    );
}

fn native_drawing(children: &[(&str, bool)]) -> document::ifccad::IfccadDocument {
    document::Ifcx
        .export_ifccad(&source(children).document)
        .unwrap()
        .drawing
}

fn native_child_metadata(unit: &str) -> document::ifccad::IfccadExternalSourceContent {
    document::ifccad::IfccadExternalSourceContent::Ifccad {
        base_point: [0.; 3],
        insertion_unit: Some(unit.into()),
    }
}

fn encode(drawing: &document::ifccad::IfccadDocument) -> Vec<u8> {
    document::ifccad::encode_ifccad_document(drawing)
        .unwrap()
        .bytes()
        .to_vec()
}

fn host_for(
    b: &document::ifccad::IfccadDocument,
    path: &str,
) -> (Document, ocs_save::IfccadSession, Handle) {
    let mut host = document::ifccad::load_ifccad_bytes(
        include_bytes!("../../../crates/ocs_save/tests/fixtures/xref.ifcx"),
        Default::default(),
    )
    .unwrap()
    .into_document();
    host.external_sources[0].location =
        document::ifccad::IfccadExternalSourceLocation::UriReference { uri: path.into() };
    host.external_sources[0].source = native_child_metadata(&b.length_unit);
    let (read, session) = ocs_save::read_ifccad_for_edit(&encode(&host)).unwrap();
    let handle = read.document.block_records.get("grondplan").unwrap().handle;
    (read.document, session.unwrap(), handle)
}

#[test]
fn nested_native_files_refresh_cycle_recovery_and_root_only_save() {
    use document::ifccad::*;
    let root = std::env::temp_dir().join(format!(
        "ocs_nested_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("refs")).unwrap();
    std::fs::create_dir_all(root.join("detail")).unwrap();
    let mut c = native_drawing(&[]);
    let mut b = native_drawing(&[("../detail/C.ifccad", false)]);
    b.external_sources[0].source = native_child_metadata(&c.length_unit);
    std::fs::write(root.join("refs/B.ifccad"), encode(&b)).unwrap();
    std::fs::write(root.join("detail/C.ifccad"), encode(&c)).unwrap();
    let (mut host, session, handle) = host_for(&b, "refs/B.ifccad");
    host.source_path = Some(root.join("A.ifccad").to_string_lossy().into_owned());
    let keys = [handle].into_iter().collect();
    let reload =
        |host: &mut Document| resolve_xrefs_for_keys_in_session(host, &root, &keys, Some(&session));
    let infos = reload(&mut host);
    assert_eq!(
        infos[0].status,
        XrefStatus::Loaded,
        "{:?}",
        infos[0].diagnostics
    );
    assert_eq!(
        host.entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        2
    );
    let bytes = ocs_save::write_external_for_edit(
        &host,
        ocs_save::external_format("ifccad").unwrap(),
        false,
        Some(&session),
    )
    .unwrap();
    let saved = load_ifccad_bytes(&bytes, Default::default()).unwrap();
    assert_eq!(saved.document().external_sources.len(), 1);
    assert!(
        saved.document().blocks.is_empty(),
        "Loaded child blocks must remain external"
    );
    assert_eq!(
        saved.document().external_sources[0].location,
        IfccadExternalSourceLocation::UriReference {
            uri: "refs/B.ifccad".into()
        }
    );
    if let IfccadEntityKind::LineSegment { end, .. } =
        &mut c.model.entities[0].as_native_mut().unwrap().kind
    {
        end[0] = 7.;
    }
    std::fs::write(root.join("detail/C.ifccad"), encode(&c)).unwrap();
    assert_eq!(reload(&mut host)[0].status, XrefStatus::Loaded);
    assert!(host
        .entities()
        .any(|e| matches!(e,EntityType::Line(l) if l.end.x==7.)));
    std::fs::remove_file(root.join("detail/C.ifccad")).unwrap();
    let infos = reload(&mut host);
    assert_eq!(
        infos[0].status,
        XrefStatus::Loaded,
        "{:?}",
        infos[0].diagnostics
    );
    assert!(infos[0]
        .diagnostics
        .iter()
        .any(|d| d.contains("previous graphics")));
    assert!(host
        .entities()
        .any(|e| matches!(e,EntityType::Line(l) if l.end.x==7.)));
    let mut cyclic = native_drawing(&[("../refs/./B.ifccad", false)]);
    cyclic.external_sources[0].source = native_child_metadata(&b.length_unit);
    std::fs::write(root.join("detail/C.ifccad"), encode(&cyclic)).unwrap();
    let infos = reload(&mut host);
    assert_eq!(infos[0].status, XrefStatus::Loaded);
    assert!(
        infos[0].diagnostics.iter().any(|d| d.contains("Circular")),
        "{:?}",
        infos[0].diagnostics
    );
    assert_eq!(
        session
            .nested_source_rows()
            .iter()
            .filter(|(_, r)| r.status == "Circular")
            .count(),
        1
    );
    std::fs::write(root.join("detail/C.ifccad"), encode(&c)).unwrap();
    assert!(!reload(&mut host)[0]
        .diagnostics
        .iter()
        .any(|d| d.contains("Circular")));
    let before = host.clone();
    std::fs::write(root.join("detail/C.ifccad"), b"malformed").unwrap();
    assert_eq!(reload(&mut host)[0].status, XrefStatus::Failed);
    assert_eq!(host, before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn browser_bound_native_chain_and_cycle_uses_resource_identity() {
    let c = native_drawing(&[]);
    let mut b = native_drawing(&[("C.ifccad", false)]);
    b.external_sources[0].source = native_child_metadata(&c.length_unit);
    let (mut host, session, handle) = host_for(&b, "B.ifccad");
    let locator = host
        .block_records
        .get("grondplan")
        .unwrap()
        .xref_path
        .clone();
    session
        .bind_external_source_bytes(handle, &locator, "same.ifccad".into(), Arc::new(encode(&b)))
        .unwrap();
    let b_id = session
        .external_source_bytes(handle, &locator)
        .unwrap()
        .resource_id;
    let keys = [handle].into_iter().collect();
    let infos = resolve_xrefs_for_keys_in_session(&mut host, Path::new("."), &keys, Some(&session));
    assert_eq!(infos[0].status, XrefStatus::Loaded);
    let rows = session.nested_source_rows();
    assert_eq!(rows[0].1.owner_resource, Some(b_id));
    assert_eq!(rows[0].1.status, "NotFound");
    session
        .bind_resource_bytes(
            b_id,
            b.external_sources[0].id.0,
            "C.ifccad",
            "same.ifccad".into(),
            Arc::new(encode(&c)),
        )
        .unwrap();
    let infos = resolve_xrefs_for_keys_in_session(&mut host, Path::new("."), &keys, Some(&session));
    assert_eq!(
        infos[0].status,
        XrefStatus::Loaded,
        "{:?}",
        infos[0].diagnostics
    );
    assert_eq!(
        host.entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        2
    );
    session
        .reuse_source_resource(b_id, b.external_sources[0].id.0, "C.ifccad", 1)
        .unwrap();
    let infos = resolve_xrefs_for_keys_in_session(&mut host, Path::new("."), &keys, Some(&session));
    assert!(infos[0].diagnostics.iter().any(|d| d.contains("Circular")));
    assert_eq!(
        host.entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        1
    );
    let mut entries = collect_entries(&host, Path::new("."), &Default::default());
    apply_native_source_status(&mut entries, Some(&session));
    assert_eq!(
        entries
            .iter()
            .find(|e| e.parent_key.is_none())
            .unwrap()
            .status,
        crate::io::xref_model::RefStatus::Loaded
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.status == crate::io::xref_model::RefStatus::Circular)
            .count(),
        1
    );
}

#[test]
fn nested_placement_subtracts_each_source_base_and_scales_once() {
    fn points(
        doc: &Document,
        owner: Handle,
        o: [f64; 2],
        basis: [[f64; 2]; 2],
        out: &mut Vec<([f64; 2], [f64; 2])>,
    ) {
        let map = |p: Vector3| {
            [
                o[0] + basis[0][0] * p.x + basis[1][0] * p.y,
                o[1] + basis[0][1] * p.x + basis[1][1] * p.y,
            ]
        };
        for e in doc.entities().filter(|e| e.common().owner_handle == owner) {
            match e {
                EntityType::Line(line) => out.push((map(line.start), map(line.end))),
                EntityType::Insert(insert) => {
                    let br = doc.block_records.get(&insert.block_name).unwrap();
                    let (s, c) = insert.rotation.to_radians().sin_cos();
                    let x = [c * insert.x_scale(), s * insert.x_scale()];
                    let y = [-s * insert.y_scale(), c * insert.y_scale()];
                    let mul = |v: [f64; 2]| {
                        [
                            basis[0][0] * v[0] + basis[1][0] * v[1],
                            basis[0][1] * v[0] + basis[1][1] * v[1],
                        ]
                    };
                    points(
                        doc,
                        br.handle,
                        map(insert.insert_point),
                        [mul(x), mul(y)],
                        out,
                    );
                }
                _ => {}
            }
        }
    }
    let mut a = (*source(&[("B", false)])).clone();
    for e in a.document.entities_mut() {
        if let EntityType::Insert(i) = e {
            i.insert_point = Vector3::new(100., 200., 0.);
            i.rotation = 90.;
            i.set_x_scale(2.);
            i.set_y_scale(2.);
            i.set_z_scale(2.);
        }
    }
    let mut b = (*source(&[("C", false)])).clone();
    b.document.header.model_space_insertion_base = Vector3::new(10., 20., 0.);
    for e in b.document.entities_mut() {
        if let EntityType::Insert(i) = e {
            i.insert_point = Vector3::new(11., 20., 0.);
            i.set_x_scale(3.);
            i.set_y_scale(3.);
            i.set_z_scale(3.);
        }
    }
    let mut access = Access::default();
    access.sources.insert(id("B"), Arc::new(b));
    access.sources.insert(id("C"), source(&[]));
    let graph = prepare_xref_closure(id("A"), Arc::new(a), 10, &[], &mut access).unwrap();
    let mut lines = Vec::new();
    points(
        &graph.document,
        graph.document.header.model_space_block_handle,
        [0., 0.],
        [[1., 0.], [0., 1.]],
        &mut lines,
    );
    assert!(
        lines.iter().any(|(a, b)| (a[0] - 100.).abs() < 1e-10
            && (a[1] - 202.).abs() < 1e-10
            && (b[0] - 100.).abs() < 1e-10
            && (b[1] - 208.).abs() < 1e-10),
        "{lines:?}"
    );
}

#[test]
fn obsolete_reload_cannot_overwrite_relink_unload_or_binding_revision() {
    let b = native_drawing(&[]);
    let (mut host, session, handle) = host_for(&b, "B.ifccad");
    let original = host
        .block_records
        .get("grondplan")
        .unwrap()
        .xref_path
        .clone();
    for change in ["relink", "unload", "binding"] {
        let mut read = read_source_bytes("B.ifccad", &encode(&b));
        read.guard = Some((original.clone(), false, session.source_generation()));
        if change == "relink" {
            host.block_records.get_mut("grondplan").unwrap().xref_path = "other.ifccad".into();
        }
        if change == "unload" {
            host.block_records
                .get_mut("grondplan")
                .unwrap()
                .flags
                .is_xref_unloaded = true;
        }
        if change == "binding" {
            session
                .bind_resource_bytes(
                    4,
                    1,
                    "elsewhere",
                    "different.ifccad".into(),
                    Arc::new(encode(&b)),
                )
                .unwrap();
        }
        let before = host.clone();
        let mut infos = Vec::new();
        merge_xref_reads_in_session(
            &mut host,
            vec![("grondplan".into(), original.clone(), handle, read)],
            &mut infos,
            None,
            Some(&session),
        );
        assert_eq!(host, before);
        assert!(infos[0].diagnostics.iter().any(|d| d.contains("obsolete")));
        host.block_records.get_mut("grondplan").unwrap().xref_path = original.clone();
        host.block_records
            .get_mut("grondplan")
            .unwrap()
            .flags
            .is_xref_unloaded = false;
    }
}

#[test]
fn preparing_nested_display_does_not_change_live_reference_merge_index() {
    let handle = Handle::new(0xFA112233);
    let mut root = (*source(&[("C", false)])).clone();
    root.document.block_records.get_mut("C").unwrap().handle = handle;
    let sentinel = [(Handle::new(91), Handle::new(92))]
        .into_iter()
        .collect::<HashMap<_, _>>();
    merged_copies()
        .lock()
        .unwrap()
        .insert(handle, sentinel.clone());
    let mut access = Access::default();
    access.sources.insert(id("C"), source(&[]));
    prepare_xref_closure(id("B"), Arc::new(root), 10, &[], &mut access).unwrap();
    let observed = merged_copies().lock().unwrap().remove(&handle).unwrap();
    assert_eq!(observed, sentinel);
}

#[test]
fn nested_mixed_chain_uses_immediate_owner_paths() {
    use document::ifccad::*;
    for format in ["dwg", "dxf", "cbor"] {
        let root = std::env::temp_dir().join(format!(
            "ocs_mixed_{format}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("refs")).unwrap();
        std::fs::create_dir_all(root.join("detail")).unwrap();
        let leaf_name = format!("C.{format}");
        let locator = format!("../detail/{leaf_name}");
        let mut b = native_drawing(&[(&locator, false)]);
        let mut cad = Document::new();
        cad.header.insertion_units = 6;
        cad.header.model_space_insertion_base = Vector3::new(10., 20., 0.);
        cad.add_entity(EntityType::Line(Line::from_coords(
            10., 20., 0., 12., 20., 0.,
        )))
        .unwrap();
        let (bytes, content) = match format {
            "dwg" => (
                document::Format::write(&document::Dwg, &cad).unwrap().0,
                document::dwg_external_source_content(&cad),
            ),
            "dxf" => (
                document::DxfWriter::new(&cad).write_to_vec().unwrap(),
                document::dxf_external_source_content(&cad),
            ),
            _ => {
                let drawing = document::Ifcx.export_ifccad(&cad).unwrap().drawing;
                (
                    encode_ifccad_cbor_document(&drawing)
                        .unwrap()
                        .bytes()
                        .to_vec(),
                    native_child_metadata(&drawing.length_unit),
                )
            }
        };
        b.external_sources[0].source = content;
        std::fs::write(root.join("detail").join(leaf_name), bytes).unwrap();
        std::fs::write(root.join("refs/B.ifccad"), encode(&b)).unwrap();
        let (mut host, session, handle) = host_for(&b, "refs/B.ifccad");
        let infos = resolve_xrefs_for_keys_in_session(
            &mut host,
            &root,
            &[handle].into_iter().collect(),
            Some(&session),
        );
        assert_eq!(
            infos[0].status,
            XrefStatus::Loaded,
            "{format}: {:?}",
            infos[0].diagnostics
        );
        assert_eq!(
            host.entities()
                .filter(|e| matches!(e, EntityType::Line(_)))
                .count(),
            2,
            "{format}"
        );
        if format != "cbor" {
            assert!(host.entities().any(|e|matches!(e,EntityType::Line(l) if l.start==Vector3::ZERO && l.end==Vector3::new(2.,0.,0.))),"{format}");
        }
        let bytes = ocs_save::write_external_for_edit(
            &host,
            ocs_save::external_format("ifccad").unwrap(),
            false,
            Some(&session),
        )
        .unwrap();
        let read = load_ifccad_bytes(&bytes, Default::default()).unwrap();
        assert!(read.document().blocks.is_empty());
        assert_eq!(read.document().external_sources.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cad_parents_share_nested_overlay_and_cycle_rules() {
    for format in ["dwg", "dxf"] {
        let root = std::env::temp_dir().join(format!(
            "ocs_cad_nested_{format}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("refs")).unwrap();
        std::fs::create_dir_all(root.join("detail")).unwrap();
        let mut b = source(&[("C", false), ("SKIP", true)]).document.clone();
        b.block_records.get_mut("C").unwrap().xref_path = "../detail/C.dwg".into();
        b.block_records.get_mut("SKIP").unwrap().xref_path = "../missing/overlay.dwg".into();
        ensure_block_entities(&mut b, "C");
        ensure_block_entities(&mut b, "SKIP");
        let mut c = source(&[("B", false)]).document.clone();
        c.block_records.get_mut("B").unwrap().xref_path = format!("../refs/B.{format}");
        ensure_block_entities(&mut c, "B");
        std::fs::write(
            root.join("detail/C.dwg"),
            document::Format::write(&document::Dwg, &c).unwrap().0,
        )
        .unwrap();
        let (bytes, content) = if format == "dwg" {
            (
                document::Format::write(&document::Dwg, &b).unwrap().0,
                document::dwg_external_source_content(&b),
            )
        } else {
            (
                document::DxfWriter::new(&b).write_to_vec().unwrap(),
                document::dxf_external_source_content(&b),
            )
        };
        std::fs::write(root.join(format!("refs/B.{format}")), bytes).unwrap();
        let (mut host, session, handle) =
            host_for(&native_drawing(&[]), &format!("refs/B.{format}"));
        session
            .register_external_source(&host, handle, content)
            .unwrap();
        let infos = resolve_xrefs_for_keys_in_session(
            &mut host,
            &root,
            &[handle].into_iter().collect(),
            Some(&session),
        );
        assert_eq!(
            infos[0].status,
            XrefStatus::Loaded,
            "{format}: {:?}",
            infos[0].diagnostics
        );
        assert_eq!(
            host.entities()
                .filter(|e| matches!(e, EntityType::Line(_)))
                .count(),
            2,
            "{format}"
        );
        let rows = session.nested_source_rows();
        assert_eq!(
            rows.iter().filter(|(_, r)| r.status == "Circular").count(),
            1,
            "{format}"
        );
        assert!(!rows.iter().any(|(_, r)| r.name == "SKIP"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn repeated_placements_share_a_definition_without_losing_an_insert() {
    let mut root = source(&[("B", false)]).document.clone();
    root.add_entity(EntityType::Insert(Insert::new(
        "B",
        Vector3::new(100., 200., 0.),
    )))
    .unwrap();
    let root = Arc::new(ParsedXrefSource {
        document: root,
        context: None,
        content: None,
        path: PathBuf::from("A.dwg"),
        unavailable: false,
    });
    let mut access = Access::default();
    access.sources.insert(id("B"), source(&[]));
    let graph = prepare_xref_closure(id("A"), root, 10, &[], &mut access).unwrap();
    assert_eq!(access.reads, 1);
    assert_eq!(graph.rows.len(), 1);
    let inserts: Vec<_> = graph
        .document
        .model_space_entities()
        .filter_map(|e| {
            if let EntityType::Insert(i) = e {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(inserts.len(), 2);
    assert_eq!(inserts[0].block_name, inserts[1].block_name);
    assert_ne!(inserts[0].insert_point, inserts[1].insert_point);
}

#[test]
fn unused_reference_is_inspectable_without_loading_or_a_cycle() {
    let mut root = source(&[("A", false)]).document.clone();
    let inserts: Vec<_> = root
        .entities()
        .filter(|e| matches!(e, EntityType::Insert(_)))
        .map(|e| e.common().handle)
        .collect();
    for handle in inserts {
        root.remove_entity(handle);
    }
    let root = Arc::new(ParsedXrefSource {
        document: root,
        context: None,
        content: None,
        path: PathBuf::from("B.dwg"),
        unavailable: false,
    });
    let mut access = Access::default();
    access.sources.insert(id("A"), source(&[]));
    let graph = prepare_xref_closure(id("B"), root, 10, &[id("A")], &mut access).unwrap();
    assert_eq!(access.reads, 0);
    assert!(graph.diagnostics.is_empty());
    assert_eq!(graph.rows.len(), 1);
    assert_eq!(graph.rows[0].status, "Unreferenced");
}

#[test]
fn direct_self_reference_reports_circular_on_its_own_palette_row() {
    use crate::io::xref_model::RefStatus;
    let dir = std::env::temp_dir().join(format!("ocs_self_palette_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (mut host, session, handle) = host_for(&native_drawing(&[]), "Self.ifccad");
    let path = dir.join("Self.ifccad");
    host.source_path = Some(path.to_string_lossy().into_owned());
    let bytes = ocs_save::write_external_for_edit(
        &host,
        ocs_save::external_format("ifccad").unwrap(),
        false,
        Some(&session),
    )
    .unwrap();
    std::fs::write(&path, bytes).unwrap();
    let infos = resolve_xrefs_for_keys_in_session(
        &mut host,
        &dir,
        &[handle].into_iter().collect(),
        Some(&session),
    );
    assert!(infos[0]
        .diagnostics
        .iter()
        .any(|d| d.contains("self-reference")));
    for native_session in [None, Some(&session)] {
        let mut rows = collect_entries(&host, &dir, &Default::default());
        apply_native_source_status(&mut rows, native_session);
        assert_eq!(rows.len(), 1, "no synthetic self child should be displayed");
        assert_eq!(rows[0].key, handle.value());
        assert_eq!(rows[0].parent_key, None);
        assert_eq!(rows[0].status, RefStatus::Circular);
        assert!(rows[0]
            .diagnostic
            .as_ref()
            .unwrap()
            .contains("self-reference"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn browser_current_drawing_reuse_reports_direct_cycle_and_can_be_replaced() {
    use crate::io::xref_model::RefStatus;
    let leaf = native_drawing(&[]);
    let (mut host, session, handle) = host_for(&leaf, "Self.ifccad");
    let locator = host
        .block_records
        .get("grondplan")
        .unwrap()
        .xref_path
        .clone();
    let source = session.external_source(handle).unwrap().id.0;
    session
        .reuse_source_resource(1, source, &locator, 1)
        .unwrap();
    resolve_xrefs_for_keys_in_session(
        &mut host,
        Path::new("."),
        &[handle].into_iter().collect(),
        Some(&session),
    );
    let mut rows = collect_entries(&host, Path::new("."), &Default::default());
    apply_native_source_status(&mut rows, Some(&session));
    assert_eq!(
        rows.len(),
        1,
        "direct browser cycle has no extra self child"
    );
    assert_eq!(rows[0].status, RefStatus::Circular);
    assert!(rows[0].diagnostic.is_some());
    session
        .bind_external_source_bytes(
            handle,
            &locator,
            "Healthy.ifccad".into(),
            Arc::new(encode(&leaf)),
        )
        .unwrap();
    resolve_xrefs_for_keys_in_session(
        &mut host,
        Path::new("."),
        &[handle].into_iter().collect(),
        Some(&session),
    );
    let mut rows = collect_entries(&host, Path::new("."), &Default::default());
    apply_native_source_status(&mut rows, Some(&session));
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        RefStatus::Loaded,
        "replacement must clear Circular"
    );
    assert!(rows[0].diagnostic.is_none());
}
