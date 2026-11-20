use ::document::{entities, ifccad::*, EntityType};

fn drawing() -> IfccadDocument {
    load_ifccad_bytes(include_bytes!("fixtures/xref.ifcx"), Default::default())
        .unwrap()
        .into_document()
}

#[test]
fn native_source_guid_observations_do_not_leak_between_identical_tabs() {
    let (a, first) = ocs_save::read_ifccad_for_edit(include_bytes!("fixtures/xref.ifcx")).unwrap();
    let (b, second) = ocs_save::read_ifccad_for_edit(include_bytes!("fixtures/xref.ifcx")).unwrap();
    let first = first.unwrap();
    let second = second.unwrap();
    let ah = a.document.block_records.get("grondplan").unwrap().handle;
    let bh = b.document.block_records.get("grondplan").unwrap().handle;
    assert_eq!(ah, bh);
    let original = "{E5B4A9AF-CF20-4FC9-A30F-FFB20FD987E1}";
    let other = "{11111111-2222-3333-4444-555555555555}";
    assert_eq!(
        first.observe_external_source_guid(ah, original),
        Some(false)
    );
    assert_eq!(second.observe_external_source_guid(bh, other), Some(true));
    assert_eq!(
        first.observe_external_source_guid(ah, original),
        Some(false)
    );
    assert_eq!(second.observe_external_source_guid(bh, other), Some(false));
    assert_eq!(
        second.observe_external_source_guid(bh, original),
        Some(true)
    );
}

#[test]
fn native_xref_missing_save_reopen_keeps_identity() {
    let original = drawing();
    let encoded = encode_ifccad_document(&original).unwrap();
    let (mut loaded, session) = ocs_save::read_ifccad_for_edit(encoded.bytes()).unwrap();
    let handle = loaded
        .document
        .block_records
        .get("grondplan")
        .unwrap()
        .handle;
    let mut line = entities::Line::from_coords(0., 0., 0., 2., 0., 0.);
    line.common.owner_handle = handle;
    let temporary = loaded.document.add_entity(EntityType::Line(line)).unwrap();
    // The external body can disappear; local editing remains possible.
    loaded.document.remove_entity(temporary);
    loaded
        .document
        .add_entity(EntityType::Line(entities::Line::from_coords(
            0., 0., 0., 3., 0., 0.,
        )))
        .unwrap();
    let bytes = ocs_save::write_external_for_edit(
        &loaded.document,
        ocs_save::external_format("ifccad").unwrap(),
        true,
        session.as_ref(),
    )
    .unwrap();
    let read = load_ifccad_bytes(&bytes, Default::default()).unwrap();
    assert_eq!(read.document().external_sources, original.external_sources);
    assert!(read.document().blocks.is_empty());
    assert_eq!(
        read.document().model.entities[0].id(),
        original.model.entities[0].id()
    );
    let (reopened, session) = ocs_save::read_ifccad_for_edit(&bytes).unwrap();
    let again = ocs_save::write_external_for_edit(
        &reopened.document,
        ocs_save::external_format("ifccad").unwrap(),
        true,
        session.as_ref(),
    )
    .unwrap();
    assert_eq!(
        load_ifccad_bytes(&again, Default::default())
            .unwrap()
            .document()
            .external_sources,
        original.external_sources
    );
}

#[test]
fn native_session_refuses_unqualified_cad_locator() {
    let mut d = drawing();
    d.external_sources[0].location = IfccadExternalSourceLocation::UriReference {
        uri: "custom:opaque".into(),
    };
    let bytes = encode_ifccad_document(&d).unwrap();
    let (loaded, session) = ocs_save::read_ifccad_for_edit(bytes.bytes()).unwrap();
    assert!(session
        .unwrap()
        .check_destination(&loaded.document, "dwg")
        .is_err());
}

#[test]
fn reload_checks_basis_and_unit_but_guid_mismatch_keeps_loading_policy() {
    let d = drawing();
    let bytes = encode_ifccad_document(&d).unwrap();
    let (loaded, session) = ocs_save::read_ifccad_for_edit(bytes.bytes()).unwrap();
    let session = session.unwrap();
    let handle = loaded
        .document
        .block_records
        .get("grondplan")
        .unwrap()
        .handle;
    let mut changed = d.external_sources[0].source.clone();
    let IfccadExternalSourceContent::Dwg {
        fingerprint_guid, ..
    } = &mut changed;
    *fingerprint_guid = Some("11111111-2222-3333-4444-555555555555".into());
    assert!(session
        .check_external_source_reload(handle, &changed)
        .is_ok());
    let IfccadExternalSourceContent::Dwg { base_point, .. } = &mut changed;
    base_point[0] += 1.;
    assert!(session
        .check_external_source_reload(handle, &changed)
        .is_err());
    let mut changed = d.external_sources[0].source.clone();
    let IfccadExternalSourceContent::Dwg { insertion_unit, .. } = &mut changed;
    *insertion_unit = Some("mm".into());
    assert!(session
        .check_external_source_reload(handle, &changed)
        .is_err());
}
