use super::*;

fn tile_inst(idx: usize, active: bool) -> ViewportInstance {
    ViewportInstance {
        handle: Handle::NULL,
        tile_idx: Some(idx),
        screen_rect: iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        camera: Camera::default(),
        render_mode: document::entities::ViewportRenderMode::Wireframe2D,
        active,
        grid_on: false,
        paper_sheet: false,
    }
}

#[test]
fn render_instance_id_is_per_scene_and_stable() {
    let (a, b) = (Scene::new(), Scene::new());
    let tile = tile_inst(0, true);
    assert_eq!(a.instance_id_for(&tile), b.instance_id_for(&tile));
    assert_ne!(a.render_instance_id(&tile), b.render_instance_id(&tile));
    assert_eq!(a.render_instance_id(&tile), a.render_instance_id(&tile));
}

#[test]
fn request_resolves_to_instance_ids_and_consumes_per_id() {
    let scene = Scene::new(); // default: one model tile, index 0, active
    let geom_before = scene.geometry_epoch;
    let block_before = scene.block_epoch;

    // Active -> the active model tile's instance_id.
    scene.request_refresh(ViewportRefreshScope::Active);
    let active_id = scene.instance_id_for(&tile_inst(0, true));
    assert!(scene.refresh_pending_any(), "a request must be pending");
    assert!(
        scene.refresh_consume(active_id),
        "active tile id must be consumed"
    );
    assert!(
        !scene.refresh_consume(active_id),
        "second consume for the same id is empty (one-shot per id)"
    );
    assert!(
        !scene.refresh_pending_any(),
        "id set empty after its only target consumed it"
    );

    // REDRAW never touches geometry / block epochs.
    assert_eq!(
        scene.geometry_epoch, geom_before,
        "REDRAW must not bump geometry_epoch"
    );
    assert_eq!(
        scene.block_epoch, block_before,
        "REDRAW must not bump block_epoch"
    );
}

#[test]
fn all_extinguishes_and_then_resurrects_active_after_consumption() {
    let scene = Scene::new();
    let t0 = scene.instance_id_for(&tile_inst(0, false));

    scene.request_refresh(ViewportRefreshScope::Active);
    assert!(scene.refresh_consume(t0), "Active targeted tile 0");
    assert!(
        !scene.refresh_pending_any(),
        "Active consumed: set now empty"
    );

    // A subsequent All must re-establish membership even after the Active
    // request was fully drained (the superset is resolved at request time).
    scene.request_refresh(ViewportRefreshScope::All);
    assert!(
        scene.refresh_pending_any(),
        "All re-pends after Active was consumed"
    );
    assert!(
        scene.refresh_consume(t0),
        "All must re-force tile 0 after Active drained"
    );
}

#[test]
fn all_targets_every_existing_model_tile() {
    let mut scene = Scene::new();
    // Create MULTIPLE real model tiles through the public split API so the
    // test exercises the per-id REDRAWALL guarantee (the very reason for the
    // FxHashSet design): every EXISTING tile id must be targeted and drained.
    scene.split_active_pane(false); // 2 tiles
    scene.split_active_pane(false); // 3 tiles
    assert!(
        scene.model_tiles.borrow().len() >= 3,
        "split must have produced tiles"
    );

    scene.request_refresh(ViewportRefreshScope::All);
    for idx in 0..scene.model_tiles.borrow().len() {
        let id = scene.instance_id_for(&tile_inst(idx, false));
        assert!(
            scene.refresh_consume(id),
            "All must target EXISTING tile {idx}, not just tile 0"
        );
    }
    assert!(
        !scene.refresh_pending_any(),
        "All drained every existing tile"
    );
    // A tile index that does not exist must NOT be drained.
    let ghost = scene.instance_id_for(&tile_inst(99, false));
    assert!(
        !scene.refresh_consume(ghost),
        "nonexistent tile 99 has no membership"
    );
}
