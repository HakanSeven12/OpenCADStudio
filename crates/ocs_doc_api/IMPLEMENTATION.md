# ocs_doc_api — Implementation status & roadmap

What `ocs_doc_api` supports **now**, and the outstanding first-level entities
and methods to be supported, in phases. "First-level" means the document's
top-level entity store (model/paper-space direct children, not block-nested
content).

## ✅ Supported (v1)

### Solids
| Capability | Method / op | Notes |
|---|---|---|
| Primitives | `create_cuboid` / `create_sphere` / `create_cylinder` / `create_cone` / `create_torus` / `create_wedge` | `CreateSolid(SolidPrimitive)` → `brep::make::*` |
| Sweep | `extrude(profile, dir)` / `revolve(profile, pivot, axis, angle)` | `Extrude`/`Revolve` → `brep::extrude`/`brep::revolve`; profile = Line/Circle/Arc/LwPolyline |
| Booleans | `intersect` / `union` / `subtract` | `SolidBoolean{op, a, b, erase_sources:true}` → `brep::combine` |
| Transform | `transform(placement)` (in-place) | `Transform{id, placement}` → `brep::transform`; solids |
| Bounds | `bounds()` | `GetBounds` → `brep::body_bounds` (lift-on-miss) |
| Volume | `volume()` | `GetVolume` → `meshes.metrics` cache, mesh-divergence fallback |
| Centroid | `centroid()` | `GetCentroid` → `meshes.metrics` cache, mesh-divergence fallback |
| Delete | `delete()` / `delete_many` | `Delete`/`DeleteMany` |

### Minimal 2D (phase-1 set, used as profiles/inputs)
| Capability | Method / op | Notes |
|---|---|---|
| Line | `create_line` → bounds, transform | `CreateCurve(Line)` |
| Circle | `create_circle` → bounds, transform | `CreateCurve(Circle)` |
| Arc | (create via host) → transform, profile | usable as sweep profile |
| Polyline | `create_polyline` → bounds, transform, `add_vertex` | `CreateCurve(Polyline)`; `AddVertex` inserts a vertex |
| Point | `create_point` / `create_points` (bulk) → transform | `CreateCurve(Point)` / `CreateMany` |
| Transform (2D) | `transform(placement)` / `transform_many` | `Line`/`Circle`/`Arc`/`Point`/`LwPolyline` via acadrust |

### Cross-cutting
| Capability | Method | Notes |
|---|---|---|
| Lookup / downcast | `entities().get(id)` → `Entity`; `as_solid()` | generic, any family |
| Bulk create | `create_points(&[..])`, `CreateMany` | all-or-nothing, one undo step |
| Bulk transform | `transform_many(&ids, placement)` | all-or-nothing, solids + 2D families |
| Bulk delete | `delete_many(&ids)` | all-or-nothing; `OpGroup::compensate` uses it |
| Read batch | `query_batch(\|q\| …)` | read-only, one round-trip, no bump |
| Revision guard | `revision()` / `assert_revision(rev)` | read-modify-write guard |
| Failure cleanup | `OpGroup` | best-effort, client-side (NOT a transaction) |
| Binding handover | `binding_schema_json()` | merged object-model + wire-layout schema |
| DWG round-trip | — | verified: intersected solids survive write→reload with valid ACIS (test) |

## 🚧 Outstanding — by phase

Phases are **spec-additive**: each is a `[[family]]`/`[[family.method]]` block in
`spec/entities.toml` + a codegen bump + facade/backend wiring (see
`ARCHITECTURE.md` § Workflow). No protocol change; new `Operation`/`Query`
variants append at enum end.

### Phase 2 — full 2D + annotations
- **Done (2a):** typed curve families `ArcCurve`/`Ellipse`/`Spline`/`Ray`/`XLine` —
  `create_arc`/`create_ellipse`/`create_spline`/`create_ray`/`create_xline`, bounds
  (Arc coarse full-circle, Ellipse major-axis radius, Spline control-point bbox;
  Ray/XLine unbounded → `Unsupported`), and transform (all five + the phase-1 set)
  via acadrust. Covered by `phase2_*` tests.
- **Remaining (2b):** annotations `Text`/`MText` (create + content get/set), `Hatch`
  (create + boundary query), `Dimension` (typed sub-types).
- **Unblocked:** `add_vertex` (polyline) and sweep profiles (Line/Circle/Arc/LwPolyline)
  are supported (Phase 0).

### Phase 3 — containers
- **Done:** `create_insert(block_name, point, scale, rotation)` → `Operation::CreateInsert`
  (validates the BlockRecord exists; unknown block → `Validation`, no mutation).
  Insert `transform` moves its `insert_point`. Covered by `phase3_*` test.
- **Remaining:** `AttributeDefinition`/`AttributeEntity` typed create + attribute
  get/set; read-only nested block-content traversal.

### Phase 4 — paper-space & viewports
- **Done:** `create_viewport(center, width, height, view_target, view_height)` →
  `Operation::CreateViewport`; `bounds()` = center ± width/2,height/2; `transform`
  moves the paper-space center. Covered by `phase4_*` test.
- **Remaining:** `set_view` (retarget/re-zoom an existing viewport) and a
  viewport-view query (view target/center/scale DTOs).

### Phase 5 — media & misc (read-mostly)
- **Entities:** raster image, underlay, table, light.
- **Methods:** CRUD + property queries; few/no geometric actions. These families
  largely stay read-only `EntityView` DTOs — typed ops only where the spec
  declares an action mapping.

## 🔧 Outstanding methods on supported families

| Method | Family | Status | Blocker |
|---|---|---|---|
| `loft(sections)` | Solid | not started | multi-profile kernel op + spec |
| Bulge-arc polyline profiles | Solid sweep | partial | `entity_to_profile_curves` emits straight segments; bulge arcs need arc-segment conversion |
| `GetCentroid`/`GetVolume` accuracy | Solid | mesh approx | render-mesh LOD tolerance vs query tolerance |

**Completed (Phase 0):** `extrude`, `revolve` (profiles: Line/Circle/Arc/LwPolyline),
`add_vertex` (polyline), non-solid `transform` + `transform_many` (Line/Circle/Arc/
Point/LwPolyline). Covered by `phase0_*` tests in `src/app/doc_api.rs`.

## Design invariants to preserve when extending

- Per-op autocommit; **no multi-op write transactions**.
- Per-op atomicity: validate+compute **before** `push_undo`; failed op = no
  mutation, no undo, no revision bump.
- Bulk ops: single op, all-or-nothing via upfront validation, item-capped.
- New `Operation`/`Query` variants **append at the end** (bincode stability).
- Host geometry stays in the host; the wire carries `ObjectId` + plain-data DTOs.
- `binding_schema.json` and `api_reference.md` regenerate from the spec and are
  CI drift-checked — update `spec/entities.toml` + `src/gen/layouts.json` for
  every wire change.
