//! An annotative dimension with a single scale representation is drawn from
//! its picture block, as AutoCAD draws it; with several it is rebuilt.

use codec::entities::{Dimension, DimensionLinear, EntityType, Line};
use codec::objects::{ObjectType, Scale};
use codec::tables::BlockRecord;
use codec::types::Vector3;
use codec::Handle;
use OpenCADStudio::scene::{annotative, Scene};

/// A dimension whose picture holds only a line no dimension would draw, so
/// finding the line means the picture was drawn.
fn scene_with_pictured_dimension() -> (Scene, Handle) {
    let mut scene = Scene::new();
    let block = scene.document.allocate_handle();
    let mut record = BlockRecord::new("*D1");
    record.handle = block;
    scene.document.block_records.add(record).unwrap();
    let mut line = EntityType::Line(Line::from_points(
        Vector3::new(500.0, 500.0, 0.0),
        Vector3::new(600.0, 700.0, 0.0),
    ));
    line.common_mut().owner_handle = block;
    scene.add_entity(line);

    let mut dim = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(150.0, 0.0, 0.0));
    dim.definition_point = Vector3::new(0.0, 50.0, 0.0);
    dim.base.block_name = "*D1".into();
    let handle = scene.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim))));
    (scene, handle)
}

fn add_scale(scene: &mut Scene, handle: Handle, name: &str, denominator: f64) {
    let scale = scene.document.allocate_handle();
    let mut s = Scale::new(name, 1.0, denominator);
    s.handle = scale;
    scene.document.objects.insert(scale, ObjectType::Scale(s));
    assert!(annotative::create_annotation_context(
        &mut scene.document,
        handle,
        scale
    ));
}

fn draws_picture(scene: &Scene) -> bool {
    scene.entity_wires().iter().any(|wire| {
        (0..wire.points.len().saturating_sub(1)).any(|index| {
            let a = wire.point_world(index, 1.0);
            let b = wire.point_world(index + 1, 1.0);
            (a.x - 500.0).abs() < 1e-3
                && (a.y - 500.0).abs() < 1e-3
                && (b.x - 600.0).abs() < 1e-3
                && (b.y - 700.0).abs() < 1e-3
        })
    })
}

#[test]
fn single_scale_annotative_dimension_draws_its_picture() {
    let (mut scene, handle) = scene_with_pictured_dimension();
    add_scale(&mut scene, handle, "1:50", 50.0);
    scene.document.header.current_annotation_scale = "1:50".into();
    assert!(annotative::is_annotative(
        &scene.document,
        scene.document.get_entity(handle).unwrap()
    ));
    assert!(draws_picture(&scene));
}

#[test]
fn multi_scale_annotative_dimension_is_rebuilt() {
    let (mut scene, handle) = scene_with_pictured_dimension();
    add_scale(&mut scene, handle, "1:50", 50.0);
    add_scale(&mut scene, handle, "1:100", 100.0);
    scene.document.header.current_annotation_scale = "1:50".into();
    assert!(!draws_picture(&scene));
}
