//! Standalone companion to the opt-in release audit. It compiles the actual
//! PERF sink source and isolates changed-state formatting/enqueue costs.
#![allow(dead_code)]

#[path = "../../src/perf/audit.rs"]
mod audit;
#[path = "../../src/perf.rs"]
mod perf;

#[cfg(target_os = "windows")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(serde::Serialize, serde::Deserialize)]
struct Counts {
    slots: usize,
    shadow: u64,
    render_targets: u64,
    text_atlas: u64,
    wire_arena: u64,
    block_geometry: u64,
    wires: u64,
    meshes: u64,
    images: u64,
    materials: u64,
    text: u64,
    overlays: u64,
    hatches: u64,
    point_clouds: u64,
    other: u64,
}

impl Counts {
    fn partial(&self) -> u64 {
        self.shadow + self.render_targets + self.text_atlas + self.wire_arena + self.block_geometry
    }
    fn total(&self) -> u64 {
        self.partial()
            + self.wires
            + self.meshes
            + self.images
            + self.materials
            + self.text
            + self.overlays
            + self.hatches
            + self.point_clouds
            + self.other
    }
}

fn human(now: &Counts, total: u64) {
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    perf::record(format_args!(
        "[perf] gpu-live slots={} total={:.1}MiB shadow={:.1} targets={:.1} \
text_atlas={:.1} wire_arena={:.1} block_geometry={:.1}",
        now.slots,
        mib(total),
        mib(now.shadow),
        mib(now.render_targets),
        mib(now.text_atlas),
        mib(now.wire_arena),
        mib(now.block_geometry),
    ));
}

fn updated(now: &Counts) {
    human(now, now.total());
    perf::record(format_args!(
        "[perf] gpu-live-json {}",
        serde_json::json!({
            "schema": 1,
            "scope": "logical live resources; excludes driver heaps/alignment, staging, iced and ViewCube; Depth24Plus estimated",
            "total_bytes": now.total(), "categories": now,
        })
    ));
}

fn sample(mut log: impl FnMut()) -> f64 {
    // Keep the background stderr queue from becoming a throughput benchmark.
    std::thread::sleep(std::time::Duration::from_millis(1));
    let start = std::time::Instant::now();
    for _ in 0..5 {
        log();
    }
    start.elapsed().as_secs_f64() * 1e6 / 5.0
}

fn main() {
    let path = std::env::args().nth(1).expect("input JSON snapshot");
    let input: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let counts: Counts = serde_json::from_value(input["categories"].clone()).unwrap();
    perf::set_ui_enabled(true);
    // Match a running PERF panel whose fixed-size history has filled up.
    for _ in 0..2100 {
        updated(&counts);
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let timing = audit::paired(
        200,
        || sample(|| human(std::hint::black_box(&counts), counts.partial())),
        || sample(|| updated(std::hint::black_box(&counts))),
    );
    audit::write_report(
        "gpu-logging",
        serde_json::json!({
            "scope":"changed-state formatting and enqueue component using the actual PERF sink source; full UI history and drained stderr queue; excludes GPU collection, change detection, background stderr drain time and PERF widget rendering; native optimized helper with production allocator",
            "input": input, "timing": timing,
        }),
    );
}
