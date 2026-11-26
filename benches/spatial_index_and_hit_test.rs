//! Benchmark suite for 2D and 3D spatial indexing, broad-phase culling,
//! interactive modify indexing, and 3D mesh raycasting / face snapping.
//!
//! Run baseline:
//!   `cargo bench --bench spatial_index_and_hit_test -- --output target/spatial_index_baseline.json`
//! Run comparison against baseline:
//!   `cargo bench --bench spatial_index_and_hit_test -- --baseline target/spatial_index_baseline.json`

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use document::entities::{Line, Solid3D};
use document::types::Vector3;
use document::{EntityType, Handle};
use glam::{DVec3, EulerRot, Mat4, Quat};
use iced::{Point as ScreenPoint, Rectangle};

use OpenCADStudio::modules::draw::modify::entity_index::ModifyEntityIndex;
use OpenCADStudio::modules::draw::modify::trim::TrimAabbIndex;
use OpenCADStudio::scene::model::wire_model::{SnapHint, WireGeom, WireModel};
use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::pick::quadtree::QuadTree;
use OpenCADStudio::scene::view::camera::{Camera, Projection};
use OpenCADStudio::scene::Scene;

#[path = "support/harness.rs"]
mod harness;

use harness::BenchmarkRunner;

const WARMUP: usize = 2;
const RUNS: usize = 7;

/// Fixture 1: 80,000 entity AABBs including a realistic proportion of long
/// straddler grid/border lines that cross quadrant midlines, plus 200 local
/// viewport query rectangles.
fn build_quadtree_fixture() -> ([f64; 4], Vec<(Handle, [f64; 4])>, Vec<[f64; 4]>) {
    let world = [-5_000.0, -5_000.0, 5_000.0, 5_000.0];
    let mut items = Vec::with_capacity(80_000);
    for i in 0..80_000usize {
        let h = Handle::new((i + 1) as u64);
        let aabb = if i % 20 == 0 {
            // Horizontal or vertical structural grid lines spanning across the origin/midlines
            let offset = ((i / 20) as f64 % 400.0 - 200.0) * 20.0;
            if i % 40 == 0 {
                [-4_800.0, offset - 0.5, 4_800.0, offset + 0.5]
            } else {
                [offset - 0.5, -4_800.0, offset + 0.5, 4_800.0]
            }
        } else {
            let col = (i % 280) as f64;
            let row = (i / 280) as f64;
            let x = -4_900.0 + col * 35.0 + ((i * 17) % 11) as f64;
            let y = -4_900.0 + row * 34.0 + ((i * 23) % 13) as f64;
            let w = 8.0 + (i % 15) as f64 * 2.5;
            let h_box = 6.0 + (i % 13) as f64 * 2.5;
            [x, y, x + w, y + h_box]
        };
        items.push((h, aabb));
    }

    let mut queries = Vec::with_capacity(200);
    for q in 0..200usize {
        let cx = -4_500.0 + ((q * 73) % 900) as f64 * 10.0;
        let cy = -4_500.0 + ((q * 137) % 900) as f64 * 10.0;
        queries.push([cx - 120.0, cy - 90.0, cx + 120.0, cy + 90.0]);
    }
    (world, items, queries)
}

/// Fixture 2: 30,000 `EntityType::Line` entities for `ModifyEntityIndex::build`,
/// 400 `nearby_handles` queries, and 200 dynamic `remove` + `push` edits
/// (simulating interactive `TRIM`/`EXTEND` sessions).
fn build_modify_entity_fixture() -> (Vec<EntityType>, Vec<Handle>, Vec<EntityType>) {
    let mut entities = Vec::with_capacity(30_000);
    let mut query_handles = Vec::with_capacity(400);
    for i in 0..30_000usize {
        let handle = Handle::new((i + 100) as u64);
        let mut line = Line::new();
        line.common.handle = handle;
        if i % 25 == 0 {
            let y = ((i / 25) as f64 - 600.0) * 4.0;
            line.start = Vector3::new(-2_500.0, y, 0.0);
            line.end = Vector3::new(2_500.0, y + 2.0, 0.0);
        } else {
            let col = (i % 180) as f64;
            let row = (i / 180) as f64;
            let x = -2_400.0 + col * 26.0;
            let y = -2_400.0 + row * 28.0;
            line.start = Vector3::new(x, y, 0.0);
            line.end = Vector3::new(x + 22.0, y + 18.0, 0.0);
        }
        if i % 75 == 0 && query_handles.len() < 400 {
            query_handles.push(handle);
        }
        entities.push(EntityType::Line(line));
    }

    let mut replacements = Vec::with_capacity(200);
    for i in 0..200usize {
        let handle = Handle::new((100_000 + i) as u64);
        let mut line = Line::new();
        line.common.handle = handle;
        let x = -1_800.0 + (i as f64) * 18.0;
        let y = -1_200.0 + (i as f64) * 12.0;
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 11.0, y + 9.0, 0.0);
        replacements.push(EntityType::Line(line));
    }

    (entities, query_handles, replacements)
}

/// Fixture 3: 30,000 boundary AABBs (including 60 unbounded entries) and 1,500
/// target query boxes for `TrimAabbIndex`.
fn build_trim_aabb_fixture() -> (Vec<Option<[f64; 4]>>, Vec<[f64; 4]>) {
    let mut bounds = Vec::with_capacity(30_000);
    for i in 0..30_000usize {
        if i % 500 == 0 {
            bounds.push(None);
        } else if i % 30 == 0 {
            // Wide horizontal span that overlaps many X intervals but narrow Y
            let y = ((i / 30) as f64 - 500.0) * 6.0;
            bounds.push(Some([-3_000.0, y - 1.0, 3_000.0, y + 1.0]));
        } else {
            let col = (i % 200) as f64;
            let row = (i / 200) as f64;
            let x = -3_000.0 + col * 30.0;
            let y = -3_000.0 + row * 40.0;
            bounds.push(Some([x, y, x + 25.0, y + 32.0]));
        }
    }
    let mut queries = Vec::with_capacity(1_500);
    for q in 0..1_500usize {
        let x = -2_800.0 + ((q * 89) % 560) as f64 * 10.0;
        let y = -2_800.0 + ((q * 131) % 560) as f64 * 10.0;
        queries.push([x, y, x + 35.0, y + 35.0]);
    }
    (bounds, queries)
}

/// Fixture 4: 15,000 3D multi-segment wires across different elevations for
/// `InteractionIndex` 3D `prepare_screen` (building `SpatialBvh3`) and tilted
/// perspective `query_screen` + `query_snap_screen_with_category`.
fn build_3d_wires_fixture() -> (Arc<Vec<WireModel>>, Mat4, DVec3, Rectangle, Vec<[f32; 4]>) {
    let mut wires = Vec::with_capacity(15_000);
    for i in 0..15_000usize {
        let col = (i % 125) as f32;
        let row = (i / 125) as f32;
        let x = -1_800.0 + col * 30.0;
        let y = -1_800.0 + row * 30.0;
        let z = ((i * 13) % 40) as f32 * 8.0 - 160.0;
        let points: Vec<[f32; 3]> = (0..12)
            .map(|s| {
                let t = s as f32 / 11.0;
                [x + t * 24.0, y + (t * 3.14).sin() * 12.0, z + t * 15.0]
            })
            .collect();
        let key_vertices: Vec<[f64; 3]> = points
            .iter()
            .step_by(3)
            .map(|&p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let snap_pts = vec![
            (
                DVec3::new(points[0][0] as f64, points[0][1] as f64, points[0][2] as f64),
                SnapHint::Vertex,
            ),
            (
                DVec3::new(points[11][0] as f64, points[11][1] as f64, points[11][2] as f64),
                SnapHint::Vertex,
            ),
        ];
        wires.push(WireModel {
            name: (i + 1).to_string(),
            geom: Arc::new(WireGeom {
                points,
                key_vertices,
                snap_pts,
                ..WireGeom::default()
            }),
            aabb: [x, y, x + 24.0, y + 12.0],
            ..WireModel::default()
        });
    }

    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };
    let mut cam = Camera::default();
    cam.target = DVec3::new(0.0, 0.0, 0.0);
    cam.distance = 2_400.0;
    cam.rotation = Quat::from_euler(EulerRot::XYZ, 0.65, 0.0, 0.55);
    cam.projection = Projection::Perspective;
    let view_rot = cam.view_proj_rte(bounds);
    let eye = cam.eye();

    let mut rects = Vec::with_capacity(200);
    for q in 0..200usize {
        let sx = 120.0 + ((q * 53) % 1040) as f32;
        let sy = 80.0 + ((q * 97) % 560) as f32;
        rects.push([sx - 14.0, sy - 14.0, sx + 14.0, sy + 14.0]);
    }

    (Arc::new(wires), view_rot, eye, bounds, rects)
}

/// Fixture 5: Scene containing 16 tessellated 3D solid tori & spheres for
/// `Scene::solid_click_hit` (3D raycasting) and `Scene::solid_face_snaps`
/// (`NearestFace` + `FacePerpendicular`).
fn build_3d_solid_scene_fixture() -> (Scene, Mat4, DVec3, Rectangle, Vec<ScreenPoint>) {
    let mut scene = Scene::new();
    for i in 0..16usize {
        let col = (i % 4) as f64;
        let row = (i / 4) as f64;
        let cx = -150.0 + col * 100.0;
        let cy = -150.0 + row * 100.0;
        let cz = ((i % 3) as f64 - 1.0) * 35.0;
        let body = if i % 2 == 0 {
            ocs_solid::model::make_torus([cx, cy, cz], 32.0, 11.0).expect("make_torus")
        } else {
            ocs_solid::model::make_sphere([cx, cy, cz], 36.0).expect("make_sphere")
        };
        let handle = scene.add_entity(EntityType::Solid3D(Solid3D::new()));
        let ok = scene.register_solid_model(handle, body);
        assert!(ok, "register_solid_model succeeded");
    }

    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };
    let mut cam = Camera::default();
    cam.target = DVec3::ZERO;
    cam.distance = 480.0;
    cam.rotation = Quat::from_euler(EulerRot::XYZ, 0.58, 0.0, 0.42);
    cam.projection = Projection::Perspective;
    let view_rot = cam.view_proj_rte(bounds);
    let eye = cam.eye();

    let mut cursors = Vec::with_capacity(80);
    for q in 0..80usize {
        let sx = 240.0 + ((q * 67) % 800) as f32;
        let sy = 140.0 + ((q * 83) % 440) as f32;
        cursors.push(ScreenPoint::new(sx, sy));
    }

    (scene, view_rot, eye, bounds, cursors)
}

fn main() {
    let mut runner = BenchmarkRunner::new(
        "OpenCADStudio 2D/3D Spatial Indexing & Hit-Testing Benchmark",
        "spatial_index_metrics.json",
    );

    // 1. QuadTree build + straddler-heavy viewport queries
    if runner.should_run("quadtree_build_and_view_query") {
        let (world, items, queries) = build_quadtree_fixture();
        let mut expected_total = None;
        for _ in 0..WARMUP {
            let mut tree = QuadTree::new(world);
            for &(h, aabb) in &items {
                tree.insert(h, aabb);
            }
            let total: usize = queries.iter().map(|&q| tree.query_rect(q).len()).sum();
            expected_total = Some(total);
        }
        let expected = expected_total.unwrap();
        assert!(expected > 1_000, "expected non-trivial query hits: {expected}");

        let mut samples_ms = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let mut tree = QuadTree::new(black_box(world));
            for &(h, aabb) in black_box(&items) {
                tree.insert(h, aabb);
            }
            let mut total = 0usize;
            for &q in black_box(&queries) {
                let mut res = tree.query_rect(q);
                res.sort_unstable_by_key(|h| h.value());
                res.dedup();
                total += res.len();
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            assert!(total > 0);
            samples_ms.push(elapsed_ms);
        }
        let median_ms = harness::median(&samples_ms);
        runner.record(
            "quadtree_build_and_view_query",
            "Build QuadTree on 80k entities (with straddlers) + 200 viewport queries",
            "ms",
            samples_ms,
            Some((1_000.0 / median_ms, "ops/s")),
            None,
        );
    }

    // 2. ModifyEntityIndex build + nearby_handles queries + dynamic remove/push
    if runner.should_run("modify_entity_index_trim_workload") {
        let (base_entities, query_handles, replacements) = build_modify_entity_fixture();
        let mut expected_nearby = None;
        for _ in 0..WARMUP {
            let mut entities = base_entities.clone();
            let mut index = ModifyEntityIndex::build(&entities);
            let mut count = 0usize;
            for &qh in &query_handles {
                if let Some(set) = index.nearby_handles(&entities, qh) {
                    count += set.len();
                }
            }
            for (k, rep) in replacements.iter().cloned().enumerate() {
                index.remove(&mut entities, query_handles[k]);
                index.push(&mut entities, rep);
            }
            for &qh in &query_handles[200..] {
                if let Some(set) = index.nearby_handles(&entities, qh) {
                    count += set.len();
                }
            }
            expected_nearby = Some(count);
        }
        let expected = expected_nearby.unwrap();

        let mut samples_ms = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let mut entities = base_entities.clone();
            let start = Instant::now();
            let mut index = ModifyEntityIndex::build(black_box(&entities));
            let mut count = 0usize;
            for &qh in black_box(&query_handles) {
                if let Some(set) = index.nearby_handles(&entities, qh) {
                    count += set.len();
                }
            }
            for (k, rep) in replacements.iter().cloned().enumerate() {
                index.remove(&mut entities, query_handles[k]);
                index.push(&mut entities, rep);
            }
            for &qh in black_box(&query_handles[200..]) {
                if let Some(set) = index.nearby_handles(&entities, qh) {
                    count += set.len();
                }
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            assert_eq!(count, expected, "ModifyEntityIndex nearby_handles count drifted");
            samples_ms.push(elapsed_ms);
        }
        let median_ms = harness::median(&samples_ms);
        runner.record(
            "modify_entity_index_trim_workload",
            "ModifyEntityIndex build (30k entities) + 600 nearby queries + 200 dynamic edits",
            "ms",
            samples_ms,
            Some((1_000.0 / median_ms, "ops/s")),
            None,
        );
    }

    // 3. TrimAabbIndex build + 1,500 overlapping queries
    if runner.should_run("trim_aabb_index_queries") {
        let (bounds, queries) = build_trim_aabb_fixture();
        let mut expected_hits = None;
        for _ in 0..WARMUP {
            let index = TrimAabbIndex::new(bounds.iter().copied());
            let hits: usize = queries.iter().map(|&q| index.overlapping(q).len()).sum();
            expected_hits = Some(hits);
        }
        let expected = expected_hits.unwrap();

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let index = TrimAabbIndex::new(black_box(&bounds).iter().copied());
            let hits: usize = black_box(&queries)
                .iter()
                .map(|&q| index.overlapping(q).len())
                .sum();
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            assert_eq!(hits, expected, "TrimAabbIndex hit count drifted");
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "trim_aabb_index_queries",
            "TrimAabbIndex build on 30k boundaries + 1,500 overlapping queries",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    // 4. InteractionIndex 3D BVH build (prepare_screen) + 200 tilted 3D screen queries
    if runner.should_run("interaction_bvh3_screen_queries") {
        let (wires, view_rot, eye, bounds, rects) = build_3d_wires_fixture();
        let index = InteractionIndex::build(&wires);
        index.prepare_screen();
        let mut expected_candidates = 0usize;
        for &r in &rects {
            let c = index.query_screen(Arc::clone(&wires), r, view_rot, eye, bounds);
            expected_candidates += c.len();
        }
        assert!(expected_candidates > 0);

        let mut samples_ms = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let mut total = 0usize;
            for &r in black_box(&rects) {
                let c = index.query_screen(
                    Arc::clone(&wires),
                    r,
                    black_box(view_rot),
                    black_box(eye),
                    bounds,
                );
                total += c.len();
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            assert_eq!(total, expected_candidates, "3D query_screen candidate count drifted");
            samples_ms.push(elapsed_ms);
        }
        let median_ms = harness::median(&samples_ms);
        runner.record(
            "interaction_bvh3_screen_queries",
            "200 tilted-camera 3D BVH screen queries over 15k 3D multi-segment wires",
            "ms",
            samples_ms,
            Some((200_000.0 / median_ms, "queries/s")),
            None,
        );
    }

    // 5. 3D Solid Mesh Raycast Picking + Face Snapping (NearestFace & FacePerpendicular)
    if runner.should_run("solid_3d_raycast_and_face_snaps") {
        let (scene, view_rot, eye, bounds, cursors) = build_3d_solid_scene_fixture();
        let base_pt = Some(DVec3::new(0.0, 0.0, 120.0));
        let mut expected_hits = 0usize;
        for &cursor in &cursors {
            if scene
                .solid_click_hit(cursor, view_rot, eye, bounds, None)
                .is_some()
            {
                expected_hits += 1;
            }
            if scene
                .solid_face_snaps(cursor, view_rot, eye, bounds, 24.0, base_pt, true, true)
                .is_some()
            {
                expected_hits += 1;
            }
        }
        assert!(expected_hits > 10, "expected 3D solid hits and face snaps, got {expected_hits}");

        let mut samples_ms = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let mut hits = 0usize;
            for &cursor in black_box(&cursors) {
                if scene
                    .solid_click_hit(cursor, black_box(view_rot), black_box(eye), bounds, None)
                    .is_some()
                {
                    hits += 1;
                }
                if scene
                    .solid_face_snaps(
                        cursor,
                        black_box(view_rot),
                        black_box(eye),
                        bounds,
                        24.0,
                        base_pt,
                        true,
                        true,
                    )
                    .is_some()
                {
                    hits += 1;
                }
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            assert_eq!(hits, expected_hits, "solid 3D raycast/snap hit count drifted");
            samples_ms.push(elapsed_ms);
        }
        let median_ms = harness::median(&samples_ms);
        runner.record(
            "solid_3d_raycast_and_face_snaps",
            "80 cursor 3D solid raycast picks + NearestFace/FacePerpendicular snaps on 16 solids",
            "ms",
            samples_ms,
            Some((80_000.0 / median_ms, "cursors/s")),
            None,
        );
    }

    runner.finish();
}
