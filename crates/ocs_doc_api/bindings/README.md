# ocs_doc_api — binding generator contract

How to build a REPL/scripting bridge to `ocs_doc_api` in **any** language
(Python, Lua, JS, …) — no Rust FFI, one versioned schema. This is the binding
handover defined in the plan (§10.2, decision #13).

## What you consume

**One artifact:** `gen/binding_schema.json` (also exposed as
`ocs_doc_api::binding_schema_json()`). It is self-contained:

```jsonc
{
  "schema": "ocs_doc_api/binding_schema",
  "envelope_version": 1,
  "object_model": { "root": "DocApi", "document": "Document",
                    "collections": ["solids","curves","entities"],
                    "handles": ["Entity","Solid","Line","Circle","Polyline","Point"] },
  "families": [ /* per family: handle, collection, methods[] */ ],
  "layouts": { /* traced wire layouts for ObjectId / Operation / Query */ }
}
```

Each `families[].methods[]` entry:
```jsonc
{ "name": "intersect", "kind": "op",            // "op" (write) | "query" (read)
  "op": "SolidBoolean", "query": null,          // the Operation/Query it maps to
  "args": [{ "name": "other", "ty": "Solid" }],
  "returns": "Solid",
  "bulk": false, "read_only": false,
  "fixed": { "op": "Intersection", "erase_sources": true } }
```

## What you implement (3 things)

1. **Code generator** — read `binding_schema.json`, emit language classes:
   `DocApi` → `document(tab)` → `solids()/curves()/entities()` → handles
   (`Solid`, `Line`, …). Each generated method builds the `Operation`/`Query` its
   schema node names (applying `fixed` fields) and ships it (step 2). Handles are
   `{ ObjectId, kind }` — an `ObjectId` is a plain `u64`.

2. **`DocApiEnvelope` (de)serializer** — the wire unit (bincode over the host's
   plugin channel). Layout:
   ```
   DocApiEnvelope { version: u16, body: EnvelopeBody }
   EnvelopeBody = Op(Operation) | Queries(Vec<Query>)
   ```
   A **write** request carries exactly ONE `Operation`; a **read** request carries
   one-or-more `Query` (read-only batching is safe). Serialize with bincode
   semantics matching the `layouts` map (enum variants are appended at the END
   only, so discriminants are stable). The host replies with a bincode
   `Result<Receipt, ApiError>`; `Receipt` carries `outcome`, `query_results`,
   `new_revision`.

3. **Session glue** — send the envelope over the existing `ocs_plugin_api`
   channel. The host already exposes the DocApi executor via
   `PluginRequest::DocApiRequest { tab_id, bytes }` →
   `PluginResponse::DocApiResponse { bytes }`. A REPL bridge typically rides the
   V4 `ExecuteCode` loop (the mechanism the `ocs_python_repl` plugin uses) or a
   direct `PluginRequestSender`.

## Errors → language exceptions

Map the structured `ApiError` to your language's exceptions:

| `ApiError` variant | meaning |
|---|---|
| `Validation { op, reason }` | input rejected before any mutation (bulk op names the failing index) |
| `Geometry { kind, msg }` | the kernel failed (boolean refused, invalid input, ACIS) |
| `UnknownId(ObjectId)` | the handle is stale/deleted/never existed |
| `Unsupported(msg)` | capability not implemented (e.g. a later-phase family) |
| `Transport(msg)` | the channel failed (disconnected/oversized/timeout) |

## Version / compatibility rule

- Check `binding_schema.json.envelope_version` against `DocApiEnvelope.version`
  (currently `1`) before speaking; a mismatch is a hard bridge error.
- `Operation`/`Query` variants are **append-only**: a bridge built against an
  older schema keeps working; unknown new variants are simply not generated until
  you regenerate.

## Reference implementation

See `bindings/python/generate.py` — it reads `binding_schema.json` and emits
`ocs_doc.py` with `DocApi`/`Document`/`Solid`/`Line`/… classes whose methods build
and ship `DocApiEnvelope` ops. Lua/JS follow the same contract; only the
serializer + codegen differ.

## Semantics to honour

- **Per-op autocommit:** every write method is one atomic host call = one undo
  step. There are no multi-op write transactions. A failed op applies nothing and
  records no undo step; earlier ops stay committed (no rollback).
- **`erase_sources` booleans** replace the first input (`a`) in place and erase
  the second (`b`); the result is returned at `a`'s id.
- **Bulk ops** (`CreateMany`/`TransformMany`/`DeleteMany`) are single ops,
  all-or-nothing via upfront validation (no rollback), item-cap `100_000`.
- **Query batching** is read-only and safe; it never bumps the geometry revision.
