//! EXPLODE regression tests: golden per-arm outputs, geometry invariants,
//! and a differential test proving `apply_explode_replacements` matches the
//! legacy per-entity erase/apply loop (including leader-annotation coupling).

use codec::entities::{
    CenterMarkAssociation, CenterMarkSource, CenterMarkSourceKind, Dimension, DimensionLinear,
    Insert, Leader, Line, LwPolyline, LwVertex, MLine, Polyline, Polyline2D, Polyline3D,
    Vertex2D, Vertex3D,
};
use codec::tables::BlockRecord;
use codec::types::{Vector2, Vector3};
use codec::{CadDocument, EntityType, Handle};
use OpenCADStudio::modules::draw::modify::explode::{
    apply_explode_replacements, explode_batch, explode_entity, plan_explode,
};
use OpenCADStudio::scene::Scene;

const BLOCK_NAME: &str = "TEST_BLOCK";

/// Register a 6-line block the way baked blocks look: members in the
/// document, `entity_handles` filled, record in `block_records`.
fn add_test_block(doc: &mut CadDocument) {
    let mut member_handles = Vec::new();
    for k in 0..6 {
        let mut line = Line::new();
        line.start = Vector3::new(k as f64, 0.0, 0.0);
        line.end = Vector3::new(k as f64, 5.0, 0.0);
        member_handles.push(doc.add_entity(EntityType::Line(line)).unwrap());
    }
    let mut block = BlockRecord::new(BLOCK_NAME);
    block.handle = doc.allocate_handle();
    block.entity_handles = member_handles;
    doc.block_records.add(block).unwrap();
}

// ── Golden per-arm outputs ────────────────────────────────────────────────

#[test]
fn explode_lwpolyline_golden_line_then_arc() {
    let doc = CadDocument::new();
    let mut pl = LwPolyline::new();
    let mut mid = LwVertex::new(Vector2::new(10.0, 0.0));
    mid.bulge = 1.0; // second segment becomes an arc
    pl.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        mid,
        LwVertex::new(Vector2::new(20.0, 0.0)),
    ];
    let pieces = explode_entity(&EntityType::LwPolyline(pl), &doc);
    assert_eq!(pieces.len(), 2);
    assert!(matches!(pieces[0], EntityType::Line(_)));
    assert!(matches!(pieces[1], EntityType::Arc(_)));
}

#[test]
fn explode_polyline_family_golden_all_lines() {
    let doc = CadDocument::new();

    let mut p2 = Polyline2D::new();
    p2.vertices = vec![
        Vertex2D::new(Vector3::new(0.0, 0.0, 0.0)),
        Vertex2D::new(Vector3::new(5.0, 0.0, 0.0)),
        Vertex2D::new(Vector3::new(5.0, 5.0, 0.0)),
    ];
    let pieces = explode_entity(&EntityType::Polyline2D(p2), &doc);
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|p| matches!(p, EntityType::Line(_))));

    let mut p3 = Polyline3D::new();
    p3.vertices = vec![
        codec::entities::Vertex3DPolyline {
            position: Vector3::new(0.0, 0.0, 0.0),
            ..Default::default()
        },
        codec::entities::Vertex3DPolyline {
            position: Vector3::new(5.0, 0.0, 0.0),
            ..Default::default()
        },
        codec::entities::Vertex3DPolyline {
            position: Vector3::new(5.0, 5.0, 0.0),
            ..Default::default()
        },
    ];
    let pieces = explode_entity(&EntityType::Polyline3D(p3), &doc);
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|p| matches!(p, EntityType::Line(_))));

    let mut p = Polyline::new();
    p.vertices = vec![
        Vertex3D::new(Vector3::new(0.0, 0.0, 0.0)),
        Vertex3D::new(Vector3::new(5.0, 0.0, 0.0)),
        Vertex3D::new(Vector3::new(5.0, 5.0, 0.0)),
    ];
    let pieces = explode_entity(&EntityType::Polyline(p), &doc);
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|p| matches!(p, EntityType::Line(_))));
}

#[test]
fn explode_insert_golden_block_members() {
    let mut doc = CadDocument::new();
    add_test_block(&mut doc);
    let insert = Insert::new(BLOCK_NAME, Vector3::new(0.0, 0.0, 0.0));
    let pieces = explode_entity(&EntityType::Insert(insert), &doc);
    assert_eq!(pieces.len(), 6, "insert must materialize all 6 block members");
    assert!(pieces.iter().all(|p| matches!(p, EntityType::Line(_))));
}

#[test]
fn explode_mline_golden_three_lines_per_segment() {
    let doc = CadDocument::new();
    let ml = MLine::from_points(&[
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(4.0, 0.0, 0.0),
        Vector3::new(4.0, 4.0, 0.0),
        Vector3::new(8.0, 4.0, 0.0),
    ]);
    let pieces = explode_entity(&EntityType::MLine(ml), &doc);
    assert_eq!(pieces.len(), 9, "3 segments × (spine + 2 offsets)");
    assert!(pieces.iter().all(|p| matches!(p, EntityType::Line(_))));
}

#[test]
fn explode_dimension_linear_golden_segments() {
    let doc = CadDocument::new();
    let mut d = DimensionLinear::new(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
    );
    d.definition_point = Vector3::new(5.0, 3.0, 0.0);
    d.base.text_middle_point = Vector3::new(5.0, 5.0, 0.0);
    let pieces = explode_entity(&EntityType::Dimension(Dimension::Linear(d)), &doc);
    // A baked linear dimension explodes to exactly 2 extension lines + 1
    // dimension line. The arrowheads bake as SOLIDs (see the unit test
    // `linear_dim_bakes_filled_arrowheads`), which is why the Lines count is
    // 3 — the plan's pre-execution "≥4 Lines" research claim was wrong.
    assert_eq!(
        pieces.iter().filter(|p| matches!(p, EntityType::Line(_))).count(),
        3,
        "linear dim must explode to extension + dimension lines, got {pieces:?}"
    );
    assert!(
        pieces.iter().filter(|p| matches!(p, EntityType::Solid(_))).count() >= 2,
        "linear dim must bake filled arrowheads (SOLIDs), got {pieces:?}"
    );
    // The measurement label is baked as a Text entity when the style path
    // yields one. If this assertion fails on an empty document, the text
    // entity legitimately was skipped — downgrade to the Lines assertion
    // above rather than inventing a style fixture.
    assert!(
        pieces
            .iter()
            .any(|p| matches!(p, EntityType::Text(_) | EntityType::MText(_))),
        "dimension label must be baked into a text entity, got {pieces:?}"
    );
}

#[test]
fn centermark_associated_line_explodes_without_leaking_association() {
    let mut doc = CadDocument::new();
    let source = doc.allocate_handle();
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(10.0, 0.0, 0.0);
    let assoc = CenterMarkAssociation {
        source: CenterMarkSource {
            handle: source,
            kind: CenterMarkSourceKind::Circle,
            segment_index: 0,
            pick_point: Vector3::new(5.0, 0.0, 0.0),
        },
        plane_origin: Vector3::new(0.0, 0.0, 0.0),
        plane_x: Vector3::new(1.0, 0.0, 0.0),
        plane_y: Vector3::new(0.0, 1.0, 0.0),
        center: Vector3::new(5.0, 0.0, 0.0),
        radius: 2.0,
        cross_size: 1.0,
        cross_gap: 0.1,
        cross_size_relative: false,
        cross_gap_relative: false,
        extension_length: 0.0,
        length_adjustments: [0.0; 4],
        overshoots: [0.0; 4],
        show_extensions: false,
        associated: true,
    };
    assoc.write(&mut line.common.extended_data);
    assert!(CenterMarkAssociation::read(&line.common.extended_data).is_some());

    let pieces = explode_entity(&EntityType::Line(line), &doc);
    assert!(!pieces.is_empty(), "centermark-attached line must explode");
    for piece in &pieces {
        let common = piece.common();
        assert!(
            CenterMarkAssociation::read(&common.extended_data).is_none(),
            "association must not leak onto exploded pieces"
        );
    }
}

// ── Geometry invariants ───────────────────────────────────────────────────

#[test]
fn lwpolyline_explode_preserves_length_and_closed_counts() {
    let doc = CadDocument::new();

    let mut pl = LwPolyline::new();
    pl.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        LwVertex::new(Vector2::new(3.0, 0.0)),
        LwVertex::new(Vector2::new(3.0, 4.0)),
    ];
    let pieces = explode_entity(&EntityType::LwPolyline(pl), &doc);
    assert_eq!(pieces.len(), 2);
    let total: f64 = pieces
        .iter()
        .map(|p| match p {
            EntityType::Line(l) => (l.end - l.start).length(),
            _ => 0.0,
        })
        .sum();
    assert!((total - 7.0).abs() < 1e-9, "total length must be preserved, got {total}");

    let mut sq = LwPolyline::new();
    sq.is_closed = true;
    sq.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        LwVertex::new(Vector2::new(6.0, 0.0)),
        LwVertex::new(Vector2::new(6.0, 6.0)),
        LwVertex::new(Vector2::new(0.0, 6.0)),
    ];
    let pieces = explode_entity(&EntityType::LwPolyline(sq), &doc);
    assert_eq!(pieces.len(), 4, "closed square explodes to 4 segments");
}

#[test]
fn plan_explode_drops_empty_and_keeps_order() {
    let mut scene = Scene::new();
    let mut line = Line::new(); // plain line → explodes to nothing
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(1.0, 0.0, 0.0);
    let survivor = scene.add_entity(EntityType::Line(line));
    let mut pl = LwPolyline::new();
    pl.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        LwVertex::new(Vector2::new(5.0, 0.0)),
    ];
    let poly = scene.add_entity(EntityType::LwPolyline(pl));

    // Select in the order: survivor first, polyline second.
    scene.select_entity(survivor, false);
    scene.select_entity(poly, false);
    let selected: Vec<(Handle, &EntityType)> = scene
        .selected_entities()
        .into_iter()
        .filter(|(h, _)| !scene.is_layer_locked(*h))
        .collect();
    let replacements = plan_explode(&selected, &scene.document);

    assert_eq!(replacements.len(), 1, "empty pieces must be dropped");
    assert_eq!(replacements[0].0, poly, "order/identity preserved");
    assert!(!replacements[0].1.is_empty());
}

// ── Differential: legacy loop vs helper pipeline ──────────────────────────

/// Frozen reference implementation of the pre-refactor EXPLODE command arm.
fn legacy_apply(scene: &mut Scene, replacements: &[(Handle, Vec<EntityType>)]) {
    for (handle, pieces) in replacements {
        scene.erase_entities(&[*handle]);
        for piece in pieces {
            scene.add_entity(piece.clone());
        }
    }
}

fn document_state(scene: &Scene) -> Vec<(Handle, EntityType)> {
    let mut state: Vec<(Handle, EntityType)> = scene
        .document
        .entities()
        .map(|e| (e.common().handle, e.clone()))
        .collect();
    state.sort_by_key(|(h, _)| *h);
    state
}

fn selected_handles(scene: &Scene) -> Vec<Handle> {
    scene
        .selected_entities()
        .into_iter()
        .map(|(h, _)| h)
        .collect()
}

#[derive(Clone, Copy)]
struct FixtureHandles {
    poly: Handle,
    mline: Handle,
    insert: Handle,
    survivor: Handle,
    leader1: Handle,
    leader2: Handle,
}

/// Deterministic identical build used for both sides of the differential:
/// explodable polyline + mline + insert, a non-explodable survivor line that
/// must survive, and two leaders attached to the polyline that must be
/// erased via annotation coupling even though they are never planned.
/// The planned replacement order poly (2 pieces) → mline (9) → insert (6)
/// is deliberate: it is what makes a mis-ordered flattened add in
/// `apply_explode_replacements` fail `document_state` below instead of
/// coincidentally passing.
fn build_differential_scene() -> (Scene, FixtureHandles) {
    let mut scene = Scene::new();
    add_test_block(&mut scene.document);

    let mut pl = LwPolyline::new();
    pl.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        LwVertex::new(Vector2::new(10.0, 0.0)),
        LwVertex::new(Vector2::new(10.0, 5.0)),
    ];
    let poly = scene.add_entity(EntityType::LwPolyline(pl));

    let mline = scene.add_entity(EntityType::MLine(MLine::from_points(&[
        Vector3::new(20.0, 0.0, 0.0),
        Vector3::new(25.0, 0.0, 0.0),
        Vector3::new(25.0, 5.0, 0.0),
        Vector3::new(30.0, 5.0, 0.0),
    ])));

    let insert =
        scene.add_entity(EntityType::Insert(Insert::new(BLOCK_NAME, Vector3::new(0.0, 20.0, 0.0))));

    let mut line = Line::new();
    line.start = Vector3::new(0.0, 50.0, 0.0);
    line.end = Vector3::new(10.0, 50.0, 0.0);
    let survivor = scene.add_entity(EntityType::Line(line));

    let mut l1 = Leader::new();
    l1.vertices = vec![Vector3::new(1.0, 1.0, 0.0), Vector3::new(2.0, 2.0, 0.0)];
    l1.annotation_handle = poly;
    let leader1 = scene.add_entity(EntityType::Leader(l1));

    let mut l2 = Leader::new();
    l2.vertices = vec![Vector3::new(3.0, 3.0, 0.0), Vector3::new(4.0, 4.0, 0.0)];
    l2.annotation_handle = poly;
    let leader2 = scene.add_entity(EntityType::Leader(l2));

    // User-style selection: survivor + all three explodable sources (the
    // two leaders ride along through annotation expansion when poly is
    // selected; they are never planned, only coupling-erased).
    scene.select_entity(survivor, false);
    scene.select_entity(poly, false);
    scene.select_entity(mline, false);
    scene.select_entity(insert, false);

    (
        scene,
        FixtureHandles { poly, mline, insert, survivor, leader1, leader2 },
    )
}

fn plan_from_selection(scene: &Scene) -> Vec<(Handle, Vec<EntityType>)> {
    let selected: Vec<(Handle, &EntityType)> = scene
        .selected_entities()
        .into_iter()
        .filter(|(h, _)| !scene.is_layer_locked(*h))
        .collect();
    plan_explode(&selected, &scene.document)
}

#[test]
fn apply_explode_replacements_matches_legacy_loop() {
    let (mut scene_legacy, handles_prior) = build_differential_scene();
    let (mut scene_batched, handles) = build_differential_scene();
    // Both builds must start from identical handle sequences: allocation is
    // a per-document monotonic counter (CadDocument::allocate_handle), never
    // global/atomic — these assertions make that assumption explicit so a
    // future allocator change fails here, not as an opaque state mismatch.
    assert_eq!(handles_prior.poly, handles.poly);
    assert_eq!(handles_prior.survivor, handles.survivor);

    let replacements = plan_from_selection(&scene_legacy);
    assert!(replacements.len() >= 3, "poly, mline, insert must be planned");
    assert_eq!(
        replacements.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
        vec![handles.poly, handles.mline, handles.insert],
        "planning must preserve selection order poly → mline → insert — this \
         multi-source ordering is the guard that a mis-ordered flattened add \
         fails the document_state comparison below"
    );
    let for_legacy = replacements.clone();

    legacy_apply(&mut scene_legacy, &for_legacy);
    apply_explode_replacements(&mut scene_batched, replacements);

    assert_eq!(
        document_state(&scene_legacy),
        document_state(&scene_batched),
        "document state (handles + entities) must be identical"
    );
    assert_eq!(
        selected_handles(&scene_legacy),
        selected_handles(&scene_batched),
        "selection state must be identical"
    );

    for scene in [&scene_legacy, &scene_batched] {
        assert!(scene.document.get_entity(handles.poly).is_none(), "poly erased");
        assert!(scene.document.get_entity(handles.mline).is_none(), "mline erased");
        assert!(scene.document.get_entity(handles.insert).is_none(), "insert erased");
        assert!(scene.document.get_entity(handles.leader1).is_none(), "leader coupling L1");
        assert!(scene.document.get_entity(handles.leader2).is_none(), "leader coupling L2");
        assert!(
            scene.document.get_entity(handles.survivor).is_some(),
            "non-explodable survivor must survive"
        );
        assert!(
            selected_handles(scene).contains(&handles.survivor),
            "survivor must stay selected"
        );
    }

    // explode_batch keeps empty entries so both sides see the same inputs.
    let items: Vec<(Handle, &EntityType)> = vec![
        (handles.survivor, scene_batched.document.get_entity(handles.survivor).unwrap()),
    ];
    assert_eq!(explode_batch(&items, &scene_batched.document), vec![vec![]]);
}
