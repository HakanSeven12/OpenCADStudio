# OpenCADStudio Performance Benchmarks

A standardized, statistical performance benchmarking suite measuring throughput, latency, and memory metrics across core CAD subsystems and the user interface.

## Quick Start

Run the full benchmark suite with native `--release` optimizations:
```bash
cargo bench
```

Run a fast validation smoke test (~2 seconds):
```bash
cargo bench --bench performance_benchmarks -- --quick
```

> [!NOTE]
> The Rust benchmarks are registered under `[[bench]]` with `harness = false`.
> They are **never** executed by regular `cargo test` runs, preventing CI slowdowns.

## Drawing startup and memory

The DXF parser benchmark below uses synthetic lines and circles; it does not
measure opening a text-heavy drawing in the GUI. For that path, build a fresh
release executable and run this Python 3.10+ benchmark in a graphical desktop
session on Windows, macOS, or Linux from the repo root, using a drawing you
have permission to read:

```sh
cargo build --release --locked --bin OpenCADStudio
python -m pip install -r benches/requirements.txt
python benches/drawing_startup.py --executable PATH_TO_RELEASE_EXECUTABLE --drawing PATH_TO_DRAWING
```

Each run starts its own editor (`--new-instance`) with `PERF` and startup
logging enabled. It measures from process launch until the application's
`open-ui-thread` diagnostic arrives, then samples process resident memory
(RSS) and, where available, unique set size (USS) 15 seconds later. It also
records the peak RSS observed at the polling interval. The open diagnostic is
emitted after document installation; it does not prove that the first frame,
window responsiveness, or all background work has completed. It is a different
metric from a window-ready measurement. GPU memory and child-process memory
are not included. USS is private resident memory, not private committed memory.
Tracing affects timings, so compare only runs with the same instrumentation.

The report defaults to `target/drawing-startup-benchmark.json`; use `-Output`
to choose another path. It records the drawing and executable SHA-256 hashes,
OS, CPU, graphics driver, launch settings, and individual runs. Compare only
reports with the same drawing hash, machine, GPU adapter and driver, viewport
size, working directory, graphics and shader-cache settings, sampling settings,
fonts, and external drawing references. Executable hashes may differ when
comparing builds. Window and viewport size are not detected; keep them fixed
manually. The script does not clear caches: the first observed launch and
later launches must be compared separately, and none is guaranteed to be a cold
start. It hashes the input and executable after the runs, which can warm the
filesystem cache for a subsequent invocation. The app may update its normal
recent-file and other user settings while running.

Each run uses its own empty GPU-crash sentinel directory, so ending a short
benchmark cannot mark the user's normal graphics session as crashed or force a
different backend on the next sample. The effective saved graphics preferences
are copied into that directory for each run. This measures normal startup, not
crash recovery. The benchmark terminates only the process it starts after each
sample. Check the report's `adapter_consistent`, `workload_consistent`,
`gpu_adapter`, and `termination` fields before comparing samples; investigate
missing or different values. Workload consistency is unknown unless the entity,
wire-model and text-vertex counts were all observed.

Run the benchmark's process-lifecycle and report tests without opening a GUI:

```sh
python -m unittest discover -s benches -p 'test_drawing_startup.py'
```

## Drawing-open Rust allocation probe

This native, cross-platform probe runs the drawing-open parser, linetype and
external-reference preparation, derived-cache construction, and model-space
geometry preparation without starting the GUI. It uses a counting wrapper over
the system allocator to report live and peak *requested Rust allocation bytes*
at each stage, plus the capacities of selected wire and text buffers. It also
reports entity counts and the drawing's SHA-256 so the same work can be checked
between runs. The input is supplied locally; no drawing is added to the repo.

```sh
cargo bench --locked --bench drawing_open_memory -- --input PATH_TO_DRAWING --expected-texts 10000
```

Use `--output PATH` to additionally write JSON to a **new** file. The optional
`--expected-texts` check fails if the fixture no longer has the specified
number of TEXT entities. Run a fresh process per sample; the probe waits two
seconds after geometry preparation for background index work, then reports the
state after dropping its document and geometry. Process-wide caches and worker
threads may remain live. A final diagnostic stage clears the shaping and
fallback caches in the probe process only; their hash-table capacity and other
process-wide caches may remain allocated. The report also includes CPU text
atlas bytes. Test the counting allocator itself with:

```sh
cargo bench --locked --bench drawing_open_memory -- --self-test
```

These numbers are not RSS: they exclude allocator bookkeeping and unused heap
pages, allocations bypassing Rust's global allocator, GUI and GPU resources,
and child processes. Peak is a process-wide high-water mark, not a per-phase
peak, and allocation totals exclude reallocation sizes. The instrumentation
also adds runtime overhead, so its timings must not be compared with an
uninstrumented GUI benchmark. The probe disables `PERF` logging in its own
process and records whether resident memoization is enabled. Keep the input
hash, external references, fonts, memoization setting, build profile and
machine constant when comparing reports.

## Text shaping and geometry

This benchmark generates a deterministic synthetic text workload; it does not
read the original drawing or require an external drawing. It reproduces the
source drawing's 10,000 TEXT entities, 3,758 distinct strings, frequency of
each repetition count, text-length and character-pattern distributions, and
three frequently repeated values (2,017, 1,981 and 1,980 occurrences). The
values are synthetic. Text height is 1.0, width factor is 0.87, obliquing is
zero, and rotations and positions are generated within the original ranges.
Their individual values and entity order do not match the source. The fixture
does not model alignment, styles beyond one TrueType family, DXF parsing,
document caches or drawing-open behavior.

The benchmark calls the production TrueType shaping and text tessellation
paths for the first 256 distinct synthetic strings. Its cold-shape sample clears the process-wide
shaping and fallback caches before timing; font discovery and the font system
are already initialized. It then times the same strings as shape-cache hits
and verifies that every hit returns the cached run. Finally it primes and times
text tessellation with fixed placement, rotation, width and obliquing. Shape
digests include glyph outlines, triangles, advances and character cells;
geometry digests include every output point. Each sample must match. It also
clears the cache and times tessellation of all 10,000 synthetic text values,
including cache misses and hits, using their generated positions and rotations.

```sh
cargo bench --locked --bench text_shaping_geometry -- --font Arial --output NEW_REPORT.json
```

`--font` selects an installed TrueType family; otherwise the first available family is selected.
Without any installed TrueType family, the default invocation reports a skip.
Use `--samples N` to choose the sample count (default 7). Output is JSON and
the optional report path must be new. The report records the ordered synthetic
fixture and font hashes and face index. Keep these, the executable version,
machine, font environment and sample count consistent when comparing runs.

Cold shaping includes text layout, outline flattening, and triangulation; it
does not split their costs. Warm tessellation includes tokenization, cache
lookups, output allocation, and point transforms; it is not a transform-only
microbenchmark. Both exclude digest calculations. Repeated samples do not
model a fresh process or cold operating-system caches. This benchmark does not
measure memory, GPU rendering, or drawing-open latency; use the drawing-open
allocation probe and GUI startup benchmark for those metrics.

For Rust allocation counts and live/peak requested bytes on the same full
synthetic workload, run a fresh process with:

```sh
cargo bench --locked --bench text_shaping_memory -- --font Arial
cargo bench --locked --bench text_shaping_memory -- --self-test
```

The allocation probe uses a counting global allocator, initializes fonts and
the fixture before its baseline, clears text caches, and records memory while
geometry is alive, after dropping it, and after clearing text caches. It emits
the same fixture, font, and geometry hashes as the timing benchmark. It counts
Rust System allocator requested bytes, excluding allocator overhead, foreign
allocations, GUI, GPU and child processes; allocation byte totals exclude
reallocation sizes. Counting allocations adds overhead, so the probe is kept
separate from the timing benchmark.

To exercise glyphs at a wider range of pen positions, pass `--fixture varied`
to either benchmark. This selects 256 deterministic distinct alphanumeric
labels of 24–40 characters at a local origin; its small cache-hit sample uses
the first 16 labels. The default `drawing` fixture is unchanged. Compare only
reports with matching fixture, font and geometry hashes; this varied workload
is synthetic and is not a model of the 10,000-text drawing.

---

## Benchmark Suite Catalog (31 Metrics)

### 1. Scene & Entity Management
- `scene_entity_ingestion_10k`: Ingestion throughput for 10,000 mixed 2D entities (Lines, Circles, Arcs, Polylines).
- `batch_entity_transform_mutation`: Translating 1,000 entities, invoking `bump_entities`, and updating the spatial acceleration structure.
- `undo_delta_recording_cycle`: Capturing before-images and transactional delta recordings for 500 entity mutations.

### 2. Geometry Tessellation & Viewport Navigation
- `analytical_curved_tessellation`: Initial wire generation for 14,000 curved entities (circles, arcs, ellipses, polylines).
- `wire_partitioning_per_frame`: Partitioning 14,000 wires into GPU analytical instances vs regular lines.
- `analytical_curved_zoom_100x`: 100 camera zoom navigations measuring frame overhead and FPS.
- `wide_and_tapered_arc_tessellation`: Tessellating 7,500 thick polyline arcs, tapered arcs, and donuts.
- `zoom_extents_bounding_box_calc`: Cold axis-aligned bounding box (AABB) calculation across 20,000 mixed entities (`ZOOM EXTENTS`).

### 3. Spatial Indexing, Snapping & Picking
- `spatial_interaction_index_build`: Building the spatial `InteractionIndex` and `prepare_screen` for 10,000 wires.
- `hit_test_click_pick_latency`: Single-point cursor click selection across 5,000 entities in screen space.
- `hit_test_box_window_enclosed`: Window selection (`box_hit` enclosed) across 5,000 entities.
- `hit_test_box_crossing_boundary`: Crossing selection (`box_hit` crossing boundary) across 5,000 entities.
- `osnap_cursor_tracking_latency`: Real-time cursor snapping across Endpoint, Midpoint, Center, Intersection, and Nearest modes.

### 4. Parametrics & DXF I/O
- `constraint_solver_dogleg_system`: Solving coupled 2D geometric constraints (Perpendicular, Parallel, Length) via the Dogleg solver.
- `dxf_export_write_throughput`: Serializing 5,000 entities to DXF binary stream.
- `dxf_import_parse_throughput`: Parsing DXF stream of 5,000 entities into `Document`.

### 5. UI & Viewport Overlay Performance
- `ui_ribbon_view_construction`: Per-frame ribbon widget tree construction.
- `ui_grid_geometry_uncached`: Multi-pane tiled viewport grid projection and segment generation.
- `ui_grid_cache_hit_evaluation`: Grid overlay cache key construction and reuse decision.
- `ui_icon_handle_cached`: Themed SVG icon cache hit (FxHashMap lookup + Handle clone).
- `ui_icon_handle_uncached`: Uncached SVG icon memory parse and Handle allocation.
- `ui_plotstyle_layer_usage`: Plot Style 256-bucket ACI layer-usage table rebuild (200 layers).
- `ui_statusbar_derived_data`: Status-bar cached derived-data hit (layout names + scale picker `Arc` clones).
- `ui_grip_budget`: Budgeted per-frame selection-grip projection (dense-polyline grips capped to `MAX_SELECTED_GRIPS`).
- `ui_grip_budget_build`: Selection-change `apply_grip_budget` cost over the full grip set (sort + FxHashSet + filter).
- `ui_constraint_glyphs`: Constraint-glyph cache hit (`cached_glyph_placements` key build + `Arc` clone over dozens of glyphs, no recompute).
- `ui_selection_overlay`: Selection overlay Element construction (widget tree with capped grip count, no canvas draw).

### 6. State Management & Draworder
- `selection_state_deep_clone`: Deep cloning of `SelectionState` (with 64 polygon points).
- `selection_state_arc_clone`: `Arc<SelectionState>::clone` (atomic reference bump).
- `draworder_depth_map_full_build`: Full draworder depth map generation across 10,000 ordered entities.
- `draworder_depth_map_incremental_patch`: Fast-path incremental depth map patch on entity modification.

---

## Command Benchmarks

First benchmarked commands: **EXPLODE** (metrics `explode_geometry`,
`explode_scene_apply`) and **FLATTEN** (metrics `flatten_plan`,
`flatten_scene_apply`).

```bash
cargo bench --bench command_benchmarks -- --filter explode
cargo bench --bench command_benchmarks -- --quick
cargo bench --bench command_benchmarks -- --filter explode --output target/explode_metrics.json
cargo bench --bench command_benchmarks -- --filter explode --baseline target/explode_before.json
```

## Filtering Benchmarks

Filter execution by subsystem or name pattern:
```bash
# Filter by UI
cargo bench --bench performance_benchmarks -- ui

# Filter by tessellation
cargo bench --bench performance_benchmarks -- tessellation

# Filter by hit testing
cargo bench --bench performance_benchmarks -- hit_test
```

---

## Baseline Tracking & Regression Detection

### 1. Save Current Metrics as Baseline
```bash
cargo bench --bench performance_benchmarks -- --output target/baseline_metrics.json
```

### 2. Compare PR / Branch Against Baseline
```bash
cargo bench --bench performance_benchmarks -- --baseline target/baseline_metrics.json
```

The benchmark runner will output statistical deltas (`% faster` / `% slower`) for each metric and flag regressions exceeding the defined thresholds.

Baselines must use the same suite, quick/full mode, operating system,
architecture, and build profile. Matching metrics must also retain their
description and units. Reports created before suite identifiers were added
must be regenerated. These checks cannot detect every workload change: keep
the inputs and measured operation identical when comparing revisions, and use
the same machine and comparable system conditions.
