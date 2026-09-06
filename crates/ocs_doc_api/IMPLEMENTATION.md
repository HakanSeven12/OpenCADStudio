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
| Volume | `volume()` | `GetVolume` → fine-tessellation `mesh_body(0.1, 1e-4)` (near-analytic); memoized per handle+epoch; budget-exhausted → approximate `meshes.metrics` |
| Centroid | `centroid()` | `GetCentroid` → same accuracy path as `volume()` |
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

**Status:** all five phases complete — Phase 0 (supported-family methods), Phase 2
(full 2D curves + annotations incl. dimension sub-types), Phase 3 (block references
+ attributes + traversal + attribute definitions), Phase 4 (viewports + set_view +
view query), Phase 5 (media read-mostly + typed raster image + typed table). All
tested: 618 host lib + 8 crate unit + 3 spec↔wire consistency tests green.

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
  (`GetDimensionMeasurement`), bounds. Covered by `phase2c_*` test.
- **Done (2c-ii, sub-types):** `create_dimension_radius` / `create_dimension_diameter`
  (radial chord) and `create_dimension_angular` (vertex + 2 legs) → radial/degrees
  measurements. Covered by `phase2cii_*` test.
- **Done (2c-iii):** `create_dimension_angular2ln(vertex, first, second, arc_location)`
  → `CreateDimensionAngular2Ln` (2-line angular; `DimensionAngular2Ln::new` computes
  the angle). Covered by `phase2ciii_*` test. All documented dimension sub-types done.
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
- **Done (attribute definitions):** `create_attribute_definition(tag, prompt,
  default, point, height, rotation)` → `Operation::CreateAttributeDefinition`;
  reports kind `AttributeDefinition`. Covered by `phase3ii_*` test. Phase 3 is complete.

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
  generic `delete` works on all. Covered by `phase5_*` test.
- **Done (read-mostly bounds):** `bounds()` now works for the vertex/point-carrying
  families — `Leader`, `Mesh`, `Face3D` (4 corners), `MLine` (vertex positions),
  `Helix` (start point) — via a points-to-AABB helper (`read_mostly_family_bounds_*`
  test). `RasterImage` has 4-corner bounds. `Region`/`Body`/`Surface`/`MultiLeader`
  stay `Unsupported` for bounds (complex/nested geometry).
- **Done (typed create):** `create_raster_image(file_path, insertion, u, v, size)`
  → `Operation::CreateRasterImage` (host auto-registers the ImageDefinition), and
  `create_table(insertion_point, data[row][col])` → `Operation::CreateTable`
  (non-rectangular grid → `Validation`; cells built as text). Covered by
  `phase5_create_raster_image_*` and `phase5ii_create_table_*` tests. `light` stays
  read-only (no meaningful geometric action).

## 🔧 Outstanding methods on supported families

**Completed (accuracy):** `GetVolume`/`GetCentroid` compute via a **fine
tessellation** (`mesh_body(0.1, 1e-4)`) for near-analytic results — the
render-mesh metrics cache is the coarse display LOD and does not drive queries;
results memoized per (handle, geometry_epoch); when the cold-tess budget is
exhausted the path degrades to the approximate `meshes.metrics` value rather than
failing. Verified by `accurate_volume_centroid_sphere_and_cube` (sphere volume
within 0.5% of 4/3πr³, centroid at centre, cube exact).

**Completed (outstanding methods):** `loft(profiles)` — `Operation::Loft{profiles}`,
resolves each profile to curve sets (all-or-nothing), `brep::loft` (`loft_two_profiles_*`
test). **Bulge-arc polyline profiles** — `entity_to_profile_curves` now converts
`LwVertex.bulge` to `geom2d::Curve::Arc` via the bulge→arc formula (θ = 4·atan(bulge)),
so bulged profiles sweep correctly (`bulge_arc_polyline_profile_converts_to_arc` test).

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
