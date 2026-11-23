use super::Scene;
use document::entities::{Block, BlockEnd, Line};
use document::types::Vector3;
use document::{EntityType, Handle};

/// Fresh scene holding two block sentinels (Block + BlockEnd) and one plain
/// line; returns their handles in that order.
fn scene_with_sentinels_and_line() -> (Scene, Handle, Handle, Handle) {
    let mut scene = Scene::new();
    let bh = scene
        .document
        .add_entity(EntityType::Block(Block::new("BATCH_TEST", Vector3::ZERO)))
        .unwrap();
    let be = scene
        .document
        .add_entity(EntityType::BlockEnd(BlockEnd::new()))
        .unwrap();
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 1.0);
    line.end = Vector3::new(3.0, 4.0, 2.0);
    let lh = scene.document.add_entity(EntityType::Line(line)).unwrap();
    (scene, bh, be, lh)
}

/// Stored entity as-is — the update shape the command arms actually pass.
fn snapshot(scene: &Scene, handle: Handle) -> EntityType {
    scene.document.get_entity(handle).unwrap().clone()
}

/// The same line with an edited Z, still carrying its original handle.
fn edited_line(scene: &Scene, handle: Handle) -> EntityType {
    let mut entity = snapshot(scene, handle);
    if let EntityType::Line(line) = &mut entity {
        line.start.z = 5.0;
        line.end.z = 6.0;
    }
    entity
}

fn delta_count(scene: &Scene) -> usize {
    scene.geometry_deltas.borrow().len()
}

#[test]
fn update_entities_batch_of_block_sentinels_collapses_to_one_delta() {
    // Batched path: one coarse publish for the whole set.
    let (mut batched, bh, be, _) = scene_with_sentinels_and_line();
    let before = delta_count(&batched);
    let updates = vec![snapshot(&batched, bh), snapshot(&batched, be)];
    let moved = batched.update_entities(updates);
    assert_eq!(moved, 2);
    {
        let ring = batched.geometry_deltas.borrow();
        assert_eq!(
            ring.len() - before,
            1,
            "batched sentinels must publish exactly one geometry delta"
        );
        let delta = ring.back().unwrap();
        for h in [bh, be] {
            assert!(
                delta.full || delta.changes.iter().any(|(c, _)| *c == h),
                "the batched delta must name sentinel {h:?}"
            );
        }
    }

    // Legacy path: each sentinel bumps on its own.
    let (mut legacy, bh, be, _) = scene_with_sentinels_and_line();
    let before = delta_count(&legacy);
    let u1 = snapshot(&legacy, bh);
    let u2 = snapshot(&legacy, be);
    assert!(legacy.update_entity(u1));
    assert!(legacy.update_entity(u2));
    assert_eq!(
        delta_count(&legacy) - before,
        2,
        "legacy per-entity updates must publish one delta each"
    );

    // Same final entity state either way.
    for h in [bh, be] {
        assert_eq!(
            format!("{:?}", batched.document.get_entity(h)),
            format!("{:?}", legacy.document.get_entity(h)),
            "entity {h:?} differs between batched and legacy updates"
        );
    }
}

#[test]
fn update_entities_mixed_batch_matches_legacy_states_with_one_delta() {
    // Batched: one delta covers the sentinel and the line, and both slots
    // are still written.
    let (mut batched, bh, _, lh) = scene_with_sentinels_and_line();
    let before = delta_count(&batched);
    let updates = vec![snapshot(&batched, bh), edited_line(&batched, lh)];
    let moved = batched.update_entities(updates);
    assert_eq!(moved, 2);
    {
        let ring = batched.geometry_deltas.borrow();
        assert_eq!(
            ring.len() - before,
            1,
            "mixed batch with a sentinel must publish one coarse delta"
        );
        let delta = ring.back().unwrap();
        for h in [bh, lh] {
            assert!(
                delta.full || delta.changes.iter().any(|(c, _)| *c == h),
                "the batched delta must name {h:?}"
            );
        }
    }

    // Legacy: one delta for the sentinel + one for the line.
    let (mut legacy, bh, _, lh) = scene_with_sentinels_and_line();
    let before = delta_count(&legacy);
    let u1 = snapshot(&legacy, bh);
    let u2 = edited_line(&legacy, lh);
    assert!(legacy.update_entity(u1));
    assert!(legacy.update_entity(u2));
    assert_eq!(
        delta_count(&legacy) - before,
        2,
        "legacy publishes one delta per update"
    );

    for h in [bh, lh] {
        assert_eq!(
            format!("{:?}", batched.document.get_entity(h)),
            format!("{:?}", legacy.document.get_entity(h)),
            "entity {h:?} differs between batched and legacy updates"
        );
    }
}
