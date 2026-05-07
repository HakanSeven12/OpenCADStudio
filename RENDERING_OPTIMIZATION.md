# Rendering Optimization Roadmap: Culling & Level of Detail

## Background

H7CAD renders all entities every frame regardless of camera position or zoom level. No spatial
acceleration, no LOD, no frustum culling. Every wire, hatch, mesh, and image is uploaded and
drawn unconditionally. This scales poorly for large drawings (100k+ entities) and dense 3D solids.

Current pipeline order:
1. Hatch fills → 2. Images → 3. Meshes → 4. Face3D fills → 5. Face3D edges →
6. Wires → 7. Wipeouts → 8. Selection overlay → 9. MSAA resolve → 10. Blit

---

## Phase 1 — Viewport Bounding-Box Culling

**Goal:** Skip CPU upload and draw calls for entities outside the camera view.

### 1.1 Entity AABB

Compute axis-aligned bounding boxes during tessellation and store alongside each model.

```rust
// Attach to WireModel, MeshModel, HatchModel, ImageModel
pub struct Aabb2 {
    pub min: DVec2,
    pub max: DVec2,
}

pub struct Aabb3 {
    pub min: DVec3,
    pub max: DVec3,
}
```

- `WireModel` / `HatchModel` → `Aabb2` (XY plane for 2D; full 3D AABB for 3D wires)
- `MeshModel` → `Aabb3`
- `ImageModel` → `Aabb2` from insertion point + extents

Compute once at tessellation time (`scene/tessellate.rs`), store in the model structs, invalidate
with the geometry epoch.

### 1.2 Camera Frustum / Viewport Rectangle

Extract view bounds from `camera.rs` each frame:

- **Orthographic (2D):** viewport rectangle in world space — a simple `Aabb2` test suffices.
- **Perspective (3D):** extract 6 frustum planes from the view-projection matrix.

```rust
pub enum ViewVolume {
    Ortho(Aabb2),
    Frustum([Vec4; 6]),  // plane equations
}
```

### 1.3 CPU-Side Cull Before Upload

In `pipeline/mod.rs`, before collecting wire/hatch/mesh draw calls:

```rust
fn is_visible(aabb: &Aabb2, view: &ViewVolume) -> bool {
    match view {
        ViewVolume::Ortho(rect) => aabb_overlap_2d(aabb, rect),
        ViewVolume::Frustum(planes) => aabb_inside_frustum(aabb, planes),
    }
}
```

Filter entity lists before the upload loop. Skip `queue.write_buffer` entirely for invisible
entities — saves both CPU and GPU bus bandwidth.

### 1.4 Scissor Rects (Already Partial)

Viewport scissoring for wires already exists (`wire_pixel_scissors`). Extend same logic to
hatch and image passes.

**Estimated gain:** 50–90% draw call reduction for typical workflows (zoomed in on one region).

---

## Phase 2 — Spatial Acceleration Structure

**Goal:** O(log n) visibility queries instead of O(n) linear scan.

### 2.1 Quadtree (2D Documents)

Partition world space into a quadtree keyed by `Aabb2`. Build once on document load, update
incrementally on entity add/remove/modify.

```
QuadTree<EntityId>
  ├── query_rect(Aabb2) → Vec<EntityId>
  └── insert / remove / update
```

Store in `scene/mod.rs` alongside the entity cache. Invalidate only the nodes touched by a
modified entity.

### 2.2 Octree (3D Solids)

For `MeshModel` / `solid3d` entities, use an octree with `Aabb3` keys. Query returns candidate
mesh IDs for frustum culling.

### 2.3 Integration

Replace linear entity scans in `pipeline/mod.rs` render preparation with spatial queries:

```rust
let candidates = scene.quadtree.query_rect(view_aabb);
// only upload GPU buffers for candidates
```

**Estimated gain:** near-constant frame cost regardless of total entity count when zoomed in.

---

## Phase 3 — Level of Detail

**Goal:** Reduce geometric complexity when entities are small or far away.

### 3.1 Screen-Space Size Filter (Sub-Pixel Cull)

Before drawing, compute projected pixel size of entity AABB diagonal. Skip entities whose
projected size is below a threshold (e.g. < 0.5 px).

```rust
let screen_px = project_size(aabb, camera);
if screen_px < MIN_VISIBLE_PX { continue; }
```

Applies to all entity types. Effectively free — just a float comparison after AABB cull.

### 3.2 Wire LOD — Curve Segment Reduction

Arcs, ellipses, and splines tessellate to N segments (currently fixed). Replace with
zoom-adaptive segment count.

```rust
fn arc_segments(radius_world: f64, px_per_unit: f64) -> u32 {
    // target: ~1 segment per 2px of arc length on screen
    let arc_px = radius_world * px_per_unit * TWO_PI;
    (arc_px / 2.0).clamp(8.0, 256.0) as u32
}
```

Recompute segments when zoom crosses a threshold (epoch-based invalidation already in place).
Store LOD level per entity; retessellate only on LOD level change.

LOD levels (example):

| Zoom (px/unit) | Segments per 90° arc |
|----------------|----------------------|
| > 100 px/u     | 64 (full detail)     |
| 10–100 px/u    | 24                   |
| 1–10 px/u      | 12                   |
| < 1 px/u       | 6 (or sub-pixel cull)|

### 3.3 Hatch LOD — Density Reduction

At low zoom, hatch line spacing appears smaller than 1px. Options:

- **Solid fill substitution:** replace hatch pattern with solid color below a density threshold.
- **Skip hatch entirely:** at very low zoom, hatches are not readable; skip the hatch pass.

Threshold: `hatch_spacing_px = hatch_spacing_world * px_per_unit`. If `< 2.0`, use solid fill.

### 3.4 Mesh LOD — Tessellation Resolution

`truck_tess.rs` / `solid3d_tess.rs` tessellate ACIS solids at a fixed tolerance. Switch to
multiple precomputed LOD meshes:

```rust
pub struct MeshModel {
    pub lod: [Option<GpuMesh>; 3],  // [high, mid, low]
}
```

| LOD | Tolerance | Use when projected diagonal |
|-----|-----------|------------------------------|
| 0   | 0.01 mm   | > 200 px                     |
| 1   | 0.5 mm    | 50–200 px                    |
| 2   | 5.0 mm    | < 50 px                      |

Build LOD 0 eagerly; build LOD 1 & 2 lazily in a background thread (already have
async tessellation pattern from `truck_tess.rs`).

### 3.5 Text & Dimension Simplification

At low zoom, dimension text and annotation geometry become unreadable. Replace with
simplified bounding-box proxies or skip entirely below a legibility threshold.

---

## Phase 4 — GPU-Side Culling (Advanced)

**Goal:** Offload culling to the GPU; zero CPU cost for large entity counts.

### 4.1 Indirect Draw + Compute Cull

Convert per-entity draw calls to indirect draw calls (`draw_indirect` / `draw_indexed_indirect`).
Run a compute shader pre-pass that tests each entity's AABB against the frustum and writes
`DrawIndirectArgs` only for visible entities.

```wgsl
// cull.wgsl
@compute @workgroup_size(64)
fn cull_entities(@builtin(global_invocation_id) id: vec3<u32>) {
    let entity = entities[id.x];
    if frustum_test(entity.aabb) {
        // atomically append to indirect draw buffer
        let slot = atomicAdd(&draw_count, 1u);
        draw_args[slot] = entity.draw_args;
    }
}
```

Requires restructuring entity data into GPU-side storage buffers. High complexity; tackle after
Phases 1–3 prove insufficient.

### 4.2 Hierarchical Z-Buffer Occlusion Culling (3D Only)

For dense 3D solid scenes, use a Hi-Z buffer to cull occluded meshes:

1. Render depth-only pass for large opaque solids.
2. Downsample depth into mip chain (Hi-Z pyramid).
3. Compute shader tests each mesh AABB against Hi-Z; skips occluded meshes.

Relevant only for perspective (3D) mode with many overlapping solids.

---

## Implementation Order

```
Phase 1.1  Entity AABB computation          low risk, high impact
Phase 1.2  ViewVolume extraction            low risk
Phase 1.3  CPU cull before upload           immediate draw call reduction
Phase 2.1  Quadtree for 2D                  medium complexity, scales 2D docs
Phase 3.1  Sub-pixel cull                   trivial, free perf
Phase 3.2  Wire curve LOD                   medium, affects tessellation cache
Phase 3.3  Hatch LOD                        low complexity
Phase 3.4  Mesh LOD                         high complexity, background thread
Phase 2.2  Octree for 3D                    medium, needed for dense 3D
Phase 3.5  Text/dim simplification          low complexity
Phase 4.1  Indirect draw + GPU cull         high complexity, defer
Phase 4.2  Hi-Z occlusion                   high complexity, 3D only, last
```

---

## Key Files to Modify

| File | Change |
|------|--------|
| `src/scene/wire_model.rs` | Add `Aabb2` field to `WireModel` |
| `src/scene/mesh_model.rs` | Add `Aabb3`, LOD mesh array |
| `src/scene/hatch_model.rs` | Add `Aabb2` field |
| `src/scene/image_model.rs` | Add `Aabb2` field |
| `src/scene/tessellate.rs` | Compute AABB during tessellation; adaptive arc segments |
| `src/scene/truck_tess.rs` | Multi-resolution mesh tessellation |
| `src/scene/camera.rs` | Expose `ViewVolume` extraction |
| `src/scene/mod.rs` | Quadtree/octree; LOD epoch tracking |
| `src/scene/pipeline/mod.rs` | Cull entities before upload loops |
| `src/shaders/cull.wgsl` | New — Phase 4 compute culling shader |

---

## Success Metrics

- **Phase 1 target:** Large drawing (100k wires), zoomed to 5% view → render time drops
  proportional to visible fraction.
- **Phase 2 target:** Pan/zoom frame cost O(visible) not O(total).
- **Phase 3 target:** Complex solid scene at far zoom → mesh triangle count < 10% of full detail.
- **Phase 4 target:** GPU-cull overhead < 0.5 ms for 1M entity scene.
