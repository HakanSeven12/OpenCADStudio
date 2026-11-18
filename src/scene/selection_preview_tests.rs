//! Tests for the selection-preview performance split: the hover generation
//! separation (rollover / marquee no longer rebuild the committed selection's
//! overlay), the selectable-group snapshot invalidation, and the marquee
//! preview cap (LOD instead of all-or-nothing).

use super::*;
use document::entities::Line;
use document::types::Vector3;

fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(a.0, a.1, 0.0),
        Vector3::new(b.0, b.1, 0.0),
    ))
}

#[test]
fn hover_bumps_hover_generation_only() {
    let mut scene = Scene::new();
    let h = scene.add_entity(line((0.0, 0.0), (1.0, 1.0)));
    let sel_gen = scene.selection_generation;
    let hover_gen = scene.hover_generation;
    scene.set_hover_highlight(Some(h));
    assert_eq!(
        scene.selection_generation, sel_gen,
        "a rollover must not rebuild the committed selection's overlay"
    );
    assert_ne!(scene.hover_generation, hover_gen);
    // Hovering the same handle again is a no-op.
    let hover_gen = scene.hover_generation;
    scene.set_hover_highlight(Some(h));
    assert_eq!(scene.hover_generation, hover_gen);
}

#[test]
fn box_preview_caps_to_the_limit_instead_of_clearing() {
    let mut scene = Scene::new();
    let handles: Vec<Handle> = (0..5)
        .map(|index| scene.add_entity(line((index as f64, 0.0), (index as f64, 1.0))))
        .collect();
    scene.set_box_hover_highlights_limited(&handles, 3);
    assert_eq!(
        scene.box_hover_highlights().len(),
        3,
        "over the limit the preview is capped, not dropped"
    );
    // Ascending-handle order keeps the subset stable across ticks: the
    // lowest three handles stay highlighted.
    let mut sorted = handles.clone();
    sorted.sort_by_key(|handle| handle.value());
    for handle in &sorted[..3] {
        assert!(
            scene.box_hover_highlights().contains(handle),
            "handle {:?} must stay highlighted",
            handle
        );
    }
    // Under the limit everything is highlighted.
    scene.set_box_hover_highlights_limited(&handles, 10);
    assert_eq!(scene.box_hover_highlights().len(), 5);
}

#[test]
fn box_preview_hover_bumps_hover_generation_only() {
    let mut scene = Scene::new();
    let handles: Vec<Handle> = (0..3)
        .map(|index| scene.add_entity(line((index as f64, 0.0), (index as f64, 1.0))))
        .collect();
    let sel_gen = scene.selection_generation;
    scene.set_box_hover_highlights_limited(&handles, usize::MAX);
    assert_eq!(
        scene.selection_generation, sel_gen,
        "a marquee tick must not rebuild the committed selection's overlay"
    );
    assert_eq!(scene.box_hover_highlights().len(), 3);
}

#[test]
fn group_expansion_snapshot_follows_group_edits() {
    let mut scene = Scene::new();
    let members: Vec<Handle> = (0..3)
        .map(|y| scene.add_entity(line((0.0, y as f64), (1.0, y as f64))))
        .collect();
    let other = scene.add_entity(line((5.0, 5.0), (6.0, 6.0)));
    scene.document.header.pick_style |= 1; // PICKSTYLE group selection on

    scene.create_group("G".into(), vec![members[0], members[1]]);
    let expanded = scene.handles_expanded_for_selectable_groups(&[members[0]]);
    assert!(expanded.contains(&members[1]));
    assert!(!expanded.contains(&members[2]), "not a member of the group");
    assert!(!expanded.contains(&other));

    // Removing one member must invalidate the snapshot.
    assert!(scene.remove_entity_from_group_named("G", members[1]));
    let expanded = scene.handles_expanded_for_selectable_groups(&[members[0]]);
    assert!(
        !expanded.contains(&members[1]),
        "the snapshot must drop after a group edit"
    );
    assert_eq!(expanded.len(), 1);

    // Deleting the group clears the expansion entirely.
    assert_eq!(scene.delete_groups_containing(&[members[0]]), 1);
    let expanded = scene.handles_expanded_for_selectable_groups(&[members[0]]);
    assert_eq!(expanded.len(), 1);
}

#[test]
fn erase_refreshes_hover_generation_without_selection_change() {
    // Erasing the hovered-but-unselected entity must still drop the stale
    // hover overlay — via the hover generation, not the selection one.
    let mut scene = Scene::new();
    let _h1 = scene.add_entity(line((0.0, 0.0), (1.0, 1.0)));
    let h2 = scene.add_entity(line((2.0, 2.0), (3.0, 3.0)));
    scene.set_hover_highlight(Some(h2));
    let sel_gen = scene.selection_generation;
    let hover_gen = scene.hover_generation;
    scene.erase_entities(&[h2]);
    assert_eq!(scene.hover_highlight, None);
    assert_ne!(
        scene.hover_generation, hover_gen,
        "erase must drop the stale hover overlay"
    );
    assert_eq!(
        scene.selection_generation, sel_gen,
        "no committed selection changed"
    );
}
