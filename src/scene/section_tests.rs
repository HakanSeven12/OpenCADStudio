use super::*;

use document::entities::{
    EntityCommon, ExtendedEntity, ExtendedEntityData, SectionObjectData, Solid3D,
};
use document::objects::ClassObjectData;
use document::types::{Color, Vector3};

fn section(state: i32, depth: f64) -> SectionObjectData {
    let vertices = vec![Vector3::new(5.0, 2.0, 5.0), Vector3::new(5.0, 8.0, 5.0)];
    SectionObjectData {
        state,
        flags: 5,
        name: "Section Plane (1)".to_string(),
        vertical_direction: Vector3::UNIT_Z,
        top_height: 1.0,
        bottom_height: 2.0,
        indicator_alpha: 70,
        indicator_color: Color::from_index(9),
        back_line_vertices: (depth > 0.0)
            .then(|| {
                vertices
                    .iter()
                    .map(|point| *point + Vector3::new(-depth, 0.0, 0.0))
                    .collect()
            })
            .unwrap_or_default(),
        vertices,
        settings_handle: Handle::NULL,
    }
}

fn bounds(body: Option<ocs_solid::model::Brep>) -> ([f64; 3], [f64; 3]) {
    let body = body.expect("section should retain material");
    assert!(ocs_solid::model::check(&body).is_empty());
    let bounds = ocs_solid::model::aabb(&body).expect("sectioned body should be bounded");
    (bounds.min, bounds.max)
}

fn assert_near(actual: [f64; 3], expected: [f64; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }
}

#[test]
fn kernel_clipping_obeys_plane_slice_boundary_and_volume_limits() {
    let body = ocs_solid::model::make_box([0.0; 3], [10.0; 3]).unwrap();

    let (min, max) = bounds(Scene::section_body(&body, &section(1, 0.0), None, None).unwrap());
    assert_near(min, [0.0, 0.0, 0.0]);
    assert_near(max, [5.0, 10.0, 10.0]);

    let (min, max) = bounds(Scene::section_body(&body, &section(1, 0.0), Some(2.0), None).unwrap());
    assert_near(min, [3.0, 0.0, 0.0]);
    assert_near(max, [5.0, 10.0, 10.0]);

    let (min, max) = bounds(Scene::section_body(&body, &section(2, 4.0), None, None).unwrap());
    assert_near(min, [1.0, 2.0, 0.0]);
    assert_near(max, [5.0, 8.0, 10.0]);

    let (min, max) = bounds(Scene::section_body(&body, &section(4, 4.0), None, None).unwrap());
    assert_near(min, [1.0, 2.0, 3.0]);
    assert_near(max, [5.0, 8.0, 6.0]);
}

#[test]
fn instance_transform_pulls_the_section_plane_into_body_space() {
    let body = ocs_solid::model::make_box([0.0; 3], [10.0; 3]).unwrap();
    let transform =
        document::types::Transform::from_scaling(document::types::Vector3::new(2.0, 1.0, 1.0))
            .then(&document::types::Transform::from_translation(
                document::types::Vector3::new(10.0, 0.0, 0.0),
            ));
    let mut data = section(1, 0.0);
    for point in &mut data.vertices {
        point.x = 20.0;
    }

    let (min, max) = bounds(Scene::section_body(&body, &data, None, Some(&transform)).unwrap());
    assert_near(min, [0.0, 0.0, 0.0]);
    assert_near(max, [5.0, 10.0, 10.0]);
}

#[test]
fn adding_and_erasing_a_section_keeps_its_object_graph_consistent() {
    let mut scene = Scene::new();
    let handle = scene.add_entity(EntityType::Extended(Box::new(ExtendedEntity {
        common: EntityCommon::new(),
        data: ExtendedEntityData::SectionObject(section(1, 0.0)),
    })));
    let settings_handle = match scene.document.get_entity(handle).unwrap() {
        EntityType::Extended(extended) => match &extended.data {
            ExtendedEntityData::SectionObject(data) => data.settings_handle,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    assert!(matches!(
        scene.document.objects.get(&settings_handle),
        Some(ObjectType::ClassObject(object))
            if object.owner == handle
                && matches!(&object.data, ClassObjectData::SectionSettings(_))
    ));

    let root = scene.document.header.named_objects_dict_handle;
    let manager_handle = match scene.document.objects.get(&root).unwrap() {
        ObjectType::Dictionary(dictionary) => dictionary
            .entries
            .iter()
            .find(|(name, _)| name == "ACAD_SECTION_MANAGER")
            .map(|(_, handle)| *handle)
            .unwrap(),
        _ => panic!("root must be a dictionary"),
    };
    assert!(matches!(
        scene.document.objects.get(&manager_handle),
        Some(ObjectType::ClassObject(object))
            if matches!(&object.data, ClassObjectData::SectionManager(manager)
                if manager.sections == vec![handle])
    ));

    scene.erase_entities(&[handle]);
    assert!(!scene.document.objects.contains_key(&settings_handle));
    assert!(matches!(
        scene.document.objects.get(&manager_handle),
        Some(ObjectType::ClassObject(object))
            if matches!(&object.data, ClassObjectData::SectionManager(manager)
                if manager.sections.is_empty())
    ));
}

#[test]
fn live_section_rebuilds_an_uncached_persisted_solid() {
    let body = ocs_solid::model::make_box([0.0; 3], [10.0; 3]).unwrap();
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body).unwrap();
    let mut solid = Solid3D::new();
    solid.set_sat_document(&sat);
    let mut scene = Scene::new();
    let handle = scene.add_entity(EntityType::Solid3D(solid));
    assert!(!scene.solid_models.contains_key(&handle));

    let clipped = scene
        .sectioned_body(
            handle,
            &LiveSection {
                data: section(1, 0.0),
                slice_depth: None,
            },
        )
        .unwrap();
    let (min, max) = bounds(clipped);
    assert_near(min, [0.0, 0.0, 0.0]);
    assert_near(max, [5.0, 10.0, 10.0]);
}
