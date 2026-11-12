//! Allocation probe for the exact synthetic text workload in text_shaping_geometry.
#[allow(dead_code)]
#[path = "text_shaping_geometry.rs"]
mod workload;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use sha2::{Digest, Sha256};
use OpenCADStudio::scene::text::{font_face::Face, sysfont, ttf_glyph};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);

fn add_live(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            add_live(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            add_live(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(p, layout, size) };
        if !result.is_null() {
            REALLOCS.fetch_add(1, Ordering::Relaxed);
            if size >= layout.size() {
                add_live(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut font = None;
    let mut fixture_name = String::from("drawing");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--font" => font = Some(args.next().expect("--font requires a family")),
            "--fixture" => fixture_name = args.next().expect("--fixture requires a name"),
            "--self-test" => {
                assert!(
                    font.is_none() && fixture_name == "drawing" && args.next().is_none(),
                    "--self-test must be used alone"
                );
                let before = LIVE.load(Ordering::Relaxed);
                let mut bytes = Vec::<u8>::with_capacity(32_768);
                black_box(&mut bytes);
                let after = LIVE.load(Ordering::Relaxed);
                assert!(after >= before + bytes.capacity());
                let prior_reallocs = REALLOCS.load(Ordering::Relaxed);
                bytes.reserve_exact(bytes.capacity() * 2);
                black_box(&mut bytes);
                assert!(REALLOCS.load(Ordering::Relaxed) > prior_reallocs);
                let before_drop = LIVE.load(Ordering::Relaxed);
                let capacity = bytes.capacity();
                drop(bytes);
                assert!(LIVE.load(Ordering::Relaxed) + capacity <= before_drop);
                assert!(PEAK.load(Ordering::Relaxed) >= after);
                println!("allocation, reallocation, peak and free accounting passed");
                return;
            }
            _ => panic!("usage: text_shaping_memory [--font FAMILY] [--fixture drawing|varied]"),
        }
    }
    let families = sysfont::families();
    let font = match font {
        Some(name) => families
            .into_iter()
            .find(|f| f.eq_ignore_ascii_case(&name))
            .expect("font not installed"),
        None => match families
            .into_iter()
            .find(|f| matches!(Face::resolve(f), Face::Ttf { .. }))
        {
            Some(name) => name,
            None => {
                eprintln!("SKIP: no installed TrueType font family");
                return;
            }
        },
    };
    assert!(
        matches!(Face::resolve(&font), Face::Ttf { .. }),
        "font must resolve to TrueType"
    );
    let (font_hash, face_index) = sysfont::with_face_data(&font, |bytes, index| {
        (format!("{:x}", Sha256::digest(bytes)), index)
    })
    .expect("font data");
    let fixture = workload::select_fixture(&fixture_name);
    let fixture_hash = workload::fixture_digest(&fixture);
    assert!(ttf_glyph::shape_run(&font, "warmup").is_some());
    ttf_glyph::clear_fallback_cache();
    let baseline = LIVE.load(Ordering::SeqCst);
    let calls = CALLS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    let reallocs = REALLOCS.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::SeqCst);
    let geometry = black_box(workload::tessellate_drawing(&font, &fixture));
    let live_geometry = LIVE.load(Ordering::SeqCst);
    let peak_geometry = PEAK.load(Ordering::SeqCst);
    let allocation_calls = CALLS.load(Ordering::Relaxed) - calls;
    let allocation_bytes = BYTES.load(Ordering::Relaxed) - bytes;
    let reallocation_calls = REALLOCS.load(Ordering::Relaxed) - reallocs;
    let digest = workload::geometry_digest(&geometry);
    drop(geometry);
    let after_drop = LIVE.load(Ordering::SeqCst);
    ttf_glyph::clear_fallback_cache();
    let after_clear = LIVE.load(Ordering::SeqCst);
    let report = json!({"benchmark":"text_shaping_memory", "schema":1, "revision":option_env!("OCS_GIT_REV"), "os":std::env::consts::OS, "arch":std::env::consts::ARCH, "profile":if cfg!(debug_assertions) {"debug"} else {"release"}, "font":font, "font_sha256":font_hash, "face_index":face_index, "fixture":fixture_name, "fixture_sha256":fixture_hash, "geometry_sha256":digest, "baseline_live_bytes":baseline, "live_with_geometry_bytes":live_geometry, "peak_live_bytes":peak_geometry, "live_after_drop_bytes":after_drop, "live_after_clear_bytes":after_clear, "allocation_calls":allocation_calls, "allocation_requested_bytes":allocation_bytes, "reallocation_calls":reallocation_calls, "scope":"Rust System allocator requested bytes; excludes allocator overhead, foreign allocations, GUI, GPU and child processes; counts exclude realloc sizes"});
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
