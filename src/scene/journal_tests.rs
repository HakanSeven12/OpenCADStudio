use super::*;

fn h(v: u64) -> Handle {
    Handle::new(v)
}

// Collect replay output into a handle->kind map for order-independent asserts.
fn as_map(v: Option<Vec<(Handle, ChangeKind)>>) -> Option<HashMap<Handle, ChangeKind>> {
    v.map(|list| list.into_iter().collect())
}

#[test]
fn replay_reports_changes_since_a_synced_epoch() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    s.bump_entities(&[(h(10), ChangeKind::Added)]);
    let e1 = s.geometry_epoch;
    s.bump_entities(&[(h(11), ChangeKind::Modified)]);

    // From e0 we see both; from e1 only the second.
    let all = as_map(s.replay_since(e0)).unwrap();
    assert_eq!(all.get(&h(10)), Some(&ChangeKind::Added));
    assert_eq!(all.get(&h(11)), Some(&ChangeKind::Modified));
    let since_e1 = as_map(s.replay_since(e1)).unwrap();
    assert_eq!(since_e1.len(), 1);
    assert_eq!(since_e1.get(&h(11)), Some(&ChangeKind::Modified));
}

#[test]
fn added_then_removed_cancels() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    s.bump_entities(&[(h(20), ChangeKind::Added)]);
    s.bump_entities(&[(h(20), ChangeKind::Removed)]);
    let m = as_map(s.replay_since(e0)).unwrap();
    assert!(
        !m.contains_key(&h(20)),
        "add+remove in one window must cancel"
    );
}

#[test]
fn modified_then_removed_is_removed() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    s.bump_entities(&[(h(21), ChangeKind::Modified)]);
    s.bump_entities(&[(h(21), ChangeKind::Removed)]);
    assert_eq!(
        as_map(s.replay_since(e0)).unwrap().get(&h(21)),
        Some(&ChangeKind::Removed)
    );
}

#[test]
fn full_bump_forces_rebuild() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    s.bump_entities(&[(h(30), ChangeKind::Added)]);
    s.bump_geometry(); // full delta
    assert!(s.replay_since(e0).is_none(), "a spanned full delta ⇒ None");
}

#[test]
fn insert_add_and_update_publish_targeted_deltas() {
    use document::entities::Insert;
    use document::types::Vector3;

    let mut s = Scene::new();
    let before_add = s.geometry_epoch;
    let handle = s.add_entity(EntityType::Insert(Insert::new(
        "EXISTING_BLOCK",
        Vector3::new(0.0, 0.0, 0.0),
    )));
    assert_eq!(
        as_map(s.replay_since(before_add)).unwrap().get(&handle),
        Some(&ChangeKind::Added),
        "adding an INSERT must not force a whole-drawing rebuild"
    );

    let before_update = s.geometry_epoch;
    let mut edited = s.document.get_entity(handle).cloned().unwrap();
    if let EntityType::Insert(insert) = &mut edited {
        insert.insert_point.x = 25.0;
    }
    assert!(s.update_entity(edited));
    assert_eq!(
        as_map(s.replay_since(before_update)).unwrap().get(&handle),
        Some(&ChangeKind::Modified),
        "retargeting or moving an INSERT changes only its render handle"
    );
}

#[test]
fn dependency_categories_invalidate_only_affected_render_handles() {
    use document::entities::{Line, MText, Point, Text};
    use document::types::Vector3;

    let mut s = Scene::new();
    let line = s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
    )));
    let point = s.add_entity(EntityType::Point(Point::new()));
    let text = s.add_entity(EntityType::Text(Text::with_value(
        "plain",
        Vector3::new(0.0, 2.0, 0.0),
    )));
    let mut annotative = MText::new();
    annotative.value = "annotative".to_string();
    annotative.is_annotative = true;
    let annotative = s.add_entity(EntityType::MText(Box::new(annotative)));

    let before_points = s.geometry_epoch;
    s.invalidate_point_dependencies();
    let points = as_map(s.replay_since(before_points)).unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points.get(&point), Some(&ChangeKind::Modified));

    let before_text = s.geometry_epoch;
    s.invalidate_text_geometry_dependencies();
    let texts = as_map(s.replay_since(before_text)).unwrap();
    assert_eq!(texts.get(&text), Some(&ChangeKind::Modified));
    assert_eq!(texts.get(&annotative), Some(&ChangeKind::Modified));
    assert!(!texts.contains_key(&line));
    assert!(!texts.contains_key(&point));

    let before_annotation = s.geometry_epoch;
    s.invalidate_annotation_dependencies();
    let annotation = as_map(s.replay_since(before_annotation)).unwrap();
    assert_eq!(annotation.len(), 1);
    assert_eq!(annotation.get(&annotative), Some(&ChangeKind::Modified));
}

#[test]
fn object_visibility_uses_entity_deltas() {
    use document::entities::Line;
    use document::types::Vector3;

    let mut s = Scene::new();
    let hidden = s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    let untouched = s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    )));

    s.selected.insert(hidden);
    let before_hide = s.geometry_epoch;
    s.hide_selected();
    let hide = as_map(s.replay_since(before_hide)).unwrap();
    assert_eq!(hide.len(), 1);
    assert_eq!(hide.get(&hidden), Some(&ChangeKind::Modified));
    assert!(!hide.contains_key(&untouched));

    let before_show = s.geometry_epoch;
    s.end_isolation();
    let show = as_map(s.replay_since(before_show)).unwrap();
    assert_eq!(show.len(), 1);
    assert_eq!(show.get(&hidden), Some(&ChangeKind::Modified));
}

#[test]
fn ring_overflow_forces_rebuild() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    for i in 0..(GEOMETRY_JOURNAL_CAP as u64 + 5) {
        s.bump_entities(&[(h(1000 + i), ChangeKind::Added)]);
    }
    assert!(
        s.replay_since(e0).is_none(),
        "a consumer older than the ring floor ⇒ None"
    );
    // A recent consumer is still replayable.
    let recent = s.geometry_epoch;
    s.bump_entities(&[(h(9999), ChangeKind::Added)]);
    assert!(s.replay_since(recent).is_some());
}

// Differential oracle: the incrementally-patched entity index must always
// equal a from-scratch rebuild after any add / move / erase.
/// A line has tangent geometry but does not feed the analytical uploads.
#[test]
fn the_partition_membership_test_separates_lines_from_curves() {
    use crate::scene::pipeline::wire_arena::feeds_analytical_uploads;
    use document::entities::{Circle, Line};
    use document::types::Vector3;

    let mut scene = Scene::new();
    let feeds = |scene: &Scene, handle: Handle| {
        let entity = scene.document.get_entity(handle).unwrap().clone();
        scene
            .tessellate_one(&entity)
            .iter()
            .any(feeds_analytical_uploads)
    };

    let line = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 5.0, 0.0),
    )));
    assert!(
        !feeds(&scene, line),
        "a drawn line must not force the partition walk",
    );

    let mut circle = Circle::new();
    circle.radius = 4.0;
    let circle = scene.add_entity(EntityType::Circle(circle));
    assert!(
        feeds(&scene, circle),
        "a drawn circle feeds the analytical uploads and must force the walk",
    );
}

/// Incremental additions preserve existing depths; full rebuilds invalidate them.
#[test]
fn adding_an_entity_leaves_existing_draw_depths_alone() {
    use document::entities::Line;
    use document::types::Vector3;

    fn add(scene: &mut Scene, x: f64) -> Handle {
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, 0.0, 0.0),
            Vector3::new(x + 1.0, 0.0, 0.0),
        )))
    }
    let mut scene = Scene::new();
    let first = add(&mut scene, 0.0);
    let second = add(&mut scene, 1.0);
    let before = scene.draw_depth_map();
    let generation = scene.draw_depth_generation();

    add(&mut scene, 2.0);
    let after = scene.draw_depth_map();
    assert_eq!(
        scene.draw_depth_generation(),
        generation,
        "an incremental add must not invalidate baked depths",
    );
    for handle in [first, second] {
        assert_eq!(
            after.get(&handle.value()),
            before.get(&handle.value()),
            "an existing entity's depth must not move when another is added",
        );
    }

    // A rebuild from scratch reassigns labels, so it has to say so.
    scene.draw_depth_cache.borrow_mut().take();
    let _ = scene.draw_depth_map();
    assert_ne!(
        scene.draw_depth_generation(),
        generation,
        "a full rebuild must invalidate anything holding a baked depth",
    );
}

/// A folded addition must match the full document-order scan.
#[test]
fn folding_one_addition_matches_the_full_scan() {
    use document::entities::Line;
    use document::types::Vector3;

    let mut scene = Scene::new();
    fn add(scene: &mut Scene, x: f64) -> Handle {
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, 0.0, 0.0),
            Vector3::new(x + 1.0, 0.0, 0.0),
        )))
    }
    add(&mut scene, 0.0);
    add(&mut scene, 1.0);
    // Build the index, then add one more so the next call folds.
    let block = scene.model_space_block_handle();
    let before = scene
        .block_members()
        .1
        .get(&block)
        .cloned()
        .unwrap_or_default();
    let third = add(&mut scene, 2.0);
    let folded = scene
        .block_members()
        .1
        .get(&block)
        .cloned()
        .unwrap_or_default();

    scene.block_members_cache.borrow_mut().take();
    let rebuilt = scene
        .block_members()
        .1
        .get(&block)
        .cloned()
        .unwrap_or_default();

    assert_eq!(
        folded, rebuilt,
        "the folded index must equal a full rebuild"
    );
    assert_eq!(
        folded.last(),
        Some(&third),
        "an appended entity belongs at the end of its block's list",
    );
    assert_eq!(folded.len(), before.len() + 1);
}

// Differential oracle: the incrementally-patched entity index must always
// equal a from-scratch rebuild after any add / move / erase.
#[test]
fn block_member_index_matches_the_full_scan() {
    use document::entities::Line;
    use document::types::Vector3;

    fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a.0, a.1, 0.0),
            Vector3::new(b.0, b.1, 0.0),
        ))
    }

    // What the membership index reports for `block`, in order.
    fn indexed(s: &Scene, block: Handle) -> Vec<Handle> {
        let members = s.block_members();
        let (_, by_block) = &*members;
        by_block.get(&block).cloned().unwrap_or_default()
    }

    // What testing every entity the old way reports, in document order.
    fn scanned(s: &Scene, block: Handle) -> Vec<Handle> {
        s.document
            .entities()
            .filter(|entity| {
                let common = entity.common();
                s.belongs_to_visible_block(common.handle, common.owner_handle, block)
            })
            .map(|entity| entity.common().handle)
            .collect()
    }

    let mut s = Scene::new();
    let block = s.model_space_block_handle();
    let h1 = s.add_entity(line((0.0, 0.0), (10.0, 10.0)));
    let h2 = s.add_entity(line((5.0, 5.0), (20.0, 3.0)));
    let _ = (h1, h2);

    // Order matters: it is the wire pass's draw order.
    assert_eq!(indexed(&s, block), scanned(&s, block), "after adds");

    let h3 = s.add_entity(line((100.0, 100.0), (140.0, 90.0)));
    assert_eq!(indexed(&s, block), scanned(&s, block), "after another add");

    // A modify must not reorder the block's contents either.
    {
        let mut moved = line((900.0, 900.0), (950.0, 970.0));
        moved.common_mut().handle = h3;
        s.update_entity(moved);
    }
    assert_eq!(indexed(&s, block), scanned(&s, block), "after modify");
}

/// Raw layer spellings used as memo keys must resolve through the
/// case-insensitive table.
#[test]
fn the_layer_memo_respects_case_insensitive_layer_names() {
    use document::entities::Line;
    use document::tables::layer::Layer;
    use document::types::Vector3;

    let mut scene = Scene::new();
    let mut hidden = Layer::new("Walls");
    hidden.flags.off = true;
    scene.document.layers.add_or_replace(hidden);

    let mut on_layer = |layer: &str| {
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = layer.to_string();
        scene.add_entity(EntityType::Line(line))
    };
    // Spellings of the switched-off layer must all remain hidden.
    on_layer("Walls");
    on_layer("WALLS");
    on_layer("walls");
    on_layer("Doors");

    let block = scene.model_space_block_handle();
    let visible: Vec<_> = scene
        .document
        .entities()
        .filter(|e| scene.resident_entity_visible(e, block, None, None, true))
        .map(|e| e.common().layer.clone())
        .collect();
    assert_eq!(
        visible,
        vec!["Doors".to_string()],
        "every spelling of a switched-off layer must be hidden",
    );

    // Each spelling resolves to the same table entry.
    let resolved: Vec<_> = ["Walls", "WALLS", "walls"]
        .iter()
        .map(|name| scene.document.layers.get(name).map(|l| l.name.clone()))
        .collect();
    assert_eq!(
        resolved,
        vec![Some("Walls".to_string()); 3],
        "the table is case-insensitive, which is what lets the memo key on \
             the raw name",
    );
}

#[test]
fn imported_model_space_hatch_without_layout_object_is_a_layer_target() {
    use document::entities::Hatch;
    use document::objects::ObjectType;
    use document::tables::layer::Layer;

    let mut scene = Scene::new();
    scene.document.layers.add_or_replace(Layer::new("Shadows"));
    let mut hatch = Hatch::new();
    hatch.common.layer = "Shadows".to_string();
    let handle = scene.add_entity(EntityType::Hatch(Box::new(hatch)));
    let model = scene.model_space_block_handle();
    assert_eq!(
        scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .owner_handle,
        model
    );
    scene
        .document
        .objects
        .retain(|_, object| !matches!(object, ObjectType::Layout(_)));
    scene.invalidate_dependency_index();

    let targets = scene.dependency_targets(DependencyKind::Layer, &["Shadows".to_string()]);
    assert!(targets.render_handles.contains(&handle));
    assert!(targets.source_handles.contains(&handle));
    assert!(!targets.touches_block_definition);

    scene.document.layers.get_mut("Shadows").unwrap().flags.off = true;
    let epoch = scene.geometry_epoch;
    scene.invalidate_layer_dependencies(&["Shadows".to_string()]);
    assert_ne!(
        scene.geometry_epoch, epoch,
        "the cached hatch needs an entity delta"
    );
}

#[test]
fn linetype_dependency_invalidation_stays_incremental_for_direct_and_bylayer_users() {
    use document::entities::Line;
    use document::tables::layer::Layer;
    use document::tables::linetype::{LineType, LineTypeElement};
    use document::types::Vector3;

    let mut scene = Scene::new();
    let mut lt = LineType::new("DASHED");
    lt.elements = vec![
        LineTypeElement {
            length: 0.5,
            complex: None,
        },
        LineTypeElement {
            length: -0.25,
            complex: None,
        },
    ];
    lt.pattern_length = 0.75;
    let _ = scene.document.line_types.add(lt);

    let mut layer = Layer::new("DashedLayer");
    layer.line_type = "DASHED".to_string();
    scene.document.layers.add_or_replace(layer);

    let mut direct_line =
        Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    direct_line.common.linetype = "dashed".to_string();
    let direct = scene.add_entity(EntityType::Line(direct_line));

    let mut bylayer_line =
        Line::from_points(Vector3::new(0.0, 2.0, 0.0), Vector3::new(10.0, 2.0, 0.0));
    bylayer_line.common.layer = "DashedLayer".to_string();
    let bylayer = scene.add_entity(EntityType::Line(bylayer_line));

    let plain_line = Line::from_points(Vector3::new(0.0, 4.0, 0.0), Vector3::new(10.0, 4.0, 0.0));
    let plain = scene.add_entity(EntityType::Line(plain_line));

    let epoch_before = scene.geometry_epoch;
    let block_epoch_before = scene.block_epoch;

    scene.invalidate_linetype_dependencies(&["DASHED".to_string()]);

    assert_ne!(scene.geometry_epoch, epoch_before);
    assert_eq!(scene.block_epoch, block_epoch_before);
    let deltas = scene
        .replay_since(epoch_before)
        .expect("linetype invalidation must emit an incremental delta");
    let changed: HashSet<Handle> = deltas.into_iter().map(|(h, _)| h).collect();
    assert!(changed.contains(&direct));
    assert!(changed.contains(&bylayer));
    assert!(!changed.contains(&plain));
}

/// The loader's handed-over draw depths must equal a local rebuild.
#[test]
fn the_handed_over_draw_depth_map_matches_a_recompute() {
    use document::entities::{Circle, Line};
    use document::types::Vector3;

    let mut scene = Scene::new();
    scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    scene.add_entity(EntityType::Circle(Circle::new()));
    scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::new(3.0, 0.0, 0.0),
    )));

    // Capture the map the loader would hand over.
    let expected = scene.draw_depth_map();
    let handed_over = scene
        .draw_depth_cache
        .borrow_mut()
        .take()
        .expect("the map was just computed, so it is cached");

    // Install it under the receiving scene's epoch.
    scene.bump_geometry();
    scene.install_prepared_open_geometry(PreparedOpenGeometry {
        dependency_index: None,
        wires: Arc::new(Vec::new()),
        interaction_index: None,
        entity_index: None,
        tess_memo: HashMap::default(),
        tess_memo_backing: MemoBacking::default(),
        tess_guard: 0,
        tess_atlas: 0,
        block_defns: Vec::new(),
        draw_depths: Some(handed_over),
        resident_layout: None,
        handle_index: None,
        insert_hatches: None,
    });
    assert_eq!(
        *scene.draw_depth_map(),
        *expected,
        "the handed-over map must be served as-is",
    );

    // A local rebuild must agree.
    *scene.draw_depth_cache.borrow_mut() = None;
    assert_eq!(
        *scene.draw_depth_map(),
        *expected,
        "a recompute must agree with what was handed over",
    );
}

#[test]
fn prepared_open_geometry_keeps_layout_for_the_first_edit_patch() {
    use document::entities::Line;
    use document::types::Vector3;

    fn line(y: f64) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(10.0, y, 0.0),
        ))
    }

    let mut document = Document::new();
    document.add_entity(line(0.0)).unwrap();
    let mut caches = build_derived_caches_impl(&document, None, None);
    let model_bg = [0.0, 0.0, 0.0, 1.0];
    let (document, prepared) = prepare_open_geometry(document, &mut caches, model_bg);
    assert!(prepared.resident_layout.is_some());
    assert!(prepared.entity_index.is_some());

    let mut scene = Scene::new();
    scene.document = document;
    scene.local_extent_max = caches.local_extent_max;
    scene.local_center = caches.local_center;
    scene.bg_color = model_bg;
    scene.current_layout = "Model".to_string();
    scene.sync_annotation_scale_from_header();
    scene.install_prepared_open_geometry(prepared);
    assert_eq!(
        scene
            .entity_index_cache
            .borrow()
            .as_ref()
            .map(|(epoch, _)| *epoch),
        Some(scene.geometry_epoch),
        "the loader's entity index must be installed under the receiving scene epoch",
    );

    let hits = scene.resident_patch_hits.get();
    scene.add_entity(line(1.0));
    let wires = scene.model_tile_wires_arc(0, &Camera::default(), 1.0, 1.0);
    drop(wires);

    assert_eq!(scene.resident_patch_hits.get(), hits + 1);
}

#[test]
fn a_second_resident_set_still_patches_after_the_first_one_does() {
    use document::entities::Line;
    use document::types::Vector3;

    fn line(y: f64) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(10.0, y, 0.0),
        ))
    }

    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    let block = scene.model_space_block_handle();
    // Two resident keys over the same wires, as a paper layout's content
    // viewports have: `None` and an empty frozen-layer set hash apart.
    let unfrozen: HashSet<Handle> = HashSet::default();
    drop(scene.resident_wires_for(block, None, None, None, None));
    drop(scene.resident_wires_for(block, None, None, Some(&unfrozen), None));
    assert_eq!(
        scene.resident_wire_sets.borrow().len(),
        2,
        "the two keys must hold separate sets, or this proves nothing"
    );

    let hits = scene.resident_patch_hits.get();
    scene.add_entity(line(1.0));
    // The first set patches forward, and must not cost the second its base.
    drop(scene.resident_wires_for(block, None, None, None, None));
    drop(scene.resident_wires_for(block, None, None, Some(&unfrozen), None));

    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 2,
        "both sets must patch: pruning stale entries by epoch alone drops \
             the set the next viewport was about to patch, and that viewport \
             then pays a full assembly plus an arena build on every edit"
    );
}

#[test]
fn entity_index_incremental_matches_full_rebuild() {
    use document::entities::Line;
    use document::types::Vector3;

    fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a.0, a.1, 0.0),
            Vector3::new(b.0, b.1, 0.0),
        ))
    }
    // Set of handles the index reports over the whole plane (bounded via the
    // quadtree + always-surfaced unbounded).
    fn indexed(s: &Scene) -> HashSet<Handle> {
        let idx = s.entity_index();
        let mut set: HashSet<Handle> = idx
            .tree
            .query_rect([-1e9, -1e9, 1e9, 1e9])
            .into_iter()
            .collect();
        set.extend(idx.unbounded_handles.iter().copied());
        set
    }
    fn full_rebuild(s: &Scene) -> HashSet<Handle> {
        *s.entity_index_cache.borrow_mut() = None;
        indexed(s)
    }

    let mut s = Scene::new();
    let h1 = s.add_entity(line((0.0, 0.0), (10.0, 10.0)));
    let h2 = s.add_entity(line((5.0, 5.0), (20.0, 3.0)));
    // Prime the index (full build), then each edit must keep it == rebuild.
    let _ = indexed(&s);

    // Add
    let h3 = s.add_entity(line((100.0, 100.0), (140.0, 90.0)));
    assert_eq!(indexed(&s), full_rebuild(&s), "after add");

    // Modify (move h1 far away)
    {
        let e = line((900.0, 900.0), (950.0, 970.0));
        let mut e = e;
        e.common_mut().handle = h1;
        s.update_entity(e);
    }
    assert_eq!(indexed(&s), full_rebuild(&s), "after modify");

    // Erase h2
    s.erase_entities(&[h2]);
    assert_eq!(indexed(&s), full_rebuild(&s), "after erase");
    assert!(indexed(&s).contains(&h3) && indexed(&s).contains(&h1));
    assert!(!indexed(&s).contains(&h2));
}

// Differential oracle for the resident-wire splice: the incrementally
// patched resident set must equal a from-scratch rebuild (same wires in the
// same draw order) after add / move / erase — and the patch path must run.
#[test]
fn resident_set_incremental_matches_full_rebuild() {
    use document::entities::Line;
    use document::types::Vector3;

    fn line(a: (f64, f64), b: (f64, f64)) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a.0, a.1, 0.0),
            Vector3::new(b.0, b.1, 0.0),
        ))
    }
    // Visible wire names, in order. Resident erase deliberately retains
    // blank tombstone slots, which are not part of a from-scratch build.
    fn resident_names(s: &Scene) -> Vec<String> {
        let cam = Camera::default();
        let arc = s.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        let names: Vec<String> = arc
            .iter()
            .filter(|wire| Scene::handle_from_wire_name(&wire.name).is_some())
            .map(|wire| wire.name.clone())
            .collect();
        drop(arc); // release so the next patch can move the wires out
        names
    }
    fn from_scratch(s: &Scene) -> Vec<String> {
        s.resident_wire_sets.borrow_mut().clear();
        resident_names(s)
    }

    let mut s = Scene::new();
    let _h1 = s.add_entity(line((0.0, 0.0), (10.0, 10.0)));
    let h2 = s.add_entity(line((5.0, 5.0), (20.0, 3.0)));
    let h2_image = s.document.get_entity_arc(h2).unwrap();
    let _ = resident_names(&s); // prime the resident cache

    let hits0 = s.resident_patch_hits.get();

    let h3 = s.add_entity(line((1.0, 1.0), (2.0, 2.0)));
    assert_eq!(resident_names(&s), from_scratch(&s), "resident after add");

    {
        let mut e = line((100.0, 100.0), (140.0, 90.0));
        e.common_mut().handle = h3;
        s.update_entity(e);
    }
    assert_eq!(
        resident_names(&s),
        from_scratch(&s),
        "resident after modify"
    );

    let original_h2_start = s
        .resident_wire_sets
        .borrow()
        .values()
        .find_map(|set| {
            set.layout
                .as_ref()
                .and_then(|layout| layout.ranges.get(&h2).map(|range| range.0))
        })
        .unwrap();
    let before_erase_names = resident_names(&s);
    s.erase_entities(&[h2]);
    let patched = resident_names(&s);
    let erase_gen = s.last_model_wire_gen.get();
    assert!(
        s.model_wire_patch_for(erase_gen)
            .is_some_and(|(_, patch)| !patch.face_pass_changed),
        "removing a plain line must keep the unrelated Face3D/fill pass warm"
    );
    assert!(
        s.model_wire_patch_for(erase_gen)
            .is_some_and(|(_, patch)| !patch.text_pass_changed),
        "removing a plain line must keep the unrelated text pass warm"
    );
    assert!(
        s.resident_wire_sets.borrow().values().any(|set| {
            set.layout
                .as_ref()
                .is_some_and(|layout| layout.vacant.contains_key(&h2))
        }),
        "erase should retain h2's physical range as a tombstone"
    );
    let expected_erased: Vec<_> = before_erase_names
        .iter()
        .filter(|name| *name != &h2.value().to_string())
        .cloned()
        .collect();
    assert_eq!(patched, expected_erased, "resident after erase");

    // Replay the same Added delta history Undo emits. The entity must return
    // to its original physical range instead of appending/shifting the set.
    let restored = s.apply_entity_delta(&[(h2, Some(Arc::clone(&h2_image)), None)], true);
    for &(handle, _) in &restored {
        s.reseed_derived_caches(handle);
    }
    s.bump_entities(&restored);
    let restored_names = resident_names(&s);
    assert_eq!(restored_names, before_erase_names);
    assert!(s.resident_wire_sets.borrow().values().any(|set| {
        set.layout.as_ref().is_some_and(|layout| {
            layout.ranges.get(&h2).map(|range| range.0) == Some(original_h2_start)
                && !layout.vacant.contains_key(&h2)
        })
    }));

    assert_eq!(
        s.resident_patch_hits.get(),
        hits0 + 4,
        "every add / modify / erase / restore edit must use the incremental \
             patch, not silently fall back to a full rebuild"
    );
}

/// Entities added one by one, to a drawing that started empty or held a
/// single entity, must get draw depths the depth buffer can tell apart, or
/// draw order, masks and wipeouts have no effect among them.
#[test]
fn added_entities_get_separable_draw_depths() {
    use document::entities::Line;
    use document::types::Vector3;

    fn add(scene: &mut Scene, y: f64) -> Handle {
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(10.0, y, 0.0),
        )))
    }
    // `apply_draw_order` moves geometry by up to 0.02 of clip w per unit
    // of depth; a 24-bit depth buffer resolves about 6e-8.
    let separable = |scene: &Scene, below: Handle, above: Handle| {
        let depths = scene.draw_depth_map();
        let gap = depths[&above.value()][0] - depths[&below.value()][0];
        assert!(gap * 0.02 > 1.0e-6, "draw depths only {gap} apart");
    };

    // A new drawing is drawn while still empty, then grows by Add.
    let mut empty = Scene::new();
    let _ = empty.draw_depth_map();
    let first = add(&mut empty, 0.0);
    let _ = empty.draw_depth_map();
    let second = add(&mut empty, 1.0);
    separable(&empty, first, second);
    let third = add(&mut empty, 2.0);
    separable(&empty, second, third);

    // A drawing loaded with one entity, then grown by Add.
    let mut single = Scene::new();
    let only = add(&mut single, 0.0);
    let _ = single.draw_depth_map();
    let next = add(&mut single, 1.0);
    separable(&single, only, next);
}

#[test]
fn draw_depth_add_and_erase_keep_existing_labels_stable() {
    use document::entities::Line;
    use document::types::Vector3;

    let line = |y: f64| {
        EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(10.0, y, 0.0),
        ))
    };
    let mut s = Scene::new();
    let h1 = s.add_entity(line(1.0));
    let h2 = s.add_entity(line(2.0));
    let initial = s.draw_depth_map();
    let d1 = initial[&h1.value()];
    let d2 = initial[&h2.value()];
    drop(initial);

    let h3 = s.add_entity(line(3.0));
    let after_add = s.draw_depth_map();
    assert_eq!(after_add[&h1.value()], d1);
    assert_eq!(after_add[&h2.value()], d2);
    let d3 = after_add[&h3.value()];
    drop(after_add);

    s.erase_entities(&[h2]);
    let after_erase = s.draw_depth_map();
    assert_eq!(after_erase[&h1.value()], d1);
    assert_eq!(after_erase[&h3.value()], d3);
    assert!(!after_erase.contains_key(&h2.value()));
}

#[test]
fn category_gate_keeps_unrelated_caches_warm() {
    let mut s = Scene::new();
    let e0 = s.geometry_epoch;
    // Editing handles NOT in the (empty) hatch category leaves it valid.
    s.bump_entities(&[(h(40), ChangeKind::Modified)]);
    assert!(
        s.category_cache_valid(e0, CACHE_CATEGORY_HATCH, |hh| {
            s.hatches.contains_key(&hh)
        }),
        "an edit outside the category must keep it warm"
    );
    // A caller that bypasses the tracked erase primitive supplies no
    // before-category hint, so removal remains conservatively invalid.
    s.bump_entities(&[(h(41), ChangeKind::Removed)]);
    assert!(
        !s.category_cache_valid(e0, CACHE_CATEGORY_HATCH, |hh| {
            s.hatches.contains_key(&hh)
        }),
        "an unclassified removal must invalidate"
    );
}

#[test]
fn plain_geometry_keeps_insert_hatch_cache_warm() {
    use document::entities::{Insert, Line};
    use document::types::Vector3;

    let mut s = Scene::new();
    let initial = s.insert_hatches_for_click();
    s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    let after_line = s.insert_hatches_for_click();
    assert!(
        Arc::ptr_eq(&initial, &after_line),
        "a line edit must not rescan and explode every block insert"
    );

    let insert = s.add_entity(EntityType::Insert(Insert::new(
        "EXISTING_BLOCK",
        Vector3::new(0.0, 0.0, 0.0),
    )));
    let after_insert = s.insert_hatches_for_click();
    assert!(!Arc::ptr_eq(&after_line, &after_insert));

    s.erase_entities(&[insert]);
    let after_erase = s.insert_hatches_for_click();
    assert!(!Arc::ptr_eq(&after_insert, &after_erase));
}

#[test]
fn added_insert_updates_insert_hatches_without_rebuilding_the_others() {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, Insert, PolylineEdge};
    use document::types::{Vector2, Vector3};

    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(path);
    let mut s = Scene::new();
    s.define_block_raw(
        "FILLED",
        Vector3::new(0.0, 0.0, 0.0),
        vec![EntityType::Hatch(Box::new(hatch))],
    );
    let first = s.add_entity(EntityType::Insert(Insert::new(
        "FILLED",
        Vector3::new(0.0, 0.0, 0.0),
    )));
    let before = s.insert_hatches_for_click();
    assert_eq!(before.get(&first).map(Vec::len), Some(1));

    let second = s.add_entity(EntityType::Insert(Insert::new(
        "FILLED",
        Vector3::new(100.0, 0.0, 0.0),
    )));
    let after = s.insert_hatches_for_click();
    let placed = after
        .get(&second)
        .expect("the new insert's hatch is pickable");
    assert_eq!(placed.len(), 1);
    assert!(
        Arc::ptr_eq(&before[&first][0].boundary, &after[&first][0].boundary),
        "adding one insert must not re-explode every other insert"
    );

    s.erase_entities(&[second]);
    let after_erase = s.insert_hatches_for_click();
    assert!(!after_erase.contains_key(&second));
    assert!(after_erase.contains_key(&first));
}

#[test]
fn patched_mesh_models_match_a_full_rebuild() {
    use document::entities::{Insert, PolyfaceFace, PolyfaceMesh, PolyfaceVertex};
    use document::types::Vector3;

    let mut mesh = PolyfaceMesh::new();
    for (x, y) in [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)] {
        mesh.add_vertex(PolyfaceVertex::new(Vector3::new(x, y, 0.0)));
    }
    mesh.add_face(PolyfaceFace::quad(1, 2, 3, 4));
    let mut s = Scene::new();
    s.define_block_from_owned_entities(
        vec![EntityType::PolyfaceMesh(mesh)],
        "MESHED",
        glam::DVec3::ZERO,
    )
    .unwrap();
    let at = |x: f64| EntityType::Insert(Insert::new("MESHED", Vector3::new(x, 0.0, 0.0)));
    let first = s.add_entity(at(0.0));
    let second = s.add_entity(at(10.0));
    let before = s.meshes_arc();
    assert_eq!(before.len(), 2, "one mesh placement per insert");
    // Mark the first insert's cached placement; a rebuild would lose the mark.
    {
        let mut cache = s.mesh_cache.borrow_mut();
        let (_, list) = cache.values_mut().next().expect("a cached list");
        let mut marked: Vec<MeshLodSet> = list.as_ref().clone();
        for set in marked
            .iter_mut()
            .filter(|set| set.instance_handle == Some(first))
        {
            set.edge_color = Some([9.0, 9.0, 9.0, 9.0]);
        }
        *list = Arc::new(marked);
    }

    // One insert leaves, one arrives: the list is patched, not rebuilt.
    s.erase_entities(&[second]);
    let third = s.add_entity(at(20.0));
    let patched = s.meshes_arc();
    assert_eq!(patched.len(), 2);
    assert!(patched.iter().any(|set| set.instance_handle == Some(first)));
    assert!(patched.iter().any(|set| set.instance_handle == Some(third)));
    assert!(!patched
        .iter()
        .any(|set| set.instance_handle == Some(second)));
    assert!(
        patched
            .iter()
            .any(|set| set.instance_handle == Some(first) && set.edge_color == Some([9.0; 4])),
        "the untouched insert's placement is carried over, not rebuilt"
    );

    // Reference: the same scene rebuilt from nothing.
    s.mesh_cache.borrow_mut().clear();
    let full = s.meshes_arc();
    let signature = |sets: &[MeshLodSet]| {
        let mut sig: Vec<_> = sets
            .iter()
            .map(|set| {
                (
                    set.instance_handle.map(|handle| handle.value()),
                    set.instance_aabb.map(|aabb| aabb.map(f64::to_bits)),
                    set.lods.len(),
                )
            })
            .collect();
        sig.sort();
        sig
    };
    assert_eq!(signature(&patched), signature(&full));
}

#[test]
fn patched_hatch_models_match_a_full_rebuild() {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, Insert, PolylineEdge};
    use document::types::{Vector2, Vector3};

    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(path);
    let mut s = Scene::new();
    s.define_block_raw(
        "FILLED",
        Vector3::new(0.0, 0.0, 0.0),
        vec![EntityType::Hatch(Box::new(hatch))],
    );
    let at = |x: f64| EntityType::Insert(Insert::new("FILLED", Vector3::new(x, 0.0, 0.0)));
    let first = s.add_entity(at(0.0));
    let second = s.add_entity(at(10.0));
    let before = s.hatch_models_arc();
    assert_eq!(before.len(), 2);

    // One insert leaves, one arrives: the list is patched, not rebuilt.
    s.erase_entities(&[second]);
    let third = s.add_entity(at(20.0));
    let patched = s.hatch_models_arc();
    assert_eq!(patched.len(), 2);
    let first_model = before
        .iter()
        .find(|m| m.render_instance.is_some_and(|i| i.translation[0] == 0.0))
        .expect("the first insert's fill");
    let kept = patched
        .iter()
        .find(|m| m.render_instance.is_some_and(|i| i.translation[0] == 0.0))
        .expect("the first insert's fill survives the patch");
    assert!(
        Arc::ptr_eq(&first_model.boundary, &kept.boundary),
        "an untouched insert's fill must be carried over, not rebuilt"
    );

    // Reference: the same scene rebuilt from nothing.
    s.hatch_cache.borrow_mut().clear();
    s.hatch_hosts.borrow_mut().clear();
    let full = s.hatch_models_arc();

    let signature = |models: &[HatchModel]| {
        let mut sig: Vec<_> = models
            .iter()
            .map(|m| {
                (
                    m.world_origin.map(f64::to_bits),
                    m.render_instance.map(|i| i.translation.map(f64::to_bits)),
                    m.boundary
                        .iter()
                        .flat_map(|p| p.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        sig.sort();
        sig
    };
    assert_eq!(signature(&patched), signature(&full));
    let _ = (first, third);
}

#[test]
fn selecting_plain_geometry_keeps_hatch_models_warm() {
    use document::entities::Line;
    use document::types::Vector3;

    let mut s = Scene::new();
    let line = s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    let before = s.hatch_models_arc();
    s.select_entity(line, false);
    let after = s.hatch_models_arc();
    assert!(
        Arc::ptr_eq(&before, &after),
        "selecting a non-hatch must not rebuild every hatch model"
    );
}

#[test]
fn plain_line_erase_keeps_unrelated_category_and_text_caches_warm() {
    use document::entities::Line;
    use document::types::Vector3;

    let mut s = Scene::new();
    let line = s.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    let cached_epoch = s.geometry_epoch;
    s.erase_entities(&[line]);

    assert!(
        s.category_cache_valid(cached_epoch, CACHE_CATEGORY_HATCH, |h| {
            s.hatches.contains_key(&h)
        })
    );
    assert!(
        s.category_cache_valid(cached_epoch, CACHE_CATEGORY_IMAGE, |h| {
            s.images.contains_key(&h)
        })
    );
    assert!(
        s.category_cache_valid(cached_epoch, CACHE_CATEGORY_MESH, |h| {
            s.meshes.contains_key(&h)
        })
    );
    assert!(s.text_unchanged(cached_epoch));
}

/// A raster image whose file cannot be resolved draws the saved path as a
/// glyph run instead of the picture, so erasing it MUST invalidate the SDF
/// text buffer — otherwise `gather_text_verts` reuses the previous vertex
/// list and the path text keeps drawing over the now-empty frame.
#[test]
fn erasing_broken_raster_reference_invalidates_text_cache() {
    let mut s = Scene::new();
    let image = s.add_entity(EntityType::RasterImage(broken_raster("erase")));
    let cached_epoch = s.geometry_epoch;
    s.erase_entities(&[image]);

    assert!(
        !s.text_unchanged(cached_epoch),
        "erasing a broken raster reference must drop its placeholder glyphs"
    );
}

/// The same classification in the other direction: attaching (and, through
/// the identical Added/Modified path, moving or undoing the erase of) a
/// broken reference must invalidate the text buffer so its path text appears.
#[test]
fn attaching_broken_raster_reference_invalidates_text_cache() {
    let mut s = Scene::new();
    let cached_epoch = s.geometry_epoch;
    s.add_entity(EntityType::RasterImage(broken_raster("attach")));

    assert!(
        !s.text_unchanged(cached_epoch),
        "attaching a broken raster reference must build its placeholder glyphs"
    );
}

/// The converse perf property: a raster that resolves contributes no glyphs,
/// so editing or erasing it must leave the text buffer warm — a grip-drag of
/// a working image cannot re-walk every glyph in the drawing per mouse move.
/// An empty path counts as resolvable to both the converter and the
/// classifier, so this needs no picture on disk.
#[test]
fn erasing_resolvable_raster_keeps_text_cache_warm() {
    use document::entities::RasterImage;
    use document::types::Vector3;

    let mut s = Scene::new();
    let image = s.add_entity(EntityType::RasterImage(RasterImage::with_size(
        "",
        Vector3::new(0.0, 0.0, 0.0),
        16.0,
        16.0,
        10.0,
        10.0,
    )));
    let cached_epoch = s.geometry_epoch;
    s.erase_entities(&[image]);

    assert!(s.text_unchanged(cached_epoch));
}

/// End-to-end on the buffer the GPU actually draws: the placeholder's path
/// glyphs must be gone from the gathered SDF vertices after the erase. This
/// is the shape of the reported bug — `gather_text_verts` misses its content-
/// id cache on every geometry edit and then reuses the previous vertex list
/// whenever `text_unchanged` says the text is untouched.
#[test]
fn erasing_broken_raster_reference_drops_its_glyphs_from_the_text_buffer() {
    let depth_map = rustc_hash::FxHashMap::default();
    let mut s = Scene::new();
    let image = s.add_entity(EntityType::RasterImage(broken_raster("gather")));

    let wires = s.entity_wires();
    let before = s.gather_text_verts(&wires, 1, 7, &depth_map);
    assert!(
        !before.is_empty(),
        "a broken raster reference must contribute its path glyphs"
    );

    s.erase_entities(&[image]);
    let wires = s.entity_wires();
    let after = s.gather_text_verts(&wires, 2, 7, &depth_map);
    assert!(
        after.is_empty(),
        "the erased placeholder left {} glyph vertices on screen",
        after.len()
    );
}

/// A raster pointing at a path that cannot exist. `resolve_image` memoises
/// per path for the life of the process, so each test uses its own path and
/// cannot inherit another's cached answer.
fn broken_raster(tag: &str) -> document::entities::RasterImage {
    use document::entities::RasterImage;
    use document::types::Vector3;

    RasterImage::with_size(
        &format!("/nonexistent-ocs-test/{tag}/missing-reference.png"),
        Vector3::new(0.0, 0.0, 0.0),
        16.0,
        16.0,
        10.0,
        10.0,
    )
}
