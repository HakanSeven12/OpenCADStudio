//! OpenCADStudio properties selection aggregation benchmark suite.
//!
//! Aggregating an N-entity selection used to cost O(N × sections × props²)
//! with per-entity i18n plus clone churn.
//!
//! Measures the REAL selection-rebuild aggregation entry production calls:
//! `aggregate_sections` (reached through the `#[doc(hidden)]`
//! `PropSection::aggregate_selection` bridge), exactly as the selection-rebuild
//! block in `src/app/properties.rs::refresh_properties` invokes it for a
//! multi-selection: identity working plane (no UCS transform), document-free
//! rows, `text_style_names` collected from the document's text-style table.
//!
//! Fixture: a Scene of Lines/Circles carrying varied layers / linetypes /
//! colors / line weights, so the merge sees both identical rows (kept as-is)
//! and disagreeing rows (collapsed to the "Varies" placeholder).
//!
//! Scenarios:
//!   (a) 500 identical lines — worst case for clone churn (nothing drops out
//!       of the intersection, every row survives every merge step)
//!   (b) 500 mixed lines/circles — varied layer/color/linetype/lineweight plus
//!       type-mixed selections (Line-specific rows drop against circles)
//!   (c) 2000 mixed lines/circles — same generator, 4x bulk (just under the
//!       `MAX_PROP_AGGREGATE = 2_000` count-only cap)
//!
//! Per scenario the bench additionally times a build-only pass
//! (`dispatch::properties_sectioned` per entity, no merge) to split the
//! aggregate into its build and merge phases. That split is diagnostic only;
//! it never feeds the sink.
//!
//! Sink (semantic-equivalence proof): total merged property count plus an
//! FNV-1a checksum over every merged row's section title, field name, and
//! Debug-rendered value, accumulated across every timed call — identical
//! before/after the fix means identical merged output (equal work both runs).
//!
//! Deterministic: fixed geometry, fixed attribute cycles, no randomness.
//! Run: `cargo bench --bench properties_selection_aggregation`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{Circle, EntityType, Line};
use document::types::{Color, Handle, LineWeight, Vector3};
use OpenCADStudio::scene::model::object::PropSection;
use OpenCADStudio::scene::view::dispatch;
use OpenCADStudio::scene::Scene;

const RUNS: usize = 5;
const ITERATIONS: usize = 5;
const WARMUP: usize = 1;

const LAYERS: [&str; 4] = ["0", "WALLS", "DOORS", "GLAZ"];
const LINETYPES: [&str; 3] = ["", "DASHED", "CONTINUOUS"];
const COLORS: [Color; 5] = [
    Color::ByLayer,
    Color::Index(1),
    Color::Index(3),
    Color::Index(5),
    Color::Rgb {
        r: 12,
        g: 200,
        b: 90,
    },
];
const LINEWEIGHTS: [LineWeight; 4] = [
    LineWeight::ByLayer,
    LineWeight::Default,
    LineWeight::Value(25),
    LineWeight::Value(50),
];

/// A line with fixed geometry and fixed common attributes (`identical`), or a
/// line whose layer/color/linetype/lineweight/geometry cycle with `i`.
fn line_entity(i: usize, identical: bool) -> EntityType {
    let mut line = Line::new();
    let x = if identical { 1.0 } else { i as f64 };
    line.start = Vector3::new(x, 2.0, 0.0);
    line.end = Vector3::new(x + 10.0, 7.0, 0.0);
    let mut entity = EntityType::Line(line);
    let common = entity.common_mut();
    common.layer = LAYERS[if identical { 1 } else { i % LAYERS.len() }].to_string();
    common.color = COLORS[if identical { 0 } else { i % COLORS.len() }];
    common.linetype =
        LINETYPES[if identical { 0 } else { (i / 3) % LINETYPES.len() }].to_string();
    common.line_weight =
        LINEWEIGHTS[if identical { 0 } else { (i / 5) % LINEWEIGHTS.len() }];
    entity
}

/// A circle cycling the same varied common attributes with `i`.
fn circle_entity(i: usize) -> EntityType {
    let mut circle = Circle::default();
    circle.center = Vector3::new((i % 97) as f64, (i % 13) as f64, 0.0);
    circle.radius = 4.0 + (i % 7) as f64;
    let mut entity = EntityType::Circle(circle);
    let common = entity.common_mut();
    common.layer = LAYERS[i % LAYERS.len()].to_string();
    common.color = COLORS[i % COLORS.len()];
    common.linetype = LINETYPES[(i / 3) % LINETYPES.len()].to_string();
    common.line_weight = LINEWEIGHTS[(i / 5) % LINEWEIGHTS.len()];
    entity
}

fn scenario_entity(i: usize, identical: bool) -> EntityType {
    if identical {
        line_entity(0, true)
    } else if i % 4 == 3 {
        circle_entity(i)
    } else {
        line_entity(i, false)
    }
}

/// (rows, fnv1a) over every section title and `field=Debug(value)` row.
fn sink_of(sections: &[PropSection]) -> (u64, u64) {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut rows = 0u64;
    let mut mix = |hash: &mut u64, bytes: &[u8]| {
        for byte in bytes {
            *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for section in sections {
        mix(&mut hash, section.title.as_bytes());
        mix(&mut hash, &[0xff]);
        for prop in &section.props {
            rows += 1;
            let rendered = format!("{}={:?}", prop.field, prop.value);
            mix(&mut hash, rendered.as_bytes());
            mix(&mut hash, &[0xfe]);
        }
    }
    (rows, hash)
}

fn run_scenario(label: &str, n: usize, identical: bool) {
    // Ingest through the same `Scene::add_entity` path production uses.
    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(n);
    for i in 0..n {
        handles.push(scene.add_entity(scenario_entity(i, identical)));
    }
    let text_style_names: Vec<String> = scene
        .document
        .text_styles
        .iter()
        .map(|style| style.name.clone())
        .collect();
    // Production's `selected_entities()` yields `(handle, &EntityType)` pairs
    // borrowed straight out of `scene.document`; this mirrors that list.
    let selected: Vec<(Handle, &EntityType)> = handles
        .iter()
        .map(|&handle| (handle, scene.document.get_entity(handle).expect("committed entity")))
        .collect();

    let mut rows_acc = 0u64;
    let mut hash_acc = 0u64;
    let mut samples = Vec::with_capacity(RUNS);
    let calls = WARMUP + RUNS * ITERATIONS;

    for _ in 0..WARMUP {
        let sections =
            PropSection::aggregate_selection(black_box(&selected), black_box(&text_style_names));
        let (rows, hash) = sink_of(&sections);
        rows_acc = rows_acc.wrapping_add(rows);
        hash_acc = hash_acc.wrapping_add(hash);
    }
    for _ in 0..RUNS {
        let mut acc_ms = 0.0;
        for _ in 0..ITERATIONS {
            let start = Instant::now();
            let sections = PropSection::aggregate_selection(
                black_box(&selected),
                black_box(&text_style_names),
            );
            acc_ms += start.elapsed().as_secs_f64() * 1000.0;
            let (rows, hash) = sink_of(&sections);
            rows_acc = rows_acc.wrapping_add(rows);
            hash_acc = hash_acc.wrapping_add(hash);
        }
        samples.push(acc_ms / ITERATIONS as f64);
    }

    // Diagnostic phase split: build every entity's rows without merging.
    let start = Instant::now();
    let mut build_rows = 0usize;
    for (handle, entity) in &selected {
        let sections = dispatch::properties_sectioned(*handle, entity, &text_style_names);
        build_rows += sections
            .iter()
            .map(|section| section.props.len())
            .sum::<usize>();
    }
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;

    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!(
        "  {label}: aggregate median {median:.4} ms/call  samples {samples:?}  \
build-only pass {build_ms:.4} ms ({build_rows} pre-merge rows)"
    );
    println!("    sink: {rows_acc} merged rows over {calls} calls  hash {hash_acc:#018x}");
}

fn main() {
    println!(
        "properties selection aggregation: production entry `aggregate_sections` \
(`PropSection::aggregate_selection` bridge), {RUNS}x{ITERATIONS} calls/sample + {WARMUP} warmup"
    );
    run_scenario("(a) 500 identical lines      ", 500, true);
    run_scenario("(b) 500 mixed lines/circles  ", 500, false);
    run_scenario("(c) 2000 mixed lines/circles ", 2000, false);
}
