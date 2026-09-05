# ocs_doc_api — Architecture

This document explains how `ocs_doc_api` is put together and, most importantly,
the **workflow for adding a new entity, family, or method**. It assumes you've
read `README.md` (the user-facing surface).

## Layered design

```
┌─ Facade (facade.rs)        DocApi → Document → collections → typed handles
│    Object-centric, hand-written. Methods build Operation/Query and call
│    Transport::apply. OpGroup (client compensation), query_batch, assert_revision.
│
├─ Wire (envelope/ops/query) DocApiEnvelope = Op(single) | Queries(batch)
│    bincode + serde. Operation/Query enums are GENERATED (src/gen) from the spec.
│    Append-only variants → stable discriminants.
│
├─ Transport (transport/)    trait Transport: Send+Sync { apply, alive }
│    InProcess (host) | OcsPluginApiIpc (plugin). The ONLY plugin↔host boundary.
│    Portability contract: &self + internal serialization + correlation + timeout.
│
├─ Executor (executor.rs)    apply_op / apply_queries  ← crate-owned logic
│    Per-op atomicity: validate+compute BEFORE push_undo; mutate; finalize_op once.
│
├─ Backend (backend.rs)      trait DocApiBackend  ← thin host hook
│    resolve_body / store_solid / update_solid / add_curve / transform_entity /
│    bounds / volume / centroid / revision / push_undo / finalize_op /
│    ensure_transformable / can_remove / ...
│
└─ Host (src/app/doc_api.rs) impl DocApiBackend for HostSession
     Maps to scene/app primitives: register_solid_model, restore_solid_models
     (lift-on-miss), solid_to_sat, edge_wires, erase_entities, meshes.metrics.
```

The same `executor` runs in-process and over IPC — one implementation, versioned
in the crate. The host only supplies scene primitives through `DocApiBackend`.

## Per-op execution model (the load-bearing invariant)

Every write op follows this exact sequence (see `executor::apply_op`):

1. **Validate + compute** — resolve inputs, run kernel geometry (`make_solid`,
  `brep::combine`, `brep::transform`), pre-flight fallible host work
  (`prepare_solid_model_display`), check `ensure_transformable` / `can_remove`.
  *No mutation yet.*
2. **`push_undo(label)`** — begin the host's undo delta capture.
3. **Mutate** — `store_solid` / `update_solid` / `add_curve` / `transform_entity` /
  `remove_entity`.
4. **`finalize_op()`** — close the undo delta + republish the document view.

On any error in steps 1–3, the op returns before `finalize_op`: **nothing is
applied, no undo step, no revision bump.** This is the per-op atomicity
guarantee. `brep::combine` is pure; `erase_sources` only runs on success.

The geometry revision advances via the host's internal `bump_geometry` inside
`add_entity`/`update_entity`/`register_prepared_solid_model`. The epoch moves
monotonically per op (one undo step per op); the exact bump count is a host
internal — do not assert on it.

## Object identity and the solid cache

- **`ObjectId`** = `acadrust::Handle` (u64). Kernel keys never cross the API.
- **Solids** live in the host's `scene.solid_models: HashMap<Handle, Body>`
  cache. `resolve_body` does **lift-on-miss** (`restore_solid_models`): if a
  solid isn't cached, its body is re-lifted from the entity's ACIS SAT. This is
  what makes DocApi-created solids visible to host-native booleans and vice
  versa, and what makes a reloaded DWG's solids usable.
- `store_solid` / `update_solid` pre-flight the mesh (`prepare_solid_model_display`)
  **before** committing, so the post-commit `register_prepared_solid_model`
  cannot fail — there is no rollback path that would bump the revision on failure.

## Bulk ops and read batching

- `CreateMany` / `TransformMany` / `DeleteMany` are **single ops**, one undo
  step, one publish — but **all-or-nothing via upfront validation**: every
  fallible check (make_solid, `ensure_transformable`, `can_remove`) runs before
  `push_undo`, so the apply loop cannot fail part-way. Item-capped
  (`BULK_ITEM_CAP`) to bound host execution.
- Read-only query batching (`query_batch`) shares one round-trip and never bumps
  the revision. Also capped.
- Why: per-op `publish_document_view` is a full-document serialization, so N
  sequential ops cost O(N²) — bulk ops collapse that to O(N) once. See
  "Performance" below.

## Codegen (build-time, spec-driven)

`build.rs` reads `spec/entities.toml` and regenerates the committed snapshots:

- `src/gen/ops_gen.rs`, `src/gen/query_gen.rs` — the `Operation`/`Query` enums
  (the canonical wire vocabulary; **append new variants at the END only**).
- `src/gen/api_reference.md` — human API reference.
- `src/gen/binding_schema.json` — the merged binding-handover schema (object
  model + method signatures + op/query mapping + wire layouts), exposed as
  `binding_schema_json()` for REPL/scripting bridges.

`cargo build` regenerates them deterministically; CI fails on drift.

---

## Workflow: adding a new entity, family, or method

This is the common task. Follow these steps in order.

### A. Add a new `Operation` or `Query` variant (the wire vocabulary)

1. **Append** the variant to `src/gen/ops_gen.rs` / `src/gen/query_gen.rs`
   **at the END** of the enum (never reorder — bincode discriminants are
   order-based). Use existing payload DTOs or add new plain-data mirrors in
   `src/ops.rs` / `src/query.rs`.
2. Add its name to `Operation::op_name` / `Query::query_name` in `src/ops.rs` /
   `src/query.rs`.
3. Regenerate: `cargo build -p ocs_doc_api` — the spec/schema pick up the wire
   change. Update `src/gen/layouts.json` so `binding_schema.json` stays
   self-contained.

### B. Add a family (e.g. a new entity kind)

1. **Spec** — add a `[[family]]` block in `spec/entities.toml` (name,
   `acadrust_variant`, `handle`, `collection`, and its `[[family.method]]`s).
2. **Handle type** — add a `handle!(NewType)` line in `facade.rs` (gives it
   `bounds`/`delete`/`transform` + `id()`), plus `HasId`. Add family-specific
   methods as an `impl NewType` block if it has geometric actions.
3. **Construction** — add a `create_*` method on the right collection
   (`SolidCollection`/`CurveCollection`/`EntityCollection`) building the op.
4. **Backend conversion** — extend `src/app/doc_api_convert.rs`
   (`curve_spec_to_entity`, `entity_kind_name`, `entity_bounds`) to map the new
   acadrust `EntityType` variant.
5. **Executor** — if the family has a new op variant, add its arm to
   `executor::apply_op`.

### C. Add a method to an existing family

1. **Wire** — if it's a new operation/query, do step A first. If it reuses an
   existing op (e.g. another boolean), skip to step 3.
2. **Spec** — add a `[[family.method]]` entry under the family (name, kind,
   op/query, args, returns, `fixed` fields).
3. **Facade** — add the method to the handle's `impl` block in `facade.rs`;
   it should build the `Operation`/`Query` and call `session.apply_op` /
   `session.one_query`, returning the typed result.
4. **Executor + backend** — if the op is new, implement its arm in
   `executor::apply_op` (driving `DocApiBackend`), and add/override the backend
   method in the host (`src/app/doc_api.rs`) mapping to the scene primitive.
5. **Test** — add a mock-backend unit test (`tests/mock_backend.rs`) and, for
   host-affecting ops, a `HostSession` integration test in `src/app/doc_api.rs`
   (model it on the existing roundtrip tests).

### D. Keep the invariants

- Validate + compute **before** `push_undo`; never mutate in the validate phase.
- One undo step per op; `finalize_op` only on success.
- New op/query variants **append at the end** of the generated enums.
- Host geometry stays in the host (kernel keys never cross the wire); the wire
  carries `ObjectId`s and plain-data DTOs.

---

## Error model

`ApiError::{Validation, Geometry, UnknownId, Unsupported, Transport}` —
serializable, so IPC returns the same structured error the in-process executor
produced. Bulk-op validation names the failing index. A stale handle's methods
return `UnknownId` (structured), never a panic.

## Performance notes

- **Per-op publish** (`finalize_op` → `publish_document_view`) is a
  full-document serialization; N sequential ops cost O(N²). This is the
  documented trade of per-op autocommit and the reason bulk ops exist.
  Measured estimate: ~10⁵ sequential creates+transforms ≈ minutes; the same as
  one `CreateMany`+`TransformMany` ≈ ~1 s. Use bulk ops for N ≳ a few hundred.
- **`volume`/`centroid`** serve from `scene.meshes[handle].metrics` when cached;
  the kernel-mesh divergence is a cold-cache fallback.
- **`resolve_body` lift-on-miss** is cheap when the cache is warm (no-op).
- **IPC** serializes the envelope once; the `bytes` fields use `serde_bytes` so
  the outer frame is a single length-prefixed copy.

---

## Changes from the original design (review-driven)

The initial implementation was reviewed (`/review`) and refined in two rounds.
This section records the concrete changes and *why* they were made, so the
reasoning isn't lost. Tests are green after every change (592 host lib + 8 crate
unit + 7 `HostSession` integration tests, incl. the regression tests added for
the CRITICAL fixes).

### Files introduced / modified in `src/app/`

The host-side integration lives in `src/app/`. Concretely:

**New files**
- **`src/app/doc_api.rs`** — the `DocApiBackend` implementation for `HostSession`
  (the thin host hook). Contains:
  - `execute_doc_api(host, tab_id, bytes)` — the dispatch entry: deserialize the
    `DocApiEnvelope`, run the crate executor, serialize `ApiResult<Receipt>`.
  - `impl DocApiBackend for HostSession` — `resolve_body` (lift-on-miss via
    `restore_solid_models`), `store_solid`/`update_solid` (pre-flight
    `prepare_solid_model_display` before commit → `register_prepared_solid_model`),
    `add_curve`, `remove_entity` (direct `erase_entities` + locked-layer error),
    `can_remove`, `ensure_transformable`, `transform_entity`, `bounds`
    (lift-on-miss for solids), `volume`/`centroid` (`meshes.metrics` cache +
    divergence fallback), `revision`, `push_undo`/`finalize_op`.
  - helpers `obj_to_handle`/`handle_to_obj`, `kernel_placement`,
    `mesh_volume_centroid` (single divergence-theorem helper).
  - 7 `#[cfg(test)]` integration tests (create/boolean/query roundtrip, unknown-id
    error, transform-many all-or-nothing, query-batch cap, 2D-entity roundtrip,
    geometric-methods roundtrip, **DWG write→reload roundtrip**).
- **`src/app/doc_api_convert.rs`** — acadrust `EntityType` ↔ DTO conversions:
  `curve_spec_to_entity` (Line/Circle/LwPolyline/Point from `Curve2Spec`),
  `entity_kind_name`, `entity_bounds` (per-family coarse bounds for non-solids).

**Modified files**
- **`src/app/mod.rs`** — registers the two new modules (`doc_api`,
  `doc_api_convert`), both gated `#[cfg(not(target_arch = "wasm32"))]`.
- **`src/app/plugin_host.rs`** — extends `HostSession`:
  - a `doc_api_pending: Option<PendingDelta>` field (the in-flight per-op undo delta);
  - accessors used by the backend: `scene()`/`scene_mut()`,
    `commit_entity_handle(entity)` passthrough, `begin_doc_api_undo(label)`
    (→ `app.begin_undo`), `commit_doc_api_undo()` (→ `app.commit_undo_delta` +
    `publish_document_view`; **no** extra `bump_geometry` — the entity op already
    bumped);
  - `impl HostApi for HostSession::doc_api_dispatch` → routes to
    `doc_api::execute_doc_api` (gated `not(wasm32)`).

**`ocs_plugin_api` (sibling crate, also touched)**
- `src/ipc/protocol.rs` — appended `PluginRequest::DocApiRequest{tab_id, bytes}`
  and `PluginResponse::DocApiResponse{bytes}` (append-only; `serde_bytes` on the
  payload).
- `src/host.rs` — appended defaulted `HostApi::doc_api_dispatch` (returns
  "not supported"; `HostSession` overrides it). Append-only, object-safe.
- `src/ipc/server.rs` — routes `DocApiRequest` → `host.doc_api_dispatch(...)`.

### Review round 1 — correctness & safety fixes

Two CRITICAL and six WARNING findings were fixed:

1. **Bulk ops are now truly all-or-nothing** (was CRITICAL). The original
   `TransformMany`/`DeleteMany`/`CreateMany` only existence-checked ids; a
   mid-apply failure (e.g. `store_solid` register failure, or `transform_entity`
   hitting a non-solid) left partial state with no undo step. Now the executor
   pre-validates *everything fallible* before `push_undo`:
   - `TransformMany` → new backend method `ensure_transformable(id)` (v1:
     solids-only) validated for **every** id first.
   - `DeleteMany` → `can_remove(id)` (existence + not-on-locked-layer) for every id.
   - `CreateMany` → its prepare loop was already pure; the residual `store_solid`
     post-commit failure is eliminated by the pre-flight in (8).
   - Regression test: `doc_api_transform_many_with_non_solid_fails_all_or_nothing`.

2. **Query batch is capped** (was CRITICAL / DoS). `apply_queries` had no bound —
   a ≤64 MiB envelope could carry millions of `GetVolume`/`GetCentroid` queries,
   each tessellating a solid on the synchronous UI thread. Now capped at
   `BULK_ITEM_CAP` like the write bulk ops. Regression test:
   `doc_api_query_batch_over_cap_is_rejected`.

3. **Locked-layer erase no longer silently succeeds** (was WARNING).
   `HostSession::remove_entity` returns `false` for locked-layer entities; the
   executor treated `Ok(false)` as success (`Delete` reported `Deleted`, boolean
   `erase_sources` left the second input live). Now:
   - backend `remove_entity` returns `Err(Unsupported)` when the entity is on a
     locked layer;
   - the boolean-erase path validates `can_remove(b)` **before** `update_solid(a)`;
   - `Delete`/`DeleteMany` pre-validate `can_remove` before `push_undo`.

4. **One publish per op for deletes** (was WARNING). The backend's `remove_entity`
   went through `HostSession::remove_entity`, which calls `publish_document_view`
   per invocation → `DeleteMany` triggered N+1 full-document publishes. The backend
   now calls `scene.erase_entities` directly (clears `solid_models`/meshes/hatches
   + records the undo delta); the single publish happens at `finalize_op`.

5. **`bounds` lift-on-miss for solids** (was WARNING). `bounds` checked
   `solid_models.contains_key` and fell through to a non-solid `Unsupported` for a
   solid with a cold cache (e.g. dropped by a host-side `update_entity`). Now it
   resolves solids through `resolve_body` (lift-on-miss), consistent with
   `volume`/`centroid`.

6. **`volume`/`centroid` use the metrics cache** (was WARNING / perf). They
   re-tessellated the full body (`mesh_body`) + cloned the B-rep per query. Now
   they serve from `scene.meshes[handle].metrics` when cached (refreshed on
   store/update), with the kernel-mesh divergence as a cold-cache fallback. The
   duplicated divergence loops were also merged into one `mesh_volume_centroid`.

7. **Double revision bump documented** (was WARNING). Solid write ops bump
   `geometry_epoch` twice (the entity add/update path + `register_prepared_solid_model`).
   This is host-internal and shared with native commands; the meaningful guarantee
   (one undo step per op, monotonic epoch) holds and is tested. Documented rather
   than changed, to avoid touching shared host scene bump behavior.

8. **`store_solid` failure no longer moves the revision** (was WARNING). The
   original rollback path (`rollback_new_entities` on `register_solid_model`
   failure) bumped the epoch *again* on a failed op, violating "failed op = no
   bump." Now `store_solid`/`update_solid` **pre-flight** `prepare_solid_model_display`
   **before** `commit_entity_handle`, so a mesh-prep failure errors before any
   mutation — the rollback path is eliminated (and the double `prepare` work is
   removed). This also feeds the `CreateMany` all-or-nothing guarantee in (1).

### Review round 2 — cleanups (SUGGESTION)

- **`serde_bytes` on the IPC payload** — `PluginRequest::DocApiRequest.bytes` /
  `PluginResponse::DocApiResponse.bytes` now use `#[serde(with = "serde_bytes")]`
  so the outer frame is a single length-prefixed copy (was a per-byte seq walk +
  a second full copy on 10–20 MB bulk payloads).
- **Dead code removed** — `src/gen/dispatch_gen.rs` (never generated/included),
  `Operation::references` (no call sites, false doc comment), the discarded
  serde-reflection tracer stub + `serde-reflection` build-dep in `build.rs`
  (it reads `layouts.json` directly now), and the dead `Session.tab` field
  (transport carries `tab_id`; `DocApi::document(tab)` keeps the arg for
  forward-compat but no longer stores it).

### Subsequent additions

- **Rename** `ocs_doc_api_2` → `ocs_doc_api` (directory, package, lib, all refs).
  The plan document under `.kilo/` keeps the historical name.
- **DWG roundtrip robustness** — the stable-DWG test writes to
  `target/doc_api_roundtrip_intersected.dwg` (kept for inspection). It releases a
  stale lock first and falls back to a process-unique path if still locked, so it
  never flakes on Windows file locking (error 33).
- **Docs** — `README.md`, this file, `IMPLEMENTATION.md`, enriched
  `spec/entities.toml` + the generated `api_reference.md` (constructors, generic
  methods, cross-cutting, bulk ops, `unsupported` markers).
- **Examples** — five runnable examples under `examples/` (e1 primitives+queries,
  e2 booleans, e3 transform-chain, e4 2D+bulk, e5 failure+guards) over a shared
  in-process mock backend (`examples/common/mod.rs`).
- **Tests** — 7 `HostSession` integration tests in `src/app/doc_api.rs`, incl.
  roundtrips for 2D entities, geometric methods, and a **DWG write→reload**
  roundtrip proving an intersected solid survives with valid ACIS and re-lifts to
  the expected volume.

### Known deviations (documented, not bugs)

- **Two `geometry_epoch` bumps per solid write op** (see #7) — host-internal,
  shared with native commands; monotonic and one-undo-step-per-op are guaranteed.
- **`extrude`/`revolve`/`add_vertex`/non-solid `transform`** return
  `ApiError::Unsupported` in v1 (profile conversion + per-family transform land in
  later phases — see `IMPLEMENTATION.md`).
- **`volume`/`centroid`** are mesh-approximate (render-mesh LOD tolerance), not
  exact analytic values.

