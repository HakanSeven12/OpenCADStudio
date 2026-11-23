use super::*;

use super::plot_owner_aci;

#[test]
fn expanded_block_wire_keeps_its_resolved_effective_aci() {
    let mut scene = crate::scene::Scene::new();

    // Real-world regression case:
    // the INSERT itself has explicit true-color white, while its block
    // children are layer 0 + ByLayer. BlockCache resolves those children
    // through the INSERT's layer and stores that effective ACI on the wire.
    let mut insert = document::entities::Insert::new("*U264", document::types::Vector3::ZERO);
    insert.common.color = document::types::Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    let insert_handle = scene
        .document
        .add_entity(document::EntityType::Insert(insert))
        .expect("insert must be added");

    let mut wire = crate::scene::WireModel::default();
    wire.name = insert_handle.value().to_string();

    // Pretend BlockCache resolved the layer-0 child to indexed color 5.
    // The plot preparation step must preserve it instead of replacing it
    // with ACI 0 from the host INSERT's true color.
    wire.aci = 5;

    plot_owner_aci(&scene, &mut wire);

    assert_eq!(
        wire.aci, 5,
        "expanded block geometry must keep the effective ACI already resolved by BlockCache"
    );
}

#[test]
fn direct_entity_still_refreshes_aci_from_its_owner() {
    let mut scene = crate::scene::Scene::new();

    let mut line = document::entities::Line::from_points(
        document::types::Vector3::ZERO,
        document::types::Vector3::new(1.0, 0.0, 0.0),
    );
    line.common.color = document::types::Color::Index(1);

    let line_handle = scene
        .document
        .add_entity(document::EntityType::Line(line))
        .expect("line must be added");

    let mut wire = crate::scene::WireModel::default();
    wire.name = line_handle.value().to_string();
    wire.aci = 5;

    plot_owner_aci(&scene, &mut wire);

    assert_eq!(
        wire.aci, 1,
        "ordinary entity plot wires must still resolve ACI from their owner"
    );
}
