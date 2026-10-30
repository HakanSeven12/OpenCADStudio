// Micro-bench for three scene pick/index cleanups (audit items S5, S6, S8).
//
//   S6 — `click_hits_all` deduped every pushed hit with a linear
//        `hits.iter().any(name ==)` scan per candidate wire (quadratic in
//        cycle depth). Replaced by a HashSet<&str> mirror of `hits`.
//        Exercised here through the `Vec<WireModel>` (non-indexed) path with
//        512 wires all crossing the cursor, each carrying fill + pick
//        triangles so the fill/pick phases reach the scan.
//
//   S5 — `SpatialGrid::query` collects entry indices from the oversized list
//        AND every overlapped grid cell, so an entry spanning k cells is
//        collected k times; the audit proposed dropping the index-level
//        sort+dedup (duplicates collapse at the value level anyway). This arm
//        measures that hypothesis through `Scene::interaction_candidates_xy`
//        with a prepared interaction index (installed via the same build-key /
//        install API the app's background builder uses) on a fully warm scene:
//        every SpatialSet (wires, segments, snap points, key
//        vertices/segments, triangles, glyphs) queried across a
//        scene-covering box. RESULT: removing the index-level dedup was a
//        +40% REGRESSION here (0.000455 -> 0.000639 s/query) — the dedup
//        shrinks both the entry lookups and the final value sort — so the
//        pass stays and this arm now pins the existing behavior.
//
//   S8 — the entity_index incremental patch ran one
//        `unbounded_handles.retain(...)` per resolved op (O(ops x handles)).
//        `replay_since` coalesces to one op per handle, so the batch can
//        sweep the unbounded list once and re-append the SetUnbounded
//        handles in op order. Exercised by bumping 500 unbounded POINT
//        entities per round and resolving candidates through
//        `interaction_candidate_handles` (the chain that reaches
//        `entity_index()`), over a scene holding 9000 unbounded POINTs.
//
// Manual-timing bench (harness = false); prints per-run seconds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codec::entities::{Line, Point};
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::{DVec3, Mat4};
use iced::{Point as ScreenPoint, Rectangle};
use OpenCADStudio::scene::model::wire_model::WireModel;
use OpenCADStudio::scene::pick::hit_test::click_hits_all;
use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::{ChangeKind, Scene};

const RUNS: usize = 5;
const WARMUP: usize = 1;
const CACHED_QUERIES: usize = 20;
const EDIT_ROUNDS: usize = 3;
const EDIT_BUMP: usize = 500;
const WIRE_COUNT: usize = 512;

const WORLD_AABB: [f64; 4] = [-10.0, -10.0, 1100.0, 1300.0];

fn build_scene() -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    for i in 0..3000 {
        let gx = (i % 60) as f64 * 17.0;
        let gy = (i / 60) as f64 * 19.0;
        let mut line = Line::new();
        line.start = Vector3::new(gx, gy, 0.0);
        line.end = Vector3::new(gx + 40.0, gy + 30.0, 0.0);
        scene.add_entity(EntityType::Line(line));
    }
    let mut point_handles = Vec::with_capacity(EDIT_BUMP);
    for i in 0..9000 {
        let mut p = Point::new();
        p.location = Vector3::new(
            ((i * 37) % 997) as f64 * 1.01,
            ((i * 101) % 991) as f64 * 1.01,
            0.0,
        );
        let h = scene.add_entity(EntityType::Point(p));
        if point_handles.len() < EDIT_BUMP {
            point_handles.push(h);
        }
    }
    (scene, point_handles)
}

/// Build and install the interaction index the way the app's background
/// builder does, so candidate queries take the indexed path instead of the
/// full-scan fallback (the app builds it off-thread between frames).
fn install_interaction_index(scene: &Scene) {
    let wires = scene.hit_test_wires();
    let Some((epoch, source)) = scene.interaction_index_build_key(&wires, 600.0) else {
        panic!("a 12000-wire scene must want an interaction index");
    };
    scene.mark_interaction_index_pending(epoch, source);
    let index = Arc::new(InteractionIndex::build(&wires));
    let installed = scene.install_prepared_interaction_index(
        epoch,
        source,
        Arc::downgrade(&wires),
        index,
    );
    assert!(installed, "prepared interaction index must install");
}

fn bench_click_hits_all() -> u64 {
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    let view = Mat4::IDENTITY;
    let eye = DVec3::ZERO;
    let cursor = ScreenPoint::new(400.0, 300.0);
    let mut wires = Vec::with_capacity(WIRE_COUNT);
    for i in 0..WIRE_COUNT {
        let mut w = WireModel::default();
        w.name = format!("w{i}");
        w.points = vec![[-0.02, 0.0, 0.0], [0.02, 0.0, 0.0]];
        w.aabb = [-0.05, -0.05, 0.05, 0.05];
        w.fill_tris = vec![[-0.01, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]];
        w.pick_tris = vec![[-0.01, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]];
        wires.push(w);
    }
    let probe = click_hits_all(cursor, &wires, view, eye, bounds, false, 6.0);
    assert_eq!(
        probe.len(),
        WIRE_COUNT,
        "fixture wires must all join the cycle, got {}",
        probe.len()
    );
    let mut checksum = probe.len() as u64;
    for run in 0..(WARMUP + RUNS) {
        let t = Instant::now();
        for _ in 0..CACHED_QUERIES {
            checksum = checksum.wrapping_add(
                click_hits_all(cursor, &wires, view, eye, bounds, false, 6.0).len() as u64,
            );
        }
        let dt = t.elapsed();
        if run >= WARMUP {
            println!(
                "click_hits_all run {}: {:.6} s ({} calls, {:.6} s/call)",
                run - WARMUP,
                dt.as_secs_f64(),
                CACHED_QUERIES,
                dt.as_secs_f64() / CACHED_QUERIES as f64
            );
        }
    }
    checksum
}

fn bench_cached_query() -> u64 {
    let (scene, _) = build_scene();
    install_interaction_index(&scene);
    let warm = scene.interaction_candidates_xy(WORLD_AABB);
    assert!(warm.iter().count() > 0, "warm query must return candidates");
    let mut checksum = warm.iter().count() as u64;
    for run in 0..(WARMUP + RUNS) {
        let t = Instant::now();
        for _ in 0..CACHED_QUERIES {
            checksum = checksum.wrapping_add(
                scene.interaction_candidates_xy(WORLD_AABB).iter().count() as u64,
            );
        }
        let dt = t.elapsed();
        if run >= WARMUP {
            println!(
                "cached_query run {}: {:.6} s ({} queries, {:.6} s/query)",
                run - WARMUP,
                dt.as_secs_f64(),
                CACHED_QUERIES,
                dt.as_secs_f64() / CACHED_QUERIES as f64
            );
        }
    }
    checksum
}

fn bench_edit_query() -> u64 {
    let mut checksum = 0u64;
    // A fresh scene per run keeps the overlay path in its steady state: the
    // installed base index goes stale on the first bump and every round
    // replays a small (< 2048 handles) change set on top of it, exactly the
    // small-edit window the app lives in between background index rebuilds.
    for run in 0..(WARMUP + RUNS) {
        let (mut scene, handles) = build_scene();
        install_interaction_index(&scene);
        let changes: Vec<(Handle, ChangeKind)> =
            handles.iter().map(|&h| (h, ChangeKind::Modified)).collect();
        // Untimed settle round: builds entity_index and the overlay caches.
        scene.bump_entities(&changes);
        let warm = scene
            .interaction_candidate_handles(&scene.interaction_candidates_xy(WORLD_AABB));
        assert!(
            warm.is_some_and(|set| !set.is_empty()),
            "warm candidate resolution must return handles"
        );
        let mut timed = Duration::ZERO;
        for _ in 0..EDIT_ROUNDS {
            scene.bump_entities(&changes);
            let t = Instant::now();
            let candidates = scene.interaction_candidates_xy(WORLD_AABB);
            let resolved = scene.interaction_candidate_handles(&candidates);
            timed += t.elapsed();
            checksum = checksum.wrapping_add(resolved.map_or(0, |set| set.len()) as u64);
        }
        if run >= WARMUP {
            println!(
                "edit_query run {}: {:.6} s ({} rounds, {:.6} s/round)",
                run - WARMUP,
                timed.as_secs_f64(),
                EDIT_ROUNDS,
                timed.as_secs_f64() / EDIT_ROUNDS as f64
            );
        }
    }
    checksum
}

fn main() {
    println!("== s6: click_hits_all cycle over {} overlapping wires ==", WIRE_COUNT);
    let c1 = bench_click_hits_all();
    println!("== s5: warm interaction_candidates_xy over 12000 wires ==");
    let c2 = bench_cached_query();
    println!(
        "== s8: {} bumped unbounded POINTs per round (9000-point scene) ==",
        EDIT_BUMP
    );
    let c3 = bench_edit_query();
    std::hint::black_box((c1, c2, c3));
}
