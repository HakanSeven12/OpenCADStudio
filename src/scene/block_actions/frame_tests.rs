use super::*;
use codec::objects::{XRecordEntry, XRecordValue};
use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
use instance::ParameterFrame;
use std::io::Cursor;

fn fixture(kind: usize) -> (Document, Handle, Handle, Parameter) {
    let (mut doc, definition, _) = super::tests::fixture();
    let base = Vector3::new(2., 3., 7.);
    let end = Vector3::new(12., 13., 7.);
    let parameter = match kind {
        0 => crate::scene::block_point_parameter::create(&mut doc, definition, base, end),
        1..=3 => crate::scene::block_distance_parameter::create(
            &mut doc,
            definition,
            match kind {
                1 => crate::scene::block_distance_parameter::Kind::Linear,
                2 => crate::scene::block_distance_parameter::Kind::Polar,
                _ => crate::scene::block_distance_parameter::Kind::XY,
            },
            base,
            end,
            Vector3::new(5., 6., 7.),
        ),
        _ => crate::scene::block_orientation_parameter::create(
            &mut doc,
            definition,
            crate::scene::block_orientation_parameter::Kind::Rotation,
            base,
            end,
            end,
        ),
    }
    .unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let p = all_parameters(&doc, definition)
        .into_iter()
        .find(|p| p.handle == parameter)
        .unwrap();
    (doc, definition, insert, p)
}

fn record(doc: &mut Document) -> &mut codec::objects::XRecord {
    doc.objects
        .values_mut()
        .find_map(|object| match object {
            ObjectType::XRecord(record) => Some(record),
            _ => None,
        })
        .unwrap()
}

#[test]
fn block_parameter_frames_preserve_both_endpoints_and_native_metadata() {
    for kind in 0..5 {
        let (mut doc, definition, insert, p) = fixture(kind);
        let original = doc.clone();
        let initial = ParameterFrame {
            base: p.base,
            end: p.end,
        };
        assert_eq!(instance::frame(&doc, insert, &p).unwrap(), initial);
        let expected = ParameterFrame {
            base: if p.point {
                p.base
            } else {
                DVec3::new(-8., -9., 7.)
            },
            end: DVec3::new(24., 27., 7.),
        };
        instance::save_frame(&mut doc, insert, &p, expected).unwrap();
        let metadata = vec![
            XRecordEntry::string(1, "retain parameter metadata"),
            XRecordEntry::new(10, XRecordValue::Point3D(51., 52., 53.)),
            XRecordEntry::new(330, XRecordValue::Handle(definition)),
        ];
        record(&mut doc).entries.extend(metadata.clone());
        let endpoint = DVec3::new(31., 34., 7.);
        instance::save(&mut doc, insert, &p, Value::Point(endpoint)).unwrap();
        let expected = ParameterFrame {
            end: endpoint,
            ..expected
        };
        // Cache edits must not modify any existing definition or grip object.
        for (handle, object) in &original.objects {
            assert_eq!(doc.objects.get(handle), Some(object));
        }
        for mut saved in [
            doc.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                .unwrap()
                .read()
                .unwrap(),
        ] {
            assert_eq!(
                instance::frame(&saved, insert, &p).unwrap(),
                expected,
                "family {kind}"
            );
            assert!(record(&mut saved).entries.ends_with(&metadata));
            let parameter = all_parameters(&saved, definition)
                .into_iter()
                .find(|parameter| parameter.handle == p.handle)
                .unwrap();
            assert_eq!(parameter.base, p.base);
            assert_eq!(parameter.end, p.end);
            for entity in original
                .entities()
                .filter(|entity| entity.common().handle != insert)
            {
                let reopened = saved.get_entity(entity.common().handle).unwrap();
                assert_eq!(
                    std::mem::discriminant(reopened),
                    std::mem::discriminant(entity)
                );
                assert_eq!(
                    reopened.as_entity().bounding_box(),
                    entity.as_entity().bounding_box()
                );
            }
            let EntityType::Insert(reference) = saved.get_entity(insert).unwrap() else {
                panic!()
            };
            assert!(reference.common.xdictionary_handle.is_some());
            assert_eq!(reference.block_name, "ACTIONS");
            assert_eq!(reference.insert_point, Vector3::ZERO);
            assert_eq!(instance::current(&saved, insert, &p).unwrap(), expected.end);

        }
        let reset = instance::reset(&doc, insert).unwrap().unwrap();
        assert_eq!(instance::frame(&reset, insert, &p).unwrap(), initial);
        assert!(instance::reset(&reset, insert).unwrap().is_none());
        let reset_test = instance::reset_test(&doc, &original, definition, insert)
            .unwrap()
            .unwrap();
        assert_eq!(instance::frame(&reset_test, insert, &p).unwrap(), initial);
        assert_eq!(doc.objects[&p.handle], original.objects[&p.handle]);
    }
}

#[test]
fn block_parameter_frame_writes_reject_invalid_or_incomplete_records_atomically() {
    for kind in 0..5 {
        let (mut doc, _, insert, p) = fixture(kind);
        let valid = ParameterFrame {
            base: p.base,
            end: p.end,
        };
        for invalid in [
            ParameterFrame {
                base: DVec3::splat(f64::NAN),
                ..valid
            },
            ParameterFrame {
                end: DVec3::splat(f64::INFINITY),
                ..valid
            },
            ParameterFrame {
                base: p.base + DVec3::Z,
                ..valid
            },
            ParameterFrame {
                end: p.end + DVec3::Z,
                ..valid
            },
        ] {
            let before = doc.clone();
            assert!(instance::save_frame(&mut doc, insert, &p, invalid).is_err());
            assert_eq!(doc, before);
        }
        let before = doc.clone();
        assert!(instance::save_frame(&mut doc, Handle::NULL, &p, valid).is_err());
        assert_eq!(doc, before);
        instance::save_frame(&mut doc, insert, &p, valid).unwrap();
        let owner = record(&mut doc).owner;
        record(&mut doc).owner = Handle::NULL;
        let before = doc.clone();
        assert!(instance::frame(&doc, insert, &p).is_err());
        assert!(instance::save_frame(&mut doc, insert, &p, valid).is_err());
        assert_eq!(doc, before);
        record(&mut doc).owner = owner;
        record(&mut doc).entries_complete = false;
        let before = doc.clone();
        assert!(instance::save_frame(&mut doc, insert, &p, valid).is_err());
        assert!(instance::save(&mut doc, insert, &p, Value::Point(p.end)).is_err());
        assert_eq!(doc, before);
        record(&mut doc).entries_complete = true;
        record(&mut doc)
            .entries
            .retain(|entry| !matches!(entry.value, XRecordValue::Point3D(..)));
        let before = doc.clone();
        assert!(instance::save_frame(&mut doc, insert, &p, valid).is_err());
        assert_eq!(doc, before);
    }
}
