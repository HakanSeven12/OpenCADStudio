use acadrust::entities::{EntityCommon, ExtendedEntity, ExtendedEntityData, SectionObjectData};
use acadrust::objects::{ClassObjectData, ObjectType};
use acadrust::types::{Color, Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::EntityType;
use OpenCADStudio::scene::Scene;

fn add_slice(scene: &mut Scene) -> Handle {
    let mut common = EntityCommon::new();
    let mut slice = ExtendedDataRecord::new("IsSlice");
    slice.values = vec![XDataValue::Integer16(0), XDataValue::Integer16(1)];
    common.extended_data.add_record(slice);
    let mut thickness = ExtendedDataRecord::new("ThicknessDepth");
    thickness.values = vec![XDataValue::Real(0.75)];
    common.extended_data.add_record(thickness);

    scene.add_entity(EntityType::Extended(ExtendedEntity {
        common,
        data: ExtendedEntityData::SectionObject(SectionObjectData {
            state: 1,
            flags: 5,
            name: "Section Plane (1)".to_string(),
            vertical_direction: Vector3::UNIT_Z,
            top_height: 5.0,
            bottom_height: 5.0,
            indicator_alpha: 70,
            indicator_color: Color::from_index(9),
            vertices: vec![Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0)],
            back_line_vertices: Vec::new(),
            settings_handle: Handle::NULL,
        }),
    }))
}

fn assert_section_graph(document: &acadrust::CadDocument, extension: &str) {
    let (handle, section) = document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Extended(extended) => match &extended.data {
                ExtendedEntityData::SectionObject(_) => Some((extended.common.handle, extended)),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("section entity should round-trip through {extension}"));
    let ExtendedEntityData::SectionObject(data) = &section.data else {
        unreachable!();
    };
    assert_eq!(data.state, 1);
    assert!(data.back_line_vertices.is_empty());
    assert!(matches!(
        section
            .common
            .extended_data
            .get_record("IsSlice")
            .map(|record| record.values.as_slice()),
        Some([XDataValue::Integer16(0), XDataValue::Integer16(1)])
    ));
    assert!(matches!(
        section
            .common
            .extended_data
            .get_record("ThicknessDepth")
            .and_then(|record| record.values.first()),
        Some(XDataValue::Real(value)) if (*value - 0.75).abs() < 1e-12
    ));
    assert!(matches!(
        document.objects.get(&data.settings_handle),
        Some(ObjectType::ClassObject(object))
            if object.owner == handle
                && matches!(&object.data, ClassObjectData::SectionSettings(_))
    ));

    let root = document
        .objects
        .get(&document.header.named_objects_dict_handle)
        .expect("root dictionary should round-trip");
    let ObjectType::Dictionary(root) = root else {
        panic!("root should be a dictionary");
    };
    let manager_handle = root
        .entries
        .iter()
        .find(|(name, _)| name == "ACAD_SECTION_MANAGER")
        .map(|(_, handle)| *handle)
        .expect("section manager should be registered");
    let Some(ObjectType::ClassObject(manager)) = document.objects.get(&manager_handle) else {
        panic!("section manager handle should resolve");
    };
    let ClassObjectData::SectionManager(manager) = &manager.data else {
        panic!("registered object should be a section manager");
    };
    assert_eq!(manager.sections, vec![handle]);
}

#[test]
fn section_plane_graph_survives_dxf_and_dwg_roundtrips() {
    let mut scene = Scene::new();
    add_slice(&mut scene);

    for extension in ["dxf", "dwg"] {
        let bytes =
            OpenCADStudio::io::save_to_bytes(&scene.document, extension, scene.document.version)
                .expect("section document should save");
        let document =
            OpenCADStudio::io::load_bytes(&format!("section-roundtrip.{extension}"), bytes)
                .expect("section document should reload");
        assert_section_graph(&document, extension);
    }
}
