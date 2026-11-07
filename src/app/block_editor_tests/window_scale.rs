use super::*;
use crate::scene::block_actions as actions;
use document::{Document, EntityType, Handle};
use glam::DVec3;

fn synthetic_fixture() -> (Document, Handle, Vec<Handle>) {
    let mut scene = crate::scene::Scene::new();
    scene.define_block_raw(
        "ACTIONS",
        document::types::Vector3::ZERO,
        vec![
            EntityType::Line(document::entities::Line::from_points(
                document::types::Vector3::UNIT_X,
                document::types::Vector3::new(2., 0., 0.),
            )),
            EntityType::Circle(document::entities::Circle::from_center_radius(
                document::types::Vector3::new(4., 3., 0.),
                2.,
            )),
        ],
    );
    let block = scene.document.block_records.get("ACTIONS").unwrap();
    let (definition, handles) = (block.handle, block.entity_handles.clone());
    (scene.document, definition, handles)
}

#[test]
fn test_synthetic_dependent_scale_preserves_geometry_refresh_and_roundtrip() {
    let (mut doc, def, targets) = synthetic_fixture();
    let p = crate::scene::block_distance_parameter::create(
        &mut doc,
        def,
        crate::scene::block_distance_parameter::Kind::Linear,
        document::types::Vector3::ZERO,
        document::types::Vector3::new(10., 0., 0.),
        document::types::Vector3::new(5., 2., 0.),
    )
    .unwrap();

    // Dependent Scale action targeting the circle
    let scale_action = actions::create(
        &mut doc,
        def,
        actions::Kind::Scale,
        p,
        &targets[1..],
        DVec3::ZERO,
    )
    .unwrap();
    assert!(actions::options::base(&doc, scale_action).unwrap().dependent);

    let insert = doc
        .add_entity(EntityType::Insert(document::entities::Insert::new(
            "ACTIONS",
            document::types::Vector3::ZERO,
        )))
        .unwrap();

    let p_info = actions::all_parameters(&doc, def)
        .into_iter()
        .find(|param| param.handle == p)
        .unwrap();

    // Edit 1: Move base to -10, end remains 10 (dist = 20, 2x scale)
    let edit1 = actions::instance::evaluate_frame(
        &doc,
        insert,
        p,
        actions::instance::ParameterFrame {
            base: DVec3::new(-10., 0., 0.),
            end: DVec3::new(10., 0., 0.),
        },
    )
    .unwrap()
    .unwrap();

    let EntityType::Insert(ins1) = edit1.get_entity(insert).unwrap() else { panic!() };
    let c1 = edit1
        .entities_in_block(&ins1.block_name)
        .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
        .unwrap();
    // Circle center was (4,3). Scaled relative to new base (-10,0) by factor 2:
    // x = -10 + (4 - (-10)) * 2 = -10 + 28 = 18.
    // y = 0 + (3 - 0) * 2 = 6.
    assert_eq!(c1.center, document::types::Vector3::new(18., 6., 0.));
    assert_eq!(c1.radius, 4.);

    // Refresh after Edit 1: Reconstructing from definition must preserve evaluated action base
    let refreshed1 = actions::instance::refresh_after_edit(&edit1, &doc, def).unwrap();
    let EntityType::Insert(ins_ref1) = refreshed1.get_entity(insert).unwrap() else { panic!() };
    let c_ref1 = refreshed1
        .entities_in_block(&ins_ref1.block_name)
        .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
        .unwrap();
    assert_eq!(c_ref1.center, document::types::Vector3::new(18., 6., 0.));
    assert_eq!(c_ref1.radius, 4.);

    // Edit 2: Further move base to -20 (dist = 30, 3x scale)
    let edit2 = actions::instance::evaluate_frame(
        &edit1,
        insert,
        p,
        actions::instance::ParameterFrame {
            base: DVec3::new(-20., 0., 0.),
            end: DVec3::new(10., 0., 0.),
        },
    )
    .unwrap()
    .unwrap();

    let EntityType::Insert(ins2) = edit2.get_entity(insert).unwrap() else { panic!() };
    let c2 = edit2
        .entities_in_block(&ins2.block_name)
        .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
        .unwrap();
    // Prior position was 18. New base is -20. Distance from new base = 18 - (-20) = 38.
    // Scaled by 30 / 20 = 1.5: 38 * 1.5 = 57. Position = -20 + 57 = 37.
    // Rebuilding must retain the accumulated base; scaling the definition about -20 would yield 52.
    assert_eq!(c2.center, document::types::Vector3::new(37., 9., 0.));
    assert_eq!(c2.radius, 6.);

    // Refresh after Edit 2
    let refreshed2 = actions::instance::refresh_after_edit(&edit2, &edit1, def).unwrap();
    let EntityType::Insert(ins_ref2) = refreshed2.get_entity(insert).unwrap() else { panic!() };
    let c_ref2 = refreshed2
        .entities_in_block(&ins_ref2.block_name)
        .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
        .unwrap();
    assert_eq!(c_ref2.center, document::types::Vector3::new(37., 9., 0.));
    assert_eq!(c_ref2.radius, 6.);

    // Edit 3: Return to definition frame
    let reset = actions::instance::evaluate_frame(
        &edit2,
        insert,
        p,
        actions::instance::ParameterFrame {
            base: p_info.base,
            end: p_info.end,
        },
    )
    .unwrap()
    .unwrap();
    let EntityType::Insert(ins_reset) = reset.get_entity(insert).unwrap() else { panic!() };
    let c_reset = reset
        .entities_in_block(&ins_reset.block_name)
        .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
        .unwrap();
    assert_eq!(c_reset.center, document::types::Vector3::new(4., 3., 0.));
    assert_eq!(c_reset.radius, 2.);

    // DXF and DWG round-trips preserve the accumulated action base and geometry
    for ext in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&edit2, ext, edit2.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("synthetic_scale.{ext}"), bytes).unwrap();
        let EntityType::Insert(ins_loaded) = loaded.get_entity(insert).unwrap() else { panic!() };
        let c_loaded = loaded
            .entities_in_block(&ins_loaded.block_name)
            .find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None })
            .unwrap();
        assert_eq!(c_loaded.center, document::types::Vector3::new(37., 9., 0.));
        assert_eq!(c_loaded.radius, 6.);
    }
}

#[test]
fn synthetic_scale_refresh_uses_changed_source_geometry() {
    let (mut doc, def, targets) = synthetic_fixture();
    let p = crate::scene::block_distance_parameter::create(&mut doc, def,
        crate::scene::block_distance_parameter::Kind::Linear,
        document::types::Vector3::ZERO, document::types::Vector3::new(10., 0., 0.),
        document::types::Vector3::new(5., 2., 0.)).unwrap();
    actions::create(&mut doc, def, actions::Kind::Scale, p, &targets[1..], DVec3::ZERO).unwrap();
    let insert = doc.add_entity(EntityType::Insert(document::entities::Insert::new("ACTIONS", document::types::Vector3::ZERO))).unwrap();
    let edited = actions::instance::evaluate_frame(&doc, insert, p,
        actions::instance::ParameterFrame { base: DVec3::new(-10., 0., 0.), end: DVec3::new(10., 0., 0.) }).unwrap().unwrap();
    let mut changed = edited.clone();
    let EntityType::Circle(circle) = changed.get_entity_mut(targets[1]).unwrap() else { panic!() };
    circle.center = document::types::Vector3::new(6., 4., 0.);
    circle.radius = 3.;
    let refreshed = actions::instance::refresh_after_edit(&changed, &edited, def).unwrap();
    let check = |doc: &Document, center: document::types::Vector3, radius: f64| {
        let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else { panic!() };
        let circle = doc.entities_in_block(&reference.block_name).find_map(|e| if let EntityType::Circle(c) = e { Some(c) } else { None }).unwrap();
        assert!((circle.center.x - center.x).abs() < 1e-8 && (circle.center.y - center.y).abs() < 1e-8);
        assert!((circle.radius - radius).abs() < 1e-8);
    };
    check(&refreshed, document::types::Vector3::new(22., 8., 0.), 6.);
    let continued = actions::instance::evaluate(&refreshed, insert, p, DVec3::new(20., 0., 0.)).unwrap().unwrap();
    check(&continued, document::types::Vector3::new(38., 12., 0.), 9.);
    let test = actions::instance::evaluate_test_value(&doc, &doc, def, insert, p,
        actions::Value::Frame { base: DVec3::new(-10., 0., 0.), end: DVec3::new(10., 0., 0.) }).unwrap().unwrap();
    let reset = actions::instance::reset_test(&test, &doc, def, insert).unwrap().unwrap();
    assert!(actions::instance::load_action_bases(&reset, insert, def).unwrap().is_empty());
    let continued = actions::instance::evaluate_test_value(&reset, &doc, def, insert, p, actions::Value::Point(DVec3::new(12., 0., 0.))).unwrap().unwrap();
    check(&continued, document::types::Vector3::new(4.8, 3.6, 0.), 2.4);
}
