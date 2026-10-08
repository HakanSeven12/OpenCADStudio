//! OpenCADStudio COMMANDS benchmark suite (`harness = false`).
//! Currently: EXPLODE (geometry pipeline + scene apply).

#[path = "support/harness.rs"]
mod harness;

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{
    Circle, Dimension, DimensionLinear, Insert, Line, LwPolyline, LwVertex, MLine, Point,
    Polyline2D, Polyline3D, Ray, Solid, Text, Vertex2D, Vertex3DPolyline,
};
use codec::tables::BlockRecord;
use codec::types::{Vector2, Vector3};
use codec::{CadDocument, EntityType, Handle};

use harness::BenchmarkRunner;
use OpenCADStudio::modules::draw::modify::explode::{
    apply_explode_replacements, explode_batch,
};
use OpenCADStudio::modules::draw::modify::flatten;
use OpenCADStudio::scene::Scene;

const BLOCK_NAME: &str = "BENCH_BLOCK";
const VERTS_PER_POLYLINE: usize = 50;
const MEMBERS_PER_BLOCK: usize = 20;

/// (lwpolylines, inserts, dimensions, mlines) — full = 2,500 compounds.
fn counts(quick: bool) -> (usize, usize, usize, usize) {
    if quick {
        (1_000, 100, 100, 50)
    } else {
        (2_000, 200, 200, 100)
    }
}

fn fixture_entities(quick: bool) -> usize {
    let (a, b, c, d) = counts(quick);
    a + b + c + d
}

/// Shared 20-member block (10 lines + 10 three-vertex polylines) registered
/// in `block_records` with `entity_handles` set — what explode_from_document
/// needs to materialize inserts.
fn add_bench_block(doc: &mut CadDocument) {
    let mut member_handles = Vec::with_capacity(MEMBERS_PER_BLOCK);
    for k in 0..MEMBERS_PER_BLOCK {
        if k % 2 == 0 {
            let mut line = Line::new();
            let x = (k / 2) as f64 * 3.0;
            line.start = Vector3::new(x, 0.0, 0.0);
            line.end = Vector3::new(x, 4.0, 0.0);
            member_handles.push(doc.add_entity(EntityType::Line(line)).unwrap());
        } else {
            let mut pl = LwPolyline::new();
            pl.vertices = vec![
                LwVertex::new(Vector2::new(0.0, (k as f64) * 0.5)),
                LwVertex::new(Vector2::new(2.0, (k as f64) * 0.5)),
                LwVertex::new(Vector2::new(2.0, (k as f64) * 0.5 + 2.0)),
            ];
            member_handles.push(doc.add_entity(EntityType::LwPolyline(pl)).unwrap());
        }
    }
    let mut block = BlockRecord::new(BLOCK_NAME);
    block.handle = doc.allocate_handle();
    block.entity_handles = member_handles;
    doc.block_records.add(block).unwrap();
}

/// Populate the document with the compound fixture; returns the handles of
/// the fixture entities (block members are NOT included).
fn push_fixture_entities(doc: &mut CadDocument, quick: bool) -> Vec<Handle> {
    let (n_pl, n_ins, n_dim, n_ml) = counts(quick);
    let mut handles = Vec::with_capacity(fixture_entities(quick));

    for i in 0..n_pl {
        let base = i as f64 * 0.01;
        let mut pl = LwPolyline::new();
        pl.vertices = (0..VERTS_PER_POLYLINE)
            .map(|k| {
                let mut v = LwVertex::new(Vector2::new(
                    base + k as f64 * 2.0,
                    (k % 5) as f64 * 0.75,
                ));
                if k % 2 == 1 {
                    v.bulge = 0.5;
                }
                v
            })
            .collect();
        handles.push(doc.add_entity(EntityType::LwPolyline(pl)).unwrap());
    }

    for i in 0..n_ins {
        let pos = Vector3::new((i % 50) as f64 * 5.0, (i / 50) as f64 * 5.0, 0.0);
        let ins = Insert::new(BLOCK_NAME, pos);
        handles.push(doc.add_entity(EntityType::Insert(ins)).unwrap());
    }

    for i in 0..n_dim {
        let x = (i % 100) as f64 * 12.0;
        let y = (i / 100) as f64 * 12.0;
        let mut d = DimensionLinear::new(
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 10.0, y, 0.0),
        );
        d.definition_point = Vector3::new(x + 5.0, y + 3.0, 0.0);
        d.base.text_middle_point = Vector3::new(x + 5.0, y + 4.0, 0.0);
        handles.push(doc.add_entity(EntityType::Dimension(Dimension::Linear(d))).unwrap());
    }

    for i in 0..n_ml {
        let x = (i % 100) as f64 * 8.0;
        let y = (i / 100) as f64 * 8.0;
        let pts = [
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 4.0, y, 0.0),
            Vector3::new(x + 4.0, y + 4.0, 0.0),
            Vector3::new(x + 8.0, y + 4.0, 0.0),
            Vector3::new(x + 8.0, y + 8.0, 0.0),
            Vector3::new(x + 12.0, y + 8.0, 0.0),
        ];
        handles.push(doc.add_entity(EntityType::MLine(MLine::from_points(&pts))).unwrap());
    }

    handles
}

fn median(times: &[f64]) -> f64 {
    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) * 0.5
    } else {
        sorted[n / 2]
    }
}

/// EXPLODE geometry: time `explode_batch` over the whole fixture.
/// Document build + `items` slice construction are untimed; `black_box` on
/// the result keeps the compiler from eliding the work.
fn bench_explode_geometry(runner: &mut BenchmarkRunner) {
    let name = "explode_geometry";
    if !runner.should_run(name) {
        return;
    }
    let quick = runner.quick_mode;
    let sample_count = if quick { 3 } else { 10 };
    let entity_count = fixture_entities(quick);
    let mut times = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let mut doc = CadDocument::new();
        add_bench_block(&mut doc);
        let handles = push_fixture_entities(&mut doc, quick);
        let items: Vec<(Handle, &EntityType)> = handles
            .iter()
            .filter_map(|&h| doc.get_entity(h).map(|e| (h, e)))
            .collect();
        assert_eq!(items.len(), entity_count);

        let t0 = Instant::now();
        let pieces = black_box(explode_batch(&items, &doc));
        times.push(t0.elapsed().as_micros() as f64);
        assert_eq!(pieces.len(), entity_count);
        assert!(
            pieces.iter().any(|p| !p.is_empty()),
            "fixture must produce non-empty pieces — block-record/explosion path broken"
        );
    }

    let med = median(&times);
    let throughput = entity_count as f64 / (med / 1_000_000.0);
    runner.record(
        name,
        "EXPLODE geometry pipeline: explode_batch over the compound fixture",
        "us",
        times,
        Some((throughput, "entities/s")),
        None,
    );
}

/// EXPLODE apply: fresh Scene per sample (untimed), replacements computed
/// untimed, `apply_explode_replacements` timed.
fn bench_explode_scene_apply(runner: &mut BenchmarkRunner) {
    let name = "explode_scene_apply";
    if !runner.should_run(name) {
        return;
    }
    let quick = runner.quick_mode;
    let sample_count = if quick { 3 } else { 10 };
    let entity_count = fixture_entities(quick);
    let mut times = Vec::with_capacity(sample_count);
    let mut total_pieces = 0usize;

    for _ in 0..sample_count {
        let mut scene = Scene::new();
        add_bench_block(&mut scene.document);
        let handles = push_fixture_entities(&mut scene.document, quick);

        let (replacements, pieces_count) = {
            let items: Vec<(Handle, &EntityType)> = handles
                .iter()
                .filter_map(|&h| scene.document.get_entity(h).map(|e| (h, e)))
                .collect();
            let per_entity = explode_batch(&items, &scene.document);
            let reps: Vec<(Handle, Vec<EntityType>)> = handles
                .into_iter()
                .zip(per_entity)
                .filter(|(_, p)| !p.is_empty())
                .collect();
            let count = reps.iter().map(|(_, p)| p.len()).sum();
            (reps, count)
        }; // `items` borrow of scene.document ends here

        let t0 = Instant::now();
        black_box(apply_explode_replacements(&mut scene, replacements));
        times.push(t0.elapsed().as_micros() as f64);
        assert!(
            pieces_count > entity_count,
            "fixture pieces ({pieces_count}) must far exceed input count ({entity_count}) — a broken explosion path returns empty lists"
        );
        if total_pieces == 0 {
            total_pieces = pieces_count; // deterministic — capture once
        }
    }

    let med = median(&times);
    let throughput = total_pieces as f64 / (med / 1_000_000.0);
    runner.record(
        name,
        "EXPLODE apply: apply_explode_replacements (erase sources + add pieces)",
        "us",
        times,
        Some((throughput, "pieces/s")),
        None,
    );
}

/// (lines, circles, polyline3d, polyline2d, texts, points, solids, rays, flat
/// lines) — full = 2,500 entities, 2,300 of which move.
fn flatten_counts(quick: bool) -> [usize; 9] {
    if quick {
        [120, 60, 80, 60, 60, 40, 40, 20, 20] // 500 total, 460 move
    } else {
        [600, 300, 400, 300, 300, 200, 200, 100, 100] // 2,500 total, 2,300 move
    }
}

fn flatten_fixture_entities(quick: bool) -> usize {
    flatten_counts(quick).iter().sum()
}

fn flatten_moved_expected(quick: bool) -> usize {
    flatten_fixture_entities(quick) - if quick { 40 } else { 200 }
}

/// Mixed-Z fixture: everything non-flat except the trailing ray + flat-line
/// groups, which FLATTEN must leave untouched (None).
fn push_flatten_fixture(doc: &mut CadDocument, quick: bool) -> Vec<Handle> {
    let [n_line, n_circle, n_pl3d, n_pl2d, n_text, n_point, n_solid, n_ray, n_flat] =
        flatten_counts(quick);
    let mut handles = Vec::with_capacity(flatten_fixture_entities(quick));

    for i in 0..n_line {
        let mut line = Line::new();
        let x = i as f64;
        line.start = Vector3::new(x, 0.0, 1.0 + (i % 5) as f64);
        line.end = Vector3::new(x + 1.0, 1.0, 2.0);
        line.thickness = 1.0;
        handles.push(doc.add_entity(EntityType::Line(line)).unwrap());
    }
    for i in 0..n_circle {
        let mut c = Circle::from_center_radius(Vector3::new(i as f64, 0.0, 1.0 + (i % 3) as f64), 2.0);
        if i % 2 == 1 {
            c.normal = Vector3::new(0.0, 0.6, 0.8); // tilted → ellipse
        }
        handles.push(doc.add_entity(EntityType::Circle(c)).unwrap());
    }
    for i in 0..n_pl3d {
        let mut pl = Polyline3D::new();
        pl.elevation = (i % 4) as f64;
        for k in 0..6 {
            pl.vertices.push(Vertex3DPolyline::new(Vector3::new(
                i as f64 + k as f64,
                k as f64,
                1.0 + (k % 3) as f64,
            )));
        }
        handles.push(doc.add_entity(EntityType::Polyline3D(pl)).unwrap());
    }
    for i in 0..n_pl2d {
        let mut pl = Polyline2D::new();
        pl.elevation = 3.0 + (i % 5) as f64;
        for k in 0..6 {
            pl.vertices.push(Vertex2D::new(Vector3::new(k as f64, i as f64, 0.0)));
        }
        handles.push(doc.add_entity(EntityType::Polyline2D(pl)).unwrap());
    }
    for i in 0..n_text {
        let mut t = Text::new();
        t.insertion_point = Vector3::new(i as f64, 0.0, 1.5);
        handles.push(doc.add_entity(EntityType::Text(t)).unwrap());
    }
    for i in 0..n_point {
        let mut p = Point::new();
        p.location = Vector3::new(i as f64, 1.0, 0.75);
        handles.push(doc.add_entity(EntityType::Point(p)).unwrap());
    }
    for i in 0..n_solid {
        let x = i as f64;
        let s = Solid::new(
            Vector3::new(x, 0.0, 1.0),
            Vector3::new(x + 1.0, 0.0, 2.0),
            Vector3::new(x + 1.0, 1.0, 3.0),
            Vector3::new(x, 1.0, 4.0),
        );
        handles.push(doc.add_entity(EntityType::Solid(s)).unwrap());
    }
    for i in 0..n_ray {
        let r = Ray::new(Vector3::new(i as f64, 0.0, 5.0), Vector3::new(0.0, 0.0, 1.0));
        handles.push(doc.add_entity(EntityType::Ray(r)).unwrap());
    }
    for i in 0..n_flat {
        let mut line = Line::new();
        let x = i as f64;
        line.start = Vector3::new(x, 0.0, 0.0);
        line.end = Vector3::new(x + 1.0, 1.0, 0.0);
        handles.push(doc.add_entity(EntityType::Line(line)).unwrap());
    }
    handles
}

/// FLATTEN plan: collect candidates + compute replacements (fresh doc/sample).
fn bench_flatten_plan(runner: &mut BenchmarkRunner) {
    let name = "flatten_plan";
    if !runner.should_run(name) {
        return;
    }
    let quick = runner.quick_mode;
    let sample_count = if quick { 3 } else { 10 };
    let entity_count = flatten_fixture_entities(quick);
    let moved_expected = flatten_moved_expected(quick);
    let mut times = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let mut scene = Scene::new();
        push_flatten_fixture(&mut scene.document, quick);

        let t0 = Instant::now();
        let candidates = black_box(flatten::collect_flatten_handles(&scene));
        let updates = black_box(flatten::plan_flatten(&scene, &candidates));
        times.push(t0.elapsed().as_micros() as f64);

        assert_eq!(candidates.len(), entity_count);
        assert_eq!(updates.len(), moved_expected);
    }

    let med = median(&times);
    let throughput = entity_count as f64 / (med / 1_000_000.0);
    runner.record(
        name,
        "FLATTEN plan: collect_flatten_handles + plan_flatten over the mixed-Z fixture",
        "us",
        times,
        Some((throughput, "entities/s")),
        None,
    );
}

/// FLATTEN apply: fresh Scene per sample, plan computed untimed, the publish
/// loop timed.
fn bench_flatten_scene_apply(runner: &mut BenchmarkRunner) {
    let name = "flatten_scene_apply";
    if !runner.should_run(name) {
        return;
    }
    let quick = runner.quick_mode;
    let sample_count = if quick { 3 } else { 10 };
    let moved_expected = flatten_moved_expected(quick);
    let mut times = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let mut scene = Scene::new();
        let handles = push_flatten_fixture(&mut scene.document, quick);
        let updates = flatten::plan_flatten(&scene, &handles);
        assert_eq!(updates.len(), moved_expected);

        let t0 = Instant::now();
        let moved = black_box(flatten::apply_flatten_updates(&mut scene, updates));
        times.push(t0.elapsed().as_micros() as f64);
        assert_eq!(moved, moved_expected);
    }

    let med = median(&times);
    let throughput = moved_expected as f64 / (med / 1_000_000.0);
    runner.record(
        name,
        "FLATTEN apply: apply_flatten_updates (per-entity update_entity publish)",
        "us",
        times,
        Some((throughput, "entities/s")),
        None,
    );
}

fn main() {
    println!("\nInitializing OpenCADStudio Command Benchmarks...");
    let mut runner = BenchmarkRunner::new(
        format!("{:^120}", "OPENCADSTUDIO COMMAND BENCHMARK REPORT"),
        "cad_command_metrics.json",
    );
    if runner.quick_mode {
        println!("Mode: QUICK (halved fixture, 3 samples per metric)");
    } else {
        println!("Mode: FULL (2,500 compounds, 10 samples per metric)");
    }

    bench_explode_geometry(&mut runner);
    bench_explode_scene_apply(&mut runner);
    bench_flatten_plan(&mut runner);
    bench_flatten_scene_apply(&mut runner);

    runner.finish();
}
