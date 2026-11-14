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
//! caches' retention.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use document::entities::{Line, RasterImage};
use document::objects::{Material, MaterialMap, ObjectType};
use document::types::Vector3;
use document::{Document, EntityType, Handle};
use serde::{Deserialize, Serialize};
use OpenCADStudio::scene::model::image_model::{self, ImageModel};
use OpenCADStudio::scene::model::material_model;
use OpenCADStudio::scene::model::pdf_raster::{self, PageAdjust};

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
}

#[derive(Serialize, Deserialize)]
struct Report {
    scenario: String,
    steps: Vec<Step>,
    peak_live_bytes: usize,
    final_live_bytes: usize,
    elapsed_ms: f64,
}

const SCENARIOS: &[&str] = &[
    "pdf_underlay_zoom",
    "image_decode_retention",
    "material_texture_retention",
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
fn bench_pdf_bytes() -> Vec<u8> {
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
    document.save(&PdfSaveOptions::default(), &mut Vec::new())
}

/// A zoom-out sweep across every display DPI band for all four pages, then
/// the same sweep again (cache hits). Each step rasterises the page, applies
/// the underlay colour adjustments, and drops the frame's handles — everything
/// still live afterwards is retained by the static caches.
fn pdf_underlay_zoom() -> Report {
    let started = Instant::now();
    let mut steps = Vec::new();

    let path = "memory://cache-memory-bench.pdf";
    pdf_raster::register_source(path, Arc::new(bench_pdf_bytes()));
    steps.push(Step {
        label: "registered-source".to_string(),
        live_bytes: live(),
    });

    let adjust = PageAdjust {
        contrast: 50,
        contrast_pivot: 500,
        monochrome: false,
        adjust_for_background: true,
        dark_background: true,
        fade_before_contrast: None,
    };
    let bands = dpi_bands();
    for pass in ["sweep", "revisit"] {
        for page in ["1", "2", "3", "4"] {
            for &dpi in &bands {
                let raster = pdf_raster::rasterize_page_display_at(path, page, dpi)
                    .expect("bench PDF must render");
                let pixels = pdf_raster::adjusted_pixels(path, page, &raster, adjust);
                black_box((&raster, &pixels));
                drop(pixels);
                drop(raster);
                steps.push(Step {
                    label: format!("{pass} p{page} dpi{dpi:.3}"),
                    live_bytes: live(),
                });
            }
        }
    }

    Report {
        scenario: "pdf_underlay_zoom".to_string(),
        final_live_bytes: live(),
        peak_live_bytes: PEAK.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        steps,
    }
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
    let started = Instant::now();
    let mut steps = Vec::new();

    const COUNT: u32 = 40;
    const SIDE: u32 = 1024;
    let paths: Vec<String> = (0..COUNT)
        .map(|i| format!("memory://image-retention-bench-{i}.png"))
        .collect();
    for (i, path) in paths.iter().enumerate() {
        image_model::register_source(path, Arc::new(bench_image_png(i as u32, SIDE)));
    }
    steps.push(Step {
        label: "registered-sources".to_string(),
        live_bytes: live(),
    });

    for pass in ["load", "revisit"] {
        for (i, path) in paths.iter().enumerate() {
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
            steps.push(Step {
                label: format!("{pass} img{i}"),
                live_bytes: live(),
            });
        }
    }

    Report {
        scenario: "image_decode_retention".to_string(),
        final_live_bytes: live(),
        peak_live_bytes: PEAK.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        steps,
    }
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
    let started = Instant::now();
    let mut steps = Vec::new();

    const COUNT: u32 = 40;
    const SIDE: u32 = 1024;
    let dir = std::env::temp_dir().join(format!("ocs-material-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("bench texture dir");
    let mut document = Document::new();
    let mut entities = Vec::new();
    for i in 0..COUNT {
        let path = dir.join(format!("texture-{i}.png"));
        std::fs::write(&path, bench_texture_png(i, SIDE)).expect("bench texture write");
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
    steps.push(Step {
        label: "written-textures".to_string(),
        live_bytes: live(),
    });

    for pass in ["load", "revisit"] {
        for (i, entity) in entities.iter().enumerate() {
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
            steps.push(Step {
                label: format!("{pass} mat{i}"),
                live_bytes: live(),
            });
        }
    }

    let report = Report {
        scenario: "material_texture_retention".to_string(),
        final_live_bytes: live(),
        peak_live_bytes: PEAK.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        steps,
    };
    let _ = std::fs::remove_dir_all(&dir);
    report
}

fn run_scenario(name: &str) -> Report {
    allocator_self_test();
    match name {
        "pdf_underlay_zoom" => pdf_underlay_zoom(),
        "image_decode_retention" => image_decode_retention(),
        "material_texture_retention" => material_texture_retention(),
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
        "  final {:.2} MiB | peak {:.2} MiB | {:.0} ms",
        report.final_live_bytes as f64 / (1024.0 * 1024.0),
        report.peak_live_bytes as f64 / (1024.0 * 1024.0),
        report.elapsed_ms
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--scenario") {
        let name = args.get(pos + 1).expect("--scenario needs a name");
        let report = run_scenario(name);
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }

    // Driver: one fresh process per scenario so static caches start empty.
    let exe = std::env::current_exe().expect("current exe");
    let mut reports = Vec::new();
    for name in SCENARIOS {
        let output = std::process::Command::new(&exe)
            .args(["--scenario", name])
            .output()
            .expect("spawn scenario process");
        assert!(
            output.status.success(),
            "scenario {name} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Report = serde_json::from_slice(&output.stdout).expect("scenario JSON report");
        print_report(&report);
        reports.push(report);
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
    }
}
