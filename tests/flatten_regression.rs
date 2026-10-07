//! FLATTEN regression: golden projections, collection filters, plan/apply
//! invariants, and a legacy-vs-batch differential.

use codec::entities::{Line, Polyline2D, Polyline3D, Solid, Vertex2D, Vertex3DPolyline};
use codec::types::Vector3;
use codec::{EntityType, Handle};

use OpenCADStudio::modules::draw::modify::flatten::{
    apply_flatten_updates, collect_flatten_handles, flatten_entity_z, plan_flatten,
};
use OpenCADStudio::scene::Scene;

fn line_with_z(z0: f64, z1: f64) -> EntityType {
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, z0);
    line.end = Vector3::new(3.0, 4.0, z1);
    line.thickness = 2.0;
    EntityType::Line(line)
}

#[test]
fn golden_line_projects_and_resets_thickness_normal() {
    let flat = flatten_entity_z(&line_with_z(3.0, 5.0)).expect("moves");
    let EntityType::Line(l) = flat else { panic!("variant") };
    assert_eq!((l.start.z, l.end.z), (0.0, 0.0));
    assert_eq!((l.start.x, l.start.y), (0.0, 0.0));
    assert_eq!((l.end.x, l.end.y), (3.0, 4.0));
    assert_eq!(l.thickness, 0.0);
    assert_eq!(l.normal, Vector3::UNIT_Z);
}

#[test]
fn golden_tilted_circle_becomes_ellipse() {
    let mut c = codec::entities::Circle::from_center_radius(Vector3::new(1.0, 2.0, 4.0), 5.0);
    c.normal = Vector3::new(0.0, 0.6, 0.8);
    let flat = flatten_entity_z(&EntityType::Circle(c)).expect("moves");
    assert!(matches!(flat, EntityType::Ellipse(_)), "tilted circle → ellipse");
}

#[test]
fn golden_polyline3d_vertices_and_elevation_zeroed() {
    let mut pl = Polyline3D::new();
    pl.elevation = 5.0;
    pl.vertices.push(Vertex3DPolyline::new(Vector3::new(1.0, 2.0, 7.5)));
    let flat = flatten_entity_z(&EntityType::Polyline3D(pl)).expect("moves");
    let EntityType::Polyline3D(pl) = flat else { panic!("variant") };
    assert_eq!(pl.elevation, 0.0);
    assert!(pl.vertices.iter().all(|v| v.position.z == 0.0 && v.position.x == 1.0));
}

#[test]
fn golden_polyline2d_elevation_zeroed() {
    let mut pl = Polyline2D::new();
    pl.elevation = 9.0;
    pl.vertices.push(Vertex2D::new(Vector3::new(3.0, 4.0, 0.0)));
    let flat = flatten_entity_z(&EntityType::Polyline2D(pl)).expect("moves");
    let EntityType::Polyline2D(pl) = flat else { panic!("variant") };
    assert_eq!(pl.elevation, 0.0);
    assert_eq!(pl.vertices[0].location.z, 0.0);
}

#[test]
fn golden_text_insertion_and_normal_zeroed() {
    let mut t = codec::entities::Text::new();
    t.insertion_point = Vector3::new(1.0, 2.0, 3.0);
    t.thickness = 1.0;
    let flat = flatten_entity_z(&EntityType::Text(t)).expect("moves");
    let EntityType::Text(t) = flat else { panic!("variant") };
    assert_eq!(t.insertion_point.z, 0.0);
    assert_eq!(t.thickness, 0.0);
    assert_eq!(t.normal, Vector3::UNIT_Z);
}

#[test]
fn golden_solid_corners_zeroed() {
    let s = Solid::new(
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 2.0),
        Vector3::new(1.0, 1.0, 3.0),
        Vector3::new(0.0, 1.0, 4.0),
    );
    let flat = flatten_entity_z(&EntityType::Solid(s)).expect("moves");
    let EntityType::Solid(s) = flat else { panic!("variant") };
    assert_eq!(
        [s.first_corner.z, s.second_corner.z, s.third_corner.z, s.fourth_corner.z],
        [0.0; 4]
    );
}

#[test]
fn already_flat_and_unsupported_return_none() {
    let mut flat_line = Line::new(); // zero thickness + z-normal by default
    flat_line.start = Vector3::new(0.0, 0.0, 0.0);
    flat_line.end = Vector3::new(3.0, 4.0, 0.0);
    assert!(flatten_entity_z(&EntityType::Line(flat_line)).is_none());
    let ray = codec::entities::Ray::new(Vector3::new(0.0, 0.0, 5.0), Vector3::new(0.0, 0.0, 1.0));
    assert!(flatten_entity_z(&EntityType::Ray(ray)).is_none());
}

fn scene_with_three_lines() -> (Scene, [Handle; 3]) {
    let mut scene = Scene::new();
    let a = scene.document.add_entity(line_with_z(1.0, 2.0)).unwrap();
    let b = scene.document.add_entity(line_with_z(1.0, 2.0)).unwrap();
    let c = scene.document.add_entity(line_with_z(1.0, 2.0)).unwrap();
    (scene, [a, b, c])
}

#[test]
fn collect_excludes_locked_layer_and_respects_selection() {
    let (mut scene, [a, b, c]) = scene_with_three_lines();
    // Lock b's layer.
    let mut layer = codec::tables::Layer::new("LOCKED");
    layer.flags.locked = true;
    scene.document.layers.add(layer).unwrap();
    scene.document.get_entity_mut(b).unwrap().common_mut().layer = "LOCKED".into();

    // Empty selection → all active/unlocked candidates (b excluded).
    let candidates = collect_flatten_handles(&scene);
    assert!(candidates.contains(&a) && candidates.contains(&c));
    assert!(!candidates.contains(&b));

    // Non-empty selection → only the selected handles (minus locked).
    scene.selected.insert(a);
    scene.selected.insert(b);
    let candidates = collect_flatten_handles(&scene);
    assert_eq!(candidates, vec![a]);
}

#[test]
fn collect_excludes_entities_outside_active_space() {
    let (mut scene, [a, b, c]) = scene_with_three_lines();
    // Enter block editor scoping on block b: entities whose owner block
    // differs from the active space must be excluded.
    scene.block_edit_block = Some(b);
    scene.document.get_entity_mut(a).unwrap().common_mut().owner_handle = c; // wrong owner
    scene.document.get_entity_mut(c).unwrap().common_mut().owner_handle = b; // active block

    let candidates = collect_flatten_handles(&scene);
    assert!(!candidates.contains(&a), "wrong-owner entity must be excluded");
    assert!(candidates.contains(&c), "owner of the active block must be included");
}

#[test]
fn plan_moves_only_movable_candidates() {
    let (mut scene, [a, b, c]) = scene_with_three_lines();
    let ray = scene
        .document
        .add_entity(EntityType::Ray(codec::entities::Ray::new(
            Vector3::new(0.0, 0.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
        )))
        .unwrap();
    let handles = collect_flatten_handles(&scene);
    assert_eq!(handles.len(), 4);
    let updates = plan_flatten(&scene, &handles);
    assert_eq!(updates.len(), 3, "ray must not be planned");
    assert!(updates.iter().all(|e| !matches!(e, EntityType::Ray(_))));

    let moved = apply_flatten_updates(&mut scene, updates);
    assert_eq!(moved, 3);
    for h in [a, b, c] {
        let EntityType::Line(l) = scene.document.get_entity(h).unwrap() else {
            panic!("variant")
        };
        assert_eq!((l.start.z, l.end.z), (0.0, 0.0));
    }
    // Ray untouched.
    assert!(matches!(scene.document.get_entity(ray).unwrap(), EntityType::Ray(_)));
}

#[test]
fn differential_legacy_loop_vs_apply_flatten_updates() {
    let build = || {
        let mut scene = Scene::new();
        let mut h = Vec::new();
        for i in 0..50 {
            let line = line_with_z(1.0 + i as f64, 2.0);
            h.push(scene.document.add_entity(line).unwrap());
            if i % 10 == 0 {
                h.push(
                    scene
                        .document
                        .add_entity(EntityType::Ray(codec::entities::Ray::new(
                            Vector3::new(0.0, 0.0, 5.0),
                            Vector3::new(0.0, 0.0, 1.0),
                        )))
                        .unwrap(),
                );
            }
            if i % 7 == 0 {
                h.push(scene.document.add_entity(line_with_z(0.0, 0.0)).unwrap());
            }
        }
        let handles = collect_flatten_handles(&scene);
        let updates = plan_flatten(&scene, &handles);
        (scene, handles, updates)
    };

    // Reference: the legacy per-entity loop (what the command arm did).
    let (mut legacy, handles, updates) = build();
    let mut moved_legacy = 0usize;
    for entity in updates {
        if legacy.update_entity(entity) {
            moved_legacy += 1;
        }
    }

    // Candidate: the helper under test.
    let (mut batched, handles2, updates2) = build();
    let moved_batched = apply_flatten_updates(&mut batched, updates2);

    assert_eq!(moved_legacy, moved_batched);
    assert_eq!(handles, handles2);
    for h in handles {
        assert_eq!(
            format!("{:?}", legacy.document.get_entity(h)),
            format!("{:?}", batched.document.get_entity(h)),
            "entity {h:?} differs between legacy loop and apply_flatten_updates"
        );
    }
}
