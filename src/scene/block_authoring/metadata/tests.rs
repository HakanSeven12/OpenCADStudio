use super::*;
use crate::scene::{
    block_basepoint, block_distance_parameter as distance, block_lookup,
    block_orientation_parameter as orientation, block_point_parameter as point, block_visibility,
    Scene,
};
use codec::types::Vector3;

fn fixture() -> (Document, Handle, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw("METADATA", Vector3::ZERO, vec![]);
    let definition = scene.document.block_records.get("METADATA").unwrap().handle;
    let doc = &mut scene.document;
    let mut handles = vec![point::create(doc, definition, Vector3::ZERO, Vector3::UNIT_X).unwrap()];
    for kind in [
        distance::Kind::Linear,
        distance::Kind::Polar,
        distance::Kind::XY,
    ] {
        handles.push(
            distance::create(
                doc,
                definition,
                kind,
                Vector3::ZERO,
                Vector3::new(2., 3., 0.),
                Vector3::UNIT_Y,
            )
            .unwrap(),
        );
    }
    for kind in [
        orientation::Kind::Rotation,
        orientation::Kind::Alignment,
        orientation::Kind::Flip,
    ] {
        handles.push(
            orientation::create(
                doc,
                definition,
                kind,
                Vector3::ZERO,
                Vector3::UNIT_Y,
                Vector3::UNIT_X,
            )
            .unwrap(),
        );
    }
    handles.push(block_basepoint::create(doc, definition, Vector3::ZERO).unwrap());
    handles.push(block_lookup::create(doc, definition, Vector3::ZERO).unwrap());
    handles.push(block_visibility::create(doc, definition, Vector3::ZERO).unwrap());
    (scene.document, definition, handles)
}
fn chain(doc: &Document, handle: Handle) -> Option<bool> {
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&handle) else {
        return None;
    };
    match &object.data {
        Data::PointParameter(p) => Some(p.parameter.parameter.chain_actions),
        Data::LinearParameter(p) => Some(p.parameter.parameter.chain_actions),
        Data::PolarParameter(p) => Some(p.parameter.parameter.chain_actions),
        Data::XYParameter(p) => Some(p.parameter.parameter.chain_actions),
        Data::RotationParameter(p) => Some(p.parameter.parameter.chain_actions),
        Data::FlipParameter(p) => Some(p.parameter.parameter.chain_actions),
        _ => None,
    }
}
fn values(doc: &Document, def: Handle, handle: Handle) -> Vec<(Field, String)> {
    read(doc, def, handle)
        .unwrap()
        .text
        .into_iter()
        .map(|text| (text.field, text.value.to_owned()))
        .collect()
}
#[test]
fn block_parameter_metadata_all_native_fields_persist_without_graph_changes() {
    let (mut doc, def, handles) = fixture();
    let before = doc.clone();
    for (index, handle) in handles.iter().copied().enumerate() {
        let fields = values(&doc, def, handle);
        for (field, _) in fields {
            let value = if field.is_name() {
                format!("Native {index} {field:?}")
            } else {
                format!("  Description {index}: 1 + 2 = 3 / état  ")
            };
            let old = values(&doc, def, handle);
            assert_eq!(set_text(&mut doc, def, handle, field, &value), Ok(true));
            for (other, text) in values(&doc, def, handle) {
                assert_eq!(
                    text,
                    if other == field {
                        value.clone()
                    } else {
                        old.iter().find(|(f, _)| *f == other).unwrap().1.clone()
                    }
                );
            }
            let unchanged = doc.clone();
            assert_eq!(set_text(&mut doc, def, handle, field, &value), Ok(false));
            assert_eq!(doc, unchanged);
        }
        if read(&doc, def, handle).unwrap().visible.is_some() {
            assert_eq!(set_visible(&mut doc, def, handle, false), Ok(true));
            assert_eq!(set_visible(&mut doc, def, handle, false), Ok(false));
        }
        if chain(&doc, handle).is_some() {
            assert_eq!(set_chain(&mut doc, def, handle, true), Ok(true));
            assert_eq!(set_chain(&mut doc, def, handle, true), Ok(false));
        }
    }
    for (handle, object) in &before.objects {
        if !handles.contains(handle) {
            assert_eq!(
                doc.objects.get(handle),
                Some(object),
                "preserve graph/grip {handle}"
            );
        }
    }
    assert_eq!(doc.block_records, before.block_records);
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("metadata.{extension}"), bytes).unwrap();
        for &handle in &handles {
            assert_eq!(
                values(&loaded, def, handle),
                values(&doc, def, handle),
                "{extension}: {handle}"
            );
            assert_eq!(
                read(&loaded, def, handle).unwrap().visible,
                read(&doc, def, handle).unwrap().visible
            );
            assert_eq!(chain(&loaded, handle), chain(&doc, handle), "{extension}");
        }
        assert_eq!(
            block_visibility::parameter(&loaded, def)
                .unwrap()
                .description,
            block_visibility::parameter(&doc, def).unwrap().description
        );
    }
    let visibility = block_visibility::parameter(&doc, def).unwrap();
    assert_eq!(
        doc.objects.get(&visibility.handle),
        Some(&ObjectType::BlockVisibilityParameter(visibility.clone()))
    );
}
#[test]
fn block_parameter_metadata_rejects_collisions_foreign_objects_and_invalid_fields_atomically() {
    let (mut doc, def, handles) = fixture();
    let xy = handles[3];
    let x = values(&doc, def, xy)[0].1.clone();
    let before = doc.clone();
    assert!(set_text(
        &mut doc,
        def,
        xy,
        Field::SecondaryName,
        &x.to_ascii_uppercase()
    )
    .is_err());
    let other = values(&doc, def, handles[2])
        .into_iter()
        .find(|(field, _)| *field == Field::SecondaryName)
        .unwrap()
        .1;
    assert!(set_text(&mut doc, def, xy, Field::Name, &other).is_err());
    for invalid in [" ", "null\0text"] {
        assert!(set_text(&mut doc, def, xy, Field::Name, invalid).is_err());
    }
    assert!(set_text(&mut doc, def, handles[5], Field::Description, "unsupported").is_err());
    assert!(set_visible(&mut doc, def, handles[5], false).is_err());
    for unsupported in [handles[5], handles[7], handles[8], handles[9]] {
        assert!(set_chain(&mut doc, def, unsupported, true).is_err());
    }
    assert!(set_chain(&mut doc, def, Handle::from(0xFFFFFFu64), true).is_err());
    assert!(set_text(
        &mut doc,
        Handle::from(0xFFFFFFu64),
        xy,
        Field::Name,
        "Foreign"
    )
    .is_err());
    let graph = crate::scene::block_authoring::graph(&doc, def).unwrap().unwrap();
    assert!(set_text(&mut doc, def, graph, Field::Name, "Graph").is_err());
    assert!(set_chain(&mut doc, def, graph, true).is_err());
    assert_eq!(doc, before);
    if let Some(ObjectType::DynamicBlock(p)) = doc.objects.get_mut(&xy) {
        p.owner = Handle::from(0xFFFFFFu64);
    }
    let foreign = doc.clone();
    assert!(set_text(&mut doc, def, xy, Field::Name, "Foreign").is_err());
    assert_eq!(doc, foreign);
    assert_eq!(
        set_text(&mut doc, def, handles[0], Field::Description, ""),
        Ok(false)
    );
}

fn name_of(doc: &Document, handle: Handle) -> String {
    element_name(doc.objects.get(&handle).unwrap())
        .unwrap()
        .clone()
}

#[test]
fn a_parameter_name_defaults_to_the_next_free_type_number() {
    let (mut doc, def, handles) = fixture();
    let polar = handles[2];
    assert_eq!(name_of(&doc, polar), "Polar1");
    assert_eq!(
        next_parameter_name(&doc, def, "Polar"),
        Ok("Polar2".to_owned())
    );
    assert_eq!(set_parameter_name(&mut doc, def, polar, "P9"), Ok(true));
    assert_eq!(
        next_parameter_name(&doc, def, "Polar"),
        Ok("Polar1".to_owned())
    );
}

#[test]
fn a_parameter_name_starts_at_one_without_an_authoring_graph() {
    let mut scene = Scene::new();
    scene.define_block_raw("PLAIN", Vector3::ZERO, vec![]);
    let definition = scene.document.block_records.get("PLAIN").unwrap().handle;
    assert_eq!(
        next_parameter_name(&scene.document, definition, "Polar"),
        Ok("Polar1".to_owned())
    );
}

#[test]
fn a_parameter_name_is_trimmed_and_must_be_new() {
    let (mut doc, def, handles) = fixture();
    let polar = handles[2];
    assert_eq!(set_parameter_name(&mut doc, def, polar, "  P9  "), Ok(true));
    assert_eq!(name_of(&doc, polar), "P9");
    assert_eq!(set_parameter_name(&mut doc, def, polar, "P9"), Ok(false));
    assert_eq!(
        set_parameter_name(&mut doc, def, polar, "   "),
        Err("Parameter name cannot be empty.")
    );
    assert_eq!(
        set_parameter_name(&mut doc, def, polar, "a\0b"),
        Err("Parameter text cannot contain a null character.")
    );
    assert_eq!(
        set_parameter_name(&mut doc, def, polar, "linear"),
        Err("Parameter names must be unique.")
    );
    assert_eq!(
        set_parameter_name(&mut doc, def, Handle::new(999_999), "P9"),
        Err("The block parameter is unavailable.")
    );
}
