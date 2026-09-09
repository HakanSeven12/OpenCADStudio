# ocs_doc_api

Object-oriented, transport-agnostic CAD entity API for OpenCADStudio plugins and
hosts (v2). One typed vocabulary — **Entity**, **ObjectId**, **Operation**,
**Query**, **Transport** — that runs identically in-process and over IPC.

```rust
use ocs_doc_api::{DocApi, Solid, OpGroup, ApiResult};

fn intersect_solids(api: &DocApi) -> ApiResult<()> {
    let doc = api.document(api.active_tab());
    let mut grp = OpGroup::new();

    // Each call is ONE atomic host op = one undo step. Per-op autocommit; no
    // multi-op write transactions. OpGroup is best-effort cleanup on failure.
    let block = grp.track(doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]))?;
    let other = grp.track(doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]))?;

    let lens: Solid = match block.intersect(&other) {   // host runs brep::combine
        Ok(l) => { grp.commit(); l }
        Err(e) => { grp.compensate(&doc)?; return Err(e); }
    };

    // Read-only query batch (one round-trip, no revision bump).
    let (bb, vol) = {
        let r = doc.query_batch(|q| { q.bounds(&lens); q.volume(&lens); })?;
        (r.bounds(0)?, r.volume(1)?)
    };
    api.push_info(&format!("lens vol={vol:.1} bounds={bb:?}"));
    Ok(())
}
```

## What it is

- **Object-centric facade** — `DocApi` (root) → `Document` → collections
  (`solids()`, `curves()`, `entities()`) → typed handles (`Solid`, `Line`,
  `Circle`, `Polyline`, `Point`, `Entity`) that carry geometric methods
  (`intersect`/`union`/`subtract`/`transform`/`bounds`/`volume`/`centroid`/`delete`).
- **Transport-agnostic** — the same typed requests run in-process
  ([`InProcess`]) or over the `ocs_plugin_api` IPC channel ([`OcsPluginApiIpc`]);
  only the `Transport` differs. The host executes them through one crate-owned
  executor.
- **Stable across async IPC** — handles are `Clone`, `Send`, `Sync` `{ObjectId,
  Arc<Session>}` tokens, safe to share across plugin worker threads. A stale
  handle's methods return `ApiError::UnknownId`, never a panic.

## Core guarantees

- **Per-op autocommit, no write transactions.** Every write op is ONE atomic
  host call = one undo step. Multi-op write transactions were removed after a
  security/integrity review (rollback-correctness of `erase_sources` + unbounded
  synchronous batch execution). There is no `doc.transaction`.
- **Per-op atomicity.** An op validates its inputs and computes its geometry
  *before* any mutation; a failed op applies nothing and records no undo step.
  `brep::combine` is pure; `erase_sources` runs only on success.
- **Bulk ops** (`CreateMany`/`TransformMany`/`DeleteMany`, and read-only query
  batching) are single ops, **all-or-nothing via upfront validation** (no
  rollback), item-capped â€” for bulk data (see `ARCHITECTURE.md` Â§ Performance).
  `Loft` and query batches are item-capped the same way (no unbounded kernel call
  from one request).
- **Accurate mass properties.** `volume`/`centroid` use a fine tessellation for
  near-analytic results, memoized per (handle, geometry_epoch) on the per-tab
  scene; the cold-tessellation budget (256/tab) persists across dispatches and
  degrades to the coarse mesh value when exhausted.
- **Tab authorization.** A request must name the tab the session is bound to;
  mismatched `tab_id` is rejected (no cross-tab access).
- **Structured errors** â€” `ApiError::{Validation, Geometry, UnknownId,
  Unsupported, Transport}`, serializable so IPC returns the same error the
  in-process executor produced.

## Scope (v1)

- **Solids** — primitives (cuboid, sphere, cylinder, cone, torus, wedge),
  booleans (union/intersect/subtract), transform, and queries
  (bounds/volume/centroid). Host resolves bodies via the `solid_models` cache
  (lift-on-miss from ACIS SAT).
- **Solids** — primitives (cuboid, sphere, cylinder, cone, torus, wedge), sweep
  (`extrude`/`revolve` from profiles, `loft` multi-profile), booleans
  (union/intersect/subtract), transform, and queries (bounds/volume/centroid).
  Host resolves bodies via the `solid_models` cache (lift-on-miss from ACIS SAT).
- **2D curves** — line, circle, arc, polyline (`add_vertex`), point (incl. bulk),
  ellipse, spline, ray, xline; construction + bounds + transform (solids & 2D via
  `transform`/`transform_many`).
- **Annotations** — `Text`/`MText` (create + `content`/`set_content`), `Hatch`
  (create + `boundary` query), linear `Dimension` (create + `measurement`).
- **Containers** — `create_insert` (block references) + `attributes`/`set_attribute`
  + `block_entities` (read-only nested block traversal).
- **Paper-space** — `create_viewport` + `set_view` (retarget/re-zoom) + `viewport_view`.
- **Media** — `create_raster_image`; other media families read-only (`EntityView`).
- See `IMPLEMENTATION.md` for the full capability matrix and phased roadmap.

## Features

| Feature | Pulls | Use |
|---|---|---|
| *(default)* | — | Pure data types (ops/query/envelope/errors) — binding generators, tooling |
| `host` | `acadrust` + `cadkernel` | The executor + `DocApiBackend` trait + in-process transport (the host side) |
| `ipc` | `acadrust` + `ocs_plugin_api` | The IPC transport adapter (the plugin side) |

## For REPL/scripting bridges (Python, Lua, …)

The object model is handed over as a single versioned schema
(`binding_schema_json()`, generated by `build.rs`). A bridge in any language is a
thin serializer + schema-driven generator + session glue over V4 `ExecuteCode` —
no Rust FFI. See `bindings/README.md` and `bindings/python/generate.py`.

## Links

- `ARCHITECTURE.md` — design decisions, the per-op execution model, how to add a
  new entity/family/method, performance notes.
- `IMPLEMENTATION.md` — what's supported now vs. the phased roadmap.
- `spec/entities.toml` — the curated source of truth for codegen.
