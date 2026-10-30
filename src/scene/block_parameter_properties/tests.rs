use super::*;
use crate::scene::{
    block_distance_parameter as distance, block_orientation_parameter as orientation,
    block_point_parameter as point, Scene,
};
use codec::entities::{Insert, Line};
use codec::types::Vector3;

fn fixture(kind: usize, action: bool) -> (Document, Handle, Handle, Parameter) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "VALUES",
        Vector3::new(5., 6., 2.),
        vec![EntityType::Line(Line::from_points(
            Vector3::new(3., 5., 7.),
            Vector3::new(4., 5., 7.),
        ))],
    );
    let def = scene.document.block_records.get("VALUES").unwrap().handle;
    let member = scene
        .document
        .block_records
        .get("VALUES")
        .unwrap()
        .entity_handles[0];
    let doc = &mut scene.document;
    let base = Vector3::new(2., 3., 7.);
    let end = Vector3::new(12., 13., 7.);
    let handle = match kind {
        0 => point::create(doc, def, base, end),
        1..=3 => distance::create(
            doc,
            def,
            match kind {
                1 => distance::Kind::Linear,
                2 => distance::Kind::Polar,
                _ => distance::Kind::XY,
            },
            base,
            end,
            Vector3::new(8., 9., 7.),
        ),
        _ => orientation::create(
            doc,
            def,
            if kind == 4 {
                orientation::Kind::Rotation
            } else {
                orientation::Kind::Flip
            },
            base,
            end,
            end,
        ),
    }
    .unwrap();
    if action {
        actions::create(
            doc,
            def,
            match kind {
                4 => actions::Kind::Rotate,
                5 => actions::Kind::Flip,
                _ => actions::Kind::Move,
            },
            handle,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
    }
    let mut reference = Insert::new("VALUES", Vector3::new(100., 200., 20.));
    reference.set_x_scale(-2.);
    reference.set_y_scale(2.);
    reference.set_z_scale(2.);
    let insert = doc.add_entity(EntityType::Insert(reference)).unwrap();
    let p = actions::all_parameters(doc, def)
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap();
    (scene.document, def, insert, p)
}
fn key(doc: &Document, insert: Handle, component: Component) -> Key {
    properties(doc, None, 42, insert)
        .into_iter()
        .find_map(|row| match row.value {
            PropValue::BlockNumber { key, .. } | PropValue::BlockChoice { key, .. }
                if key.component == component =>
            {
                Some(key)
            }
            _ => None,
        })
        .unwrap()
}
fn numeric(doc: &Document, insert: Handle, component: Component) -> f64 {
    properties(doc, None, 42, insert)
        .into_iter()
        .find_map(|row| match row.value {
            PropValue::BlockNumber { key, value, .. } if key.component == component => {
                value.parse().ok()
            }
            _ => None,
        })
        .unwrap()
}
fn close(a: DVec3, b: DVec3) {
    assert!(a.distance(b) < 1e-8, "{a:?} != {b:?}");
}
#[test]
fn block_reference_properties_all_components_drive_frames_and_isolate_native_geometry() {
    for kind in 0..6 {
        for with_action in [false, true] {
            let (mut doc, _def, insert, p) = fixture(kind, with_action);
            let original = doc.clone();
            let other = doc
                .add_entity(EntityType::Insert(Insert::new("VALUES", Vector3::ZERO)))
                .unwrap();
            let other_before = doc.get_entity(other).unwrap().clone();
            let mut current = instance::frame(&doc, insert, &p).unwrap();
            if kind != 0 && kind != 5 {
                current.base += DVec3::new(20., -4., 0.);
                current.end += DVec3::new(20., -4., 0.);
                instance::save_frame(&mut doc, insert, &p, current).unwrap();
            }
            let components = match kind {
                0 | 3 => vec![Component::X, Component::Y],
                1 => vec![Component::Distance],
                2 => vec![Component::Distance, Component::Angle],
                4 => vec![Component::Angle],
                _ => vec![Component::Flip],
            };
            for component in components {
                let field = key(&doc, insert, component);
                let input = if component == Component::Flip {
                    Input::Choice(1)
                } else {
                    Input::Number(if component == Component::Angle {
                        90.
                    } else {
                        20.
                    })
                };
                let evaluated = evaluate(&doc, None, field, input).unwrap().unwrap();
                if component == Component::Flip {
                    assert_eq!(
                        instance::value(&evaluated, insert, &p).unwrap(),
                        Value::Flip(true)
                    );
                } else {
                    let frame = instance::frame(&evaluated, insert, &p).unwrap();
                    close(frame.base, current.base);
                    let delta = current.end - current.base;
                    let expected = match component {
                        Component::X => {
                            DVec3::new(current.base.x + 20., current.end.y, current.end.z)
                        }
                        Component::Y => {
                            DVec3::new(current.end.x, current.base.y + 20., current.end.z)
                        }
                        Component::Distance => {
                            current.base + DVec3::new(1., 1., 0.).normalize() * 20.
                        }
                        Component::Angle => current.base + DVec3::Y * delta.length(),
                        _ => unreachable!(),
                    };
                    close(frame.end, expected);
                    assert!(
                        (numeric(&evaluated, insert, component)
                            - if component == Component::Angle {
                                90.
                            } else {
                                20.
                            })
                        .abs()
                            < 1e-8
                    );
                    let via_grip = instance::evaluate_value(
                        &doc,
                        insert,
                        p.handle,
                        Value::Frame {
                            base: current.base,
                            end: expected,
                        },
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(
                        evaluated, via_grip,
                        "{kind}: properties and grip share complete evaluation"
                    );
                }
                for (h, object) in original
                    .objects
                    .iter()
                    .filter(|(_, object)| matches!(object, ObjectType::DynamicBlock(_)))
                {
                    assert_eq!(
                        evaluated.objects.get(h),
                        Some(object),
                        "shared native object {h}"
                    );
                }
                for h in &original.block_records.get("VALUES").unwrap().entity_handles {
                    assert_eq!(evaluated.get_entity(*h), original.get_entity(*h));
                }
                assert_eq!(evaluated.get_entity(other), Some(&other_before));
                let EntityType::Insert(reference) = evaluated.get_entity(insert).unwrap() else {
                    panic!()
                };
                let represented = &evaluated
                    .block_records
                    .get(&reference.block_name)
                    .unwrap()
                    .entity_handles;
                if !with_action {
                    assert!(represented.iter().filter_map(|h| evaluated.get_entity(*h)).any(|entity| matches!(entity, EntityType::Line(line) if line.start == Vector3::new(3., 5., 7.))));
                }
                for extension in ["dwg", "dxf"] {
                    let bytes =
                        crate::io::save_to_bytes(&evaluated, extension, evaluated.version).unwrap();
                    let loaded =
                        crate::io::load_bytes(&format!("reference-values.{extension}"), bytes)
                            .unwrap();
                    assert_eq!(
                        properties(&loaded, None, 42, insert),
                        properties(&evaluated, None, 42, insert),
                        "{extension}: {kind}"
                    );
                }
            }
        }
    }
}
#[test]
fn block_reference_properties_constraints_visibility_invalid_inputs_and_reset() {
    let (mut doc, def, insert, p) = fixture(1, true);
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&p.handle) else {
        panic!()
    };
    let Data::LinearParameter(linear) = &mut object.data else {
        panic!()
    };
    linear.value_set.flags = 8;
    linear.value_set.values = vec![10., 20.];
    linear.distance_description = "Native length description".into();
    let field = key(&doc, insert, Component::Distance);
    let changed = evaluate(&doc, None, field, Input::Number(18.))
        .unwrap()
        .unwrap();
    assert_eq!(numeric(&changed, insert, Component::Distance), 20.);
    assert!(evaluate(&changed, None, field, Input::Number(18.))
        .unwrap()
        .is_none());
    for input in [
        Input::Number(f64::NAN),
        Input::Number(f64::INFINITY),
        Input::Number(-1.),
        Input::Choice(1),
    ] {
        assert!(evaluate(&doc, None, field, input).is_err());
    }
    assert!(evaluate(
        &doc,
        None,
        Key {
            component: Component::X,
            ..field
        },
        Input::Number(10.)
    )
    .is_err());
    assert!(evaluate(
        &doc,
        None,
        Key {
            definition: Handle::from(0xFFFFFFu64),
            ..field
        },
        Input::Number(10.)
    )
    .is_err());
    let before = doc.clone();
    super::super::block_authoring::metadata::set_visible(&mut doc, def, p.handle, false).unwrap();
    assert!(properties(&doc, None, 42, insert).is_empty());
    assert!(evaluate(&doc, None, field, Input::Number(10.)).is_err());
    assert_eq!(actions::all_parameters(&doc, def).len(), 1);
    super::super::block_authoring::metadata::set_visible(&mut doc, def, p.handle, true).unwrap();
    assert_eq!(doc, before);
    let reset = instance::reset(&changed, insert).unwrap().unwrap();
    close(instance::frame(&reset, insert, &p).unwrap().end, p.end);
}
#[test]
fn block_reference_properties_duplicate_flip_labels_keep_distinct_states() {
    let (mut doc, _, insert, p) = fixture(5, true);
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&p.handle) else {
        panic!()
    };
    let Data::FlipParameter(flip) = &mut object.data else {
        panic!()
    };
    flip.base_state_label = "Same label".into();
    flip.flipped_state_label = "Same label".into();
    let field = key(&doc, insert, Component::Flip);
    let flipped = evaluate(&doc, None, field, Input::Choice(1))
        .unwrap()
        .unwrap();
    assert!(matches!(
        properties(&flipped, None, 42, insert)[0].value,
        PropValue::BlockChoice { selected: 1, .. }
    ));
    let restored = evaluate(&flipped, None, field, Input::Choice(0))
        .unwrap()
        .unwrap();
    assert_eq!(
        instance::value(&restored, insert, &p).unwrap(),
        Value::Flip(false)
    );
    assert!(evaluate(&doc, None, field, Input::Choice(2)).is_err());
}

#[test]
fn block_reference_property_values_print_at_the_drawing_precision() {
    // The rows format from the drawing they belong to, so a headless caller or
    // a non-active tab never prints with another drawing's settings.
    use crate::entities::common::{set_unit_context, UnitContext};
    set_unit_context(UnitContext {
        lunits: 2,
        luprec: 4,
        aunits: 0,
        auprec: 0,
        ..Default::default()
    });
    let (mut doc, _def, insert, _p) = fixture(2, false);
    doc.header.linear_unit_format = 2;
    doc.header.linear_unit_precision = 2;
    doc.header.angular_unit_precision = 1;
    let rows = properties(&doc, None, 42, insert);
    let text = |component| {
        rows.iter()
            .find_map(|row| match &row.value {
                PropValue::BlockNumber { key, value, .. } if key.component == component => {
                    Some(value.clone())
                }
                _ => None,
            })
            .unwrap()
    };
    // Polar distance is 10² + 10² squared; the angle is exactly 45°.
    assert_eq!(text(Component::Distance), "14.14");
    assert_eq!(text(Component::Angle), "45.0");
}
