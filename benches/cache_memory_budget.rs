//! Deterministic resident-memory probe for the raster/underlay caches.
//!
//! Each scenario runs in a fresh process (the driver re-executes this binary
//! with `--scenario`), so the process-wide static caches start empty on every
//! run and no scenario can contaminate another. Workloads are fixed synthetic
//! inputs allocated in a fixed order: the workload must stay byte-identical
//! before and after an optimization so the numbers remain comparable.
//!
//! Run everything: `cargo bench --bench cache_memory_budget`
//! One scenario:  `cargo bench --bench cache_memory_budget -- --scenario pdf_underlay_zoom`
//!
//! The counting allocator matches `drawing_open_memory`: requested bytes only,
//! no allocator overhead, no GPU memory. `live_bytes` after a step is what the
//! process still holds — with the frame's own handles dropped, that is the
//! caches' retention plus fixture and process overhead (see the baseline).

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use document::entities::{Line, RasterImage};
use document::objects::{Material, MaterialMap, ObjectType};
use document::types::Vector3;
use document::{Document, EntityType, Handle};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use OpenCADStudio::scene::model::image_model::{self, ImageModel};
use OpenCADStudio::scene::model::material_model;
use OpenCADStudio::scene::model::pdf_raster::{self, PageAdjust};
use OpenCADStudio::scene::model::point_cloud;

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn add_live(bytes: usize) {
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add_live(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
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

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn allocator_self_test() {
    let before = live();
    let mut data = Vec::<u8>::with_capacity(32_768);
    let capacity = data.capacity();
    black_box(&mut data);
    assert!(live() >= before + capacity);
    drop(data);
    assert!(live() <= before);
}

#[derive(Clone, Serialize, Deserialize)]
struct Step {
    label: String,
    live_bytes: usize,
    elapsed_ms: f64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Report {
    schema: u32,
    revision: String,
    os: String,
    arch: String,
    profile: String,
    logical_cpus: usize,
    workload_sha256: String,
    cache_budgets_bytes: BTreeMap<String, usize>,
    scenario: String,
    steps: Vec<Step>,
    baseline_live_bytes: usize,
    setup_peak_live_bytes: usize,
    setup_ms: f64,
    /// High-water mark reset after fixture and measurement storage setup.
    peak_live_bytes: usize,
    final_live_bytes: usize,
    retained_bytes: usize,
    elapsed_ms: f64,
}

/// Allocate all labels and sample storage before the baseline. Recording a
/// sample must not grow the heap and masquerade as cache retention.
struct Probe {
    steps: Vec<Step>,
    next: usize,
    baseline: usize,
    setup_peak: usize,
    setup_ms: f64,
}

impl Probe {
    fn new(labels: Vec<String>, setup: Instant) -> Self {
        let steps = labels
            .into_iter()
            .map(|label| Step {
                label,
                live_bytes: 0,
                elapsed_ms: 0.0,
            })
            .collect();
        let baseline = live();
        let setup_peak = PEAK.swap(baseline, Ordering::Relaxed);
        Self {
            steps,
            next: 0,
            baseline,
            setup_peak,
            setup_ms: setup.elapsed().as_secs_f64() * 1000.0,
        }
    }

    fn measure(&mut self, operation: impl FnOnce()) {
        let started = Instant::now();
        operation();
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let live_bytes = live();
        self.steps[self.next].live_bytes = live_bytes;
        self.steps[self.next].elapsed_ms = elapsed_ms;
        self.next += 1;
    }

    fn finish(self, scenario: &str, workload: Sha256) -> Report {
        assert_eq!(self.next, self.steps.len());
        // Snapshot before allocating report metadata or encoding JSON.
        let final_live_bytes = live();
        let peak_live_bytes = PEAK.load(Ordering::Relaxed);
        Report {
            schema: 2,
            revision: env!("OCS_GIT_REV").into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .into(),
            logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
            workload_sha256: format!("{:x}", workload.finalize()),
            cache_budgets_bytes: cache_budgets(),
            scenario: scenario.into(),
            baseline_live_bytes: self.baseline,
            setup_peak_live_bytes: self.setup_peak,
            setup_ms: self.setup_ms,
            peak_live_bytes,
            final_live_bytes,
            retained_bytes: final_live_bytes.saturating_sub(self.baseline),
            elapsed_ms: self.steps.iter().map(|step| step.elapsed_ms).sum(),
            steps: self.steps,
        }
    }
}

fn cache_budgets() -> BTreeMap<String, usize> {
    [
        "OCS_PDF_RASTER_CACHE_MB",
        "OCS_IMAGE_CACHE_MB",
        "OCS_MATERIAL_CACHE_MB",
        "OCS_POINTCLOUD_CACHE_MB",
    ]
    .into_iter()
    .map(|name| {
        let mib = std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(64);
        (name.into(), mib.saturating_mul(1024 * 1024))
    })
    .collect()
}

fn fixture_hash(specification: &str) -> Sha256 {
    let mut digest = Sha256::new();
    hash_part(&mut digest, specification.as_bytes());
    digest
}

fn hash_part(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn accesses(count: usize) -> Vec<(&'static str, usize)> {
    let mut requests: Vec<_> = ["load", "revisit"]
        .into_iter()
        .flat_map(|pass| (0..count).map(move |i| (pass, i)))
        .collect();
    // Revisit the most recently loaded four entries with realistic locality,
    // separately from the larger-than-budget sequential sweep.
    for _ in 0..3 {
        requests.extend((count.saturating_sub(4)..count).map(|i| ("hot", i)));
    }
    requests
}

const SCENARIOS: &[&str] = &[
    "pdf_underlay_zoom",
    "image_decode_retention",
    "material_texture_retention",
    "point_cloud_retention",
];

/// Half-octave DPI bands from the display ceiling down to the floor, the same
/// quantisation the underlay display path applies to the zoom (`quantize_wpp`
/// steps by sqrt(2); the display call site floors the requested DPI at 8).
/// Fixed f32 arithmetic: the identical band list on every run.
fn dpi_bands() -> Vec<f32> {
    let mut bands = Vec::new();
    let mut dpi = 150.0f32;
    while dpi > 8.0 {
        bands.push(dpi);
        dpi /= 2.0f32.sqrt();
    }
    bands.push(8.0);
    bands
}

/// Four A3 landscape pages, eight filled rectangles each, fixed geometry and
/// colours. Real ink so rasterisation and the colour adjustments do real work.
fn bench_pdf_bytes() -> (Vec<u8>, Vec<u8>) {
    use printpdf::{
        Color, LinePoint, Mm, Op, PaintMode, PdfDocument, PdfPage as OutputPage, PdfSaveOptions,
        Point, Polygon, PolygonRing, Rgb, WindingOrder,
    };
    let mut document = PdfDocument::new("cache memory bench");
    for page in 0..4 {
        let mut ops = Vec::new();
        for i in 0..8 {
            let x = 20.0 + (i % 4) as f32 * 100.0;
            let y = 20.0 + (i / 4) as f32 * 120.0;
            let shade = ((i + page * 8) % 16) as f32 / 16.0;
            ops.push(Op::SetFillColor {
                col: Color::Rgb(Rgb {
                    r: shade,
                    g: 0.25,
                    b: 1.0 - shade,
                    icc_profile: None,
                }),
            });
            ops.push(Op::DrawPolygon {
                polygon: Polygon {
                    rings: vec![PolygonRing {
                        points: vec![
                            LinePoint {
                                p: Point {
                                    x: Mm(x).into(),
                                    y: Mm(y).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(x + 80.0).into(),
                                    y: Mm(y).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(x + 80.0).into(),
                                    y: Mm(y + 90.0).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(x).into(),
                                    y: Mm(y + 90.0).into(),
                                },
                                bezier: false,
                            },
                        ],
                    }],
                    mode: PaintMode::Fill,
                    winding_order: WindingOrder::NonZero,
                },
            });
        }
        document
            .pages
            .push(OutputPage::new(Mm(420.0), Mm(297.0), ops));
    }
    // Fingerprint the page operations, excluding generated PDF timestamps/IDs.
    let content = serde_json::to_vec(&document.pages).expect("PDF page fingerprint");
    (
        document.save(&PdfSaveOptions::default(), &mut Vec::new()),
        content,
    )
}

/// A zoom-out sweep across every display DPI band for all four pages, then
/// the same sweep again (eviction pressure), followed by a small hot subset. Each step rasterises the page, applies
/// the underlay colour adjustments, and drops the frame's handles — everything
/// still live afterwards is retained by the static caches.
fn pdf_underlay_zoom() -> Report {
    let setup = Instant::now();
    let path = "memory://cache-memory-bench.pdf";
    let (source, content) = bench_pdf_bytes();
    let mut workload = fixture_hash("pdf-v2:A3;contrast=50;pivot=500;dark=true;hot-tail=4x3");
    hash_part(&mut workload, &content);
    drop(content);
    pdf_raster::register_source(path, Arc::new(source));
    let adjust = PageAdjust {
        contrast: 50,
        contrast_pivot: 500,
        monochrome: false,
        adjust_for_background: true,
        dark_background: true,
        fade_before_contrast: None,
    };
    let bands = dpi_bands();
    let mut requests = Vec::new();
    for pass in ["sweep", "revisit"] {
        for page in ["1", "2", "3", "4"] {
            for &dpi in &bands {
                requests.push((pass, page, dpi));
            }
        }
    }
    for _ in 0..3 {
        for &dpi in &bands[bands.len() - 4..] {
            requests.push(("hot", "4", dpi));
        }
    }
    hash_part(&mut workload, &serde_json::to_vec(&requests).unwrap());
    let mut probe = Probe::new(
        requests
            .iter()
            .map(|(pass, page, dpi)| format!("{pass} p{page} dpi{dpi:.3}"))
            .collect(),
        setup,
    );
    for (_, page, dpi) in requests.iter().copied() {
        probe.measure(|| {
            let raster = pdf_raster::rasterize_page_display_at(path, page, dpi)
                .expect("bench PDF must render");
            let pixels = pdf_raster::adjusted_pixels(path, page, &raster, adjust);
            black_box((&raster, &pixels));
        });
    }
    probe.finish("pdf_underlay_zoom", workload)
}

/// One deterministic synthetic raster, encoded as PNG in-process. The pixel
/// pattern is a pure function of (seed, x, y), so the compressed bytes — and
/// therefore the decoded RGBA the caches retain — are identical on every run.
/// Smooth gradients keep the compressed source small so the measurement is
/// dominated by the decoded and adjusted pixel buffers, not the PNG bytes.
fn bench_image_png(seed: u32, side: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(side, side);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let r = ((x * 255) / side) as u8;
        let g = ((y * 255) / side) as u8;
        let b = ((seed * 7) & 0xFF) as u8;
        *px = image::Rgba([r, g, b, 255]);
    }
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("bench PNG encode");
    bytes
}

/// A drawing that references many distinct raster images. Each is registered
/// (compressed bytes), resolved (decoded into the per-path image cache) and
/// built into an `ImageModel` with a non-neutral brightness/contrast (so the
/// adjusted-pixel cache stores a recoloured copy). The frame's handles are
/// dropped after every step, so everything still live is cache retention.
/// The revisit pass re-reads the same references: with unbounded caches it is
/// all hits and retention stays at the full working set.
fn image_decode_retention() -> Report {
    let setup = Instant::now();

    const COUNT: u32 = 40;
    const SIDE: u32 = 1024;
    let paths: Vec<String> = (0..COUNT)
        .map(|i| format!("memory://image-retention-bench-{i}.png"))
        .collect();
    let mut workload = fixture_hash("image-v2:40x1024;brightness=60;contrast=40");
    for (i, path) in paths.iter().enumerate() {
        let bytes = bench_image_png(i as u32, SIDE);
        hash_part(&mut workload, &bytes);
        image_model::register_source(path, Arc::new(bytes));
    }
    let requests = accesses(COUNT as usize);
    hash_part(&mut workload, &serde_json::to_vec(&requests).unwrap());
    let mut probe = Probe::new(
        requests
            .iter()
            .map(|(pass, i)| format!("{pass} img{i}"))
            .collect(),
        setup,
    );
    for &(_, i) in &requests {
        let path = &paths[i];
        probe.measure(|| {
            let mut entity = RasterImage::new(
                path,
                Vector3::new(0.0, 0.0, 0.0),
                f64::from(SIDE),
                f64::from(SIDE),
            );
            entity.brightness = 60;
            entity.contrast = 40;
            let model = ImageModel::from_raster_image(&entity).expect("bench image must decode");
            black_box(&model);
            drop(model);
        });
    }
    probe.finish("image_decode_retention", workload)
}

/// One deterministic synthetic texture, encoded as PNG on disk. The pixel
/// pattern is a pure function of (seed, x, y), so the compressed bytes — and
/// therefore the decoded RGBA the material cache retains — are identical on
/// every run. Smooth gradients keep the compressed source small so the
/// measurement is dominated by the decoded pixel buffers, not the PNG bytes.
fn bench_texture_png(seed: u32, side: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(side, side);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let r = ((x * 255) / side) as u8;
        let g = ((y * 255) / side) as u8;
        let b = ((seed * 11) & 0xFF) as u8;
        *px = image::Rgba([r, g, b, 255]);
    }
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("bench texture encode");
    bytes
}

/// A drawing whose materials each reference a distinct texture file. Every
/// material is resolved through the public entry point (decode into the
/// per-path material texture cache), then the resolved mesh material is
/// dropped — everything still live afterwards is cache retention. The
/// revisit pass re-resolves the same materials: with an unbounded cache it is
/// all hits and retention stays at the full working set.
fn material_texture_retention() -> Report {
    let setup = Instant::now();

    const COUNT: u32 = 40;
    const SIDE: u32 = 1024;
    let dir = std::env::temp_dir().join(format!("ocs-material-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("bench texture dir");
    let mut workload = fixture_hash("material-v2:40x1024;diffuse-map-source=1");
    let mut document = Document::new();
    let mut entities = Vec::new();
    for i in 0..COUNT {
        let path = dir.join(format!("texture-{i}.png"));
        let bytes = bench_texture_png(i, SIDE);
        hash_part(&mut workload, &bytes);
        std::fs::write(&path, bytes).expect("bench texture write");
        let mut material = Material::new();
        material.name = format!("bench-material-{i}");
        material.diffuse_map = MaterialMap {
            source: 1,
            file_name: path.to_string_lossy().into_owned(),
            ..MaterialMap::default()
        };
        let handle = Handle::new(u64::from(i) + 1);
        document
            .objects
            .insert(handle, ObjectType::Material(material));
        let mut line = Line::new();
        line.common.material_flags = 3;
        line.common.material_handle = Some(handle);
        entities.push(EntityType::Line(line));
    }
    let requests = accesses(COUNT as usize);
    hash_part(&mut workload, &serde_json::to_vec(&requests).unwrap());
    let mut probe = Probe::new(
        requests
            .iter()
            .map(|(pass, i)| format!("{pass} mat{i}"))
            .collect(),
        setup,
    );
    for &(_, i) in &requests {
        let entity = &entities[i];
        probe.measure(|| {
            let material = material_model::resolve_material_with_base(
                &document,
                entity,
                [1.0, 1.0, 1.0, 1.0],
                None,
                Some(&dir),
            );
            assert!(
                material.diffuse_map.image.is_some(),
                "bench texture {i} must decode"
            );
            black_box(&material);
            drop(material);
        });
    }
    let report = probe.finish("material_texture_retention", workload);
    let _ = std::fs::remove_dir_all(&dir);
    report
}

/// A minimal single-node indexed scan (.rcs, "ADOCT" v3) with `points`
/// deterministic points. The layout mirrors `point_cloud::rcs::decode`:
/// header transforms/flags, a two-entry section directory, one 368-byte node
/// record (all octree levels empty so the block is just the point records),
/// then 16-byte records carrying three 18-bit millimetre offsets. The decoded
/// positions/colours are a pure function of (seed, index), so the retained
/// cloud is byte-identical on every run.
fn bench_rcs_bytes(seed: u32, points: u32) -> Vec<u8> {
    const DIR: usize = 0x15D;
    const NODE_RECORD: usize = 368;
    let data = DIR + 8 + NODE_RECORD;
    let n = points as usize;
    let block_size = n * 20;
    let mut b = vec![0u8; data + block_size];
    b[0..5].copy_from_slice(b"ADOCT");
    let put_f64 = |b: &mut [u8], at: usize, v: f64| b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    let put_u32 = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let put_u64 = |b: &mut [u8], at: usize, v: u64| b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    put_f64(&mut b, 0x40, 1.0);
    put_f64(&mut b, 0x48, 1.0);
    put_f64(&mut b, 0x50, 1.0);
    put_f64(&mut b, 0xB8, 256.0);
    put_f64(&mut b, 0xC0, 256.0);
    put_f64(&mut b, 0xC8, 256.0);
    b[0xE0] = 1; // RGB
    b[0xE2] = 1; // intensity
    b[0xE3..0xE3 + 7].copy_from_slice(b"{bench}");
    put_u32(&mut b, 0x129, 2);
    put_u64(&mut b, 0x12D, 2);
    put_u64(&mut b, 0x12D + 8, DIR as u64);
    put_u64(&mut b, 0x12D + 16, (8 + NODE_RECORD) as u64);
    put_u64(&mut b, 0x12D + 24, 1);
    put_u64(&mut b, 0x12D + 32, data as u64);
    put_u64(&mut b, 0x12D + 40, block_size as u64);
    put_u64(&mut b, DIR, 1);
    let rec = DIR + 8;
    put_u32(&mut b, rec + 100, points);
    put_u32(&mut b, rec + 104, block_size as u32);
    let records = data + n * 4;
    for i in 0..n {
        let at = records + i * 16;
        let x = (i as u32 + seed) % 200_000;
        let y = (i as u32 * 7 + seed) % 200_000;
        let z = (i as u32 * 13 + seed) % 200_000;
        let v = u64::from(x) | (u64::from(y) << 18) | (u64::from(z) << 36);
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
        b[at + 8] = (i % 256) as u8;
        b[at + 9] = (seed % 256) as u8;
        b[at + 10] = ((i * 3) % 256) as u8;
        b[at + 11] = ((i * 5) % 256) as u8;
    }
    b
}

/// A drawing references several distinct scan files. Each is decoded into the
/// per-path point-cloud cache, then the frame's handle is dropped — everything
/// still live afterwards is cache retention. The revisit pass re-loads the same
/// paths: with an unbounded cache it is all hits and retention stays at the full
/// working set.
fn point_cloud_retention() -> Report {
    let setup = Instant::now();

    const COUNT: u32 = 16;
    const POINTS: u32 = 200_000;
    let dir = std::env::temp_dir().join(format!("ocs-pointcloud-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("bench scan dir");
    let mut workload = fixture_hash("pointcloud-v2:16x200000;rcs-v3;rgb+intensity");
    let mut paths = Vec::new();
    for i in 0..COUNT {
        let path = dir.join(format!("scan-{i}.rcs"));
        let bytes = bench_rcs_bytes(i, POINTS);
        hash_part(&mut workload, &bytes);
        std::fs::write(&path, bytes).expect("bench scan write");
        paths.push(path);
    }
    let requests = accesses(COUNT as usize);
    hash_part(&mut workload, &serde_json::to_vec(&requests).unwrap());
    let mut probe = Probe::new(
        requests
            .iter()
            .map(|(pass, i)| format!("{pass} scan{i}"))
            .collect(),
        setup,
    );
    for &(_, i) in &requests {
        let path = &paths[i];
        probe.measure(|| {
            let cloud = point_cloud::load(path).expect("bench scan must decode");
            assert_eq!(cloud.positions.len(), POINTS as usize);
            black_box(&cloud);
            drop(cloud);
        });
    }
    let report = probe.finish("point_cloud_retention", workload);
    let _ = std::fs::remove_dir_all(&dir);
    report
}

fn run_scenario(name: &str) -> Report {
    allocator_self_test();
    match name {
        "pdf_underlay_zoom" => pdf_underlay_zoom(),
        "image_decode_retention" => image_decode_retention(),
        "material_texture_retention" => material_texture_retention(),
        "point_cloud_retention" => point_cloud_retention(),
        other => panic!("unknown scenario: {other}"),
    }
}

fn print_report(report: &Report) {
    println!("scenario: {}", report.scenario);
    for step in &report.steps {
        println!(
            "  {:<28} {:>10.2} MiB live",
            step.label,
            step.live_bytes as f64 / (1024.0 * 1024.0)
        );
    }
    println!(
        "  final {:.2} MiB | measured peak {:.2} MiB | operations {:.0} ms",
        report.final_live_bytes as f64 / (1024.0 * 1024.0),
        report.peak_live_bytes as f64 / (1024.0 * 1024.0),
        report.elapsed_ms
    );
}

/// Refuse comparisons across workloads/settings; executable revisions may differ.
fn compare(current: &Report, baseline: &Report, percent: f64) -> Result<(), String> {
    if current.schema != baseline.schema
        || current.scenario != baseline.scenario
        || current.workload_sha256 != baseline.workload_sha256
        || current.os != baseline.os
        || current.arch != baseline.arch
        || current.profile != baseline.profile
        || current.logical_cpus != baseline.logical_cpus
        || current.cache_budgets_bytes != baseline.cache_budgets_bytes
        || current
            .steps
            .iter()
            .map(|s| &s.label)
            .ne(baseline.steps.iter().map(|s| &s.label))
    {
        return Err(format!(
            "{}: incompatible baseline workload, schema, machine, or cache settings",
            current.scenario
        ));
    }
    for (name, now, old) in [
        ("retained", current.retained_bytes, baseline.retained_bytes),
        (
            "peak above baseline",
            current
                .peak_live_bytes
                .saturating_sub(current.baseline_live_bytes),
            baseline
                .peak_live_bytes
                .saturating_sub(baseline.baseline_live_bytes),
        ),
    ] {
        // Ignore at most 64 KiB of process/bookkeeping noise. Latency is
        // diagnostic only: allocator instrumentation and cache misses affect it.
        let allowed = (old as f64 * (1.0 + percent / 100.0)) as usize;
        let allowed = allowed.saturating_add(64 * 1024);
        eprintln!(
            "{} {name}: {old} -> {now} bytes (limit {allowed})",
            current.scenario
        );
        if now > allowed {
            return Err(format!(
                "{}: {name} memory regressed beyond {percent}% + 64 KiB",
                current.scenario
            ));
        }
    }
    Ok(())
}

fn report_self_test() {
    allocator_self_test();
    let mut probe = Probe::new(vec!["allocate/drop".into()], Instant::now());
    probe.measure(|| {
        black_box(vec![0u8; 256 * 1024]);
    });
    let report = probe.finish("self-test", fixture_hash("self-test-v1"));
    assert!(report.peak_live_bytes >= report.baseline_live_bytes + 256 * 1024);
    assert!(report.retained_bytes < 64 * 1024);
    compare(&report, &report, 10.0).unwrap();
    let mut changed = report.clone();
    changed.workload_sha256.push('x');
    assert!(compare(&changed, &report, 10.0).is_err());
    changed = report.clone();
    changed.cache_budgets_bytes.clear();
    assert!(compare(&changed, &report, 10.0).is_err());
    changed = report.clone();
    changed.retained_bytes += 1024 * 1024;
    assert!(compare(&changed, &report, 10.0).is_err());
    changed = report.clone();
    changed.peak_live_bytes += 1024 * 1024;
    assert!(compare(&changed, &report, 10.0).is_err());
    println!("allocator, phase peak, workload compatibility and regression checks passed");
}

fn cli() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut scenario = None;
    let mut output = None;
    let mut baseline = None;
    let mut json = false;
    let mut max_retained = None;
    let mut regression_percent = 10.0f64;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => scenario = Some(args.next().ok_or("--scenario needs a name")?),
            "--output" => {
                output = Some(std::path::PathBuf::from(
                    args.next().ok_or("--output needs a path")?,
                ))
            }
            "--baseline" => {
                baseline = Some(std::path::PathBuf::from(
                    args.next().ok_or("--baseline needs a path")?,
                ))
            }
            "--json" => json = true,
            "--bench" => {}
            "--self-test" => {
                report_self_test();
                return Ok(());
            }
            "--max-retained-mib" | "--max-regression-percent" => {
                let value: f64 = args
                    .next()
                    .ok_or_else(|| format!("{arg} needs a number"))?
                    .parse()
                    .map_err(|_| format!("invalid {arg}"))?;
                if !value.is_finite() || value < 0.0 {
                    return Err(format!("{arg} must be finite and nonnegative"));
                }
                if arg == "--max-retained-mib" {
                    max_retained = Some(value * 1024.0 * 1024.0);
                } else {
                    regression_percent = value;
                }
            }
            "--help" => {
                println!("cache_memory_budget [--scenario NAME] [--json] [--output NEW.json] [--baseline REPORT.json] [--max-regression-percent 10] [--max-retained-mib N] [--self-test]");
                return Ok(());
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    if output.as_ref().is_some_and(|path| path.exists()) {
        return Err("--output must be a new file".into());
    }
    if scenario
        .as_ref()
        .is_some_and(|name| !SCENARIOS.contains(&name.as_str()))
    {
        return Err("unknown scenario".into());
    }
    let baselines: Option<Vec<Report>> = baseline
        .map(|path| {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("invalid baseline (schema 2 array required): {e}"))
        })
        .transpose()?;

    let reports = if let Some(name) = &scenario {
        vec![run_scenario(name)]
    } else {
        // Fresh process per scenario; JSON is the only child stdout content.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut reports = Vec::new();
        for name in SCENARIOS {
            let child = std::process::Command::new(&exe)
                .args(["--scenario", name])
                .output()
                .map_err(|e| e.to_string())?;
            if !child.status.success() {
                return Err(format!(
                    "scenario {name} failed: {}",
                    String::from_utf8_lossy(&child.stderr)
                ));
            }
            reports
                .push(serde_json::from_slice::<Report>(&child.stdout).map_err(|e| e.to_string())?);
        }
        reports
    };
    // Save even a failing result so regressions can be inspected.
    if let Some(path) = output {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(&reports).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    if scenario.is_some() {
        println!("{}", serde_json::to_string_pretty(&reports[0]).unwrap());
    } else if json {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
    } else {
        for report in &reports {
            print_report(report);
        }
    }
    for report in &reports {
        if max_retained.is_some_and(|limit| report.retained_bytes as f64 > limit) {
            return Err(format!(
                "{}: retained {} bytes exceeds --max-retained-mib",
                report.scenario, report.retained_bytes
            ));
        }
        if let Some(baselines) = &baselines {
            let matching: Vec<_> = baselines
                .iter()
                .filter(|old| old.scenario == report.scenario)
                .collect();
            if matching.len() != 1 {
                return Err(format!(
                    "{}: baseline needs exactly one matching scenario",
                    report.scenario
                ));
            }
            compare(report, matching[0], regression_percent)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = cli() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
