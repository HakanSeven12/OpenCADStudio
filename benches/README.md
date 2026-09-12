# OpenCADStudio Performance Benchmarks

A standardized, statistical performance benchmarking suite measuring throughput, latency, and memory metrics across core CAD subsystems and the user interface.

## Quick Start

Run the full benchmark suite with native `--release` optimizations:
```powershell
cargo bench
```

Run a fast validation smoke test (~2 seconds):
```powershell
cargo bench --bench performance_benchmarks -- --quick
```

> [!NOTE]
> Benchmarks are registered under `[[bench]]` with `harness = false`. They are **never** executed by regular `cargo test` runs, preventing CI slowdowns.

---

## Benchmark Suite Catalog (25 Metrics)

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
- `dxf_import_parse_throughput`: Parsing DXF stream of 5,000 entities into `CadDocument`.

### 5. UI & Viewport Overlay Performance
- `ui_ribbon_view_construction`: Per-frame ribbon widget tree construction.
- `ui_grid_geometry_uncached`: Multi-pane tiled viewport grid projection and segment generation.
- `ui_grid_cache_hit_evaluation`: Grid overlay cache key construction and reuse decision.
- `ui_icon_handle_cached`: Themed SVG icon cache hit (FxHashMap lookup + Handle clone).
- `ui_icon_handle_uncached`: Uncached SVG icon memory parse and Handle allocation.

### 6. State Management & Draworder
- `selection_state_deep_clone`: Deep cloning of `SelectionState` (with 64 polygon points).
- `selection_state_arc_clone`: `Arc<SelectionState>::clone` (atomic reference bump).
- `draworder_depth_map_full_build`: Full draworder depth map generation across 10,000 ordered entities.
- `draworder_depth_map_incremental_patch`: Fast-path incremental depth map patch on entity modification.

---

## Filtering Benchmarks

Filter execution by subsystem or name pattern:
```powershell
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
```powershell
cargo bench --bench performance_benchmarks -- --output target/baseline_metrics.json
```

### 2. Compare PR / Branch Against Baseline
```powershell
cargo bench --bench performance_benchmarks -- --baseline target/baseline_metrics.json
```

The benchmark runner will output statistical deltas (`% faster` / `% slower`) for each metric and flag regressions exceeding the defined thresholds.
