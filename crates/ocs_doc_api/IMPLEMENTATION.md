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

## 🚧 Roadmap — phase status

Phases are **spec-additive**: each is a `[[family]]`/`[[family.method]]` block in
`spec/entities.toml` + a codegen bump + facade/backend wiring (see
`ARCHITECTURE.md` § Workflow). No protocol change; new `Operation`/`Query`
variants append at enum end.

**Status:** Phase 0 (supported-family methods), Phase 2a (full 2D curves),
Phase 3 (block references), Phase 4 (viewports), Phase 5 (media read-mostly) are
**done** (all tested; 601 host lib + 8 crate unit tests green). Remaining sub-items:
Phase 2b (annotations/hatch/dimension typed ops), attributes + nested traversal
(P3), set_view + view query (P4), typed media ops (P5).

### Phase 2 — full 2D + annotations
- **Done (2a):** typed curve families `ArcCurve`/`Ellipse`/`Spline`/`Ray`/`XLine` —
  `create_arc`/`create_ellipse`/`create_spline`/`create_ray`/`create_xline`, bounds
  (Arc coarse full-circle, Ellipse major-axis radius, Spline control-point bbox;
  Ray/XLine unbounded → `Unsupported`), and transform (all five + the phase-1 set)
  via acadrust. Covered by `phase2_*` tests.
- **Done (2b-a):** annotations `Text`/`MText` — `create_text`/`create_mtext`
  (`CreateText`/`CreateMText` wire ops), `content()`/`set_content()` (new
  `GetTextContent` query + `SetTextContent` op), transform (insertion point),
  coarse bounds. Covered by `phase2b_*` test.
- **Done (2b-b):** `Hatch` — `create_hatch(boundary, solid)` (`CreateHatch`,
  validates ≥3 points), `boundary()` → loops (`GetHatchBoundary`), bounds from
  boundary edges, delete. Covered by `phase2b_hatch_*` test.
- **Done (2b-c):** linear `Dimension` — `create_dimension_linear(first, second,
  definition)` (`CreateDimensionLinear`), `measurement()` → distance
  (`GetDimensionMeasurement`), bounds. Covered by `phase2c_*` test. Radius/diameter/
  angular dimension sub-types are a later refinement.
- **Unblocked:** `add_vertex` (polyline) and sweep profiles (Line/Circle/Arc/LwPolyline)
  are supported (Phase 0).

### Phase 3 — containers
- **Done:** `create_insert(block_name, point, scale, rotation)` → `Operation::CreateInsert`
  (validates the BlockRecord exists; unknown block → `Validation`, no mutation).
  Insert `transform` moves its `insert_point`. Covered by `phase3_*` test.
- **Done (attributes + traversal):** `set_attribute(tag, value)` (adds/updates an
  attribute on an insert, `SetAttribute` op), `attributes()` → (tag, value) pairs
  (`GetAttributes`), and `block_entities(block_name)` → read-only nested traversal
  of a block definition's entities (`GetBlockEntities`). Covered by
  `phase3_attributes_and_block_traversal` test.
- **Remaining:** typed `AttributeDefinition` (in-block attribute-definition)
  create.

### Phase 4 — paper-space & viewports
- **Done:** `create_viewport(center, width, height, view_target, view_height)` →
  `Operation::CreateViewport`; `bounds()` = center ± width/2,height/2; `transform`
  moves the paper-space center. Covered by `phase4_*` test.
- **Done (set_view + view query):** `set_view(view_target, view_height)` retargets/
  re-zooms in place (`SetViewportView` op), `viewport_view()` reads the view back
  (`GetViewportView` query → target + zoom height). Covered by
  `phase4_set_view_and_view_query` test.

### Phase 5 — media & misc (read-mostly)
- **Done:** media/misc families report a **real `kind`** via `GetEntity`
  (`RasterImage`, `Table`, `Leader`, `MultiLeader`, `MLine`, `Mesh`, `Helix`,
  `Region`, `Body`, `Surface`, `Face3D`, `Dimension`, `Hatch`) instead of "Other";
  `RasterImage` has coarse bounds (insertion + u·width + v·height); generic
  `delete` works on all. Covered by `phase5_*` test.
- **Done (typed create):** `create_raster_image(file_path, insertion, u, v, size)`
  → `Operation::CreateRasterImage`; the host auto-registers the `ImageDefinition`
  (asserted by `phase5_create_raster_image_registers_definition`). Covered by that
  test. `Table`/light stay read-only `EntityView` DTOs (typed ops only where the
  spec declares an action mapping).

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
