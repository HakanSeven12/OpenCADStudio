//! OpenCADStudio Performance Benchmarking Suite
//!
//! Measures latency, percentiles, throughput, and memory metrics across core CAD subsystems:
//! 1. Scene Entity Ingestion & Population
//! 2. Analytical & Curved Entity Tessellation (incorporating existing 14,000-entity benchmark)
//! 3. Wide & Tapered Arc Tessellation (incorporating existing 7,500-entity benchmark)
//! 4. Spatial Broadphase Index Construction (InteractionIndex)
//! 5. Interactive Hit-Testing & Picking (click_hit, window box_hit, crossing box_hit, click_hits_all)
//! 6. Real-Time Object Snapping (Endpoint, Midpoint, Center, Intersection, Nearest)
//! 7. 2D Parametric Geometric Constraint Solving (Dogleg solver)
//! 8. DXF File I/O Serialization & Parsing Throughput
//! 9. Selection State Deep vs Arc Cloning
//! 10. UI Ribbon View Widget Tree Construction
//! 11. UI Viewport Grid Geometry Projection & Overlay Cache Key Evaluation
//! 12. UI Themed SVG Icon Lookup Caching vs Uncached Parse
//! 13. Wide & Tapered Arc + Donut Tessellation (7.5k offset polyline curves)
//! 14. Model Space Extents & Bounding Box Calculation (ZOOM EXTENTS)
//! 15. Batch Entity Transformation & Incremental Dirty-Tracking
//! 16. Draworder Depth Map Full Build & Incremental Patch
//! 17. Delta-Undo Transaction Before-Image Recording
//!
//! Output:
//! - Human-readable ASCII / Markdown summary table on stdout
//! - Machine-readable JSON metrics saved to `target/cad_performance_metrics.json`
//! - Baseline regression comparison via `--baseline <file>` or `CAD_BENCH_BASELINE=<path>`
//!
//! Run with:
//!   `cargo bench` (runs in --release mode, does not run on cargo test)
//!   `cargo run --release --bench performance_benchmarks`
//!   `cargo run --release --bench performance_benchmarks -- --quick` (quick smoke test)

#![allow(clippy::field_reassign_with_default, clippy::manual_is_multiple_of)]

use std::collections::HashMap;
use std::env;
use std::f64::consts::{PI, TAU};
use std::hint::black_box;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use acadrust::entities::{Arc as AcadArc, Circle, Ellipse, Line, LwPolyline, LwVertex};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, DxfReader, DxfWriter, EntityType};
use glam::{DVec3, Mat4, Vec3};
use iced::{Color, Point, Rectangle};

use OpenCADStudio::scene::pick::hit_test::{box_hit, click_hit};
use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::pick::selection_state::SelectionState;
use OpenCADStudio::scene::pipeline::wire_arena::partition_wires;
use OpenCADStudio::scene::sketch_constraints::{ConstraintKind, SketchRef, SketchScope};
use OpenCADStudio::scene::view::camera::Camera;
use OpenCADStudio::scene::{ChangeKind, Scene};
use OpenCADStudio::snap::Snapper;
use OpenCADStudio::ui::icons::{self, CHECK};
use OpenCADStudio::ui::overlay::{grid_segments, GridCanvasState, GridKey, GridParams, GridStyle, should_reuse};
use OpenCADStudio::ui::properties::LinetypeItem;
use OpenCADStudio::ui::ribbon::{LayerInfo, Ribbon};

// ── Metric Structures ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMetric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub samples: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub std_dev: f64,
    pub throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub target_threshold: Option<f64>,
    pub passed_target: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuiteReport {
    pub timestamp: String,
    pub os: String,
    pub arch: String,
    pub profile: String,
    pub is_quick_mode: bool,
    pub metrics: Vec<BenchmarkMetric>,
}

pub struct BenchmarkRunner {
    pub quick_mode: bool,
    pub filter: Option<String>,
    pub baseline_path: Option<PathBuf>,
    pub output_path: PathBuf,
    pub results: Vec<BenchmarkMetric>,
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchmarkRunner {
    pub fn new() -> Self {
        let args: Vec<String> = env::args().collect();
        let quick_mode = args.iter().any(|a| a == "--quick" || a == "-q")
            || env::var("CAD_BENCH_QUICK").is_ok();

        let filter = args
            .windows(2)
            .find(|w| w[0] == "--filter")
            .map(|w| w[1].clone())
            .or_else(|| env::var("CAD_BENCH_FILTER").ok());

        let baseline_path = args
            .windows(2)
            .find(|w| w[0] == "--baseline")
            .map(|w| PathBuf::from(&w[1]))
            .or_else(|| env::var("CAD_BENCH_BASELINE").ok().map(PathBuf::from));

        let output_path = args
            .windows(2)
            .find(|w| w[0] == "--output")
            .map(|w| PathBuf::from(&w[1]))
            .or_else(|| env::var("CAD_BENCH_OUTPUT").ok().map(PathBuf::from))
            .unwrap_or_else(|| {
                let target_dir = Path::new("target");
                if !target_dir.exists() {
                    let _ = std::fs::create_dir_all(target_dir);
                }
                target_dir.join("cad_performance_metrics.json")
            });

        Self {
            quick_mode,
            filter,
            baseline_path,
            output_path,
            results: Vec::new(),
        }
    }

    pub fn should_run(&self, name: &str) -> bool {
        if let Some(ref filter) = self.filter {
            name.to_lowercase().contains(&filter.to_lowercase())
        } else {
            true
        }
    }

    /// Record a measured benchmark with multiple timing samples.
    pub fn record(
        &mut self,
        name: &str,
        description: &str,
        unit: &str,
        mut samples: Vec<f64>,
        throughput: Option<(f64, &str)>,
        target_threshold: Option<f64>,
    ) {
        if samples.is_empty() {
            return;
        }
        samples.sort_by(|a, b| a.total_cmp(b));

        let n = samples.len();
        let min = samples[0];
        let max = samples[n - 1];
        let sum: f64 = samples.iter().sum();
        let mean = sum / (n as f64);
        let median = if n % 2 == 0 {
            (samples[n / 2 - 1] + samples[n / 2]) * 0.5
        } else {
            samples[n / 2]
        };

        let p95_idx = ((n as f64) * 0.95).floor() as usize;
        let p95 = samples[p95_idx.min(n - 1)];

        let p99_idx = ((n as f64) * 0.99).floor() as usize;
        let p99 = samples[p99_idx.min(n - 1)];

        let variance: f64 = samples.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / (n as f64);
        let std_dev = variance.sqrt();

        let passed_target = match target_threshold {
            Some(thresh) => median <= thresh,
            None => true,
        };

        let (thru_val, thru_unit) = match throughput {
            Some((val, u)) => (Some(val), Some(u.to_string())),
            None => (None, None),
        };

        self.results.push(BenchmarkMetric {
            name: name.to_string(),
            description: description.to_string(),
            unit: unit.to_string(),
            samples: n,
            min,
            max,
            mean,
            median,
            p95,
            p99,
            std_dev,
            throughput: thru_val,
            throughput_unit: thru_unit,
            target_threshold,
            passed_target,
        });
    }

    /// Print summary table to stdout and persist JSON report.
    pub fn finish(&self) {
        let baseline_map: HashMap<String, BenchmarkMetric> = if let Some(ref path) = self.baseline_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(report) = serde_json::from_str::<BenchmarkSuiteReport>(&content) {
                    println!("\nLoaded baseline from '{}' ({} metrics)", path.display(), report.metrics.len());
                    report.metrics.into_iter().map(|m| (m.name.clone(), m)).collect()
                } else {
                    eprintln!("Warning: Failed to parse baseline JSON at '{}'", path.display());
                    HashMap::new()
                }
            } else {
                eprintln!("Warning: Could not read baseline file at '{}'", path.display());
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        println!("\n========================================================================================================================");
        println!("                                      OPENCADSTUDIO PERFORMANCE BENCHMARK REPORT                                        ");
        println!("========================================================================================================================");
        println!(
            "{:<38} | {:>8} | {:>8} | {:>8} | {:>8} | {:>16} | {:>10} | {:^6}",
            "Benchmark Name", "Median", "Mean", "P95", "Min", "Throughput", "Baseline", "Status"
        );
        println!("------------------------------------------------------------------------------------------------------------------------");

        for m in &self.results {
            let throughput_str = match (&m.throughput, &m.throughput_unit) {
                (Some(val), Some(unit)) => {
                    if *val >= 1_000_000.0 {
                        format!("{:.2}M {}", val / 1_000_000.0, unit)
                    } else if *val >= 1_000.0 {
                        format!("{:.1}k {}", val / 1_000.0, unit)
                    } else {
                        format!("{:.1} {}", val, unit)
                    }
                }
                _ => "-".to_string(),
            };

            let baseline_col = if let Some(base) = baseline_map.get(&m.name) {
                if base.median > 0.0 {
                    let delta_pct = ((m.median - base.median) / base.median) * 100.0;
                    if delta_pct < -1.0 {
                        format!("{:+.1}% (fast)", delta_pct)
                    } else if delta_pct > 1.0 {
                        format!("{:+.1}% (slow)", delta_pct)
                    } else {
                        "~0.0%".to_string()
                    }
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            };

            let status = if m.passed_target { "PASS" } else { "FAIL" };

            println!(
                "{:<38} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>16} | {:>10} | {:^6}",
                m.name,
                m.median,
                m.unit,
                m.mean,
                m.unit,
                m.p95,
                m.unit,
                m.min,
                m.unit,
                throughput_str,
                baseline_col,
                status,
            );
        }
        println!("========================================================================================================================\n");

        let report = BenchmarkSuiteReport {
            timestamp: chrono_now(),
            os: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            profile: if cfg!(debug_assertions) { "debug".to_string() } else { "release".to_string() },
            is_quick_mode: self.quick_mode,
            metrics: self.results.clone(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&report) {
            if let Some(parent) = self.output_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&self.output_path, json) {
                eprintln!("Error saving metrics to '{}': {}", self.output_path.display(), e);
            } else {
                println!("Saved complete performance metrics JSON to: {}\n", self.output_path.display());
            }
        }
    }
}

fn chrono_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("unix_{}", d.as_secs())
}

// ── 1. Scene Entity Ingestion & Batch Creation ───────────────────────────────

fn bench_scene_entity_ingestion(runner: &mut BenchmarkRunner) {
    let name = "scene_entity_ingestion_10k";
    if !runner.should_run(name) {
        return;
    }

    let n_entities = if runner.quick_mode { 2_000 } else { 10_000 };
    let runs = if runner.quick_mode { 3 } else { 10 };
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let mut scene = Scene::new();
        let t0 = Instant::now();

        let n_lines = (n_entities * 4) / 10;
        let n_circles = (n_entities * 3) / 10;
        let n_arcs = (n_entities * 2) / 10;
        let n_polylines = n_entities - n_lines - n_circles - n_arcs;

        for i in 0..n_lines {
            let x = (i % 100) as f64 * 20.0;
            let y = (i / 100) as f64 * 20.0;
            let mut line = Line::new();
            line.start = Vector3::new(x, y, 0.0);
            line.end = Vector3::new(x + 15.0, y + 15.0, 0.0);
            scene.add_entity(EntityType::Line(line));
        }

        for i in 0..n_circles {
            let x = (i % 100) as f64 * 30.0;
            let y = (i / 100) as f64 * 30.0 + 2000.0;
            let mut circle = Circle::default();
            circle.center = Vector3::new(x, y, 0.0);
            circle.radius = 12.0;
            scene.add_entity(EntityType::Circle(circle));
        }

        for i in 0..n_arcs {
            let x = (i % 100) as f64 * 30.0;
            let y = (i / 100) as f64 * 30.0 + 4000.0;
            let mut arc = AcadArc::default();
            arc.center = Vector3::new(x, y, 0.0);
            arc.radius = 10.0;
            arc.start_angle = 0.0;
            arc.end_angle = PI * 0.75;
            scene.add_entity(EntityType::Arc(arc));
        }

        for i in 0..n_polylines {
            let x = (i % 50) as f64 * 50.0;
            let y = (i / 50) as f64 * 50.0 + 6000.0;
            let mut pl = LwPolyline::new();
            pl.vertices = vec![
                LwVertex { location: Vector2::new(x, y), bulge: 0.0, start_width: 0.0, end_width: 0.0, vertex_id: 0 },
                LwVertex { location: Vector2::new(x + 20.0, y), bulge: 0.5, start_width: 0.0, end_width: 0.0, vertex_id: 1 },
                LwVertex { location: Vector2::new(x + 20.0, y + 20.0), bulge: 0.0, start_width: 0.0, end_width: 0.0, vertex_id: 2 },
            ];
            scene.add_entity(EntityType::LwPolyline(pl));
        }

        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        samples.push(elapsed_ms);
        black_box(&scene);
    }

    let median_sec = samples[samples.len() / 2] / 1000.0;
    let throughput = (n_entities as f64) / median_sec;

    runner.record(
        name,
        "Ingesting 10,000 mixed entities (Lines, Circles, Arcs, Polylines) into Scene",
        "ms",
        samples,
        Some((throughput, "entities/s")),
        Some(150.0),
    );
}

// ── 2. Analytical & Curved Entity Tessellation & Zoom ────────────────────────
// Incorporates the existing `bench_analytical_rendering` (14,000 curved entities)

fn bench_analytical_tessellation_zoom(runner: &mut BenchmarkRunner) {
    let name_tess = "analytical_curved_tessellation";
    let name_zoom = "analytical_curved_zoom_100x";
    if !runner.should_run("analytical") {
        return;
    }

    let (n_circles, n_arcs, n_ellipses, n_polylines) = if runner.quick_mode {
        (1000, 1000, 400, 400)
    } else {
        (5000, 5000, 2000, 2000)
    };
    let total_entities = n_circles + n_arcs + n_ellipses + n_polylines;

    let mut scene = Scene::new();
    for i in 0..n_circles {
        let x = (i % 100) as f64 * 50.0;
        let y = (i / 100) as f64 * 50.0;
        let mut circle = Circle::default();
        circle.center = Vector3::new(x, y, 0.0);
        circle.radius = 10.0 + (i % 20) as f64;
        scene.add_entity(EntityType::Circle(circle));
    }

    for i in 0..n_arcs {
        let x = (i % 100) as f64 * 50.0 + 25.0;
        let y = (i / 100) as f64 * 50.0;
        let mut arc = AcadArc::default();
        arc.center = Vector3::new(x, y, 0.0);
        arc.radius = 15.0;
        arc.start_angle = ((i % 8) as f64) * 0.25 * PI;
        arc.end_angle = arc.start_angle + 0.75 * PI;
        scene.add_entity(EntityType::Arc(arc));
    }

    for i in 0..n_ellipses {
        let x = (i % 50) as f64 * 100.0;
        let y = (i / 50) as f64 * 100.0 + 5000.0;
        let mut el = Ellipse::default();
        el.center = Vector3::new(x, y, 0.0);
        el.major_axis = Vector3::new(20.0, 0.0, 0.0);
        el.minor_axis_ratio = 0.5;
        el.start_parameter = 0.0;
        el.end_parameter = TAU;
        scene.add_entity(EntityType::Ellipse(el));
    }

    for i in 0..n_polylines {
        let x = (i % 50) as f64 * 100.0 + 50.0;
        let y = (i / 50) as f64 * 100.0 + 5000.0;
        let mut pline = LwPolyline::new();
        pline.is_closed = true;
        pline.vertices = vec![
            LwVertex { location: Vector2::new(x, y), bulge: 1.0, start_width: 0.0, end_width: 0.0, vertex_id: 0 },
            LwVertex { location: Vector2::new(x + 30.0, y), bulge: 1.0, start_width: 0.0, end_width: 0.0, vertex_id: 1 },
        ];
        scene.add_entity(EntityType::LwPolyline(pline));
    }

    let cam = Camera::default();
    let tess_runs = if runner.quick_mode { 3 } else { 5 };
    let mut tess_samples = Vec::with_capacity(tess_runs);
    let mut wires_opt = None;

    for _ in 0..tess_runs {
        scene.bump_geometry();
        let t0 = Instant::now();
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
        tess_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        wires_opt = Some(wires);
    }

    let wires = wires_opt.unwrap();
    let median_tess_sec = tess_samples[tess_samples.len() / 2] / 1000.0;
    let tess_throughput = (total_entities as f64) / median_tess_sec;

    runner.record(
        name_tess,
        "Initial wire generation for 14,000 curved entities (circles, arcs, ellipses, polylines)",
        "ms",
        tess_samples,
        Some((tess_throughput, "entities/s")),
        Some(120.0),
    );

    let depths = rustc_hash::FxHashMap::default();
    let part_runs = if runner.quick_mode { 20 } else { 50 };
    let mut part_samples = Vec::with_capacity(part_runs);
    for _ in 0..part_runs {
        let t0 = Instant::now();
        let p = partition_wires(&wires, &depths);
        part_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        black_box(p);
    }
    runner.record(
        "wire_partitioning_per_frame",
        "Partitioning 14k wires into GPU circles/ellipses and regular lines",
        "ms",
        part_samples,
        None,
        Some(2.0),
    );

    let zoom_levels = [0.1f32, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 50.0, 100.0, 1000.0];
    let zoom_runs = if runner.quick_mode { 2 } else { 5 };
    let mut zoom_samples = Vec::with_capacity(zoom_runs);

    for _ in 0..zoom_runs {
        let t0 = Instant::now();
        let mut dynamic_cam = Camera::default();
        for &zoom in zoom_levels.iter().cycle().take(100) {
            dynamic_cam.distance = 1000.0 / zoom;
            let _w = scene.model_tile_wires_arc(0, &dynamic_cam, 1.0, 1000.0);
        }
        zoom_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let median_zoom_ms = zoom_samples[zoom_samples.len() / 2];
    let fps = 100.0 / (median_zoom_ms / 1000.0);

    runner.record(
        name_zoom,
        "100 camera zoom navigations over 14,000 curved entities",
        "ms",
        zoom_samples,
        Some((fps, "FPS")),
        Some(50.0),
    );
}

// ── 3. Spatial Broadphase Index Construction ────────────────────────────────

fn bench_spatial_index_construction(runner: &mut BenchmarkRunner) {
    let name = "spatial_interaction_index_build";
    if !runner.should_run(name) {
        return;
    }

    let n_lines = if runner.quick_mode { 2_000 } else { 10_000 };
    let mut scene = Scene::new();
    for i in 0..n_lines {
        let x = (i % 100) as f64 * 30.0;
        let y = (i / 100) as f64 * 30.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 25.0, y + 25.0, 0.0);
        scene.add_entity(EntityType::Line(line));
    }

    let cam = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);

    let runs = if runner.quick_mode { 5 } else { 15 };
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let t0 = Instant::now();
        let index = InteractionIndex::build(&wires);
        index.prepare_screen();
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        black_box(index);
    }

    let median_sec = samples[samples.len() / 2] / 1000.0;
    let throughput = (wires.len() as f64) / median_sec;

    runner.record(
        name,
        "Constructing spatial InteractionIndex & prepare_screen for 10k wires",
        "ms",
        samples,
        Some((throughput, "wires/s")),
        Some(35.0),
    );
}

// ── 4. Interactive Picking & Hit-Testing ─────────────────────────────────────

fn bench_hit_test_picking(runner: &mut BenchmarkRunner) {
    if !runner.should_run("hit_test") && !runner.should_run("pick") {
        return;
    }

    let n_wires = if runner.quick_mode { 1_000 } else { 5_000 };
    let mut scene = Scene::new();
    for i in 0..n_wires {
        let x = (i % 100) as f64 * 20.0;
        let y = (i / 100) as f64 * 20.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 18.0, y + 18.0, 0.0);
        scene.add_entity(EntityType::Line(line));
    }

    let cam = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
    let index = InteractionIndex::build(&wires);
    index.prepare_screen();

    let view_rot = Mat4::IDENTITY;
    let eye = DVec3::new(1000.0, 1000.0, 2000.0);
    let bounds = Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 };

    let n_queries = if runner.quick_mode { 200 } else { 1_000 };
    let mut click_samples = Vec::with_capacity(5);

    for _ in 0..5 {
        let t0 = Instant::now();
        for q in 0..n_queries {
            let cx = ((q * 37) % 1920) as f32;
            let cy = ((q * 53) % 1080) as f32;
            let hit = click_hit(Point::new(cx, cy), wires.as_slice(), view_rot, eye, bounds, false, 8.0);
            black_box(hit);
        }
        let per_query_us = (t0.elapsed().as_secs_f64() * 1_000_000.0) / (n_queries as f64);
        click_samples.push(per_query_us);
    }

    let median_click_us = click_samples[click_samples.len() / 2];
    runner.record(
        "hit_test_click_pick_latency",
        "Single-point click_hit query across 5,000 entities in screen space",
        "µs",
        click_samples,
        Some((1_000_000.0 / median_click_us, "queries/s")),
        Some(100.0),
    );

    let mut box_win_samples = Vec::with_capacity(5);
    for _ in 0..5 {
        let t0 = Instant::now();
        for q in 0..n_queries {
            let cx = ((q * 41) % 1500) as f32;
            let cy = ((q * 47) % 800) as f32;
            let p1 = Point::new(cx, cy);
            let p2 = Point::new(cx + 150.0, cy + 150.0);
            let hits = box_hit(p1, p2, false, wires.as_slice(), view_rot, eye, bounds);
            black_box(hits);
        }
        let per_query_us = (t0.elapsed().as_secs_f64() * 1_000_000.0) / (n_queries as f64);
        box_win_samples.push(per_query_us);
    }
    let median_win_us = box_win_samples[box_win_samples.len() / 2];
    runner.record(
        "hit_test_box_window_enclosed",
        "Window selection box_hit (enclosed) across 5,000 entities",
        "µs",
        box_win_samples,
        Some((1_000_000.0 / median_win_us, "queries/s")),
        Some(250.0),
    );

    let mut box_cross_samples = Vec::with_capacity(5);
    for _ in 0..5 {
        let t0 = Instant::now();
        for q in 0..n_queries {
            let cx = ((q * 41) % 1500) as f32;
            let cy = ((q * 47) % 800) as f32;
            let p1 = Point::new(cx, cy);
            let p2 = Point::new(cx + 150.0, cy + 150.0);
            let hits = box_hit(p1, p2, true, wires.as_slice(), view_rot, eye, bounds);
            black_box(hits);
        }
        let per_query_us = (t0.elapsed().as_secs_f64() * 1_000_000.0) / (n_queries as f64);
        box_cross_samples.push(per_query_us);
    }
    let median_cross_us = box_cross_samples[box_cross_samples.len() / 2];
    runner.record(
        "hit_test_box_crossing_boundary",
        "Crossing selection box_hit (boundary intersection) across 5,000 entities",
        "µs",
        box_cross_samples,
        Some((1_000_000.0 / median_cross_us, "queries/s")),
        Some(500.0),
    );
}

// ── 5. Real-Time Object Snapping (OSNAP) ─────────────────────────────────────

fn bench_osnap_evaluation(runner: &mut BenchmarkRunner) {
    let name = "osnap_cursor_tracking_latency";
    if !runner.should_run("snap") && !runner.should_run(name) {
        return;
    }

    let n_entities = if runner.quick_mode { 1_000 } else { 5_000 };
    let mut scene = Scene::new();
    for i in 0..n_entities {
        let x = (i % 100) as f64 * 30.0;
        let y = (i / 100) as f64 * 30.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 25.0, y + 25.0, 0.0);
        scene.add_entity(EntityType::Line(line));
    }

    let cam = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
    let index = InteractionIndex::build(&wires);
    index.prepare_screen();

    let mut snapper = Snapper::default();
    snapper.toggle_global();
    snapper.enable_all();

    let view_rot = Mat4::IDENTITY;
    let eye = DVec3::new(500.0, 500.0, 1000.0);
    let bounds = Rectangle { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 };
    let grid_origin = Vec3::ZERO;
    let grid_axes = (Vec3::X, Vec3::Y, Vec3::Z);

    let n_evals = if runner.quick_mode { 300 } else { 1_500 };
    let mut samples = Vec::with_capacity(5);

    for _ in 0..5 {
        let t0 = Instant::now();
        for q in 0..n_evals {
            let wx = (q as f64 * 7.3) % 2500.0;
            let wy = (q as f64 * 11.7) % 2500.0;
            let cursor_world = DVec3::new(wx, wy, 0.0);
            let cursor_screen = Point::new(
                ((q * 23) % 1920) as f32,
                ((q * 31) % 1080) as f32,
            );

            let hit = snapper.snap(
                cursor_world,
                cursor_screen,
                wires.as_slice(),
                view_rot,
                eye,
                bounds,
                grid_origin,
                grid_axes,
                None,
            );
            black_box(hit);
        }
        let per_snap_us = (t0.elapsed().as_secs_f64() * 1_000_000.0) / (n_evals as f64);
        samples.push(per_snap_us);
    }

    let median_us = samples[samples.len() / 2];
    runner.record(
        name,
        "Real-time OSNAP evaluation (Endpoint, Mid, Center, Intersect, Nearest) across 5k entities",
        "µs",
        samples,
        Some((1_000_000.0 / median_us, "snaps/s")),
        Some(350.0),
    );
}

// ── 6. 2D Parametric Geometric Constraint Solving ───────────────────────────

fn bench_geometric_constraint_solving(runner: &mut BenchmarkRunner) {
    let name = "constraint_solver_dogleg_system";
    if !runner.should_run("constraint") && !runner.should_run("solve") {
        return;
    }

    let n_chains = if runner.quick_mode { 5 } else { 15 };
    let runs = if runner.quick_mode { 5 } else { 15 };
    let mut samples = Vec::with_capacity(runs);

    for run_idx in 0..runs {
        let mut scene = Scene::new();
        let mut lines = Vec::new();

        for i in 0..n_chains {
            let y = i as f64 * 30.0;
            let mut l1 = Line::new();
            l1.start = Vector3::new(0.0, y, 0.0);
            l1.end = Vector3::new(50.0, y, 0.0);
            let h1 = scene.add_entity(EntityType::Line(l1));

            let mut l2 = Line::new();
            l2.start = Vector3::new(50.0, y, 0.0);
            l2.end = Vector3::new(50.0, y + 20.0, 0.0);
            let h2 = scene.add_entity(EntityType::Line(l2));

            let mut l3 = Line::new();
            l3.start = Vector3::new(50.0, y + 20.0, 0.0);
            l3.end = Vector3::new(0.0, y + 20.0, 0.0);
            let h3 = scene.add_entity(EntityType::Line(l3));

            lines.push((h1, h2, h3));
        }

        let cs = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
        for &(h1, h2, h3) in &lines {
            cs.add(ConstraintKind::Perpendicular, vec![SketchRef::whole(h1), SketchRef::whole(h2)], None);
            cs.add(ConstraintKind::Perpendicular, vec![SketchRef::whole(h2), SketchRef::whole(h3)], None);
            cs.add(ConstraintKind::Parallel, vec![SketchRef::whole(h1), SketchRef::whole(h3)], None);
        }

        let target = lines[run_idx % lines.len()].0;
        if let Some(EntityType::Line(ref mut l)) = scene.document.get_entity_mut(target) {
            l.end.y += 10.0;
        }

        let t0 = Instant::now();
        scene.bump_entities(&[(target, ChangeKind::Modified)]);
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let median_ms = samples[samples.len() / 2];
    runner.record(
        name,
        "Solving coupled nonlinear 2D constraints (Perp, Parallel, Length) via Dogleg solver",
        "ms",
        samples,
        Some((1000.0 / median_ms, "solves/s")),
        Some(10.0),
    );
}

// ── 7. DXF File I/O Serialization & Parsing Throughput ──────────────────────

fn bench_dxf_io_throughput(runner: &mut BenchmarkRunner) {
    if !runner.should_run("dxf") && !runner.should_run("io") {
        return;
    }

    let n_entities = if runner.quick_mode { 1_000 } else { 5_000 };
    let mut doc = CadDocument::new();

    for i in 0..n_entities {
        let x = (i % 100) as f64 * 20.0;
        let y = (i / 100) as f64 * 20.0;
        if i % 2 == 0 {
            let mut line = Line::new();
            line.start = Vector3::new(x, y, 0.0);
            line.end = Vector3::new(x + 10.0, y + 10.0, 0.0);
            let _ = doc.add_entity(EntityType::Line(line));
        } else {
            let mut circle = Circle::default();
            circle.center = Vector3::new(x, y, 0.0);
            circle.radius = 8.0;
            let _ = doc.add_entity(EntityType::Circle(circle));
        }
    }

    let runs = if runner.quick_mode { 3 } else { 8 };
    let mut write_samples = Vec::with_capacity(runs);
    let mut dxf_bytes = Vec::new();

    for _ in 0..runs {
        let t0 = Instant::now();
        let bytes = DxfWriter::new(&doc).write_to_vec().expect("DXF write succeeds");
        write_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        dxf_bytes = bytes;
    }

    let mb_size = (dxf_bytes.len() as f64) / (1024.0 * 1024.0);
    let median_write_sec = write_samples[write_samples.len() / 2] / 1000.0;
    let write_throughput_mb = mb_size / median_write_sec;

    runner.record(
        "dxf_export_write_throughput",
        "Serializing 5,000 entities to DXF binary stream",
        "ms",
        write_samples,
        Some((write_throughput_mb, "MB/s")),
        Some(100.0),
    );

    let mut read_samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t0 = Instant::now();
        let parsed_doc = DxfReader::from_reader(Cursor::new(dxf_bytes.clone()))
            .expect("DXF reader created")
            .read()
            .expect("DXF parse succeeds");
        read_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        black_box(parsed_doc);
    }

    let median_read_sec = read_samples[read_samples.len() / 2] / 1000.0;
    let read_throughput_mb = mb_size / median_read_sec;

    runner.record(
        "dxf_import_parse_throughput",
        "Parsing DXF stream of 5,000 entities into CadDocument",
        "ms",
        read_samples,
        Some((read_throughput_mb, "MB/s")),
        Some(80.0),
    );
}

// ── 8. Selection State Deep vs Arc Cloning ───────────────────────────────────
// Incorporates the existing `bench_selection_state_clone`

fn bench_selection_cloning(runner: &mut BenchmarkRunner) {
    if !runner.should_run("selection") && !runner.should_run("clone") {
        return;
    }

    let mut state = SelectionState {
        vp_size: (1920.0, 1080.0),
        poly_points: vec![Point::new(10.0, 10.0); 64],
        ..Default::default()
    };
    state.box_anchor = Some(Point::new(0.0, 0.0));
    state.box_current = Some(Point::new(100.0, 100.0));

    let n = if runner.quick_mode { 1_000 } else { 5_000 };
    let runs = 5;

    let mut deep_samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t0 = Instant::now();
        for _ in 0..n {
            black_box(state.clone());
        }
        let per_ns = (t0.elapsed().as_nanos() as f64) / (n as f64);
        deep_samples.push(per_ns);
    }
    let median_deep_ns = deep_samples[deep_samples.len() / 2];
    runner.record(
        "selection_state_deep_clone",
        "SelectionState::clone (deep clone with 64 polygon vertices)",
        "ns",
        deep_samples,
        Some((1_000_000_000.0 / median_deep_ns, "clones/s")),
        Some(1000.0),
    );

    let arc_state = Arc::new(state);
    let mut arc_samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t0 = Instant::now();
        for _ in 0..n {
            black_box(Arc::clone(&arc_state));
        }
        let per_ns = (t0.elapsed().as_nanos() as f64) / (n as f64);
        arc_samples.push(per_ns);
    }
    let median_arc_ns = arc_samples[arc_samples.len() / 2];
    runner.record(
        "selection_state_arc_clone",
        "Arc<SelectionState>::clone (atomic reference count increment)",
        "ns",
        arc_samples,
        Some((1_000_000_000.0 / median_arc_ns, "clones/s")),
        Some(20.0),
    );
}

// ── 10. UI Ribbon View Construction Benchmark ──────────────────────────────

fn bench_ui_ribbon_view_construction(runner: &mut BenchmarkRunner) {
    if !runner.should_run("ui_ribbon_view_construction") {
        return;
    }

    let mut ribbon = Ribbon::new();
    ribbon.set_styles(
        vec![
            "Standard".to_string(),
            "Title".to_string(),
            "Annotative".to_string(),
        ],
        "Standard",
        vec!["Standard".to_string()],
        "Standard",
        vec!["Standard".to_string()],
        "Standard",
        vec!["Standard".to_string()],
        "Standard",
    );
    ribbon.set_layers(
        vec![
            LayerInfo {
                name: "0".to_string(),
                color: Color::TRANSPARENT,
                visible: true,
                frozen: false,
                locked: false,
            },
            LayerInfo {
                name: "DRAWING".to_string(),
                color: Color::TRANSPARENT,
                visible: true,
                frozen: false,
                locked: false,
            },
            LayerInfo {
                name: "DIMENSIONS".to_string(),
                color: Color::TRANSPARENT,
                visible: true,
                frozen: false,
                locked: false,
            },
            LayerInfo {
                name: "ANNOTATIONS".to_string(),
                color: Color::TRANSPARENT,
                visible: true,
                frozen: false,
                locked: false,
            },
        ],
        "0",
    );
    ribbon.set_available_linetypes(vec![
        LinetypeItem {
            name: "Continuous".to_string(),
            art: String::new(),
        },
        LinetypeItem {
            name: "DASHED".to_string(),
            art: String::new(),
        },
        LinetypeItem {
            name: "CENTER".to_string(),
            art: String::new(),
        },
        LinetypeItem {
            name: "HIDDEN".to_string(),
            art: String::new(),
        },
    ]);

    // Warm-up for allocator and tree settling
    for _ in 0..10 {
        let _ = black_box(ribbon.view(false, false, 0, 0, false));
    }

    let n = if runner.quick_mode { 20 } else { 100 };
    let runs = 5;
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let t0 = Instant::now();
        for _ in 0..n {
            let elem = ribbon.view(false, false, 0, 0, false);
            black_box(elem);
        }
        let per_us = (t0.elapsed().as_micros() as f64) / (n as f64);
        samples.push(per_us);
    }

    let median_us = samples[samples.len() / 2];
    runner.record(
        "ui_ribbon_view_construction",
        "Ribbon::view widget tree construction per frame (populated tabs, styles, layers)",
        "µs",
        samples,
        Some((1_000_000.0 / median_us, "views/s")),
        Some(3000.0), // Target threshold < 3.0 ms
    );
}

// ── 11. UI Grid Geometry Generation & Cache Hit Benchmark ──────────────────

fn bench_ui_grid_geometry(runner: &mut BenchmarkRunner) {
    let view_rot1 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
    let eye1 = glam::DVec3::new(4.0, 3.5, 9.0);
    let bounds1 = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };
    let step1 = 80.0_f32;
    let origin1 = glam::DVec3::new(0.0, 0.0, 0.0);
    let axes1 = (Vec3::X, Vec3::Y, Vec3::Z);
    let limits1: Option<(glam::DVec2, glam::DVec2)> = None;

    let view_rot2 = Mat4::from_rotation_x(0.15) * Mat4::from_rotation_y(0.05);
    let eye2 = glam::DVec3::new(4.0, 3.5, 9.0);
    let bounds2 = Rectangle {
        x: 1280.0,
        y: 0.0,
        width: 640.0,
        height: 720.0,
    };
    let step2 = 160.0_f32;
    let origin2 = glam::DVec3::new(0.0, 0.0, 0.0);
    let axes2 = (Vec3::X, Vec3::Y, Vec3::Z);
    let limits2: Option<(glam::DVec2, glam::DVec2)> = None;

    // ── 1. Uncached grid geometry generation ──
    if runner.should_run("ui_grid_geometry_uncached") {
        for _ in 0..10 {
            let _ = black_box(grid_segments(view_rot1, eye1, bounds1, step1, origin1, axes1, limits1));
            let _ = black_box(grid_segments(view_rot2, eye2, bounds2, step2, origin2, axes2, limits2));
        }

        let n = if runner.quick_mode { 20 } else { 100 };
        let runs = 5;
        let mut samples = Vec::with_capacity(runs);

        for _ in 0..runs {
            let t0 = Instant::now();
            for _ in 0..n {
                let g1 = grid_segments(view_rot1, eye1, bounds1, step1, origin1, axes1, limits1);
                let g2 = grid_segments(view_rot2, eye2, bounds2, step2, origin2, axes2, limits2);
                black_box(g1);
                black_box(g2);
            }
            let per_us = (t0.elapsed().as_micros() as f64) / (n as f64);
            samples.push(per_us);
        }

        let median_us = samples[samples.len() / 2];
        runner.record(
            "ui_grid_geometry_uncached",
            "Viewport 2-pane tiled grid projection & segment generation (uncached)",
            "µs",
            samples,
            Some((1_000_000.0 / median_us, "frames/s")),
            Some(800.0), // Target threshold < 800 µs
        );
    }

    // ── 2. Cached grid key hit evaluation ──
    if runner.should_run("ui_grid_cache_hit_evaluation") {
        let params1 = GridParams {
            view_rot: view_rot1,
            eye: eye1,
            bounds: bounds1,
            step: step1,
            origin: origin1,
            axes: axes1,
            limits: limits1,
        };
        let params2 = GridParams {
            view_rot: view_rot2,
            eye: eye2,
            bounds: bounds2,
            step: step2,
            origin: origin2,
            axes: axes2,
            limits: limits2,
        };
        let grids = vec![params1, params2];
        let canvas_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 720.0,
        };

        let state = GridCanvasState::default();
        let stored_key = GridKey::from_grids(&grids, canvas_bounds, GridStyle::default());
        *state.key.borrow_mut() = Some(stored_key);

        for _ in 0..50 {
            let key = GridKey::from_grids(&grids, canvas_bounds, GridStyle::default());
            let hit = should_reuse(state.key.borrow().as_ref(), &key);
            black_box(hit);
        }

        let n = if runner.quick_mode { 1_000 } else { 10_000 };
        let runs = 5;
        let mut samples = Vec::with_capacity(runs);

        for _ in 0..runs {
            let t0 = Instant::now();
            let mut hit_count = 0u32;
            for _ in 0..n {
                let key = GridKey::from_grids(black_box(&grids), black_box(canvas_bounds), GridStyle::default());
                if should_reuse(state.key.borrow().as_ref(), &key) {
                    hit_count += 1;
                }
            }
            assert_eq!(hit_count as usize, n);
            let per_ns = (t0.elapsed().as_nanos() as f64) / (n as f64);
            samples.push(per_ns);
        }

        let median_ns = samples[samples.len() / 2];
        runner.record(
            "ui_grid_cache_hit_evaluation",
            "Grid overlay cache key construction + reuse hit decision",
            "ns",
            samples,
            Some((1_000_000_000.0 / median_ns, "evals/s")),
            Some(500.0), // Target threshold < 500 ns
        );
    }
}

// ── 12. UI Themed SVG Icon Caching Benchmark ───────────────────────────────

fn bench_ui_icon_caching(runner: &mut BenchmarkRunner) {
    let bytes = CHECK;

    // Warm up the themed handle cache
    let _ = icons::themed_handle(bytes);

    if runner.should_run("ui_icon_handle_cached") {
        let n = if runner.quick_mode { 5_000 } else { 50_000 };
        let runs = 5;
        let mut samples = Vec::with_capacity(runs);

        for _ in 0..runs {
            let t0 = Instant::now();
            for _ in 0..n {
                let handle = icons::themed_handle(black_box(bytes));
                black_box(handle);
            }
            let per_ns = (t0.elapsed().as_nanos() as f64) / (n as f64);
            samples.push(per_ns);
        }

        let median_ns = samples[samples.len() / 2];
        runner.record(
            "ui_icon_handle_cached",
            "Themed SVG icon cache hit (FxHashMap lookup + Handle clone)",
            "ns",
            samples,
            Some((1_000_000_000.0 / median_ns, "lookups/s")),
            Some(100.0), // Target threshold < 100 ns
        );
    }

    if runner.should_run("ui_icon_handle_uncached") {
        let n = if runner.quick_mode { 200 } else { 2_000 };
        let runs = 5;
        let mut samples = Vec::with_capacity(runs);

        for _ in 0..runs {
            let t0 = Instant::now();
            for _ in 0..n {
                let handle = iced::widget::svg::Handle::from_memory(black_box(bytes));
                black_box(handle);
            }
            let per_ns = (t0.elapsed().as_nanos() as f64) / (n as f64);
            samples.push(per_ns);
        }

        let median_ns = samples[samples.len() / 2];
        runner.record(
            "ui_icon_handle_uncached",
            "Uncached SVG icon memory parse + Handle allocation",
            "ns",
            samples,
            Some((1_000_000_000.0 / median_ns, "parses/s")),
            None, // Reference comparison
        );
    }
}

// ── 13. Wide & Tapered Arc + Donut Tessellation ─────────────────────────────

fn bench_wide_and_tapered_arc_tessellation(runner: &mut BenchmarkRunner) {
    let name = "wide_and_tapered_arc_tessellation";
    if !runner.should_run(name) {
        return;
    }

    let (n_wide, n_taper, n_donut) = if runner.quick_mode {
        (500, 500, 500)
    } else {
        (2500, 2500, 2500)
    };
    let total_entities = n_wide + n_taper + n_donut;

    let runs = if runner.quick_mode { 3 } else { 5 };
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let mut scene = Scene::new();

        // 1. Wide polyline arcs (constant width 6.0)
        for i in 0..n_wide {
            let x = (i % 50) as f64 * 80.0;
            let y = (i / 50) as f64 * 80.0;
            let mut pline = LwPolyline::new();
            pline.constant_width = 6.0;
            pline.vertices = vec![
                LwVertex {
                    location: Vector2::new(x, y),
                    bulge: 0.5,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 0,
                },
                LwVertex {
                    location: Vector2::new(x + 40.0, y),
                    bulge: 0.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 1,
                },
            ];
            scene.add_entity(EntityType::LwPolyline(pline));
        }

        // 2. Tapered arcs (width: 2.0 -> 12.0)
        for i in 0..n_taper {
            let x = (i % 50) as f64 * 80.0;
            let y = (i / 50) as f64 * 80.0 + 4500.0;
            let mut pline = LwPolyline::new();
            pline.vertices = vec![
                LwVertex {
                    location: Vector2::new(x, y),
                    bulge: 0.7,
                    start_width: 2.0,
                    end_width: 12.0,
                    vertex_id: 0,
                },
                LwVertex {
                    location: Vector2::new(x + 35.0, y + 10.0),
                    bulge: 0.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 1,
                },
            ];
            scene.add_entity(EntityType::LwPolyline(pline));
        }

        // 3. Donuts (2 closed arc segments, inner 10.0, outer 26.0)
        for i in 0..n_donut {
            let cx = (i % 50) as f64 * 80.0;
            let cy = (i / 50) as f64 * 80.0 + 9000.0;
            let inner_r = 10.0;
            let outer_r = 26.0;
            let r_avg = (inner_r + outer_r) * 0.5;
            let width = outer_r - inner_r;
            let mut pline = LwPolyline::new();
            pline.is_closed = true;
            pline.constant_width = width;
            pline.vertices = vec![
                LwVertex {
                    location: Vector2::new(cx - r_avg, cy),
                    bulge: 1.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 0,
                },
                LwVertex {
                    location: Vector2::new(cx + r_avg, cy),
                    bulge: 1.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 1,
                },
            ];
            scene.add_entity(EntityType::LwPolyline(pline));
        }

        let cam = Camera::default();
        let t0 = Instant::now();
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
        black_box(wires);
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        samples.push(elapsed_ms);
    }

    let median_ms = samples[samples.len() / 2];
    let throughput = (total_entities as f64) / (median_ms / 1000.0);
    runner.record(
        name,
        "Tessellating 7.5k wide arcs, tapered arcs & donuts (mesh offset curves)",
        "ms",
        samples,
        Some((throughput, "entities/s")),
        Some(100.0),
    );
}

// ── 14. Model Space Extents Calculation (ZOOM EXTENTS) ──────────────────────

fn bench_zoom_extents_calculation(runner: &mut BenchmarkRunner) {
    let name = "zoom_extents_bounding_box_calc";
    if !runner.should_run(name) {
        return;
    }

    let n = if runner.quick_mode { 4_000 } else { 20_000 };
    let mut scene = Scene::new();
    for i in 0..n {
        let x = (i % 200) as f64 * 50.0;
        let y = (i / 200) as f64 * 50.0;
        match i % 4 {
            0 => {
                let mut line = Line::new();
                line.start = Vector3::new(x, y, 0.0);
                line.end = Vector3::new(x + 40.0, y + 40.0, 0.0);
                scene.add_entity(EntityType::Line(line));
            }
            1 => {
                let mut c = Circle::new();
                c.center = Vector3::new(x + 20.0, y + 20.0, 0.0);
                c.radius = 15.0;
                scene.add_entity(EntityType::Circle(c));
            }
            2 => {
                let mut a = AcadArc::new();
                a.center = Vector3::new(x + 20.0, y + 20.0, 0.0);
                a.radius = 15.0;
                a.start_angle = 0.5;
                a.end_angle = 3.5;
                scene.add_entity(EntityType::Arc(a));
            }
            _ => {
                let mut pl = LwPolyline::new();
                pl.vertices = vec![
                    LwVertex::new(Vector2::new(x, y)),
                    LwVertex::new(Vector2::new(x + 20.0, y + 10.0)),
                    LwVertex::new(Vector2::new(x + 40.0, y)),
                ];
                scene.add_entity(EntityType::LwPolyline(pl));
            }
        }
    }

    let runs = if runner.quick_mode { 5 } else { 15 };
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        // Invalidate extents cache to measure cold AABB traversal across all entities
        scene.invalidate_draw_depth();
        scene.bump_entities(&[]);

        let t0 = Instant::now();
        let ext = scene.model_space_extents();
        black_box(ext);
        let elapsed_us = t0.elapsed().as_micros() as f64;
        samples.push(elapsed_us);
    }

    let median_us = samples[samples.len() / 2];
    let throughput = (n as f64) / (median_us / 1_000_000.0);
    runner.record(
        name,
        "Computing model space bounding box across 20k mixed entities (ZOOM EXTENTS)",
        "µs",
        samples,
        Some((throughput, "entities/s")),
        Some(10000.0), // Target threshold < 10.0 ms
    );
}

// ── 15. Batch Entity Transformation & Mutation ──────────────────────────────

fn bench_batch_entity_mutation(runner: &mut BenchmarkRunner) {
    let name = "batch_entity_transform_mutation";
    if !runner.should_run(name) {
        return;
    }

    let total_entities = if runner.quick_mode { 2_000 } else { 10_000 };
    let mutate_count = if runner.quick_mode { 200 } else { 1_000 };

    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(total_entities);
    for i in 0..total_entities {
        let x = (i % 100) as f64 * 30.0;
        let y = (i / 100) as f64 * 30.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 20.0, y + 20.0, 0.0);
        let h = scene.add_entity(EntityType::Line(line));
        handles.push(h);
    }
    let cam = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
    let index = InteractionIndex::build(&wires);
    index.prepare_screen();

    let runs = if runner.quick_mode { 5 } else { 10 };
    let mut samples = Vec::with_capacity(runs);

    for run_idx in 0..runs {
        let offset = (run_idx as f64 + 1.0) * 2.0;
        let mut modified = Vec::with_capacity(mutate_count);

        for &h in handles.iter().take(mutate_count) {
            if let Some(EntityType::Line(ref mut line)) = scene.document.get_entity_mut(h) {
                line.start.x += offset;
                line.end.x += offset;
            }
            modified.push((h, ChangeKind::Modified));
        }

        let t0 = Instant::now();
        scene.bump_entities(&modified);
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
        let idx = InteractionIndex::build(&wires);
        idx.prepare_screen();
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        black_box(idx);
        samples.push(elapsed_ms);
    }

    let median_ms = samples[samples.len() / 2];
    let throughput = (mutate_count as f64) / (median_ms / 1000.0);
    runner.record(
        name,
        "Incremental dirty propagation & spatial index update for 1,000 modified entities",
        "ms",
        samples,
        Some((throughput, "entities/s")),
        Some(10.0), // Target threshold < 10.0 ms
    );
}

// ── 16. Draworder Evaluation (Full Build & Incremental Patch) ───────────────

fn bench_draworder_evaluation(runner: &mut BenchmarkRunner) {
    let total_entities = if runner.quick_mode { 2_000 } else { 10_000 };
    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(total_entities);
    for i in 0..total_entities {
        let x = (i % 100) as f64 * 30.0;
        let y = (i / 100) as f64 * 30.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 20.0, y + 20.0, 0.0);
        let h = scene.add_entity(EntityType::Line(line));
        handles.push(h);
    }

    // ── Full build ──
    if runner.should_run("draworder_depth_map_full_build") {
        let runs = if runner.quick_mode { 5 } else { 10 };
        let mut samples = Vec::with_capacity(runs);
        for _ in 0..runs {
            scene.invalidate_draw_depth();
            let t0 = Instant::now();
            let depths = scene.draw_depth_map();
            black_box(depths);
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            samples.push(elapsed_ms);
        }
        let median_ms = samples[samples.len() / 2];
        let throughput = (total_entities as f64) / (median_ms / 1000.0);
        runner.record(
            "draworder_depth_map_full_build",
            "Initial draworder depth map construction across 10,000 entities",
            "ms",
            samples,
            Some((throughput, "entities/s")),
            Some(15.0),
        );
    }

    // ── Incremental patch ──
    if runner.should_run("draworder_depth_map_incremental_patch") {
        let _ = scene.draw_depth_map();

        let n_patch = if runner.quick_mode { 100 } else { 1_000 };
        let runs = 5;
        let mut samples = Vec::with_capacity(runs);

        for _ in 0..runs {
            let t0 = Instant::now();
            for j in 0..n_patch {
                let h = handles[j % handles.len()];
                scene.bump_entities(&[(h, ChangeKind::Modified)]);
                let depths = scene.draw_depth_map();
                black_box(depths);
            }
            let per_us = (t0.elapsed().as_micros() as f64) / (n_patch as f64);
            samples.push(per_us);
        }
        let median_us = samples[samples.len() / 2];
        runner.record(
            "draworder_depth_map_incremental_patch",
            "Draworder depth map fast-path retrieval under entity modifications",
            "µs",
            samples,
            Some((1_000_000.0 / median_us, "patches/s")),
            Some(50.0), // Target threshold < 50 µs
        );
    }
}

// ── 17. Delta-Undo Transaction Before-Image Recording ───────────────────────

fn bench_undo_delta_recording(runner: &mut BenchmarkRunner) {
    let name = "undo_delta_recording_cycle";
    if !runner.should_run(name) {
        return;
    }

    let total = if runner.quick_mode { 1_000 } else { 5_000 };
    let mutate_count = if runner.quick_mode { 100 } else { 500 };

    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(total);
    for i in 0..total {
        let x = (i % 100) as f64 * 30.0;
        let y = (i / 100) as f64 * 30.0;
        let mut line = Line::new();
        line.start = Vector3::new(x, y, 0.0);
        line.end = Vector3::new(x + 20.0, y + 20.0, 0.0);
        let h = scene.add_entity(EntityType::Line(line));
        handles.push(h);
    }

    let runs = if runner.quick_mode { 5 } else { 15 };
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let t0 = Instant::now();
        scene.begin_undo_recording();
        for &h in handles.iter().take(mutate_count) {
            let before = scene.document.get_entity(h).map(|e| Arc::new(e.clone()));
            scene.record_undo_before(h, before);
        }
        let rec = scene.take_undo_recording();
        black_box(rec);
        let elapsed_us = t0.elapsed().as_micros() as f64;
        samples.push(elapsed_us);
    }

    let median_us = samples[samples.len() / 2];
    let throughput = (mutate_count as f64) / (median_us / 1_000_000.0);
    runner.record(
        name,
        "Capturing delta-undo transaction before-images for 500 entity mutations",
        "µs",
        samples,
        Some((throughput, "records/s")),
        Some(1000.0), // Target threshold < 1000 µs (1 ms)
    );
}

// ── Main Entrypoint ─────────────────────────────────────────────────────────

fn main() {
    println!("\nInitializing OpenCADStudio Performance Benchmarks...");
    let mut runner = BenchmarkRunner::new();

    if runner.quick_mode {
        println!("Mode: QUICK (reduced iteration/entity counts for fast validation)");
    } else {
        println!("Mode: FULL (production scale benchmarks for baseline metrics)");
    }

    bench_scene_entity_ingestion(&mut runner);
    bench_analytical_tessellation_zoom(&mut runner);
    bench_spatial_index_construction(&mut runner);
    bench_hit_test_picking(&mut runner);
    bench_osnap_evaluation(&mut runner);
    bench_geometric_constraint_solving(&mut runner);
    bench_dxf_io_throughput(&mut runner);
    bench_selection_cloning(&mut runner);
    bench_ui_ribbon_view_construction(&mut runner);
    bench_ui_grid_geometry(&mut runner);
    bench_ui_icon_caching(&mut runner);
    bench_wide_and_tapered_arc_tessellation(&mut runner);
    bench_zoom_extents_calculation(&mut runner);
    bench_batch_entity_mutation(&mut runner);
    bench_draworder_evaluation(&mut runner);
    bench_undo_delta_recording(&mut runner);

    runner.finish();
}
