use super::*;

use document::entities::{EntityCommon, ExtendedEntity, ExtendedEntityData, SectionObjectData};
use document::types::{Color, Vector3};

fn live_section() -> EntityType {
    EntityType::Extended(Box::new(ExtendedEntity {
        common: EntityCommon::new(),
        data: ExtendedEntityData::SectionObject(SectionObjectData {
            state: 1,
            flags: 1,
            name: "Section Plane (1)".to_string(),
            vertical_direction: Vector3::new(0.0, 0.0, 1.0),
            top_height: 1.0,
            bottom_height: 1.0,
            indicator_alpha: 70,
            indicator_color: Color::from_index(9),
            vertices: vec![Vector3::ZERO, Vector3::new(0.0, 10.0, 0.0)],
            back_line_vertices: Vec::new(),
            settings_handle: Handle::NULL,
        }),
    }))
}

/// With no section object the lookup answers at once; adding one, or
/// erasing it again, is seen through the entity edit alone.
#[test]
fn live_section_follows_added_and_erased_section_objects() {
    let mut scene = Scene::new();
    let model = scene.model_space_block_handle();
    scene.add_entity(EntityType::Line(document::entities::Line::from_points(
        Vector3::ZERO,
        Vector3::new(1.0, 0.0, 0.0),
    )));
    assert!(scene.active_live_section(model).is_none());
    assert_eq!(scene.has_section_objects.get(), Some(false));

    let section = scene.add_entity(live_section());
    assert!(scene.active_live_section(model).is_some());

    scene.erase_entities(&[section]);
    assert!(scene.active_live_section(model).is_none());
}

/// Without a table a click skips the cell pick; a table added by an
/// edit is picked at once, and none after it is erased.
#[test]
fn table_cell_pick_follows_added_and_erased_tables() {
    let mut scene = Scene::new();
    let inside = glam::DVec3::new(10.0, -5.0, 0.0);
    assert!(scene.table_cell_pick(inside, 0.0).is_none());
    assert_eq!(scene.has_tables.get(), Some(false));

    let mut table = document::entities::Table::new(Vector3::ZERO, 3, 4);
    for column in &mut table.columns {
        column.width = 20.0;
    }
    for row in &mut table.rows {
        row.height = 10.0;
    }
    let handle = scene.add_entity(EntityType::Table(Box::new(table)));
    assert_eq!(
        scene.table_cell_pick(inside, 0.0).map(|(table, _)| table),
        Some(handle)
    );

    scene.erase_entities(&[handle]);
    assert!(scene.table_cell_pick(inside, 0.0).is_none());
}
