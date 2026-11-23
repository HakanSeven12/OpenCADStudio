use super::*;

use document::entities::{Insert, Line};
use document::types::Vector3;

#[test]
fn bump_entities_with_block_deps_preserves_unrelated_caches() {
    let mut scene = Scene::new();
    let bg_line = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(100.0, 0.0, 0.0),
        Vector3::new(110.0, 0.0, 0.0),
    )));

    scene.define_block_raw(
        "Unrelated",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
        ))],
    );
    let mut ins_unrelated = Insert::new("Unrelated", Vector3::new(50.0, 0.0, 0.0));
    ins_unrelated.common.handle = Handle::NULL;
    let unrelated_insert = scene.add_entity(EntityType::Insert(ins_unrelated));

    scene.define_block_raw(
        "Child",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ))],
    );
    let child_line = scene
        .document
        .block_records
        .get("Child")
        .unwrap()
        .entity_handles[0];

    let mut nested_child = Insert::new("Child", Vector3::new(1.0, 0.0, 0.0));
    nested_child.common.handle = Handle::NULL;
    scene.define_block_raw(
        "Parent",
        Vector3::ZERO,
        vec![EntityType::Insert(nested_child)],
    );

    let mut ins_child = Insert::new("Child", Vector3::new(10.0, 0.0, 0.0));
    ins_child.common.handle = Handle::NULL;
    let child_insert = scene.add_entity(EntityType::Insert(ins_child));

    let mut ins_parent = Insert::new("Parent", Vector3::new(20.0, 0.0, 0.0));
    ins_parent.common.handle = Handle::NULL;
    let parent_insert = scene.add_entity(EntityType::Insert(ins_parent));

    let _ = scene.entity_wires();
    assert!(scene.resident_tess_memo.borrow().contains_key(&bg_line));
    assert!(scene
        .resident_tess_memo
        .borrow()
        .contains_key(&unrelated_insert));

    let before_cache = scene.active_block_cache_arc();
    let unrelated_defn_before = before_cache.defn_ptr("Unrelated").unwrap();
    let child_defn_before = before_cache.defn_ptr("Child").unwrap();
    let epoch_before = scene.geometry_epoch;

    if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(child_line) {
        line.end.x = 6.0;
    }
    scene.bump_entities_with_block_deps(&[(child_line, ChangeKind::Modified)], &[], &[], false);

    let delta = scene
        .replay_since(epoch_before)
        .expect("scoped block edit must emit an incremental delta, not a full reset");
    let changed_handles: HashSet<Handle> = delta.into_iter().map(|(h, _)| h).collect();
    assert!(changed_handles.contains(&child_line));
    assert!(changed_handles.contains(&child_insert));
    assert!(changed_handles.contains(&parent_insert));
    assert!(!changed_handles.contains(&bg_line));
    assert!(!changed_handles.contains(&unrelated_insert));

    assert!(scene.has_resident_tess_memo(bg_line));
    assert!(scene.has_resident_tess_memo(unrelated_insert));

    let _ = scene.entity_wires();
    let after_cache = scene.active_block_cache_arc();
    assert_eq!(
        unrelated_defn_before,
        after_cache.defn_ptr("Unrelated").unwrap()
    );
    assert_ne!(child_defn_before, after_cache.defn_ptr("Child").unwrap());
}

#[test]
fn with_rebuilt_blocks_drops_deleted_block_definitions() {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "KeepBlock",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(2.0, 0.0, 0.0),
        ))],
    );
    scene.define_block_raw(
        "TempBlock",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(4.0, 0.0, 0.0),
        ))],
    );
    let mut ins_keep = Insert::new("KeepBlock", Vector3::ZERO);
    ins_keep.common.handle = Handle::NULL;
    scene.add_entity(EntityType::Insert(ins_keep));
    let mut ins_temp = Insert::new("TempBlock", Vector3::new(10.0, 0.0, 0.0));
    ins_temp.common.handle = Handle::NULL;
    let temp_ins = scene.add_entity(EntityType::Insert(ins_temp));

    let _ = scene.entity_wires();
    let before_cache = scene.active_block_cache_arc();
    let keep_before = before_cache.defn_ptr("KeepBlock").unwrap();
    assert!(before_cache.defn_ptr("TempBlock").is_some());

    scene.document.block_records.remove("TempBlock");
    if let Some(EntityType::Insert(ins)) = scene.document.get_entity_mut(temp_ins) {
        ins.block_name = "KeepBlock".to_string();
    }
    scene.bump_entities_with_block_deps(
        &[(temp_ins, ChangeKind::Modified)],
        &[],
        &["TempBlock".to_string()],
        false,
    );

    let _ = scene.entity_wires();
    let after_cache = scene.active_block_cache_arc();
    assert!(after_cache.defn_ptr("TempBlock").is_none());
    assert_eq!(keep_before, after_cache.defn_ptr("KeepBlock").unwrap());
}
