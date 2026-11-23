use super::Scene;
use document::entities::{Insert, Line};
use document::types::Vector3;
use document::EntityType;

fn line(a: [f64; 2], b: [f64; 2]) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(a[0], a[1], 0.0),
        Vector3::new(b[0], b[1], 0.0),
    ))
}

/// STRETCH gathers its objects through this query: block copies and
/// lines away from the window must stay out of it (#1891).
#[test]
fn world_box_handles_leave_out_far_blocks_and_lines() {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "CHAIR",
        Vector3::new(0.0, 0.0, 0.0),
        vec![line([0.0, 0.0], [1.0, 1.0])],
    );
    let insert = |x: f64, y: f64| {
        EntityType::Insert(Insert::new("CHAIR".to_string(), Vector3::new(x, y, 0.0)))
    };
    let far_block = scene.add_entity(insert(-500.0, 0.0));
    let far_line = scene.add_entity(line([-500.0, 10.0], [-490.0, 10.0]));
    let near_block = scene.add_entity(insert(12.0, 2.0));
    let near_line = scene.add_entity(line([5.0, 5.0], [20.0, 5.0]));

    let handles = scene.interaction_handles_in_world_aabb([10.0, 0.0, 30.0, 10.0]);

    assert!(handles.contains(&near_line) && handles.contains(&near_block));
    assert!(
        !handles.contains(&far_block),
        "far block copy joined the box"
    );
    assert!(!handles.contains(&far_line), "far line joined the box");
}
