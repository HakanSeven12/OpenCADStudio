//! OpenCADStudio model import benchmark suite.
//!
//! STEP, IGES and SAT imports currently read, parse and place the model in
//! one go on the UI thread; this measures what the app is busy with while it
//! handles one import of a fixed file into a fresh drawing, through the
//! `#[doc(hidden)]` `ModelImportBenchHarness` bridge, and separately the
//! reader alone — the part a background import moves off the UI thread.
//!
//! Fixtures (deterministic, written once to the temp directory):
//!   * STEP — the synthesized AP203 tessellation shared with the tokenizer
//!     benchmark: one product whose solid is 4000 triangle faces.
//!   * SAT — a 1200-sided planar profile committed as ACIS text.
//!   * IGES — the repository's rational-spline fixture (one curve).
//!
//! Metrics, times in milliseconds per import:
//!   * `<format>_ui`    — the UI-thread time of the import.
//!   * `<format>_total` — the whole import (parse included).
//!   * `<format>_read`  — the reader alone on the same bytes.
//!
//! Sink (semantic-equivalence proof): entity and block counts, block names,
//! and each imported object's type, layer, colour and geometry (ACIS text,
//! spline control points, insert placement). Identical before/after means the
//! import leaves the drawing in the same state; a split that loses or
//! misplaces objects diverges here.
//!
//! Deterministic: fixed geometry, fixed grid coordinates, no randomness.
//! Run: `cargo bench --bench model_import`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::app::ModelImportBenchHarness;
use OpenCADStudio::io::model_import::{self, ModelFormat};

#[path = "support/step_fixture.rs"]
mod step_fixture;

const RUNS: usize = 5;
const ITERATIONS: usize = 3;
const WARMUP: usize = 1;
const SAT_EDGES: usize = 1200;

/// A planar profile of `SAT_EDGES` lines committed as ACIS text: one face
/// with as many edges and vertices.
fn sat_fixture() -> String {
    use document::entities::acis::SatDocument;
    use kernel::brep;
    use kernel::geom2d::{Curve as KCurve, Line as KLine};
    use kernel::space::Plane;

    let plane = Plane::from_axes([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let outer: Vec<KCurve> = (0..SAT_EDGES)
        .map(|edge| {
            let a = std::f64::consts::TAU * edge as f64 / SAT_EDGES as f64;
            let b = std::f64::consts::TAU * (edge + 1) as f64 / SAT_EDGES as f64;
            KCurve::Line(KLine {
                start: [10.0 * a.cos(), 10.0 * a.sin()],
                end: [10.0 * b.cos(), 10.0 * b.sin()],
            })
        })
        .collect();
    let body = brep::planar_region(plane, &[outer]).expect("planar profile");
    let mut document = SatDocument::new();
    ocs_acis::append(&body, &mut document).expect("sat append");
    document.to_sat_string()
}

fn write_fixture(name: &str, contents: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ocs-model-import-bench-{}-{name}",
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("write the benchmark fixture");
    path
}

fn report(name: &str, samples: &mut [u64], sink: u64) {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    println!(
        "{name:<16} median={:.3} ms  min={:.3} ms  max={:.3} ms  (sink={sink})",
        median as f64 / 1_000_000.0,
        samples[0] as f64 / 1_000_000.0,
        samples[samples.len() - 1] as f64 / 1_000_000.0,
    );
}

/// One import per sample through the production entry, split into the
/// UI-thread time and the whole call.
fn bench_import(
    harness: &mut ModelImportBenchHarness,
    name: &str,
    path: &str,
    format: ModelFormat,
) {
    let mut expected = None;
    for _ in 0..WARMUP {
        let (_, _, sink) = harness.import(path, format);
        expected = Some(sink);
    }
    let expected = expected.expect("warmup runs");
    let mut ui = Vec::with_capacity(RUNS * ITERATIONS);
    let mut total = Vec::with_capacity(RUNS * ITERATIONS);
    for _ in 0..RUNS * ITERATIONS {
        let (ui_ns, total_ns, sink) = harness.import(path, format);
        assert_eq!(
            sink, expected,
            "every {name} import must import the same model"
        );
        ui.push(black_box(ui_ns));
        total.push(black_box(total_ns));
    }
    report(&format!("{name}_ui"), &mut ui, expected);
    report(&format!("{name}_total"), &mut total, expected);
}

/// The reader alone on the fixture's bytes, as the background worker would
/// call it.
fn bench_read(name: &str, bytes: &[u8], format: ModelFormat, drawing_mm: Option<f64>) {
    let stem = "bench";
    let mut expected = None;
    for _ in 0..WARMUP {
        let imported = model_import::read(format, bytes, drawing_mm, stem)
            .expect("the fixture is a model the reader takes");
        expected = Some(sink_imported(&imported));
    }
    let expected = expected.expect("warmup runs");
    let mut samples = Vec::with_capacity(RUNS * ITERATIONS);
    for _ in 0..RUNS * ITERATIONS {
        let started = Instant::now();
        let imported = model_import::read(format, black_box(bytes), drawing_mm, stem)
            .expect("the fixture is a model the reader takes");
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(
            sink_imported(&imported),
            expected,
            "every {name} read must produce the same model"
        );
        samples.push(elapsed);
    }
    report(&format!("{name}_read"), &mut samples, expected);
}

/// Everything the reader produces: block names, entity count, and each
/// object's type, layer, colour and geometry.
fn sink_imported(imported: &model_import::Imported) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    imported.entities.len().hash(&mut hasher);
    let mut names: Vec<&str> = imported
        .blocks
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort();
    names.hash(&mut hasher);
    for (_, entities) in &imported.blocks {
        entities.len().hash(&mut hasher);
    }
    for entity in imported
        .blocks
        .iter()
        .flat_map(|(_, entities)| entities)
        .chain(&imported.entities)
    {
        sink_entity(entity, &mut hasher);
    }
    hasher.finish()
}

fn sink_entity(
    entity: &document::EntityType,
    hasher: &mut std::collections::hash_map::DefaultHasher,
) {
    use std::hash::Hash;
    std::mem::discriminant(entity).hash(hasher);
    entity.common().layer.hash(hasher);
    entity.common().color.index().hash(hasher);
    match entity {
        document::EntityType::Region(region) => region.acis_data.sat_data.hash(hasher),
        document::EntityType::Surface(surface) => surface.acis_data.sat_data.hash(hasher),
        document::EntityType::Spline(spline) => {
            spline.degree.hash(hasher);
            for point in &spline.control_points {
                point.x.to_bits().hash(hasher);
                point.y.to_bits().hash(hasher);
                point.z.to_bits().hash(hasher);
            }
        }
        document::EntityType::Insert(insert) => {
            insert.block_name.hash(hasher);
            insert.insert_point.x.to_bits().hash(hasher);
            insert.insert_point.y.to_bits().hash(hasher);
            insert.rotation.to_bits().hash(hasher);
        }
        _ => {}
    }
}

fn run() {
    let step = write_fixture("model.step", step_fixture::build_step().as_bytes());
    let sat = write_fixture("model.sat", sat_fixture().as_bytes());
    let iges_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/iges/rational_arc.igs"
    ))
    .expect("read the IGES fixture");
    let iges = write_fixture("model.igs", &iges_bytes);

    let mut harness = ModelImportBenchHarness::setup();
    let drawing_mm = harness.drawing_mm();
    println!(
        "model import bench (step {} faces, sat {SAT_EDGES} edges, iges 1 spline)",
        step_fixture::TRIANGLES
    );

    bench_import(&mut harness, "step", path_str(&step), ModelFormat::Step);
    bench_import(&mut harness, "sat", path_str(&sat), ModelFormat::Sat);
    bench_import(&mut harness, "iges", path_str(&iges), ModelFormat::Iges);

    let step_bytes = std::fs::read(&step).expect("read the STEP fixture");
    let sat_bytes = std::fs::read(&sat).expect("read the SAT fixture");
    bench_read("step", &step_bytes, ModelFormat::Step, drawing_mm);
    bench_read("sat", &sat_bytes, ModelFormat::Sat, drawing_mm);
    bench_read("iges", &iges_bytes, ModelFormat::Iges, drawing_mm);

    let _ = std::fs::remove_file(&step);
    let _ = std::fs::remove_file(&sat);
    let _ = std::fs::remove_file(&iges);
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().expect("the fixture path is UTF-8")
}

fn main() {
    std::thread::Builder::new()
        .name("model_import".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run)
        .expect("spawn bench thread")
        .join()
        .expect("bench thread panicked");
}
