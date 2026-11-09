//! Cross-platform Rust-allocation probe for the native drawing-open pipeline.
//!
//! Run a single fresh process with the same drawing each time:
//! `cargo bench --bench drawing_open_memory -- --input /path/to/drawing.dxf --expected-texts 10000`
//! The counting allocator adds overhead. Its requested-byte totals exclude the
//! allocator's overhead, foreign allocations, the GUI, GPU and child processes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use document::entities::EntityType;
use serde::Serialize;
use sha2::{Digest, Sha256};
use OpenCADStudio::scene::model::wire_model::{BgAdaptInputs, SearchableTextRun, WireModel};
use OpenCADStudio::scene::pipeline::text_gpu::TextVertex;

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static REALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);

fn add_live(bytes: usize) {
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            add_live(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            add_live(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            REALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            if new_size >= layout.size() {
                add_live(new_size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy, Serialize)]
struct Snapshot {
    live_requested_bytes: usize,
    peak_live_requested_bytes: usize,
    allocation_calls: usize,
    allocated_requested_bytes: usize,
    reallocation_calls: usize,
}

fn snapshot() -> Snapshot {
    Snapshot {
        live_requested_bytes: LIVE.load(Ordering::Relaxed),
        peak_live_requested_bytes: PEAK.load(Ordering::Relaxed),
        allocation_calls: ALLOC_CALLS.load(Ordering::Relaxed),
        allocated_requested_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
        reallocation_calls: REALLOC_CALLS.load(Ordering::Relaxed),
    }
}

fn allocator_self_test() {
    let before = snapshot();
    let mut data = Vec::<u8>::with_capacity(32_768);
    let initial_capacity = data.capacity();
    black_box(&mut data);
    let after_alloc = snapshot();
    assert!(after_alloc.live_requested_bytes >= before.live_requested_bytes + initial_capacity);
    data.reserve_exact(initial_capacity * 2);
    black_box(&mut data);
    let grown_capacity = data.capacity();
    let before_drop = snapshot();
    assert!(before_drop.reallocation_calls > after_alloc.reallocation_calls);
    assert!(
        before_drop.live_requested_bytes
            >= after_alloc.live_requested_bytes + grown_capacity - initial_capacity
    );
    assert!(before_drop.peak_live_requested_bytes >= before_drop.live_requested_bytes);
    drop(data);
    assert!(snapshot().live_requested_bytes + grown_capacity <= before_drop.live_requested_bytes);
    println!("allocation, reallocation, peak and free accounting passed");
}

#[derive(Serialize)]
struct Stage {
    name: &'static str,
    elapsed_ms: f64,
    snapshot: Snapshot,
}

fn record(stages: &mut Vec<Stage>, name: &'static str, start: Instant) {
    let measured = snapshot();
    stages.push(Stage {
        name,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        snapshot: measured,
    });
}

#[derive(Default, Serialize)]
struct WireMemory {
    wire_models: usize,
    wire_vector_capacity_bytes: usize,
    names_capacity_bytes: usize,
    text_vertices: usize,
    text_vertex_capacity_bytes: usize,
    background_adapt_boxes_bytes: usize,
    background_text_color_capacity_bytes: usize,
    searchable_run_capacity_bytes: usize,
    searchable_string_capacity_bytes: usize,
}

fn wire_memory(wires: &[WireModel], capacity: usize) -> WireMemory {
    let mut out = WireMemory {
        wire_models: wires.len(),
        wire_vector_capacity_bytes: capacity * size_of::<WireModel>(),
        ..Default::default()
    };
    for wire in wires {
        out.names_capacity_bytes += wire.name.capacity();
        out.text_vertices += wire.text_verts.len();
        out.text_vertex_capacity_bytes += wire.text_verts.capacity() * size_of::<TextVertex>();
        if let Some(bg) = &wire.bg_adapt {
            out.background_adapt_boxes_bytes += size_of::<BgAdaptInputs>();
            out.background_text_color_capacity_bytes +=
                bg.text_raw_colors.capacity() * size_of::<[f32; 4]>();
        }
        out.searchable_run_capacity_bytes +=
            wire.searchable_text.capacity() * size_of::<SearchableTextRun>();
        for run in &wire.searchable_text {
            out.searchable_string_capacity_bytes += run.text.capacity() + run.font.capacity();
        }
    }
    out
}

fn sha256(path: &Path) -> String {
    use std::io::Read;
    let mut file = std::fs::File::open(path).expect("open input for hashing");
    let mut hash = Sha256::new();
    let mut buf = [0_u8; 65536];
    loop {
        let n = file.read(&mut buf).expect("read input for hashing");
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    format!("{:x}", hash.finalize())
}

fn main() {
    // PERF starts a logging thread and retains diagnostic strings; keep those
    // allocations out of the measured open pipeline in this process.
    std::env::remove_var("PERF");
    let mut args = std::env::args_os().skip(1);
    let mut input = None::<PathBuf>;
    let mut expected = None::<usize>;
    let mut output = None::<PathBuf>;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--bench") => (),
            Some("--self-test") => {
                assert!(
                    input.is_none()
                        && expected.is_none()
                        && output.is_none()
                        && args.next().is_none(),
                    "--self-test must be used alone"
                );
                allocator_self_test();
                return;
            }
            Some("--input") => input = Some(args.next().expect("--input requires a path").into()),
            Some("--expected-texts") => {
                expected = Some(
                    args.next()
                        .expect("--expected-texts requires a count")
                        .to_string_lossy()
                        .parse()
                        .expect("valid text count"),
                )
            }
            Some("--output") => {
                output = Some(args.next().expect("--output requires a path").into())
            }
            _ => panic!(
                "usage: drawing_open_memory --input PATH [--expected-texts N] [--output REPORT]"
            ),
        }
    }
    let input = input
        .expect("--input is required")
        .canonicalize()
        .expect("input exists");
    let initial_metadata = std::fs::metadata(&input).expect("input metadata");
    if let Some(path) = &output {
        assert!(!path.exists(), "output must be a new file");
    }

    // Reserve bookkeeping before the baseline; reporting and hashing happen
    // only after all snapshots. Global caches and worker pools remain live.
    let mut stages = Vec::with_capacity(8);
    let start = Instant::now();
    record(&mut stages, "baseline", start);
    let start = Instant::now();
    let mut doc = OpenCADStudio::io::load_file(&input).expect("load drawing");
    record(&mut stages, "parse", start);
    let mut total = 0;
    let mut text = 0;
    for entity in doc.entities() {
        total += 1;
        if matches!(entity, EntityType::Text(_)) {
            text += 1;
        }
    }
    if let Some(expected) = expected {
        assert_eq!(text, expected, "fixture TEXT count changed");
    }
    let base = input.parent().expect("input has a parent");
    let start = Instant::now();
    OpenCADStudio::io::linetypes::repair_document(&mut doc);
    OpenCADStudio::io::xref::register_underlay_sources(&doc, base);
    let xrefs = OpenCADStudio::io::xref::resolve_xrefs(&mut doc, base);
    record(&mut stages, "linetypes_and_xrefs", start);
    let start = Instant::now();
    let mut caches =
        OpenCADStudio::scene::build_derived_caches_with_progress(&doc, &|_| {}, Some(base));
    record(&mut stages, "derived_caches", start);
    let start = Instant::now();
    let (doc, geometry) = OpenCADStudio::scene::prepare_open_geometry(
        doc,
        &mut caches,
        [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0],
    );
    record(&mut stages, "prepared_geometry", start);
    let memory = wire_memory(&geometry.wires, geometry.wires.capacity());
    let atlas_bytes = {
        let atlas = OpenCADStudio::scene::text::sdf_atlas::text_atlas()
            .lock()
            .unwrap();
        atlas.data().len()
    };
    black_box((&doc, &caches, &geometry, &xrefs));
    let start = Instant::now();
    std::thread::sleep(std::time::Duration::from_secs(2));
    record(&mut stages, "settled_2s", start);
    let start = Instant::now();
    drop((doc, caches, geometry, xrefs));
    record(&mut stages, "after_drop", start);
    let start = Instant::now();
    OpenCADStudio::scene::text::ttf_glyph::clear_fallback_cache();
    record(&mut stages, "after_clear_shaping_and_fallback", start);

    let hash = sha256(&input);
    let final_metadata = std::fs::metadata(&input).expect("input metadata after measurement");
    assert_eq!(
        (initial_metadata.len(), initial_metadata.modified().ok()),
        (final_metadata.len(), final_metadata.modified().ok()),
        "input changed during measurement"
    );
    let report = serde_json::json!({
        "schema": 1, "benchmark": "drawing_open_memory", "input": {"path": input, "bytes": initial_metadata.len(), "sha256": hash},
        "revision": env!("OCS_GIT_REV"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "profile": if cfg!(debug_assertions) {"debug"} else {"release"},
        "resident_memo_enabled": std::env::var_os("OCS_NO_RESIDENT_MEMO").is_none(),
        "model_background": [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0],
        "text_entities": text, "total_entities": total, "wire_memory": memory, "cpu_text_atlas_bytes": atlas_bytes, "stages": stages,
        "scope": "Rust System allocator requested bytes only; excludes allocator overhead, foreign allocations, GUI, GPU and child processes; timings include instrumentation overhead"
    });
    let rendered = serde_json::to_string_pretty(&report).expect("serialize report");
    if let Some(path) = output {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create new report");
        file.write_all(rendered.as_bytes()).expect("write report");
    }
    println!("{rendered}");
}
