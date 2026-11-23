use super::*;

use document::classes::DxfClass;
use document::entities::{EntityType, Insert, Viewport};
use document::types::Vector3;

/// The pre-memo implementation, kept literal: one full scan per call.
fn literal(scene: &Scene, vp: Handle) -> HashSet<Handle> {
    let mut hidden: HashSet<Handle> = match scene.document.get_entity(vp) {
        Some(EntityType::Viewport(v)) => v.frozen_layers.iter().copied().collect(),
        _ => HashSet::default(),
    };
    if scene
        .document
        .classes
        .get_by_name("ACIDBLOCKREFERENCE")
        .is_some()
    {
        let model = scene.model_space_block_handle();
        hidden.extend(scene.document.entities().filter_map(|e| match e {
            EntityType::Insert(i)
                if i.common.owner_handle == model
                    && i.view_rep_handle.is_some_and(|bound| bound != vp) =>
            {
                Some(i.common.handle)
            }
            _ => None,
        }));
    }
    hidden
}

struct Fixture {
    scene: Scene,
    first: Handle,
    second: Handle,
    to_first: Handle,
    to_second: Handle,
    unbound: Handle,
    paper_owned: Handle,
}

fn fixture(declare: bool) -> Fixture {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    let mut first = Viewport::new();
    first.frozen_layers = vec![Handle::new(9001)];
    let first = scene.add_entity(EntityType::Viewport(Box::new(first)));
    let second = scene.add_entity(EntityType::Viewport(Box::new(Viewport::new())));
    // Owned by the sheet, so it is not a model-space reference.
    let mut paper_owned = Insert::new("ViewRep", Vector3::new(0.0, 0.0, 0.0));
    paper_owned.view_rep_handle = Some(first);
    let paper_owned = scene.add_entity(EntityType::Insert(paper_owned));

    scene.set_current_layout("Model".to_string());
    let bound = |vp: Handle, x: f64| {
        let mut insert = Insert::new("ViewRep", Vector3::new(x, 0.0, 0.0));
        insert.view_rep_handle = Some(vp);
        EntityType::Insert(insert)
    };
    let to_first = scene.add_entity(bound(first, 0.0));
    let to_second = scene.add_entity(bound(second, 10.0));
    let unbound = scene.add_entity(EntityType::Insert(Insert::new(
        "ViewRep",
        Vector3::new(20.0, 0.0, 0.0),
    )));
    if declare {
        scene.document.classes.add_or_update(DxfClass {
            dxf_name: "ACIDBLOCKREFERENCE".to_string(),
            cpp_class_name: "AcDbBlockRepresentationContext".to_string(),
            application_name: "ObjectDBX Classes".to_string(),
            proxy_flags: document::classes::ProxyFlags(1153),
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            item_class_id: 0x1F3,
            dwg_version: 25,
            maintenance_version: 0,
            unknown1: 0,
            unknown2: 0,
        });
    }
    scene.set_current_layout("Sheet".to_string());
    Fixture {
        scene,
        first,
        second,
        to_first,
        to_second,
        unbound,
        paper_owned,
    }
}

#[test]
fn the_memo_matches_a_literal_scan() {
    let Fixture {
        scene,
        first,
        second,
        to_first,
        to_second,
        unbound,
        paper_owned,
    } = fixture(true);
    let hidden_first = scene.viewport_hidden_handles(first);
    let hidden_second = scene.viewport_hidden_handles(second);
    assert_eq!(hidden_first, literal(&scene, first));
    assert_eq!(hidden_second, literal(&scene, second));
    assert!(hidden_first.contains(&Handle::new(9001)));
    assert!(!hidden_first.contains(&to_first));
    assert!(hidden_first.contains(&to_second));
    assert!(hidden_second.contains(&to_first));
    assert!(!hidden_second.contains(&to_second));
    for handle in [unbound, paper_owned] {
        assert!(!hidden_first.contains(&handle));
        assert!(!hidden_second.contains(&handle));
    }
}

#[test]
fn without_the_class_only_the_viewport_frozen_layers_hide() {
    let Fixture {
        scene,
        first,
        second,
        ..
    } = fixture(false);
    let hidden = scene.viewport_hidden_handles(first);
    assert_eq!(hidden.len(), 1);
    assert!(hidden.contains(&Handle::new(9001)));
    assert!(scene.viewport_hidden_handles(second).is_empty());
}

#[test]
fn a_reference_added_or_rebound_after_a_lookup_is_seen() {
    let Fixture {
        mut scene,
        first,
        second,
        to_first,
        ..
    } = fixture(true);
    scene.viewport_hidden_handles(first);

    scene.set_current_layout("Model".to_string());
    let mut late = Insert::new("ViewRep", Vector3::new(30.0, 0.0, 0.0));
    late.view_rep_handle = Some(second);
    let late = scene.add_entity(EntityType::Insert(late));
    scene.set_current_layout("Sheet".to_string());
    assert_eq!(scene.viewport_hidden_handles(first), literal(&scene, first));
    assert!(scene.viewport_hidden_handles(first).contains(&late));

    if let Some(EntityType::Insert(insert)) = scene.document.get_entity_mut(to_first) {
        insert.view_rep_handle = Some(first);
    }
    scene.bump_entities(&[(to_first, ChangeKind::Modified)]);
    assert_eq!(scene.viewport_hidden_handles(first), literal(&scene, first));
    assert!(!scene.viewport_hidden_handles(first).contains(&to_first));
    assert!(scene.viewport_hidden_handles(second).contains(&to_first));
}
