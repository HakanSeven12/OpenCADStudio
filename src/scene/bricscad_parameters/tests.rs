use super::*;
use document::DxfVersion;

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

fn block(document: &Document, name: &str) -> Handle {
    document.block_records.get(name).unwrap().handle
}

/// The block reference with handle `value`.
fn reference(value: u64) -> Handle {
    Handle::from(value)
}

/// Each parameter as its name and the state it shows.
fn shown(parameters: &[VisibilityParameter]) -> Vec<(&str, &str)> {
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_str(),
                parameter
                    .current
                    .map_or("?", |index| parameter.states[index].as_str()),
            )
        })
        .collect()
}

#[test]
fn the_definition_names_its_parameters_their_states_and_the_ones_it_shows() {
    let document = fixture();
    let parameters =
        visibility_parameters(&document, block(&document, "ocs-bricscad-visibility-block"));
    assert_eq!(
        parameters,
        vec![
            VisibilityParameter {
                name: "LAMP".into(),
                states: vec!["On".into(), "Off".into()],
                current: Some(0),
                feature: Handle::from(0x16Fu64),
            },
            VisibilityParameter {
                name: "LABEL".into(),
                states: vec!["Shown".into(), "Hidden".into()],
                current: Some(0),
                feature: Handle::from(0x176u64),
            },
        ]
    );
}

#[test]
fn each_reference_shows_the_states_it_was_set_to() {
    let document = fixture();
    assert_eq!(
        shown(&reference_parameters(&document, reference(0xEE))),
        [("LAMP", "On"), ("LABEL", "Shown")]
    );
    assert_eq!(
        shown(&reference_parameters(&document, reference(0x146))),
        [("LAMP", "Off"), ("LABEL", "Shown")]
    );
    assert_eq!(
        shown(&reference_parameters(&document, reference(0x19E))),
        [("LAMP", "Off"), ("LABEL", "Hidden")]
    );
}

#[test]
fn a_copy_names_the_block_it_was_made_from() {
    let document = fixture();
    let definition = block(&document, "ocs-bricscad-visibility-block");
    for copy in ["*U2", "*U3"] {
        assert_eq!(
            original_block(&document, block(&document, copy)),
            Some(definition),
            "{copy}"
        );
    }
    assert_eq!(original_block(&document, definition), None);
}

#[test]
fn other_blocks_and_references_have_no_parameters() {
    let document = fixture();
    let model = document.header.model_space_block_handle;
    assert!(visibility_parameters(&document, model).is_empty());
    assert_eq!(original_block(&document, model), None);
    let line = document
        .entities()
        .find(|entity| matches!(entity, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle;
    assert!(reference_parameters(&document, line).is_empty());
}

#[test]
fn a_cut_record_decodes_to_nothing_rather_than_a_wrong_answer() {
    let document = fixture();
    let mut records = 0;
    for object in document.objects.values() {
        let ObjectType::Unknown {
            type_name,
            raw_dwg_data: Some(data),
            raw_dwg_handle_bits,
            raw_dwg_version: Some(version),
            ..
        } = object
        else {
            continue;
        };
        if type_name != "BDMDBCONFIGURATIONTABLE" {
            continue;
        }
        let whole = record::decode(type_name, data, *raw_dwg_handle_bits, *version)
            .expect("the whole record");
        assert!(whole.body.properties().get("KeyValues").is_some());
        records += 1;
        for length in 0..data.len() {
            // Shorter by whole bytes: the streams no longer line up.
            let _ = record::decode(type_name, &data[..length], *raw_dwg_handle_bits, *version);
        }
        assert_eq!(
            record::decode(type_name, data, *raw_dwg_handle_bits, DxfVersion::AC1021),
            None
        );
    }
    assert_eq!(records, 6, "two tables in each of the three blocks");
}
