use document::ifccad::*;

fn drawing() -> IfccadDocument {
    let mut drawing = load_ifccad_bytes(include_bytes!("fixtures/xref.ifcx"), Default::default())
        .unwrap()
        .into_document();
    drawing.external_sources[0].source = IfccadExternalSourceContent::Dxf {
        base_point: [10., 20., 0.],
        insertion_unit: Some("m".into()),
        fingerprint_guid: Some("e5b4a9af-cf20-4fc9-a30f-ffb20fd987e1".into()),
    };
    drawing
}

#[test]
fn dxf_source_session_survives_offline_save_and_refuses_cad_destination() {
    let drawing = drawing();
    let bytes = encode_ifccad_document(&drawing).unwrap();
    let (loaded, session) = ocs_save::read_ifccad_for_edit(bytes.bytes()).unwrap();
    let session = session.unwrap();
    for ext in ["dwg", "dxf", "dwt", "dws"] {
        let error = session
            .check_destination(&loaded.document, ext)
            .unwrap_err();
        assert!(error.contains("DWG"), "{error}");
    }
    let bytes = ocs_save::write_external_for_edit(
        &loaded.document,
        ocs_save::external_format("ifccad").unwrap(),
        false,
        Some(&session),
    )
    .unwrap();
    let read = load_ifccad_bytes(&bytes, Default::default()).unwrap();
    assert_eq!(read.document().external_sources, drawing.external_sources);
    assert!(read.document().blocks.is_empty());
}

#[test]
fn reload_refuses_a_changed_source_format_before_replacing_graphics() {
    let drawing = drawing();
    let bytes = encode_ifccad_document(&drawing).unwrap();
    let (loaded, session) = ocs_save::read_ifccad_for_edit(bytes.bytes()).unwrap();
    let session = session.unwrap();
    let handle = loaded
        .document
        .block_records
        .get("grondplan")
        .unwrap()
        .handle;
    assert!(session
        .check_external_source_reload(handle, &drawing.external_sources[0].source)
        .is_ok());
    let wrong = IfccadExternalSourceContent::Dwg {
        base_point: [10., 20., 0.],
        insertion_unit: Some("m".into()),
        fingerprint_guid: Some("e5b4a9af-cf20-4fc9-a30f-ffb20fd987e1".into()),
    };
    assert!(session
        .check_external_source_reload(handle, &wrong)
        .is_err());
}
