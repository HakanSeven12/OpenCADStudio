# OpenCAD Studio Automation API — Specification

Specification version **1.0** · API protocol **1** · applies to builds `2026.38` and later.

This document is the complete, self-contained reference for the OpenCAD Studio
automation API. Everything a client needs — conventions, transports, the
operation catalogue, validation rules, error codes and worked examples — lives
here. The API is transport-neutral: one operation dispatcher serves every
client, so a request has identical semantics whether it arrives over a local
HTTP port, a stdio pipe, a TCP socket or an AI-tool (MCP) session.

---

## 1. Overview

| Concept | Meaning |
|---|---|
| **Session** | One running editor process. Owns a session id, an event cursor and 1…N open documents. |
| **Document** | One drawing (a tab). Identified by a stable `document_id`. Exactly one document is **active** at a time. |
| **Handle** | Identity of every database object — a hexadecimal string (e.g. `"2A"`). Handles are stable across save/open and are the key for all read, edit, erase and clone operations. |
| **Operation** | One atomic, undoable step. Validated in full before anything commits; a rejected request changes nothing. |
| **Envelope** | The mutation wrapper: `"protocol":1`, a caller-generated `request_id`, and the `document_id` under edit. |

Design rules, in priority order:

1. **Idempotency** — a mutation replayed with the *same* `request_id` returns
   the cached result instead of executing twice. Generate one fresh
   `request_id` per logical operation (≤ 128 bytes) and reuse it only to
   retry after a timeout.
2. **Optimistic state** — read `state` first, pass `document_id` (and
   `revision`/`geometry_revision` when you hold them) back on mutations. A
   refusals whose code is in the retryable set (§5.2) means "your snapshot is
   behind": refresh state and retry once with a *new* `request_id`.
3. **Atomicity** — every operation is all-or-nothing and produces exactly one
   undo step. Multi-step transactions are the client's `batch` op or a
   sequence of ops against a held `revision`.
4. **Geometry stays in the engine** — measure, containment, intersection and
   ranking are computed by the built-in geometry kernel; clients never
   reimplement them.

---

## 2. Conventions

| Convention | Value |
|---|---|
| Angles | **degrees** on the wire, counterclockwise positive (`rotation_deg`, `angle_deg`, `start_angle_deg`, `end_angle_deg`) |
| Points | `[x, y]` (z = 0) or `[x, y, z]`, world coordinates (WCS). Interactive input additionally accepts `"space":"ucs"` / `"relative"`. |
| Distances, sizes | drawing units (millimetres only where a field is explicitly paper-space, e.g. `paper_width`) |
| Colors | integer index `1–255` (`1` red … `7` white), `256` by-layer, `0` by-block; true color accepted where noted |
| Handles | hexadecimal strings, case-insensitive, no `0x` prefix required |
| Names | layer / block / style names are case-insensitively unique; `*`-prefixed names are reserved |
| Booleans | JSON `true`/`false` |
| Templates | a `*.dwt` file is written and read as DWG bytes — same container, template extension |
| Paper sizes | catalog names, e.g. `"ISO_A4_(210.00_x_297.00_MM)"` (canonical name is echoed back in results) |
| Scales | `"fit"` or `"paper:drawing"` ratio like `"1:100"` (both parts > 0) |

---

## 3. Transports

### 3.1 HTTP REST (`--http <port>`)

Base URL `http://127.0.0.1:<port>/api/v1` (loopback bind only, permissive
CORS, JSON bodies, one request per connection). A machine-readable OpenAPI 3
description of the whole surface is served at `GET /api/v1/openapi`.

| Status | Meaning |
|---|---|
| `200` | Success (read or update) |
| `201` | A resource was created (document, entities, block, group, selection set, layout) |
| `400` | Validation failure — the body keeps the symbolic `code` and message |
| `404` | Unknown route or unknown handle (`code:"entity_absent"`) |
| `409` | State conflict (`stale_state`, `document_not_active`) or GUI required (`gui_required`) |
| `503` | Dispatcher busy |

The REST layer generates `request_id`s and caches the active `document_id`
server-side, retrying once on retryable refusals — plain HTTP clients do not
need envelope bookkeeping. Power clients may instead POST any operation name
to `/api/v1/{op}` with an explicit body.

### 3.2 stdio JSONL (`--serve`)

One JSON value per line over stdin/stdout. The **first** stdout line is the
ready greeting: `{"ok":true,"ready":true,"version":"…","session_id":"…"}`.
Every subsequent line is one request/response pair (legacy reads need no
envelope; mutations use the envelope of §1).

### 3.3 TCP (`--serve --port N`)

The same JSONL protocol as §3.2 over `127.0.0.1:<N>`, one client at a time.

### 3.4 AI-tool session (`--mcp`)

Exposes four tools — `ocs_sessions`, `ocs_read`, `ocs_execute`, `ocs_capture`
— with JSON-Schema validated arguments (`ocs_execute` alone accepts all 35
mutation operations). Session discovery and capability advertisement follow
the same protocol-1 semantics.

---

## 4. Session and state

`{"protocol":1,"op":"state"}` (or `GET /api/v1/state`) returns everything a
client needs to act:

| Field | Meaning |
|---|---|
| `session_id` | Identity of the running session; required by the MCP transport, accepted-and-ignored elsewhere (only a *wrong* value is refused) |
| `document_id` | The active document |
| `revision`, `geometry_revision` | Optimistic-concurrency tokens for data and geometry |
| `hand_seed` | The drawing's handle seed, hex — a monotonically advancing per-file object counter |
| `documents[]` | Every open document: `id`, `title`, `path`, `dirty`, `revision` |
| `selection[]` | Handles currently selected, in pick order |
| `command` | An interactive command's state, when one is running: `accepts[]`, `options[]`, `input_example` |
| `capabilities` | Feature list (also available as the dedicated `capabilities` read) |

`file_identity` (§6.1) additionally gives every drawing a stable GUID.

---

## 5. Error model

### 5.1 Response envelope

Every settled response carries:

```json
{"ok":true,"status":"completed","request_id":"…","result":{…},"changes":[…],"state":{…}}
```

`status` is one of `completed`, `accepted`, `running` (async work — poll with
`{"protocol":1,"op":"operation","request_id":…}`), `waiting_input` (an
interactive command wants more input), `cancelled`, `failed`. `changes[]`
lists affected handles (bounded to 1000) so clients can refresh cheaply.

### 5.2 Error codes

Failures are `{"ok":false,"code":"…","error":"…"}`. The `code` is symbolic
and stable; branch on it, not on message text.

| Code | Meaning | Retryable |
|---|---|---|
| `stale_state`, `session_changed` | The caller's snapshot is behind | yes — refresh `state`, retry once with a new `request_id` |
| `document_required`, `document_not_active`, `document_closed` | Envelope addressing problem | yes (refresh state) |
| `document_dirty` | Closing would lose unsaved changes | no — resend with `"discard":true` to confirm |
| `entity_absent` | A listed handle does not exist (the message names them) | refresh and re-read |
| `command_busy` | An interactive command is active | finish or `cancel` first |
| `gui_required` | Operation needs the editor window (e.g. `view_focus`) | headless sessions only |
| `busy` | Async work still running | poll `operation` |
| `selection_changed` | Precondition `selection[]` no longer matches | re-read and confirm |
| `name_required`, `entities_required`, `selection_required`, `app_required`, `path_required` | Missing mandatory field | no |
| `unknown_entity_type`, `invalid_radius`, `invalid_vertices`, `invalid_point`, `invalid_text`, `invalid_scale`, `invalid_paper`, `invalid_handle`, `invalid_xdata`, `invalid_sysvar_value` | Validation failures — nothing commits | no |
| `unknown_sysvar` | Variable outside the registry (§6.7) | no |
| `layout_exists`, `layout_missing`, `layer_failed`, `block_failed`, `block_missing`, `block_protected`, `selection_set_missing`, `template_missing`, `template_failed`, `save_failed` | Domain refusals, self-explanatory | no |

Unknown routes answer `code:"unknown_route"` (HTTP 404).

---

## 6. Operations reference

Every mutation below is an envelope op (§1). Reads (`query`, `records`,
`layers`, `header`, `xdata_get`, `capabilities`, …) need no envelope. All 35
mutations are listed in the `capabilities` advertisement and in the MCP
schema; the REST passthrough accepts any of them at `POST /api/v1/{op}`.

### 6.1 Documents, session, lifecycle

| Op | Request (beyond envelope) | Result highlights |
|---|---|---|
| `new` | optional `"template":"<.dwt/.dwg/.dxf>"` — start an untitled drawing whose tables/styles/entities come from the template; the document keeps no source path | `total` (entities imported) |
| `open` | `"path"` — opens a drawing as the active document | entity summary |
| `activate` | `"document_id"` | — |
| `close` | optional `"discard":true` — refused with `document_dirty` while unsaved changes exist; closing the last tab leaves a fresh empty drawing | `closed` |
| `save` | optional `"path"` (required while untitled). `*.dwg`/`*.dxf` follow the extension; a `*.dwt` path writes a template (DWG bytes) | `saved` |
| `file_identity` | optional `"renew":true` — stable RFC 4122 v4 GUID, minted on first call, persisted inside the drawing; survives save/open and clone | `identity`, `created` |
| `undo` / `redo` | — | — |

REST: `POST /documents` (`{}` → fresh document, `{"template":…}`, `{"path":…}`),
`DELETE /documents/{id}?discard=true`, `POST /save`.

### 6.2 Entity creation — `entities_create`

`{"entities":[{…},…]}` — a batch of typed definitions, validated in full
before anything commits; one undo step; missing layers are auto-created and
reported in `result.layers_created`. Every definition also accepts `layer`
and `color` (index).

| Type | Fields |
|---|---|
| `Line` | `start`, `end`, optional `thickness` |
| `Circle` | `center`, `radius` (≥ 0) |
| `Arc` | `center`, `radius` (> 0), `start_angle_deg`, `end_angle_deg` |
| `LwPolyline` | `vertices` (≥ 2), `closed`, optional `constant_width` |
| `Point` | `location` |
| `Text` | `value`, `position`, optional `height` (default 2.5), `rotation_deg`, `style` |
| `MText` | `value`, `position`, optional `height`, `width`, `rotation_deg` |
| `Insert` | `block` (must exist), `position`, optional `scale` (number or `[x,y,z]`, nonzero), `rotation_deg` |
| `Solid` | `corners` (3 or 4 `[x,y,z]`) |
| `Hatch` | `boundary` (≥ 3 points, closed), `solid`, or `pattern` (catalog name, default `ANSI31`), `pattern_scale`, `pattern_angle_deg` |

Result: `{"handles":[…],"created":N,"layers_created":[…]}` → HTTP 201.

### 6.3 Entity modification

| Op | Fields | Notes |
|---|---|---|
| `entities_delete` | `handles` | Every handle must exist; nothing is erased otherwise (`entity_absent` names them) |
| `entities_transform` | `handles`, `action`, per-action params | `move`/`copy`: `vector:[dx,dy(,dz)]` (copy returns new handles in `result.created`); `rotate`: `center`, `angle_deg`; `scale`: `center`, `factor` (≠ 0); `mirror`: `axis:[[x1,y1],[x2,y2]]`, optional `copy:true`; `array`: `rows`, `columns`, `row_spacing`, `column_spacing` |
| `entities_copy_to` | `handles`, `document_id` (target) | Cross-document clone; referenced layer definitions travel along; result `created[]` + `count` |

### 6.4 Query and spatial filters — `query`

Parameters (all optional, combine freely — AND semantics):

| Parameter | Meaning |
|---|---|
| `type`, `layer` | Entity type name / layer name |
| `handles` | Exact handle list |
| `detail` | `summary` (identity + common), `geometry` (+ shape fields), `full` (+ complete serialized properties incl. bounds) |
| `offset`, `limit` | Paging (limit ≤ 10000) |
| `fields` | Projection (property paths) |
| `bounds` | `[x0,y0,x1,y1]` world-window filter (entities intersecting the box) — for a "fully inside" test, compare each entity's own `bounds` client-side |
| `near` | `[x,y(,z)]` — rank by kernel-computed distance |
| `contains_point` | `[x,y]` — closed curves containing the point (kernel ray test) |
| `intersections` | `[h1,h2]` — exact intersection points of two planar curves (kernel) |
| `where` | Property filter list over serialized entities (below) |

`where` filters use JSON Pointer paths into the serialized entity
(`"/radius"`, `"/common/layer"`) with the operators `eq`, `ne`, `lt`, `lte`,
`gt`, `gte`, `contains`, `starts_with`, `ends_with`, `in`, `exists`,
`not_exists`. Filters are validated up front; a bad pointer or unknown
operator fails the query before any entity is inspected. On REST the same
filter rides `GET /entities?where=<json-encoded>`.

Text entities return the raw string in `value` plus a formatting-free
rendering in `text`; degenerate-width text bounds are widened with a
documented estimate (height × 0.8 × character count) so region filters stay
usable.

Companion reads: `entities` (alias), `layers`, `header`, `records`
(paged, filterable database records), `record_schema` (generated type
registry with write rules), `properties`, `measure` (length/area/bounds),
`xdata_get`.

### 6.5 Blocks

| Op | Fields | Semantics |
|---|---|---|
| `block_define` | `name`, `base:[x,y,z]`, `handles`, optional `insert_at`, optional `"replace":true` | The sources move into the new definition (flattened at `base` = block origin) and one `Insert` is placed at `insert_at` (default `base`), so the drawing looks unchanged. `"replace":true` drops a same-named definition — children, markers and inserts — inside the same undo step before creating the new one (idempotent re-import). Result `{"block":name,"insert":"<handle>"}`. Names are unique, `*` is reserved. |
| `block_delete` | `name` | Removes the definition completely: child entities, block markers, every `Insert` referencing it and the table record. One undo step. Result `erased`. Unknown → `block_missing`; `*`-records → `block_protected`. |
| `wblock` | `path`, `handles` *or* `block`; optional `"template":"<file>"` (the export inherits that file's tables/styles), optional `"normalize":true` (the export is shifted so its overall bounds minimum lands on the origin) | Writes a standalone DWG/DXF (extension decides). The source document is untouched. Result `{path, entities, normalized}`. |

REST: `POST /blocks`, `DELETE /blocks/{name}`, `POST /wblock`.

### 6.6 Layouts and plotting

| Op | Fields |
|---|---|
| `layout_create` | `name` — adds a layout with the default page setup and a sheet viewport; duplicates refused (`layout_exists`) |
| `page_setup_set` | `layout` plus any of `paper` (catalog name), `orientation` (`portrait`/`landscape`), `fit:true` or `scale:"1:100"`, `center`, `window:[x0,y0,x1,y1]`, `plot_style`. Explicit fields win; the rest keeps the stored setup. The stored setup is what `plot` consumes by default. |
| `plot` | `path`, `layout` (`"Model"` default, a name, or `"all"`), `area` (`"extents"` default, `"display"`, `"limits"`, `"window"` + `window:[…]`, `"layout"`), `paper`, `orientation`, `fit`/`scale`, `center`, `offset_x/offset_y`, `upside_down`, `plot_style` (CTB/STB), output toggles `transparency`, `lineweights`, `merge_lines`, `stamp`, and `"per_page":true` (with `layout:"all"` writes **one PDF per layout**, returning `result.files`) |

Plotting writes PDF only, synchronously, with no device dialogs; result
reports `{path, pages, page_sizes}` (millimetres).

### 6.7 Metadata and variables

**Extended data (`xdata_set` / `xdata_get`)** — typed key-value records per
application name on any entity; the application table entry is registered
implicitly on write. An empty (or absent) `data` list removes the record.

```json
{"op":"xdata_set","handles":["65"],"app":"SPEC","data":[{"code":1000,"value":"PAGE-01"},{"code":1070,"value":3}]}
```

| Code | Value type |
|---|---|
| `1000` | string |
| `1003` | layer name |
| `1004` | hex bytes |
| `1005` | hex handle |
| `1010`–`1013` | `[x,y,z]` point / position / displacement / direction |
| `1040` / `1041` / `1042` | real |
| `1070` / `1071` | 16-bit / 32-bit integer |

**System variables (`sysvar`)** — atomic get/set over a fixed registry:

| Var | Meaning | Writable |
|---|---|---|
| `ltscale`, `celtscale` | global / current-entity linetype scale | ✓ |
| `clayer`, `ctextstyle` | current layer / text style (must exist) | ✓ |
| `textsize`, `filletrad`, `pdmode`, `pdsize` | defaults for new content | ✓ |
| `mirrtext` (`0`/`1`), `insunits`, `osmode` | behaviour flags | ✓ |
| `extmin`, `extmax` | model-space extents `[x,y,z]` | read-only |

`{"op":"sysvar","set":{"ltscale":2.5}}` applies the whole object atomically
(`unknown_sysvar` / `invalid_sysvar_value` refuse it all);
`{"op":"sysvar","get":["ltscale"]}` → `result.values`. Omitted `get` returns
every readable name.

**Record editing (`set_properties`)** — typed, validated property writes on
any database record via JSON Pointer paths, with compare-and-set support;
`records`/`record_schema` describe every writable field.

### 6.8 Groups and selection sets

| Op | Fields | Notes |
|---|---|---|
| `group_create` | `name`, `handles` | Named group over existing entities; result reports the group's own object `handle` |
| `selection_set_save` | `name`, `handles` | Stores a named handle set for the session |
| `selection_set_load` | `name`, optional `"select":true` (default) | Recalls the set; with `select` it also becomes the current selection (`result.selected`) |

REST: `POST /groups`, `POST /selection-sets`, `GET /selection-sets/{name}[?select=true]`.

### 6.9 Images — `embed_image`

`{"path":"<png/jpg>","at":[x,y],"width":<drawing units>,…}`

| Mode | Behaviour |
|---|---|
| **Embedded (default)** | The bytes are stored inside the drawing as a self-contained frame entity. There is no external path, so the image cannot break when the source file is moved or deleted, and it travels through block definitions, clones and exports like any entity. Trade-offs: the DWG grows by the image size, and the picture never updates when the source file changes. Use this for barcodes/QR and any image that is part of the deliverable. |
| `"linked":true` | Stores a path-referenced image. The file must travel with the drawing and *can* break; the mode exists for images that must be swappable without touching the drawing. |

Repairing a broken linked image from a drawing authored elsewhere — no
dedicated op needed:

1. `query` with `type:"RasterImage"`, `detail:"full"` — the entity exposes
   its `file_path` and world `bounds`.
2. `entities_delete` the broken reference by handle.
3. `embed_image` a replacement at the same `at`/`width` derived from the old
   bounds — embedded, so it cannot break again.

### 6.10 Commands, batches, events

| Op | Purpose |
|---|---|
| `run` | Execute one command line headlessly (`"cmd":"LINE 0,0 10,10"` — command name + space-separated prompt answers; points `x,y[,z]`, options by displayed token). Response `status` is `completed` or `waiting_input`. |
| `start` / `input` | Interactive dialog: `start` opens a command, `state.command.accepts` lists the valid input kinds (`text`, `token`, `point`, `entity`, `structure`, `selection`, `enter`), `input` feeds one. |
| `cancel` | Aborts the running command. |
| `batch` | `steps[]` — sequential ops (≤ 64) where each step sees the previous step's state; stops at the first failure and reports `completed_steps` / `next_step`. |
| `select` | Change the current selection (`handles`, `clear`, filters). |
| `events` | Cursor-paged event stream (command lifecycle, document and selection changes, 128-event ring, `resync` flag). Polling by design. |
| `history` | Command-line history. |
| `capture` | PNG snapshot of the viewport or full window (`scope`, `max_dimension`). |
| `view_focus` | Zoom-to-fit + highlight the listed handles (GUI sessions; headless answers `gui_required`). |
| `action` | The 20 UI toggles (grid, ortho, …) by name. |

### 6.11 Complete REST endpoint table

| Method & path | Purpose |
|---|---|
| `GET /ready` · `/state` · `/capabilities` · `/openapi` | Readiness · full state · feature advertisement · OpenAPI 3 document |
| `GET /documents` · `POST /documents` · `DELETE /documents/{id}?discard=` | List · open/template/new · close |
| `GET /sysvars?names=` · `POST /sysvars` | Read · set |
| `POST /layouts` · `PUT /layouts/{name}/page-setup` | Create · write plot configuration |
| `GET /entities` · `GET /entities/{h}` · `POST /entities` · `DELETE /entities?handles=` | Query (incl. `where`) · one entity · create · erase |
| `POST /entities/transform` · `POST /entities/copy-to` | Modify · cross-document copy |
| `GET/PUT/DELETE /entities/{h}/xdata[/{app}]` | Extended data |
| `POST /blocks` · `DELETE /blocks/{name}` | Define (+`replace`) · delete |
| `POST /groups` · `POST /selection-sets` · `GET /selection-sets/{name}` | Groups · selection sets |
| `POST /plot` · `/wblock` · `/images` · `/commands` · `/undo` · `/redo` · `/save` | Publishing, export, command line, lifecycle |
| `GET /layers` · `/header` · `/records` | Database reads |
| `POST /file-identity` | Stable drawing GUID |
| `POST /{op}` | Passthrough for any envelope op |

---

## 7. Worked examples

### 7.1 Draw, verify, publish (REST)

```sh
# Create (missing layers auto-created; the batch is atomic).
curl -s -X POST http://127.0.0.1:8090/api/v1/entities -H "Content-Type: application/json" -d '{
  "entities":[
    {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
    {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0}
  ]}'
# → 201 {"result":{"handles":["63","64"],"created":2,"layers_created":["FRAME"]}}

# Filter by property.
curl -s "http://127.0.0.1:8090/api/v1/entities?type=Line&where=%5B%7B%22path%22%3A%22%2Flayer%22%2C%22op%22%3A%22eq%22%2C%22value%22%3A%22FRAME%22%7D%5D"

# Mark, wrap into a reusable block, plot every sheet, save as a template.
curl -s -X PUT http://127.0.0.1:8090/api/v1/entities/64/xdata/SPEC \
  -H "Content-Type: application/json" -d '[{"code":1000,"value":"PAGE-01"}]'
curl -s -X POST http://127.0.0.1:8090/api/v1/blocks \
  -H "Content-Type: application/json" -d '{"name":"MARK","base":[0,0,0],"handles":["63","64"]}'
curl -s -X POST http://127.0.0.1:8090/api/v1/plot \
  -H "Content-Type: application/json" -d '{"path":"C:/out/pages.pdf","layout":"all","per_page":true}'
curl -s -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwt"}'
```

### 7.2 Clone a region into a new, normalized drawing

```json
{"protocol":1,"op":"wblock","request_id":"clone-1","document_id":1,
 "handles":["63","64"],
 "template":"C:/tpl/base.dwt",
 "normalize":true,
 "path":"C:/out/page-01.dwg"}
```

The export inherits the template's tables/styles and is shifted so its
minimum lands on the origin. Open it afterwards with `{"op":"open","path":…}`
and verify with `query`.

### 7.3 Stable identity + idempotent retry

```json
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ {"ok":true,"result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true}}
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ same response served from the idempotency cache — never executed twice
```

---

## 8. Versioning and compatibility

- The wire protocol version is **1**; it is declared in every envelope and in
  the greeting line. Additions (new ops, new optional fields, new error
  codes) are non-breaking; clients should ignore unknown fields and treat
  unknown op names in `capabilities` as "not available in this build".
- Feature detection: read `capabilities` before unfamiliar work — every op in
  this document is listed there when the running build supports it.
- Drawing files: DWG and DXF readers/writers are version-tolerant; templates
  (`*.dwt`) are byte-compatible DWG files.

*Companion documents: the REST walkthrough (`README.md`) and the OpenAPI 3
description served at `GET /api/v1/openapi`.*
