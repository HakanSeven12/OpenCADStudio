//! Synthetic benchmark for `blk_cache` build and lookup inside `wires-build`.
//!
//! Constructs a deterministic block-heavy drawing:
//! - `100_000` plain model-space `Line` entities (`collect_referenced_blocks` scan)
//! - `300` leaf block definitions (`Leaf0..Leaf299`, 12 lines each)
//! - `40` nested wrapper blocks (`Nest0..Nest39`, 3 child inserts + 4 lines each)
//! - `2` large block definitions (`Heavy0`, `Heavy1`, 1,500 lines each)
//! - `12_000` `Insert` references (including lowercase block-name casing on a
//!   subset to exercise case-insensitive `BlockCache::defn` lookup)
//!
//! Measures four steady-state arms:
//! 1. `blk_cache_build` — isolated `BlockCache::build` (`refs + defns + metrics`)
//! 2. `blk_cache_lookup_cold_protos` — parallel `expand_insert` over all
//!    `12_000` inserts against a freshly built `BlockCache`
//! 3. `blk_cache_lookup_warm_protos` — parallel `expand_insert` when
//!    `expansion_prototypes` is already warm
//! 4. `wires_build_block_heavy` — full `Scene::model_tile_wires_arc` after
//!    `bump_geometry()` (`blk_cache` build + `expand_block_object` lookup + build)
//!
//! Run: `cargo bench --bench blk_cache_wires_build`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{Insert, Line};
use document::types::{Color as AcadColor, Handle, Vector3};
use document::EntityType;
use rayon::prelude::*;
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;
use OpenCADStudio::scene::{BlockScalePolicy, Scene};

const MODEL_LINES: usize = 100_000;
const LEAF_BLOCKS: usize = 300;
const LEAF_LINES: usize = 12;
const NESTED_BLOCKS: usize = 40;
const NESTED_LOCAL_LINES: usize = 4;
const HEAVY_BLOCKS: usize = 2;
const HEAVY_LINES: usize = 1_500;

const LEAF_EXACT_INSERTS: usize = 6_000;
const LEAF_LOWER_INSERTS: usize = 3_000;
const NESTED_INSERTS: usize = 2_500;
const HEAVY_INSERTS: usize = 500;

const WARMUP: usize = 1;
const RUNS: usize = 5;

fn report_samples(name: &str, mut samples: Vec<u64>, extra: &str) {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let min = samples[0];
    let max = samples[samples.len() - 1];
    println!(
        "{:<30} median={:.3} ms  min={:.3} ms  max={:.3} ms  ({extra})",
        name,
        median as f64 / 1_000_000.0,
        min as f64 / 1_000_000.0,
        max as f64 / 1_000_000.0,
    );
}

fn build_scene() -> (Scene, Vec<(Handle, Insert)>) {
    let mut scene = Scene::new();

    for b in 0..LEAF_BLOCKS {
        let name = format!("Leaf{b}");
        let mut ents = Vec::with_capacity(LEAF_LINES);
        for i in 0..LEAF_LINES {
            let y = i as f64 * 0.5;
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(6.0, y + 0.25, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
    }

    for n in 0..NESTED_BLOCKS {
        let name = format!("Nest{n}");
        let mut ents = Vec::with_capacity(3 + NESTED_LOCAL_LINES);
        for c in 0..3 {
            let leaf_name = format!("Leaf{}", (n * 3 + c) % LEAF_BLOCKS);
            ents.push(EntityType::Insert(Insert::new(
                &leaf_name,
                Vector3::new(c as f64 * 8.0, 0.0, 0.0),
            )));
        }
        for i in 0..NESTED_LOCAL_LINES {
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(0.0, i as f64, 0.0),
                Vector3::new(24.0, i as f64, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
    }

    for h in 0..HEAVY_BLOCKS {
        let name = format!("Heavy{h}");
        let mut ents = Vec::with_capacity(HEAVY_LINES);
        for i in 0..HEAVY_LINES {
            let x = (i % 50) as f64 * 2.0;
            let y = (i / 50) as f64 * 2.0;
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(x, y, 0.0),
                Vector3::new(x + 1.5, y + 0.5, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
    }

    let mut inserts = Vec::with_capacity(
        LEAF_EXACT_INSERTS + LEAF_LOWER_INSERTS + NESTED_INSERTS + HEAVY_INSERTS,
    );

    for i in 0..LEAF_EXACT_INSERTS {
        let name = format!("Leaf{}", i % LEAF_BLOCKS);
        let pos = Vector3::new((i % 200) as f64 * 10.0, (i / 200) as f64 * 10.0, 0.0);
        let ins = Insert::new(&name, pos);
        let handle = scene.add_entity(EntityType::Insert(ins.clone()));
        inserts.push((handle, ins));
    }

    for i in 0..LEAF_LOWER_INSERTS {
        let name = format!("leaf{}", i % LEAF_BLOCKS);
        let pos = Vector3::new(
            (i % 200) as f64 * 10.0,
            (i / 200) as f64 * 10.0 + 400.0,
            0.0,
        );
        let ins = Insert::new(&name, pos);
        let handle = scene.add_entity(EntityType::Insert(ins.clone()));
        inserts.push((handle, ins));
    }

    for i in 0..NESTED_INSERTS {
        let name = format!("Nest{}", i % NESTED_BLOCKS);
        let pos = Vector3::new(
            (i % 100) as f64 * 30.0,
            (i / 100) as f64 * 20.0 + 800.0,
            0.0,
        );
        let ins = Insert::new(&name, pos);
        let handle = scene.add_entity(EntityType::Insert(ins.clone()));
        inserts.push((handle, ins));
    }

    for i in 0..HEAVY_INSERTS {
        let name = format!("Heavy{}", i % HEAVY_BLOCKS);
        let pos = Vector3::new(
            (i % 25) as f64 * 120.0,
            (i / 25) as f64 * 80.0 + 1600.0,
            0.0,
        );
        let ins = Insert::new(&name, pos);
        let handle = scene.add_entity(EntityType::Insert(ins.clone()));
        inserts.push((handle, ins));
    }

    for i in 0..MODEL_LINES {
        let x = (i % 500) as f64 * 10.0;
        let y = (i / 500) as f64 * 10.0 + 4000.0;
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 8.0, y + 2.0, 0.0),
        )));
    }

    let cam = scene.camera.borrow().clone();
    let _ = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    (scene, inserts)
}

fn expand_all(doc: &document::Document, cache: &BlockCache, inserts: &[(Handle, Insert)]) -> usize {
    let bg = [0.0_f32, 0.0, 0.0, 1.0];
    let l0 = InheritStyle {
        color: [1.0, 1.0, 1.0, 1.0],
        pat_len: 0.0,
        pat: [0.0; 8],
        lw_px: 1.0,
    };
    inserts
        .par_iter()
        .map(|(handle, ins)| {
            expand_insert(
                doc,
                cache,
                ins,
                *handle,
                [1.0, 1.0, 1.0, 1.0],
                7,
                0.0,
                [0.0; 8],
                1.0,
                l0,
                7,
                true,
                false,
                1.0,
                None,
                None,
                false,
                bg,
                1.0,
                BlockScalePolicy::FromInsert,
                false,
                1.0,
            )
            .map_or(0, |wires| {
                assert!(
                    !wires.is_empty(),
                    "block {} expanded to 0 wires",
                    ins.block_name
                );
                wires.len()
            })
        })
        .sum()
}

fn main() {
    let (mut scene, inserts) = build_scene();
    let depth_map = scene.draw_depth_map();
    let cam = scene.camera.borrow().clone();
    let bg = [0.0_f32, 0.0, 0.0, 1.0];
    assert_eq!(AcadColor::WHITE, AcadColor::Index(7));

    println!(
        "blk_cache_wires_build bench (lines={MODEL_LINES}, defs={}, inserts={})",
        LEAF_BLOCKS + NESTED_BLOCKS + HEAVY_BLOCKS,
        inserts.len(),
    );

    // 1. Isolated BlockCache::build
    let mut build_sink = 0usize;
    for _ in 0..WARMUP {
        let cache = BlockCache::build(&scene.document, 1.0, None, true, bg, None, &depth_map);
        assert!(cache.defn("Leaf0").is_some());
        assert!(cache.defn("leaf0").is_some());
        assert!(cache.defn("Nest0").is_some());
        assert!(cache.defn("Heavy0").is_some());
        build_sink = build_sink.wrapping_add(1);
    }
    let mut build_samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let cache = BlockCache::build(
            black_box(&scene.document),
            1.0,
            None,
            true,
            bg,
            None,
            black_box(&depth_map),
        );
        build_samples.push(start.elapsed().as_nanos() as u64);
        assert!(cache.defn("leaf0").is_some());
        build_sink = build_sink.wrapping_add(1);
    }
    report_samples(
        "blk_cache_build",
        build_samples,
        &format!("sink={build_sink}"),
    );

    // 2. Parallel expand_insert lookup against a freshly built BlockCache (cold prototypes)
    let mut cold_expand_sink = 0usize;
    let mut expected_expand_wires = None;
    for _ in 0..WARMUP {
        let cache = BlockCache::build(&scene.document, 1.0, None, true, bg, None, &depth_map);
        let count = expand_all(&scene.document, &cache, &inserts);
        cold_expand_sink = cold_expand_sink.wrapping_add(count);
    }
    let mut cold_lookup_samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let cache = BlockCache::build(&scene.document, 1.0, None, true, bg, None, &depth_map);
        let start = Instant::now();
        let count = expand_all(&scene.document, black_box(&cache), black_box(&inserts));
        cold_lookup_samples.push(start.elapsed().as_nanos() as u64);
        if let Some(prev) = expected_expand_wires {
            assert_eq!(count, prev);
        } else {
            expected_expand_wires = Some(count);
        }
        cold_expand_sink = cold_expand_sink.wrapping_add(count);
    }
    report_samples(
        "blk_cache_lookup_cold_protos",
        cold_lookup_samples,
        &format!(
            "wires={}, sink={cold_expand_sink}",
            expected_expand_wires.unwrap_or(0)
        ),
    );

    // 3. Parallel expand_insert lookup against an already-warm BlockCache
    let warm_cache = BlockCache::build(&scene.document, 1.0, None, true, bg, None, &depth_map);
    let populating_count = expand_all(&scene.document, &warm_cache, &inserts);
    assert_eq!(Some(populating_count), expected_expand_wires);
    let warm_count = expand_all(&scene.document, &warm_cache, &inserts);
    let mut warm_lookup_samples = Vec::with_capacity(RUNS);
    let mut warm_expand_sink = 0usize;
    for _ in 0..RUNS {
        let start = Instant::now();
        let count = expand_all(&scene.document, black_box(&warm_cache), black_box(&inserts));
        warm_lookup_samples.push(start.elapsed().as_nanos() as u64);
        assert_eq!(count, warm_count);
        warm_expand_sink = warm_expand_sink.wrapping_add(count);
    }
    report_samples(
        "blk_cache_lookup_warm_protos",
        warm_lookup_samples,
        &format!("wires={warm_count}, sink={warm_expand_sink}"),
    );

    // 4. Full Scene wires-build (bump_geometry -> block_cache_arc_for + tess)
    let mut wires_sink = 0usize;
    let mut expected_total_wires = None;
    for _ in 0..WARMUP {
        scene.bump_geometry();
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        wires_sink = wires_sink.wrapping_add(wires.len());
    }
    let mut wires_samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        scene.bump_geometry();
        let start = Instant::now();
        let wires = scene.model_tile_wires_arc(0, black_box(&cam), 1.0, 1.0);
        wires_samples.push(start.elapsed().as_nanos() as u64);
        let stub_count = wires.iter().filter(|w| w.stub.is_some()).count();
        assert!(stub_count > 0, "expected instanced stubs in wires-build");
        if let Some(prev) = expected_total_wires {
            assert_eq!(wires.len(), prev);
        } else {
            expected_total_wires = Some(wires.len());
        }
        wires_sink = wires_sink.wrapping_add(wires.len());
    }
    report_samples(
        "wires_build_block_heavy",
        wires_samples,
        &format!(
            "wires={}, sink={wires_sink}",
            expected_total_wires.unwrap_or(0)
        ),
    );
}
