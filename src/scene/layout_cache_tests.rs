use super::*;

#[test]
fn model_space_block_handle_memo_follows_the_document_structure() {
    let mut s = Scene::new();
    let fallback = s
        .document
        .block_records
        .get("*Model_Space")
        .expect("Document::new creates the *Model_Space block record")
        .handle;
    let model_layout = s
        .document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Layout(layout) if layout.name == "Model" => Some(*handle),
            _ => None,
        })
        .expect("Document::new creates a Model layout");
    assert_eq!(s.model_space_block_handle(), fallback);

    // Repoint the Model layout without moving an epoch: the memo holds.
    let repointed = s.document.header.paper_space_block_handle;
    if let Some(ObjectType::Layout(layout)) = s.document.objects.get_mut(&model_layout) {
        layout.block_record = repointed;
    }
    assert_eq!(
        s.model_space_block_handle(),
        fallback,
        "the memo deliberately holds until an epoch moves"
    );

    // An object insert changes the count, so the memo re-resolves.
    let extra = s.document.allocate_handle();
    s.document.objects.insert(
        extra,
        ObjectType::XRecord(document::objects::XRecord::new()),
    );
    assert_eq!(s.model_space_block_handle(), repointed);

    // Layout edits move the layout epoch, so the new target stays seen.
    if let Some(ObjectType::Layout(layout)) = s.document.objects.get_mut(&model_layout) {
        layout.block_record = fallback;
    }
    assert_eq!(s.model_space_block_handle(), repointed);
    s.bump_layout_epoch();
    assert_eq!(s.model_space_block_handle(), fallback);

    // A coarse structural bump re-resolves too and keeps the answer.
    s.bump_geometry();
    assert_eq!(s.model_space_block_handle(), fallback);
}

#[test]
fn a_restricted_build_matches_the_full_build_for_its_handles() {
    use document::entities::Line;
    use document::tables::layer::Layer;
    use document::types::Vector3;

    let mut s = Scene::new();
    let mut hidden = Layer::new("Walls");
    hidden.flags.off = true;
    s.document.layers.add_or_replace(hidden);
    let mut line_at = |y: f64, layer: &str| {
        let mut line = Line::from_points(Vector3::new(0.0, y, 0.0), Vector3::new(10.0, y, 0.0));
        line.common.layer = layer.to_string();
        s.add_entity(EntityType::Line(line))
    };
    let shown = line_at(0.0, "0");
    let off = line_at(100.0, "Walls");
    let unselected = line_at(50.0, "0");
    let only: HashSet<Handle> = [shown, off].into_iter().collect();
    let build = |s: &Scene, only: Option<&HashSet<Handle>>| {
        let mut wires: Vec<(String, Vec<[f32; 3]>)> = s
            .wires_for_block_culled(
                s.model_space_block_handle(),
                None,
                None,
                None,
                None,
                None,
                true,
                None,
                only,
            )
            .into_iter()
            .map(|wire| (wire.name, std::sync::Arc::unwrap_or_clone(wire.geom).points))
            .collect();
        wires.sort_by(|a, b| a.0.cmp(&b.0));
        wires
    };

    let restricted = build(&s, Some(&only));
    let full: Vec<_> = build(&s, None)
        .into_iter()
        .filter(|(name, _)| *name == shown.value().to_string())
        .collect();
    assert!(!full.is_empty());
    assert_eq!(
        restricted, full,
        "the hidden handle is dropped, the shown one is unchanged"
    );
    assert!(restricted
        .iter()
        .all(|(name, _)| *name != unselected.value().to_string()));

    s.selected = only;
    s.selection_generation += 1;
    let pivot = s.orbit_pivot().expect("the shown line is selected");
    assert!((pivot - glam::DVec3::new(5.0, 0.0, 0.0)).length() < 1e-9);
}

#[test]
fn memoized_plain_line_matches_fresh_background() {
    let mut s = Scene::new();
    s.bg_color = [0.1, 0.1, 0.1, 1.0];
    let handle = s.add_entity(EntityType::Line(document::entities::Line::from_points(
        document::types::Vector3::new(0.0, 0.0, 0.0),
        document::types::Vector3::new(10.0, 0.0, 0.0),
    )));
    let build = |s: &Scene| {
        s.wires_for_block_culled(
            s.model_space_block_handle(),
            None,
            None,
            None,
            None,
            None,
            true,
            None,
            None,
        )
        .into_iter()
        .find(|wire| wire.name == handle.value().to_string())
        .unwrap()
    };
    let original = build(&s);
    assert!(s.resident_tess_memo.borrow().contains_key(&handle));
    s.paper_bg_color = [1.0; 4];
    s.document.add_layout("Review").unwrap();
    s.set_current_layout("Review".to_string());
    let cached = build(&s);
    s.resident_tess_memo.borrow_mut().clear();
    let fresh = build(&s);
    assert_ne!(
        original.color, fresh.color,
        "fixture must depend on background"
    );
    assert_eq!(
        cached.color, fresh.color,
        "memo hit should match fresh tessellation on a white background"
    );
}

#[test]
fn model_extents_reuse_resident_geometry_in_every_layout() {
    let mut s = Scene::new();
    s.add_entity(EntityType::Line(document::entities::Line::from_points(
        document::types::Vector3::new(1_000_000.0, 2_000_000.0, 30.0),
        document::types::Vector3::new(1_000_010.0, 2_000_020.0, 40.0),
    )));
    s.document.add_layout("Review").unwrap();
    for layout in ["Model", "Review"] {
        s.set_current_layout(layout.to_string());
        *s.model_extents_cache.borrow_mut() = None;
        let scale = crate::scene::annotative::scale_handle_by_name(
            &s.document,
            &s.document.header.current_annotation_scale,
        );
        let source = s.resident_wires_for(
            s.model_space_block_handle(),
            Some(s.annotation_scale),
            scale,
            None,
            None,
        );
        let generation = s.last_model_wire_gen.get();
        let bounds = s.model_space_extents().unwrap();
        assert_eq!(bounds.0, glam::Vec3::new(1_000_000.0, 2_000_000.0, 30.0));
        assert_eq!(bounds.1, glam::Vec3::new(1_000_010.0, 2_000_020.0, 40.0));
        assert_eq!(
            s.last_model_wire_gen.get(),
            generation,
            "fitting must reuse the resident source"
        );
        assert!(!source.is_empty());
    }
}

#[test]
fn model_space_extents_mixed_entities_accuracy() {
    use document::types::{Vector2, Vector3};
    let mut s = Scene::new();
    // Line from (0, 0, 10) to (50, 100, 20)
    let mut line = document::entities::Line::new();
    line.start = Vector3::new(0.0, 0.0, 10.0);
    line.end = Vector3::new(50.0, 100.0, 20.0);
    s.add_entity(EntityType::Line(line));

    // Circle at (200, 200, 0) with radius 50
    let mut circle = document::entities::Circle::new();
    circle.center = Vector3::new(200.0, 200.0, 0.0);
    circle.radius = 50.0;
    s.add_entity(EntityType::Circle(circle));

    // Arc at (-100, -50, -5) with radius 25, angles 0 to PI
    let mut arc = document::entities::Arc::new();
    arc.center = Vector3::new(-100.0, -50.0, -5.0);
    arc.radius = 25.0;
    arc.start_angle = 0.0;
    arc.end_angle = std::f64::consts::PI;
    s.add_entity(EntityType::Arc(arc));

    // LwPolyline from (300, -200) to (400, -100)
    let mut pl = document::entities::LwPolyline::new();
    pl.vertices = vec![
        document::entities::LwVertex::new(Vector2::new(300.0, -200.0)),
        document::entities::LwVertex::new(Vector2::new(400.0, -100.0)),
    ];
    s.add_entity(EntityType::LwPolyline(pl));

    let bounds = s.model_space_extents().expect("Extents must exist");
    // Every entity counts, curves included. The circle and the arc used to
    // contribute nothing at all: the walk read only `key_vertices`, which
    // is empty for anything tessellated, so the drawing looked 275 units
    // narrower and 150 shorter than it is.
    //   Line:       (0, 0, 10) .. (50, 100, 20)
    //   Circle:     (150, 150, 0) .. (250, 250, 0)      centre 200,200 r50
    //   Arc:        (-125, -50, -5) .. (-75, -25, -5)   centre -100,-50 r25, 0..PI
    //   LwPolyline: (300, -200, 0) .. (400, -100, 0)
    for (got, want, axis) in [
        (bounds.0.x, -125.0, "min.x"),
        (bounds.1.x, 400.0, "max.x"),
        (bounds.0.y, -200.0, "min.y"),
        (bounds.1.y, 250.0, "max.y"),
        (bounds.0.z, -5.0, "min.z"),
        (bounds.1.z, 20.0, "max.z"),
    ] {
        assert!(
            (got - want).abs() < 1e-3,
            "{axis} mismatch: got {got}, want {want}"
        );
    }
}

#[test]
fn resident_wires_are_zoom_independent_and_gpu_analytical() {
    let mut s = Scene::new();
    let mut circle = document::entities::Circle::default();
    circle.radius = 100.0;
    let handle = s.add_entity(EntityType::Circle(circle));

    // Far camera: distance is large
    let mut cam_far = Camera::default();
    cam_far.distance = 1000.0;

    // Close camera: distance is small
    let mut cam_close = Camera::default();
    cam_close.distance = 1.0;

    let wires_far = s.model_tile_wires_arc(0, &cam_far, 1.0, 1000.0);
    let circle_wire_far = wires_far
        .iter()
        .find(|w| w.name == handle.value().to_string())
        .unwrap();

    let wires_close = s.model_tile_wires_arc(0, &cam_close, 1.0, 1000.0);

    // Zooming must reuse the exact same resident wire set (zero re-tessellation)
    assert!(
        Arc::ptr_eq(&wires_far, &wires_close),
        "Resident wires must be identical Arc across zoom levels"
    );
    // Circle must carry TangentGeom for GPU analytical rendering
    assert_eq!(circle_wire_far.tangent_geoms.len(), 1);
    assert!(matches!(
        circle_wire_far.tangent_geoms[0],
        crate::scene::model::wire_model::TangentGeom::PlanarCircle { .. }
            | crate::scene::model::wire_model::TangentGeom::Circle { .. }
    ));
}

#[test]
fn block_circles_and_arcs_extract_as_analytical_gpu_instances() {
    let mut s = Scene::new();
    // Create initial entities
    let mut circle = document::entities::Circle::default();
    circle.radius = 50.0;
    let c_h = s.add_entity(EntityType::Circle(circle));

    let mut arc = document::entities::Arc::default();
    arc.radius = 25.0;
    arc.start_angle = 0.0;
    arc.end_angle = std::f64::consts::PI;
    let a_h = s.add_entity(EntityType::Arc(arc));

    let line = document::entities::Line::from_points(
        document::types::Vector3::new(0.0, 0.0, 0.0),
        document::types::Vector3::new(100.0, 100.0, 0.0),
    );
    let l_h = s.add_entity(EntityType::Line(line));

    // Create block and place insert at (200, 300, 0)
    let xform = document::types::Transform::from_translation(document::types::Vector3::new(
        200.0, 300.0, 0.0,
    ));
    let _ = s
        .create_block_from_entities(
            &[c_h, a_h, l_h],
            "TEST_ANALYTICAL_BLOCK",
            &document::types::Transform::identity(),
            &xform,
        )
        .unwrap();

    let wires = s.model_tile_wires_arc(0, &Camera::default(), 1.0, 1000.0);
    let depths = rustc_hash::FxHashMap::default();
    let partitioned = crate::scene::pipeline::wire_arena::partition_wires(&wires, &depths);

    // Both the circle and the arc inside the block must be extracted as analytical GPU circle instances!
    assert!(
        partitioned.circle_instances.len() >= 2,
        "Expected at least 2 analytical circle instances from block, got {}",
        partitioned.circle_instances.len()
    );
}

#[test]
fn single_bulge_polyline_extracts_as_analytical_gpu_arc() {
    let mut s = Scene::new();
    let mut pline = document::entities::LwPolyline::new();
    pline.vertices = vec![
        document::entities::LwVertex {
            location: document::types::Vector2::new(0.0, 0.0),
            bulge: 1.0, // semicircle
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        },
        document::entities::LwVertex {
            location: document::types::Vector2::new(100.0, 0.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        },
    ];
    let _ = s.add_entity(EntityType::LwPolyline(pline));

    let wires = s.model_tile_wires_arc(0, &Camera::default(), 1.0, 1000.0);
    let depths = rustc_hash::FxHashMap::default();
    let partitioned = crate::scene::pipeline::wire_arena::partition_wires(&wires, &depths);

    assert_eq!(
        partitioned.circle_instances.len(),
        1,
        "Single bulge arc polyline must be extracted as analytical GPU arc instance"
    );
}

#[test]
fn multi_bulge_polyline_extracts_as_analytical_gpu_arcs() {
    let mut s = Scene::new();
    let mut pline = document::entities::LwPolyline::new();
    // A circle represented as a closed 2-vertex polyline with two semicircle bulges (standard CAD polyline circle)
    pline.is_closed = true;
    pline.vertices = vec![
        document::entities::LwVertex {
            location: document::types::Vector2::new(0.0, 0.0),
            bulge: 1.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        },
        document::entities::LwVertex {
            location: document::types::Vector2::new(100.0, 0.0),
            bulge: 1.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 1,
        },
    ];
    let _ = s.add_entity(EntityType::LwPolyline(pline));

    let wires = s.model_tile_wires_arc(0, &Camera::default(), 1.0, 1000.0);
    let depths = rustc_hash::FxHashMap::default();
    let partitioned = crate::scene::pipeline::wire_arena::partition_wires(&wires, &depths);

    assert_eq!(
        partitioned.circle_instances.len(),
        2,
        "Multi-bulge closed polyline circle must be extracted as 2 analytical GPU arc instances"
    );
    assert_eq!(
        partitioned.regular.len(),
        0,
        "Multi-bulge pure arc polyline must not be sent to regular line arena"
    );
}

#[test]
fn mixed_bulge_polyline_splits_into_analytical_arcs_and_straight_lines() {
    use document::entities::{LwPolyline, LwVertex};
    use document::types::Vector2;

    let mut scene = Scene::new();
    let mut pline = LwPolyline::new();
    // Rounded slot: 2 straight lines and 2 semicircular ends (bulge = 1.0)
    // Vertex 0: (0, 0), bulge 1.0 (arc from 0,0 to 0,10)
    // Vertex 1: (0, 10), bulge 0.0 (straight from 0,10 to 50,10)
    // Vertex 2: (50, 10), bulge 1.0 (arc from 50,10 to 50,0)
    // Vertex 3: (50, 0), bulge 0.0 (straight from 50,0 to 0,0)
    let v0 = LwVertex::with_bulge(Vector2::new(0.0, 0.0), 1.0);
    let v1 = LwVertex::new(Vector2::new(0.0, 10.0));
    let v2 = LwVertex::with_bulge(Vector2::new(50.0, 10.0), 1.0);
    let v3 = LwVertex::new(Vector2::new(50.0, 0.0));
    pline.vertices = vec![v0, v1, v2, v3];
    pline.is_closed = true;

    let _ = scene.add_entity(EntityType::LwPolyline(pline));

    let camera = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &camera, 1.0, 1000.0);
    let depths = rustc_hash::FxHashMap::default();
    let partitioned = crate::scene::pipeline::wire_arena::partition_wires(&wires, &depths);

    assert_eq!(
        partitioned.circle_instances.len(),
        2,
        "Mixed polyline should have its 2 bulge arcs routed to analytical CircleGpu"
    );
}

#[test]
fn cached_layout_names_returns_same_arc_until_bump() {
    let scene = Scene::new();
    let first = scene.cached_layout_names();
    let second = scene.cached_layout_names();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    // Fresh `Document::new()` ships a default "Layout1" paper layout.
    assert_eq!(&first[..], &["Model".to_string(), "Layout1".to_string()]);
}

#[test]
fn cached_layout_names_invalidated_by_add_layout() {
    let mut scene = Scene::new();
    let before = scene.cached_layout_names();
    scene.add_layout("EXTRA").unwrap();
    let after = scene.cached_layout_names();
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "add_layout must bump the epoch so the cached Arc is replaced"
    );
    assert!(after.iter().any(|n| n == "EXTRA"));
}

#[test]
fn cached_scale_picker_returns_same_arc_until_bump() {
    let scene = Scene::new();
    let first = scene.cached_scale_picker_list();
    let second = scene.cached_scale_picker_list();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
}

#[test]
fn cached_scale_picker_invalidated_by_add_scale() {
    let mut scene = Scene::new();
    let before = scene.cached_scale_picker_list();
    assert!(scene.add_scale("TEST_CACHED_SCALE", 1.0, 100.0));
    let after = scene.cached_scale_picker_list();
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "add_scale must bump the epoch so the cached Arc is replaced"
    );
    assert!(after.iter().any(|(n, _, _)| n == "TEST_CACHED_SCALE"));
}

#[test]
fn cached_scale_picker_invalidated_by_current_scale_change() {
    let mut scene = Scene::new();
    // Force the metric family so the architectural entry is filtered out
    // unless it is the kept-active current scale.
    scene.document.header.insertion_units = 4;
    assert!(scene.add_scale("1:50", 1.0, 50.0));
    assert!(scene.add_scale("1/2\" = 1'-0\"", 0.5, 12.0));
    // Current = opposite-family entry with no metric equivalent factor, so
    // the keep-active path appends it to the visible list.
    assert!(scene.set_annotation_scale_named("1/2\" = 1'-0\"").is_some());
    let before = scene.cached_scale_picker_list();
    assert!(
        before.iter().any(|(n, _, _)| n == "1/2\" = 1'-0\""),
        "warm cache must contain the active opposite-family entry"
    );
    // Production setter: changes `current_annotation_scale` without
    // bumping `scale_epoch` — the old (epoch, factor) key stayed hit here.
    assert!(scene.set_annotation_scale_named("1:50").is_some());
    let after = scene.cached_scale_picker_list();
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "current-scale change must miss the cache (new Arc)"
    );
    assert!(
        !after.iter().any(|(n, _, _)| n == "1/2\" = 1'-0\""),
        "after switching to 1:50 the architectural entry must drop out"
    );
    assert!(after.iter().any(|(n, _, _)| n == "1:50"));
}

#[test]
fn cached_ucs_names_returns_same_arc_until_bump() {
    // RED: `Scene::cached_ucs_names` does not exist yet (#34).
    let scene = Scene::new();
    let first = scene.cached_ucs_names();
    let second = scene.cached_ucs_names();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
}

#[test]
fn cached_ucs_names_invalidated_by_bump() {
    // RED: `Scene::bump_ucs_epoch` does not exist yet (#34).
    let mut scene = Scene::new();
    let before = scene.cached_ucs_names();
    scene
        .document
        .ucss
        .add_or_replace(document::tables::Ucs::new("TEST_CACHED_UCS"));
    scene.bump_ucs_epoch();
    let after = scene.cached_ucs_names();
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "ucs mutation + bump must replace the cached Arc"
    );
    assert!(after.iter().any(|n| n == "TEST_CACHED_UCS"));
}

// #48: with no active/selected viewport, the pill number, label and sync
// badge must all describe the SAME viewport (the first-content fallback
// the number already used) — not a mix of viewport number, paper label
// and hidden badge.
#[test]
fn scale_pill_sources_agree_without_targeted_viewport() {
    let mut scene = Scene::new();
    scene.add_layout("P1").unwrap();
    scene.current_layout = "P1".to_string();
    assert!(scene.add_scale("1:50", 1.0, 50.0));
    let mut vp = document::entities::Viewport::new();
    vp.width = 100.0;
    vp.height = 100.0;
    let handle = scene
        .document
        .add_entity_to_layout(document::EntityType::Viewport(Box::new(vp)), "P1")
        .unwrap();
    assert!(scene.set_viewport_scale_named_for(handle, "1:50").is_some());
    assert!(scene.active_viewport.is_none());
    let number = scene.first_viewport_scale();
    let name = scene.displayed_annotation_scale_name();
    let synced = scene.viewport_annotation_scale_synced();
    eprintln!("number={number:?} name={name:?} synced={synced:?}");
    assert!(
        number.is_some() && name == "1:50" && synced == Some(true),
        "pill trio must agree"
    );
}

/// D#2542: a metre drawing's own "1:100 m" scale (1000 paper = 100
/// drawing) sets the viewport to exactly that ratio; INSUNITS is not
/// applied a second time. The standard set the program brings in for a
/// metre drawing is stored already converted.
#[test]
fn a_metre_drawing_scale_sets_the_viewport_to_its_own_ratio() {
    let mut scene = Scene::new();
    scene.document.header.insertion_units = 6;
    scene.add_layout("P1").unwrap();
    scene.current_layout = "P1".to_string();
    assert!(scene.add_scale("1:1", 1.0, 1.0));
    assert!(scene.add_scale("1:100 m", 1000.0, 100.0));
    let mut vp = document::entities::Viewport::new();
    vp.width = 100.0;
    vp.height = 100.0;
    let handle = scene
        .document
        .add_entity_to_layout(document::EntityType::Viewport(Box::new(vp)), "P1")
        .unwrap();
    assert!(scene
        .set_viewport_scale_named_for(handle, "1:100 m")
        .is_some());
    let Some(document::EntityType::Viewport(vp)) = scene.document.get_entity(handle) else {
        panic!("viewport");
    };
    assert!((vp.custom_scale - 10.0).abs() < 1e-9, "{}", vp.custom_scale);
    assert!(scene
        .scale_list()
        .iter()
        .any(|(name, _, factor)| name == "1:100 m" && (factor - 10.0).abs() < 1e-9));

    let mut fresh = Scene::new();
    fresh.document.header.insertion_units = 6;
    assert!(fresh.ensure_real_scale_list());
    let (paper, drawing) = fresh.scale_paper_drawing("1:100").unwrap();
    assert!((paper / drawing - 10.0).abs() < 1e-9, "{paper} = {drawing}");
}

/// A physical 10 m line measures 100/50/20 mm at 1:100/1:200/1:500.
/// Legacy files keep bare ratios; newer files may store converted ratios.
/// Both must behave the same, including after the viewport is synchronized.
#[test]
fn standard_viewport_scales_use_physical_paper_units_without_double_conversion() {
    for (units, mm_per_model_unit) in [(4, 1.0), (5, 10.0), (6, 1000.0)] {
        for (name, denominator) in [("1:100", 100.0), ("1:200", 200.0), ("1:500", 500.0)] {
            for stored_converted in [false, true] {
                let mut scene = Scene::new();
                scene.document.header.insertion_units = units;
                scene.add_layout("P1").unwrap();
                scene.current_layout = "P1".to_string();

                let drawing_units = if stored_converted {
                    denominator / mm_per_model_unit
                } else {
                    denominator
                };
                assert!(scene.add_scale(name, 1.0, drawing_units));

                let mut vp = document::entities::Viewport::new();
                vp.width = 200.0;
                vp.height = 100.0;
                let viewport = scene
                    .document
                    .add_entity_to_layout(document::EntityType::Viewport(Box::new(vp)), "P1")
                    .unwrap();

                assert!(scene.set_viewport_scale_named_for(viewport, name).is_some());
                let expected_factor = mm_per_model_unit / denominator;
                let actual = scene.viewport_effective_scale(viewport).unwrap();
                assert!(
                        (actual - expected_factor).abs() < 1.0e-9,
                        "{name}: INSUNITS={units}, stored_converted={stored_converted}, got {actual}, expected {expected_factor}"
                    );
                let model_length_10m = 10000.0 / mm_per_model_unit;
                let expected_length_mm = 10000.0 / denominator;
                assert!((model_length_10m * actual - expected_length_mm).abs() < 1.0e-7);
                assert_eq!(scene.viewport_annotation_scale_synced(), Some(true));
                assert!(scene.scale_list().iter().any(|(label, _, factor)| {
                    label == name && (factor - expected_factor).abs() < 1.0e-9
                }));
                // Stored SCALE object remains untouched, while the picker
                // and viewport apply the physically correct effective ratio.
                let (paper, drawing) = scene.scale_ratio(name).unwrap();
                assert!((paper / drawing - 1.0 / drawing_units).abs() < 1.0e-9);
                // The annotation multiplier is the scale's own ratio,
                // whatever the units: DIMTXT and text heights are in
                // drawing units (#2354).
                assert!(scene.set_annotation_scale_named(name).is_some());
                assert!(((scene.annotation_scale as f64) - drawing_units).abs() < 1.0e-5);
            }
        }
    }
}

#[test]
fn virtual_standard_scale_in_metre_drawing_is_unit_aware() {
    let mut scene = Scene::new();
    scene.document.header.insertion_units = 6;
    // scale_ratio intentionally exposes literal stored/name ratios for
    // plot settings; scale_list is the unit-aware viewport picker.
    let (paper, drawing) = scene.scale_ratio("1:200").unwrap();
    assert!((paper / drawing - 0.005).abs() < 1.0e-9);
    let (_, _, fallback) = scene
        .scale_list()
        .into_iter()
        .find(|(name, _, _)| name == "1:200")
        .unwrap();
    assert!((fallback - 5.0).abs() < 1.0e-9);
    assert!(scene.ensure_real_scale_list());
    let (_, _, stored) = scene
        .scale_list()
        .into_iter()
        .find(|(name, _, _)| name == "1:200")
        .unwrap();
    assert!((stored - 5.0).abs() < 1.0e-9);
}

#[test]
fn custom_metre_scale_keeps_its_exact_stored_paper_to_drawing_ratio() {
    let mut scene = Scene::new();
    scene.document.header.insertion_units = 6;
    scene.add_layout("P1").unwrap();
    scene.current_layout = "P1".to_string();
    assert!(scene.add_scale("1:200 m", 1000.0, 200.0));
    let mut vp = document::entities::Viewport::new();
    vp.width = 200.0;
    vp.height = 100.0;
    let viewport = scene
        .document
        .add_entity_to_layout(document::EntityType::Viewport(Box::new(vp)), "P1")
        .unwrap();
    assert!(scene
        .set_viewport_scale_named_for(viewport, "1:200 m")
        .is_some());
    assert!((scene.viewport_effective_scale(viewport).unwrap() - 5.0).abs() < 1.0e-9);
}

/// #2626: with nothing selected, a layout's only viewport takes the scale
/// picked from the list; with two, neither changes until one is picked.
#[test]
fn a_picked_scale_reaches_the_layouts_only_viewport() {
    let mut scene = Scene::new();
    scene.document.header.insertion_units = 4; // The existing 0.02 assertion is in mm.
    scene.add_layout("P1").unwrap();
    scene.current_layout = "P1".to_string();
    assert!(scene.add_scale("1:50", 1.0, 50.0));
    let viewport = |scene: &mut Scene| {
        let mut vp = document::entities::Viewport::new();
        vp.width = 100.0;
        vp.height = 100.0;
        scene
            .document
            .add_entity_to_layout(document::EntityType::Viewport(Box::new(vp)), "P1")
            .unwrap()
    };
    let only = viewport(&mut scene);
    scene.bump_geometry();
    assert_eq!(scene.scale_target_viewport(), Some(only));
    assert!(scene.set_viewport_scale_named("1:50").is_some());
    let Some(document::EntityType::Viewport(vp)) = scene.document.get_entity(only) else {
        panic!("viewport");
    };
    assert!((vp.custom_scale - 0.02).abs() < 1e-9);

    viewport(&mut scene);
    scene.bump_geometry();
    assert_eq!(scene.scale_target_viewport(), None);
}

#[test]
fn scale_pill_model_layout_unchanged() {
    // Model layout has no viewports: number None, header label, no badge.
    let scene = Scene::new();
    assert!(scene.current_layout == "Model");
    assert!(scene.first_viewport_scale().is_none());
    assert_eq!(
        scene.displayed_annotation_scale_name(),
        scene.document.header.current_annotation_scale
    );
    assert!(scene.viewport_annotation_scale_synced().is_none());
}
