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
| Booleans | `intersect` / `union` / `subtract` | `SolidBoolean{op, a, b, erase_sources:true}` → `brep::combine` |
| Transform | `transform(placement)` (in-place) | `Transform{id, placement}` → `brep::transform`; solids only |
| Bounds | `bounds()` | `GetBounds` → `brep::body_bounds` (lift-on-miss) |
| Volume | `volume()` | `GetVolume` → `meshes.metrics` cache, mesh-divergence fallback |
| Centroid | `centroid()` | `GetCentroid` → `meshes.metrics` cache, mesh-divergence fallback |
| Delete | `delete()` / `delete_many` | `Delete`/`DeleteMany` |

### Minimal 2D (phase-1 set, used as profiles/inputs)
| Capability | Method / op | Notes |
|---|---|---|
| Line | `create_line` → bounds | `CreateCurve(Line)` |
| Circle | `create_circle` → bounds | `CreateCurve(Circle)` |
| Polyline | `create_polyline` → bounds | `CreateCurve(Polyline)` |
| Point | `create_point` / `create_points` (bulk) | `CreateCurve(Point)` / `CreateMany` |

### Cross-cutting
| Capability | Method | Notes |
|---|---|---|
| Lookup / downcast | `entities().get(id)` → `Entity`; `as_solid()` | generic, any family |
| Bulk create | `create_points(&[..])`, `CreateMany` | all-or-nothing, one undo step |
| Bulk transform | `transform_many(&ids, placement)` | all-or-nothing, solids only (v1) |
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
- **Entities:** ellipse, spline, xline/ray, arc (typed), text, mtext, dimension,
  hatch.
- **Methods:** boundary/vertex actions (`add_vertex` on polyline — currently
  `Unsupported`), hatch boundary query, text content get/set.
- **Blocked by:** `AddVertex` backend impl (currently returns `Unsupported`);
  curve → `geom2d::Curve` conversion for non-trivial curves.

### Phase 3 — containers
- **Entities:** insert/block references, attribute definitions/entities.
- **Methods:** `create_insert`, placement, attribute get/set.
- **Read-only:** nested block content traversal (nested ≠ first-level).

### Phase 4 — paper-space & viewports
- **Entities:** viewport entities.
- **Methods:** `create_viewport`, set-view (target, center, scale), layout
  queries.
- **Blocked by:** a viewport query model (view target/center/scale DTOs).

### Phase 5 — media & misc (read-mostly)
- **Entities:** raster image, underlay, table, light.
- **Methods:** CRUD + property queries; few/no geometric actions. These families
  largely stay read-only `EntityView` DTOs — typed ops only where the spec
  declares an action mapping.

## 🔧 Outstanding methods on supported families

| Method | Family | Status | Blocker |
|---|---|---|---|
| `extrude(profile, dir)` | Solid | stub | `profile_curves` backend returns `Unsupported`; needs entity → `geom2d::Curve` conversion |
| `revolve(profile, pivot, axis, angle)` | Solid | stub | same `profile_curves` blocker |
| `loft(sections)` | Solid | not started | multi-profile kernel op + spec |
| `add_vertex(at, pt)` | Polyline | stub | backend `add_vertex` returns `Unsupported` |
| `transform` (non-solid) | 2D entities | `Unsupported` | per-family transform via acadrust (not kernel) |
| `GetCentroid`/`GetVolume` accuracy | Solid | mesh approx | render-mesh LOD tolerance vs query tolerance |

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
