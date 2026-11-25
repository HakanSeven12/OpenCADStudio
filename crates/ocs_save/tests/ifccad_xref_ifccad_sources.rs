use document::{entities::Line, ifccad::*, Document, EntityType, Ifcx};

fn source() -> IfccadDocument {
    let mut source = Document::new();
    source.header.insertion_units = 6;
    source
        .add_entity(EntityType::Line(Line::from_coords(0., 0., 0., 2., 0., 0.)))
        .unwrap();
    Ifcx.export_ifccad(&source).unwrap().drawing
}

#[test]
fn native_source_json_and_cbor_use_the_production_reader_without_a_guid() {
    let source = source();
    let json = encode_ifccad_document(&source).unwrap();
    let cbor = encode_ifccad_cbor_document(&source).unwrap();
    for bytes in [json.bytes(), cbor.bytes()] {
        let source = ocs_save::read_ifccad_source(bytes).unwrap();
        let read = source.outcome;
        let content = source.content;
        assert!(read.stats.stream_completed);
        assert_eq!(
            content,
            IfccadExternalSourceContent::Ifccad {
                base_point: [0.; 3],
                insertion_unit: Some("m".into()),
            }
        );
        assert_eq!(content.fingerprint_guid(), None);
        assert_eq!(
            read.document
                .model_space_entities()
                .filter(|entity| matches!(entity, EntityType::Line(_)))
                .count(),
            1
        );
    }
}

#[test]
fn nested_native_source_reader_retains_child_context_json_cbor() {
    let source = load_ifccad_bytes(include_bytes!("fixtures/xref.ifcx"), Default::default())
        .unwrap()
        .into_document();
    let json = encode_ifccad_document(&source).unwrap();
    let cbor = encode_ifccad_cbor_document(&source).unwrap();
    for bytes in [json.bytes(), cbor.bytes()] {
        let read = ocs_save::read_ifccad_source(bytes).unwrap();
        let br = read
            .outcome
            .document
            .block_records
            .get("grondplan")
            .unwrap();
        assert_eq!(
            read.context.external_source(br.handle),
            Some(&source.external_sources[0])
        );
    }
}

#[test]
fn native_source_session_keeps_offline_identity_and_never_observes_a_guid() {
    let content = ocs_save::read_ifccad_source(encode_ifccad_document(&source()).unwrap().bytes())
        .unwrap()
        .content;
    let mut main = load_ifccad_bytes(include_bytes!("fixtures/xref.ifcx"), Default::default())
        .unwrap()
        .into_document();
    main.external_sources[0].source = content;
    let bytes = encode_ifccad_document(&main).unwrap();
    let (loaded, session) = ocs_save::read_ifccad_for_edit(bytes.bytes()).unwrap();
    let session = session.unwrap();
    let handle = loaded
        .document
        .block_records
        .get("grondplan")
        .unwrap()
        .handle;
    for guid in [
        "{11111111-2222-3333-4444-555555555555}",
        "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}",
    ] {
        assert_eq!(
            session.observe_external_source_guid(handle, guid),
            Some(false)
        );
    }
    for ext in ["dwg", "dxf"] {
        assert!(session.check_destination(&loaded.document, ext).is_err());
    }
    let bytes = ocs_save::write_external_for_edit(
        &loaded.document,
        ocs_save::external_format("ifccad").unwrap(),
        false,
        Some(&session),
    )
    .unwrap();
    let read = load_ifccad_bytes(&bytes, Default::default()).unwrap();
    assert_eq!(read.document().external_sources, main.external_sources);
    assert!(read.document().blocks.is_empty());
}
