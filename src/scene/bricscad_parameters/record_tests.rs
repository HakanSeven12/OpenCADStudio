use super::record::{decode, encode, Body, Ref, Value};
use super::*;

/// The BricsCAD drawing in `tests/fixtures/bricscad` (see its README).
fn fixture() -> Document {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bricscad/visibility-states.dwg"
    );
    let bytes = std::fs::read(path).expect("read the fixture");
    crate::io::load_bytes_finalized(std::path::Path::new("visibility-states.dwg"), bytes)
        .expect("open the fixture")
}

/// Every BricsCAD object of `document` with its class, record bytes, handle
/// stream bits and version.
fn bricscad_records(
    document: &Document,
) -> Vec<(Handle, String, Vec<u8>, i64, document::DxfVersion)> {
    let mut records: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::Unknown {
                type_name,
                raw_dwg_data: Some(data),
                raw_dwg_handle_bits,
                raw_dwg_version: Some(version),
                ..
            } if type_name.to_ascii_uppercase().starts_with("BDM") => Some((
                *handle,
                type_name.clone(),
                data.clone(),
                *raw_dwg_handle_bits,
                *version,
            )),
            _ => None,
        })
        .collect();
    records.sort_by_key(|record| record.0);
    records
}

#[test]
fn every_bricscad_record_encodes_to_the_bits_it_was_read_from() {
    let document = fixture();
    let records = bricscad_records(&document);
    assert_eq!(records.len(), 87, "the fixture's BricsCAD objects");
    for (handle, class, data, handle_bits, version) in records {
        let record = decode(&class, &data, handle_bits, version)
            .unwrap_or_else(|| panic!("{class} {handle:?} does not decode"));
        assert_eq!(record.handle, handle, "{class}");
        assert_eq!(
            encode(&record),
            (data, handle_bits),
            "{class} {handle:?} encodes to other bits"
        );
    }
}

#[test]
fn the_three_classes_around_a_property_list_decode_to_their_parts() {
    let document = fixture();
    let manager = record(&document, Handle::from(0x1A5u64)).unwrap();
    let Body::ConstraintManager {
        counts, children, ..
    } = &manager.body
    else {
        panic!("{:?}", manager.body);
    };
    assert_eq!(*counts, [2, 2, 0, 0, 0]);
    assert_eq!(
        children
            .iter()
            .map(|child| child.handle)
            .collect::<Vec<_>>(),
        [Handle::from(0x1A6u64), Handle::from(0x1E1u64)]
    );
    let object = record(&document, Handle::from(0x1A6u64)).unwrap();
    let Body::Object {
        entity, properties, ..
    } = &object.body
    else {
        panic!("{:?}", object.body);
    };
    assert_eq!(
        entity.handle,
        Handle::from(0x146u64),
        "the reference it stands for"
    );
    assert_eq!(properties.text("NAME"), Some("Objekt_1"));
    let dependency = record(&document, Handle::from(0x1A7u64)).unwrap();
    let Body::Dependency { entity, .. } = &dependency.body else {
        panic!("{:?}", dependency.body);
    };
    assert_eq!(entity.handle, Handle::from(0x146u64));
}

#[test]
fn a_changed_record_reads_back_as_changed() {
    let document = fixture();
    let mut feature = record(&document, Handle::from(0x1C7u64)).unwrap();
    assert_eq!(feature.body.properties().text("LastKeyValue"), Some("Off"));
    // Longer text, a new handle further away and every reference moved: the
    // offset forms are recomputed from the new handle.
    feature.body.properties_mut().set(
        "LastKeyValue",
        Value::Text("A much longer state name".into()),
    );
    feature.handle = Handle::from(0x12_3456u64);
    feature.refs_mut(|reference: &mut Ref| {
        reference.handle = Handle::from(reference.handle.value() + 0x1000);
    });
    let (data, handle_bits) = encode(&feature);
    let read = decode("BDMDBFEATUREDATA", &data, handle_bits, feature.version).unwrap();
    assert_eq!(read, feature);
}
