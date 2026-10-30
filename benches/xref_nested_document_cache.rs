//! OpenCADStudio nested-xref document cache benchmark suite.
//!
//! Full deep clone vs `Arc` sharing.
//!
//! `collect_entries_with_prev` (`src/io/xref.rs`) walks the nested-xref work
//! queue with a `HashMap<String, Document>`: on a cache miss it stored a
//! FULL deep clone of the freshly parsed document while keeping the original,
//! and on every cache hit it deep-cloned the document again — O(document
//! size) memory plus an O(document size) copy per access. The fix stores
//! `Arc<Document>` and clones only the `Arc`.
//!
//! End-to-end DWG nested-xref loading is hard to synthesize faithfully, so
//! this bench isolates the cache operations themselves at the function
//! level: a representative `Document` (mixed lines/circles/polylines plus
//! block records) is built once, and each timed cycle models one cache miss
//! (insert) followed by K hits, then drops the cache — the exact access
//! pattern of the nested enumeration loop, minus file I/O. The simulated
//! `load_file` output (one clone of the shared fixture) is timed identically
//! in both paths as stand-in overhead for the real parse, so the OLD/NEW
//! ratio understates the win: the absolute per-op delta is the removed
//! clone cost. Reads per access are identical in both paths
//! (`entities().count()` through `black_box`), so that delta is purely the
//! removed deep clones (store + hits + their drops). The sinks are
//! deterministic, so baseline and post-fix runs must print identical values.
//!
//! Run: `cargo bench --bench xref_nested_document_cache`

use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use codec::entities::{Circle, Line, LwPolyline, LwVertex};
use codec::tables::BlockRecord;
use codec::types::{Vector2, Vector3};
use codec::{Document, EntityType};

const DOC_ENTITIES: usize = 4_000;
const BLOCK_RECORDS: usize = 24;
const CACHE_KEY: &str = "C:/drawings/nested_xref.dwg";
/// K cache hits per miss — one work-queue pop per nested path per cycle.
const HITS: usize = 8;
const RUNS: usize = 7;
/// Path A deep-clones ~(HITS + 2) documents per op; few iterations suffice.
const ITERATIONS_A: usize = 8;
const WARMUP_A: usize = 1;
/// Path B is sub-microsecond per op; many more iterations for a stable clock.
const ITERATIONS_B: usize = 20_000;
const WARMUP_B: usize = 2_000;

/// Representative nested-xref drawing: mostly 2D geometry plus a handful of
/// block records — the two regions the nested enumeration loop reads.
fn synthetic_document() -> Document {
    let mut doc = Document::new();
    for i in 0..DOC_ENTITIES {
        let x = (i % 200) as f64 * 10.0;
        let y = (i / 200) as f64 * 10.0;
        match i % 3 {
            0 => {
                let mut line = Line::new();
                line.start = Vector3::new(x, y, 0.0);
                line.end = Vector3::new(x + 8.0, y + 6.0, 0.0);
                doc.add_entity(EntityType::Line(line)).unwrap();
            }
            1 => {
                let mut circle = Circle::default();
                circle.center = Vector3::new(x, y, 0.0);
                circle.radius = 4.0;
                doc.add_entity(EntityType::Circle(circle)).unwrap();
            }
            _ => {
                let mut pl = LwPolyline::new();
                pl.vertices = vec![
                    LwVertex::new(Vector2::new(x, y)),
                    LwVertex::new(Vector2::new(x + 10.0, y)),
                    LwVertex::new(Vector2::new(x + 10.0, y + 10.0)),
                    LwVertex::new(Vector2::new(x, y + 10.0)),
                ];
                doc.add_entity(EntityType::LwPolyline(pl)).unwrap();
            }
        }
    }
    for i in 0..BLOCK_RECORDS {
        let mut block = BlockRecord::new(format!("NESTED_BLOCK_{i}"));
        block.handle = doc.allocate_handle();
        block.xref_path = format!("nested_{i}.dwg");
        doc.block_records.add(block).unwrap();
    }
    doc
}

/// The read the nested enumeration loop performs on each cached document,
/// used as the document-identity sink so no access can be elided.
fn entity_count(doc: &Document) -> usize {
    black_box(doc.entities().count())
}

/// The removed production pattern (xref.rs nested work queue): store a full
/// deep clone of the freshly loaded document on miss, deep-clone it again on
/// every hit, drop everything at cycle end.
fn old_ops(loaded: Document, hits: usize) -> usize {
    let mut cache: HashMap<String, Document> = HashMap::default();
    cache.insert(CACHE_KEY.to_owned(), loaded.clone());
    let mut sum = entity_count(&loaded);
    for _ in 0..hits {
        let hit = cache.get(CACHE_KEY).expect("cached").clone();
        sum += entity_count(&hit);
    }
    sum
}

/// The fixed pattern: move the freshly loaded document into an `Arc`, store
/// an `Arc` clone on miss, `Arc`-clone on every hit — no deep copy anywhere.
fn new_ops(loaded: Document, hits: usize) -> usize {
    let mut cache: HashMap<String, Arc<Document>> = HashMap::default();
    let doc = Arc::new(loaded);
    cache.insert(CACHE_KEY.to_owned(), Arc::clone(&doc));
    let mut sum = entity_count(&doc);
    for _ in 0..hits {
        let hit = Arc::clone(cache.get(CACHE_KEY).expect("cached"));
        sum += entity_count(&hit);
    }
    sum
}

/// Median ms/op over `RUNS` samples. Each op is one cycle: simulated load
/// (timed identically in both paths), cache miss + insert, `HITS` hits,
/// reads, and the drops. The accumulated per-op sums escape into the printed
/// `sink`, so neither path can be optimized away.
fn measure(
    label: &str,
    source: &Document,
    iterations: usize,
    warmup: usize,
    mut op: impl Fn(Document) -> usize,
) -> f64 {
    let mut sink = 0usize;
    for _ in 0..warmup {
        let loaded = source.clone();
        sink = sink.wrapping_add(op(loaded));
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..iterations {
            let loaded = source.clone();
            sink = sink.wrapping_add(op(loaded));
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!("  {label} median {median:.6} ms/op  samples {samples:?}  sink {sink}");
    median
}

fn main() {
    let source = synthetic_document();
    println!(
        "nested-xref document cache: {DOC_ENTITIES} entities + {BLOCK_RECORDS} block records per Document, {HITS} cache hits per miss"
    );

    let a_median = measure(
        "OLD (HashMap<String, Document> deep clone):",
        &source,
        ITERATIONS_A,
        WARMUP_A,
        |loaded| old_ops(loaded, HITS),
    );
    let b_median = measure(
        "NEW (HashMap<String, Arc<Document>>):      ",
        &source,
        ITERATIONS_B,
        WARMUP_B,
        |loaded| new_ops(loaded, HITS),
    );

    println!(
        "  ratio OLD/NEW: {:.1}x (removed per-cycle clone cost)",
        a_median / b_median
    );
}
