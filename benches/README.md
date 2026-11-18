# OpenCADStudio Performance Benchmarks

The Rust suite measures CAD subsystems and UI construction using controlled
fixtures. Separate GUI scripts measure real drawing workloads. Subsystem timing,
automation response time, present cadence, and displayed-frame timing are
different metrics; none establishes the others.

Internal measurements, captures and analysis must remain untracked in
[`local_measurements/`](local_measurements/README.md). Commit reusable benchmark
code, tests and general instructions only. Keep actual results and
machine/drawing-specific conclusions out of tracked documentation and commit
descriptions, including sanitized results. Correctness tests alone do not
establish a performance improvement.

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

## Coverage audit and PERF-off GUI interactions

Use a local baseline to identify hypotheses, isolate one cause at a time, then
compare a proposed change against matching workloads. A single current-build
capture does not establish the impact of previous optimizations.

Memory-isolation controls are available with `--scenario idle`, `edits`,
`navigation`, `layout_churn`, and `state_requests`. Fixed-layout controls require
one `--layouts` value and retain the same panes across cycles. `layout_churn`
deliberately recreates them. Use `--cycles` and `--cycle-seconds` to compare equal
work counts and pacing; `--autosave-minutes 0` removes periodic saving from these
controls, and `--history-max-entries` overrides retention only in the owned child.
Compare `state_requests` over `--transport native` and `--transport http` to
identify control-channel overhead. A read-only request control is essential:
automation itself can allocate memory even when the drawing is unchanged.
Reports retain transport call counts, including native operation-status polls.
Background memory controls do not establish displayed-frame performance.
The fixed `edits` control activates and fits each pane once after creating its
layout, then returns to the first pane. Its separate `fit_panes` phase records
each fitted camera; compare these alongside viewport dimensions before pooling
edit timings. Pane setup is excluded from the `move_retained` measurements.

| Existing benchmark | What it measures | Remaining gap |
| --- | --- | --- |
| `performance_benchmarks`, `command_benchmarks` | Synthetic ingestion, geometry, hit testing, snapping, solver, IO, UI/overlay/pipeline construction and command work | Actual drawing mix, window presentation and sustained interactive sessions |
| `navigation_selection_bounds`, `scene_pick_micro`, `large_overlay_interaction`, `overlay_interaction_candidates` | Controlled CPU navigation bounds, picking and overlay queries, including large synthetic scenes | Real GUI input, render/present cadence and actual drawing geometry |
| `selection_overlay_frame`, `wire_view_culling`, `wires_build_visible` | Isolated overlay replay, culling and wire preparation | Full-window compositing/display, multi-viewport GUI interaction |
| `drawing_open_memory`, `cache_memory_budget`, `save_batch_memory`, history/GPU audit probes | Requested Rust allocations, retained capacities or instrumented subsystem costs | Normal production allocator residency and interactive frame behavior |
| `drawing_startup.py` | Real drawing installation diagnostic with `PERF` enabled; settled RSS/USS | PERF-off interactions, first display, edits and long sessions |
| **`drawing_interaction.py`** | **PERF-off real DWG/DXF GUI replay, RSS/USS/private commit, normal `MOVE`/undo, retained edit history, 1/2/4 viewports; optional external ETW present/display data** | Hardware input-to-photon latency, hours-long human workflows, other drawing/feature combinations |

Build again before collecting a baseline. The interaction harness records CLI
version/revision, binary and drawing hashes, the live editor version, settings
hashes, adapter/driver, viewport dimensions and foreground checks. Use
`--expected-revision` to reject a stale executable:

```powershell
cargo build --locked --release --bin OpenCADStudio
python -m pip install -r benches/requirements.txt
python benches/drawing_interaction.py --executable target/release/OpenCADStudio.exe --drawing "C:/drawings/typical.dwg" --runs 1 --session-seconds 900 --expected-revision YOUR_BUILD_REVISION --output benches/local_measurements/interaction-baseline.json
```

This Windows harness starts a new GUI process with saving disabled
(`--read-only` permits in-memory editing). The original drawing path is retained
for relative reference resolution. It copies the user's settings into an
isolated config directory, uses a fresh GPU crash-sentinel directory, suppresses
managed-deployment notices, removes `PERF` from the child environment (setting
`PERF=0` would still enable tracing), and refuses a PERF panel or PERF log output.
It checks source-file hashes after successful runs. Private drawing files,
settings, screenshots and local paths remain in the output/artifact directory;
never commit those artifacts, including sanitized summaries.

The drawing and nonzero viewport must become queryable before setup begins.
Readiness is **not first paint**. Setup selects a real, visible model-space line,
proves a normal `MOVE` changes it, and proves undo restores its geometry. The
model-space owner comes from the block records; capped screenshot annotations
can omit it in drawings containing many block definitions. The edit target must
also be selected by a pointer hit before replay.
Use `--wait-for-start-seconds 300` to arrange the owned benchmark window before
timing. The run writes `ready.json` in its artifact directory, waits for
`start.signal` there, then records the actual viewport dimensions. Creating that
signal starts measurement; keep the window foreground and unobstructed for
display comparisons.

The workload replays middle-button pan and wheel zoom through the editor's pointer
handlers, performs pointer hit selection, then measures normal `MOVE`/undo and
paired forward/backward moves that retain undo history. Selection clicks are
spaced beyond Windows' double-click interval to avoid opening an inline editor.
`VPORTS` must confirm each requested layout; pan/zoom must change the camera;
edits must restore the original line. The GUI currently exposes presets up to
four model viewports; the eight-slot GPU collection probe is a separate synthetic
workload.

Each pointer event uses a separate request so the event loop can redraw between
events. The default input cadence is at most 30 requests/second; server response
time can lower it. Readiness uses HTTP, then measurement uses the owned editor's
private loopback control channel with 2 ms operation polling. This avoids the GUI
HTTP adapter's 100 ms settlement-poll floor. **Request roundtrips still include
automation, serialization, scheduling and polling costs. They are not physical
input latency or maximum interaction throughput.** Edit phases separate selection,
command and undo distributions rather than mixing their timings. Viewport setup,
initial screenshots, hashing and memory checkpoints are outside those request
timings. The memory sampler polls RSS and foreground state every 250 ms; USS,
private commit, CPU seconds, handles and thread counts are sampled at checkpoints.
Sampled RSS peaks can miss short spikes. These are process metrics, excluding
children and physical VRAM. An increasing RSS alone does not establish a leak.

Add [PresentMon's standalone console executable](https://github.com/GameTechDev/PresentMon/blob/main/README-ConsoleApplication.md)
to collect external frame data without enabling OCS's PERF instrumentation:

```powershell
python benches/drawing_interaction.py --executable target/release/OpenCADStudio.exe --drawing "C:/drawings/typical.dwg" --runs 1 --session-seconds 900 --presentmon "C:/tools/PresentMon-2.6.0-x64.exe" --wait-for-foreground-seconds 180 --output benches/local_measurements/interaction-frames.json
```

Bring the benchmark window to the foreground during that wait and leave it
unobstructed; avoid interacting with it during replay. The harness reads window
state; it does not steal focus from another application. PresentMon requires ETW
rights, normally an administrator or a member
of [Performance Log Users](https://learn.microsoft.com/en-us/windows/win32/etw/controlling-event-tracing-sessions).
After a membership change, an existing login token can remain unchanged. For the
current session, `--presentmon-elevated` requests UAC for the tracer only; the
editor remains at normal permissions. Each capture has a unique session name;
cleanup stops only that session and the editor process created by this run.
The harness never changes group membership or downloads/installs PresentMon.

`--read-only` blocks manual saving; the application still permits periodic
recovery autosave. Recovery copies remain inside the isolated config; the
harness rejects a custom recovery destination. To isolate autosave, compare
matching `state_requests` controls with `--dirty-start --autosave-minutes 0`
and `--dirty-start --autosave-minutes 1`, running long enough for the timer.
Dirty-start applies inverse edits, verifies unchanged geometry, and leaves the
owned document dirty so recovery saving actually occurs. Reports retain request
start/end QPC timestamps and sampled recovery-file changes. File observations
show filesystem changes, not save start or completion (an empty destination can
appear before replacement), and have the memory sampler's polling uncertainty.
Mutation completion can wait for active save jobs by design; slow mutation
replies alone do not establish an
event-loop stall. Read-only probes isolate responsiveness without that gate.

The CSV contract is `--v1_metrics --qpc_time` (validated with PresentMon 2.6.0).
Frame data is filtered by editor PID and split by swap chain and phase using QPC.
The first present interval in each phase/chain is excluded because it can span
setup or an idle period. Reports retain displayed and dropped counts; missing
ETW data is unknown, never zero. Present-to-display delay is **not input-to-display
latency**, and pointer replay does not generate hardware-input timestamps. Check
foreground samples and present modes before interpreting display data. A
foreground check at a sampling instant does not prove continuous lack of
occlusion. PresentMon itself also has overhead; compare captures with matching
instrumentation. A redraw notification or screenshot completion is not a display
timestamp.

Use `--drawing` multiple times for a local corpus; runs are sequential. The
default is three fresh processes per drawing and at least 15 minutes per process.
The last complete cycle can extend the requested duration. A short validation is
`--runs 1 --session-seconds 0 --steps 6 --edit-steps 2 --settle-seconds 1`.
Compare repeated processes with the same drawing hash, references, fonts, graphics
settings, driver, viewport dimensions, cadence, settings, foreground conditions
and power/background workload. This is a current-build baseline, not proof of
zero performance impact from earlier changes. Add multi-hour runs, drawing swaps,
save/reopen, large selections, heavy edits, xrefs, rasters and 3D corpora before
generalizing to all CAD use.

```powershell
python -m unittest discover -s benches -p 'test_drawing*.py'
python benches/summarize_drawing_interaction.py benches/local_measurements/interaction-frames.json --output benches/local_measurements/interaction-summary.json
```

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
are not included in RSS/USS. The report separately parses structured
`gpu-live-json` samples, retaining the last sample and highest observed total.
These count logical sizes of live renderer resources, deduplicating shared GPU
handles across slots and caches: targets, shadow maps, text atlases, wire arenas,
fallback/analytical wires, meshes, image/material textures, text buffers,
selection/preview overlays, hatches and point clouds. They exclude driver heap
alignment, staging/commands, pipelines, iced and ViewCube resources; Depth24Plus
storage is estimated. They are not physical VRAM usage and must not be added to
RSS (especially on shared-memory GPUs). Missing structured telemetry is unknown,
not zero. USS is private resident memory, not private committed memory.
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

## Raster and point-cloud cache retention

```sh
cargo bench --locked --bench cache_memory_budget -- --output target/cache-before.json
cargo bench --locked --bench cache_memory_budget -- --baseline target/cache-before.json --output target/cache-after.json
cargo bench --locked --bench cache_memory_budget -- --scenario image_decode_retention --max-retained-mib 130
cargo bench --locked --bench cache_memory_budget -- --self-test
```

Every scenario runs in a fresh process. The fixed PDF, image, material, and
point-cloud workloads include a sequential load/revisit sweep and three passes
over a small, recently used subset. A revisit is not guaranteed to hit: the
sequential working set exceeds the default budgets. Per-operation times expose
the cost of redecoding; they include counting-allocator instrumentation.

Schema 2 records revision, platform, profile, CPU count, effective cache budgets,
and a SHA-256 of fixture content and access order. PDF fingerprints use page
operations rather than generated timestamps/IDs. Fixture generation, source
registration, file writes, hashing, labels, and sample storage precede the
baseline. Setup time/peak are separate; `peak_live_bytes` is reset at that
baseline, and `retained_bytes` is final live bytes minus baseline live bytes.
Live totals include fixture/process overhead; these are requested System
allocator bytes, not RSS, private commit, GPU bytes, or production mimalloc
residency. Keep the same machine and build settings when comparing runs.

`--output` writes a new JSON array file (also for one scenario). `--json` emits
only that array to stdout; `--scenario` emits one JSON object. Baselines must
match workload, labels, platform, profile, CPU count and cache budgets. A memory
regression over 10% plus 64 KiB fails the run; override the percentage with
`--max-regression-percent`. `--max-retained-mib` optionally sets an absolute
retention ceiling per scenario. Reports are saved before these checks so a
failure can be inspected. Timings are diagnostic and do not gate comparisons.
Old reports must be regenerated because their peaks included fixture setup.

Undo history uses capacity estimates for common large geometry, text, ACIS,
graphic-byte and raw-DWG payloads. Shared entity images and raw records are
charged once across undo and redo; structural/style and uncommon entity
payloads remain approximate. This is an eviction budget, not a process-memory
ceiling, and deferred destruction can temporarily keep evicted entries live.
Run its regression cases and optional dense-history accounting latency probe:

```sh
cargo test --locked --lib undo_budget -- --test-threads=1
cargo test --locked --release --lib undo_budget_dense_history_probe -- --ignored --nocapture
cargo test --locked --lib undo_budget_before_after_retention_probe -- --ignored --nocapture --test-threads=1
```

The latency probe scans 256 entries with 1,000 shared entity images each, without
changing the workload between checks. Debug timings are not release baselines.
The retention probe compares the pre-change 512-byte entity allowance with the
current policy at the default 512 MiB / 256-entry limits. Each sequential arm
creates 160 undo entries holding populated 100,000-vertex polylines (about 733 MiB
of vertex buffers), then reports retained entry count, budget estimate, and
unique vertex-buffer capacity after dropping evicted entries. Capacity results
are valid in debug builds; these are not RSS or deferred-drop backlog readings.

## Multi-document save preparation

```sh
cargo bench --locked --bench save_batch_memory -- --output target/save-batch.json
cargo bench --locked --bench save_batch_memory -- --flow autosave
cargo bench --locked --bench save_batch_memory -- --self-test
cargo test --locked --lib prepare_save_tests -- --test-threads=1
```

Six synthetic drawings each contain 250 linear dimensions without baked blocks.
The probe keeps the live documents and captures their snapshots before its
baseline, then compares eager preparation with the production lazy save-batch
iterator. Each strategy/flow uses a fresh process, with font/serialization warmup
before measurement. AUTOSAVE drops each written byte buffer; SAVEALL retains
them until the batch finishes, matching their different completion paths.
Both drop each prepared document after its writer finishes.

Reports include fixture hashes, per-document entity/block counts, output byte
counts and SHA-256 hashes. Eager/lazy outputs must match, and lazy preparation
must reduce the measured peak on this fixture. `--output` writes a new JSON
array file before checking peak reduction; `--strategy eager|lazy` runs one arm
and accepts `--flow autosave|saveall` (default autosave).

The peak delta counts requested Rust System allocation bytes above the captured
snapshot baseline for preparation and serial DXF writing. It excludes fixture
setup, allocator overhead, GUI/GPU, browser/JS memory and worker transfers. This
native probe exercises shared preparation/writer code; it does not measure
browser residency or desktop concurrent saves. `--profile dev` can validate
allocation metrics without a release build; no timing metrics are collected.
Compare on the same machine with the same profile and matching fixture/output
hashes. The unit cases also check captured contents after a tab edit/close,
pending-snapshot lifetimes, versions, and identical DWG/DXF writer output.

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
