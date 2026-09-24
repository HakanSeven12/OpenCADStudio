# OpenCADStudio MCP control

> **Client developers start here:** [`API-SPEC.md`](API-SPEC.md) is the
> consolidated, transport-neutral API specification — conventions, envelope,
> error codes and the full operation catalogue in one document. This README
> remains the operator-facing guide (setup, smoke tests, walkthroughs).

Every native OpenCADStudio build contains the same MCP server as the editor. `OpenCADStudio --mcp` starts it over stdio, opens the desktop editor when needed, and exposes the live document without Python, a package manager, a sidecar service, or client-specific code.MCP lets an AI client inspect the open drawing, execute editor commands, and verify the result through a shared protocol. Install OpenCADStudio, then add a local MCP server in the client and set its command to:

```sh
OpenCADStudio --mcp
```

The client must start that command over standard input/output. If it asks for the executable and arguments separately, select the installed `OpenCADStudio` executable and enter `--mcp` as its only argument. The exact registration screen or configuration location belongs to the client. Once connected, the four tools below should appear in the client's MCP tool list.

The server provides four tools:

- `ocs_sessions` finds running editor sessions and opens OpenCADStudio when none exists.
- `ocs_read` discovers capabilities and reads document state, complete database records, command manifests, entities, properties, kernel measurements and spatial relationships, history, events, and operation status.
- `ocs_execute` performs one operation, an atomic record update, or a sequential batch against the real editor.
- `ocs_capture` returns a bounded PNG of the drawing viewport or complete window.

Run `python docs/automation/mcp_acceptance.py target/debug/OpenCADStudio.exe`
for a repeatable end-to-end acceptance. It creates visible geometry through
MCP, audits and verified-saves DWG 2000/2013/2018 plus DXF 2000, and records a
viewport PNG and JSON report below `target/acceptance/`.

On a Windows workstation with a reference CAD application's command-line
console, run
`docs/automation/reference_acceptance.ps1 -Directory target/acceptance/TIMESTAMP -ConsolePath <console.exe>`.
It opens the three generated DWGs read-only in isolated console profiles, runs
AUDIT without repairs, and keeps one log per version. A console that stays
alive after `QUIT` is terminated after the timeout (only the process the runner
launched), and `ForcedTermination` is recorded separately from the audit result.

Native builds also accept a second, headless entry point (`OpenCADStudio --serve`, one
JSON request per line) for scripts and CI; the two native channels, their startup dialogs
and their limits are described in [native.md](native.md).

The normal flow is to call `ocs_sessions`, pass its returned `session_id` as `ocs_session_id` to the other tools, read the chosen session with `ocs_read`, then pass the returned document state into `ocs_execute`. For example, an undo request has this shape:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "undo",
    "request_id": "undo-1",
    "document_id": 1,
    "revision": 12
  }
}
```

Call `ocs_read` with `op: "capabilities"` before unfamiliar work. It reports the command, geometry, transaction, capture, and database facilities supported by the running build. The `records.collections` list gives every available database collection, its record count, and whether it can be edited.

Before delivery, call `ocs_read` with `op: "audit"` and the intended
`target_format` / `target_version`. The report includes entity manifests,
layers, blocks, finite bounds, duplicate entity handles, DXF dangling handle
references, source hash, and `dropped_on_save`. A clean in-memory document can
still be lossy for a different format, so target selection is part of the
audit:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "op": "audit",
  "parameters": {"target_format": "dwg", "target_version": "2000"}
}
```

Use `save_verified` for the delivery write. It requires an explicit absolute
path, refuses overwrite and lossy conversion unless separately acknowledged,
writes the requested version, reopens it, checks the actual version, compares
the semantic entity manifest, audits raw DXF handle references, and returns a
SHA-256 hash. A verification failure preserves the output for diagnosis and
reports it as failed instead of silently declaring success.

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "save_verified",
    "request_id": "deliver-2026-09-22-1",
    "path": "C:\\drawings\\issued\\plan.dwg",
    "target_format": "dwg",
    "target_version": "2000",
    "overwrite": false
  }
}
```

Call `ocs_read` with `op: "record_schema"` before editing an unfamiliar record. With no parameters it lists the complete generated type registry. A collection returns the record types accepted by that collection even when the current drawing has no instance of a type. Supplying both `collection` and `type` returns the type's complete dependency graph, flattened property paths, JSON types, optional and sequence markers, enum variants, integer bounds, unambiguous unit annotations, identity fields, and write rules:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "op": "record_schema",
  "parameters": {
    "collection": "entities",
    "type": "Insert"
  }
}
```

Sequence fields include an `item_path_template` such as `/attributes/{index}`. Replace `{index}` with an index returned by `records` before using the path in `set_properties`. When a matching record exists, `runtime_schema` adds its exact JSON shape and bounded scalar examples. The embedded registry is generated from all 48 entity variants, all 36 non-graphical object variants, symbol-table records, header data, summary data, and the remaining stored record roots at build time.

`op: "records"` returns every serializable property of entities, non-graphical objects, symbol-table records, the complete header, document metadata, and derived database views. Omitting `collection` returns the same collection manifest; use `collection: "all"` for a paged query across the complete database. Records have a stable collection plus a handle or name, a type, and `properties`. Entity records also include their display and file record type names.

Property filters and projections use RFC 6901 JSON Pointer paths relative to `properties`. Multiple `where` filters are combined with AND. Supported operators are `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `contains`, `starts_with`, `ends_with`, `in`, `exists`, and `not_exists`:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "op": "records",
  "parameters": {
    "collection": "entities",
    "type": "Insert",
    "where": [
      {"path": "/common/layer", "value": "Equipment"},
      {"path": "/attributes/0/tag", "value": "COMPANY"}
    ],
    "paths": ["/block_name", "/insert_point", "/attributes"]
  }
}
```

Use `set_properties` to replace one or more fields atomically. OCS verifies the record identity, JSON types, optional compare-and-set values, layer locks, document revision, and request ID before committing one undoable edit. Handles, ownership slots, table names, and other database identity fields are read-only; their dedicated editor commands preserve cross-record references.

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "set_properties",
    "request_id": "attributes-1",
    "document_id": 1,
    "revision": 12,
    "collection": "entities",
    "handle": "2A",
    "updates": [
      {"path": "/attributes/0/value", "expected": "BPH", "value": "Updated"},
      {"path": "/insert_point/z", "value": 3.0}
    ]
  }
}
```

The same operation edits entries in `objects`, every symbol table, `header`, and `summary_info`. Collections marked `mutable: false` are derived or storage metadata and remain queryable. Their source records must be changed so OCS can rebuild those views consistently.

To discover command syntax without reading application source, call `ocs_read` with `op: "commands"`. The unfiltered response lists commands and actions. Add `parameters: {"name": "PLINE"}` for a command's batch examples and interactive guidance.

A complete command can be sent with `run`. Prompt answers are separated by spaces, points use `x,y` or `x,y,z`, and option answers use their displayed token:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "run",
    "request_id": "polyline-1",
    "document_id": 1,
    "revision": 12,
    "cmd": "PLINE 0,0 10,0 10,10 C"
  }
}
```

Use `batch` when the steps are already known. OCS supplies each step with the state produced by the previous one and stops on the first failure. `completed_steps` and `next_step` show exactly what committed:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "response_detail": "changed_entities",
  "request": {
    "op": "batch",
    "request_id": "shape-1",
    "steps": [
      {"op": "run", "cmd": "LINE 0,0 10,0"},
      {"op": "run", "cmd": "CIRCLE 5,5 2"}
    ]
  }
}
```

Execute responses use `response_detail: "compact"` by default and return only the state needed for the next edit. Use `changed_entities` to receive the current geometry of affected handles in the same response, or `full` when the complete editor state is needed.

`ocs_read` query accepts exact handles, type/layer filters, field projection, world-XY bounds, nearest-curve ranking, closed-curve containment, and exact intersections between two planar curves. `detail: "full"` adds the complete serialized entity properties. Nearest points, containment, curve length, area, and intersections are calculated by the geometry kernel:

```json
{
  "ocs_session_id": "SESSION_FROM_OCS_SESSIONS",
  "op": "query",
  "parameters": {"intersections": ["2A", "31"]}
}
```

For unfamiliar or conditional commands, use `start`, then inspect `state.command` in every response. Its `accepts` array gives the valid MCP input kinds, `options` gives the current tokens, and `input_example` gives the next request shape. Add the current state fields and a new `request_id` to each step.

Every `ocs_execute` request requires a caller-generated `request_id`. Reuse that ID only to retry the identical request after a timeout, together with the same session ID, document ID, expected revision, and selection. Commands report `waiting_input` while more input is required and asynchronous work remains `running` until its real callback finishes.

A `run` reply also carries two fields that describe what happened to the line:

* `blocked_by` — `null` when the command finished, otherwise what *this line*
  left open and waiting: `"command"` (an interactive command), `"text_editor"`
  (the in-place editor opened by the `TEXT` content step), `"mtext_editor"`, or
  `"modal:<Kind>"` for a dialog. A surface that was already open when the line
  started (an earlier line's editor, another tab, a startup dialog) is not
  attributed to this line, so a caller polling for `completed` is never stuck
  waiting on someone else's editor. `blocked_by` is set whenever `status` is
  `waiting_input`.
* `unconsumed` — the tokens of the line that no prompt ever asked for, in order.
  A fully consumed line reports `[]`; `CIRCLE 0,0 5 9` reports `["9"]` because
  the radius step ends the command. Over-typed input is reported instead of
  being dropped on the floor.

When a line ends at the `TEXT` content step (`TEXT 0,0 5 0 hello`) the text is
typed into the in-place editor and committed, so the line behaves exactly as the
same input typed into the GUI command line would: one `Text` entity, nothing
unconsumed, and the command closed rather than left armed for a second line. Geometry stays in OpenCADStudio and its geometry kernel.

`ocs_capture` defaults to the drawing viewport and a longest edge of 1600 pixels. Set `scope` to `window` for the full interface or change `max_dimension` between 256 and 4096.

Clients using MCP 2026-07-28 can advertise `io.modelcontextprotocol/tasks`. OCS then returns a standard task handle when an operation is still running and accepts `tasks/get`, `tasks/update`, and `tasks/cancel`. Other clients continue to receive the existing operation status and can read it with `ocs_read`.

The source-tree protocol smoke test treats the executable as a black box and uses only the Python standard library. It checks both supported protocol styles, the published schemas, structured errors, and the four-tool surface:

```sh
cargo build
python3 docs/automation/mcp_smoke.py target/debug/OpenCADStudio
```

The repeatable live-editor evaluation draws three isolated entities in one batch, verifies exact intersections, nearest geometry and kernel measurements, removes the entities, and reports call count, elapsed time and wire bytes:

```sh
python3 docs/automation/mcp_eval.py target/debug/OpenCADStudio
```

The web build is driven through two JavaScript functions instead of a process; see [web.md](web.md).
## Headless automation server (`--serve`)

Application clients that do not speak MCP spawn `OpenCADStudio --serve` and exchange one JSON object per line over stdin/stdout (`--port N` switches to `127.0.0.1:<N>`, one client at a time). The greeting line carries the build version and the session identity:

```json
{"ok":true,"ready":true,"version":"2026.38","session_id":"…"}
```

Legacy ops (`new`, `open`, `run`, `entities`, `query`, `records`, `layers`, `header`, `save`, …) act on the active document and need no envelope. Mutation ops of the shared control protocol — `wblock`, `plot`, `embed_image`, `set_properties`, `undo`, … — are addressed with an envelope: `"protocol":1`, a caller-generated `request_id` (≤128 bytes), and the `document_id` from `{"protocol":1,"op":"state"}`. An absent `session_id` is accepted; only a wrong one is refused. Settled responses carry `{ok, status, request_id, result, changes, state}`; `status` stays `accepted`/`running` for asynchronous work, polled with `{"protocol":1,"op":"operation","request_id":…}`.

### `wblock` — export entities or a block to a new DWG/DXF

Either a handle list or a block name, never both. The open document is not modified; the output format follows the path extension (`.dwg` or `.dxf`).

```json
{"protocol":1,"op":"wblock","request_id":"clone-1","document_id":1,"path":"C:/out/page-01.dwg","handles":["2A","31"]}
{"protocol":1,"op":"wblock","request_id":"clone-2","document_id":1,"path":"C:/out/cover.dwg","block":"A_CPT"}
```

The result reports `{"path":…, "entities":<count>}`. Referenced layer definitions travel with the entities; block definitions export flattened into model space.

### `plot` — render to PDF

Plots the active document without any dialog. `layout` selects `"Model"` (default), one layout name, or `"all"`. `area` is `"extents"` (default), `"display"`, `"limits"`, `"window"` (needs `window:[x0,y0,x1,y1]`), or `"layout"` (the laid-out sheet, layouts only). `paper` names a catalog sheet, `orientation` is `"Portrait"`/`"Landscape"`, sizing is `"fit":true` (default) or `"scale":"1:100"`, plus `center`, `offset_x/offset_y`, `upside_down`, `plot_style` (CTB path or discovered name) and the output toggles `transparency`, `lineweights`, `merge_lines`, `stamp`:

```json
{"protocol":1,"op":"plot","request_id":"plot-1","document_id":1,"path":"C:/out/pages.pdf","layout":"all","paper":"ISO_A4_(210.00_x_297.00_MM)","fit":true,"center":true}
```

Explicitly supplied fields override a layout's stored page setup; unspecified fields keep it. The result reports `{"path":…, "pages":<n>, "page_sizes":[[w,h],…]}` in millimetres.

### `embed_image` — attach a picture

Places the picture at `at:[x,y]` with `width` in drawing units (default ≈ pixels/100). The default embeds the raster into the drawing as a self-contained OLE2FRAME (`"kind":"Ole2Frame"`). `"linked":true` stores a `RasterImage` + `ImageDefinition` referencing the file path instead (`"kind":"RasterImage"`) — the image file must then travel with the drawing.

#### Embedded vs linked — when images can "break"

The embedded default carries the image bytes **inside** the DWG. There is no
path to lose, so the classic broken-image failure (PNG moved, renamed or
deleted after placing) cannot happen: the drawing renders the same on any
machine, and embedded images travel normally through `block_define`,
`wblock` and `entities_copy_to` like any other entity. Barcode/QR placement
(the SPM flow: attach → wrap into a block → plot → clone) should stay on
this default. Two deliberate trade-offs: the DWG grows by the image's file
size, and the picture never updates when the source PNG changes — which is
exactly what self-contained means.

`"linked":true` keeps the path and therefore keeps the failure mode. It
exists for images that must be swappable without touching the drawing.
Legacy SPM.ACAD files arrive with linked QR images, so a broken one is
still possible there; repair is a three-step composition, no dedicated op
needed:

1. Find it: `query` with `type:"RasterImage"` (or the `records` op) —
   `detail:"full"` exposes the definition path and the entity `bounds`.
2. Erase the broken reference by handle (`entities_delete`).
3. Re-place a fresh `embed_image` (embedded, so it cannot break again) at
   the same `at`/`width` read from the old `bounds`.

An image inside a block definition is reached the same way after opening
the block's content — SPM's `FindAndChangeRasterImagePath` (Catalog §5.3)
maps onto these three steps rather than a native op.

### Text content and bounds

`query` entities of type `TEXT`/`MTEXT` return the raw stored string in `value` plus a formatting-free rendering in `text` (MTEXT inline codes such as `\A1;` or `\P` are resolved; `%%d`-style TEXT codes become their glyphs). With `detail:"full"`, degenerate-width text bounds are widened with a documented estimate (height × 0.8 × character count) so `bounds`-based region filters stay usable.

### Smoke test

The stdio path is covered black-box by a Python script that draws two entities, exports them with `wblock`, plots them with `plot`, and checks the capability advertisement:

```sh
python3 docs/automation/serve_smoke.py target/debug/OpenCADStudio
```

## Capability areas

The automation surface is organised around the capability areas a CAD
automation client needs — application and documents, database objects, the
editor, events and plotting.

## REST API (`--http`) — for ordinary HTTP clients

`OpenCADStudio --http 8090` runs the same headless session behind a
resource-oriented REST surface on `http://127.0.0.1:<port>/api/v1`. Any
HTTP client works — curl, Python, C# `HttpClient`, JS `fetch`; there is no
SDK and nothing AI-specific. The server keeps one drawing session alive
across requests, binds loopback only, sends permissive CORS headers, and
serves its machine-readable description at `GET /api/v1/openapi`
(OpenAPI 3, embedded from `src/rest_openapi.json`).

```sh
OpenCADStudio --http 8090            # start the REST server (headless)
curl http://127.0.0.1:8090/api/v1/ready
```

### GUI-hosted REST channel (`--http` + file)

`OpenCADStudio.exe "<file>" --http 8090` boots the **normal editor** and
hosts a loopback REST channel on this very process, bound to
`http://127.0.0.1:8090/api/v1`: the **full REST API aimed at the live
session** — control the drawing (create, move, plot, query, records, …)
while the person keeps working, plus two person-in-the-loop picks only this
process can answer. Routes resolve through the same table as the headless
server, with the same ergonomics.

`GET /api/v1/state` caches the active `document_id` and later mutations
address the drawing without the caller repeating it; a stale cache (the
person switched documents) costs one state refresh and a retry, not an
error. `session_id` is not needed (the channel has no descriptor handshake;
a guessed value is stripped). Unknown POST paths are treated as automation
ops and forwarded — MCP-style, any op works; truly unrouted paths answer
`404 {"code":"unknown_route"}`.

On top of the shared surface, two **person-in-the-loop** picks exist only
on this channel — they need the GUI and the person at the screen:

| Call | Purpose |
|---|---|
| `POST /api/v1/getpoint` | Ask the person to pick one point; the connection stays open until they click (or Esc). |
| `POST /api/v1/user_select` | Ask the person to pick a sample set; the connection stays open until they press Enter (or Esc). |

Requests run one thread per connection, so a parked `getpoint`/`user_select`
never blocks other calls; a pick may park up to 30 minutes before the bridge
answers `response_timeout`.

**`getpoint`** — `{"prompt"}` is optional (shown on the command line):

```json
{"ok":true,"status":"completed","result":{"point":[125.5,64.25,0.0]}}
```

The next left-click in the drawing answers with the same snapped world
point a command would receive; **Escape** (or `{"op":"cancel"}`) answers
`{"ok":true,"status":"cancelled","result":{"cancelled":true}}`. While the
pick is pending, other mutations wait (`busy`), so `status:"running"` on a
poll means the person has not clicked yet.

**`user_select`** — interactive selection sampling: the person picks entities with the
normal gestures, **Enter** confirms, **Escape** cancels. The request body is
the `user_select` op verbatim (`type`/`layer` filter what counts as picked,
`prompt` replaces the command-line hint, `detail` is `summary`/`geometry`/
`full`, `clear` starts a fresh selection — all optional):

```json
POST /api/v1/user_select
{"request_id":"sample-1","type":"LINE","prompt":"Chọn đối tượng mẫu","detail":"full","clear":true}
```

The connection stays open while the person picks and answers with the same
payload the MCP/native callers see:

```json
{"ok":true,"status":"completed","result":{"cancelled":false,"count":1,"ignored":0,
 "handles":["2A"],"entities":[{"handle":"2A","type":"Line","layer":"0","start":[0,0,0],"end":[10,10,0],…}]}}
```

Escape (or `{"op":"cancel"}`) answers `status:"cancelled"` with
`{"cancelled":true,"count":0}`. Picks outside the `type`/`layer` filter are
deselected and reported in `ignored`.

**`get_selection` response** — `handle`/`type`/`layer`/`bounds` match the
`query` output exactly so clients share one parser; `text` is the
MTEXT-stripped string and `value` the raw one (both `null` for non-text
entities), and `block`/`position` identify an INSERT's block definition and
insertion point (`null` otherwise):

```json
{"ok":true,"status":"completed","result":{"count":2,"entities":[
  {"handle":"2A","type":"LwPolyline","layer":"CUT","bounds":{"min":[0,0,0],"max":[100,50,0]},"text":null,"value":null,"block":null,"position":null},
  {"handle":"31","type":"Block Reference","layer":"TITLE","bounds":{"min":[0,0,0],"max":[420,297,0]},
   "text":null,"value":null,"block":"A3","position":[0,0,0]}]}}
```

The read is passive — it never changes the selection, and the person can
keep working while the client polls.

### Endpoints

| Method & path | Purpose |
|---|---|
| `GET  /api/v1/ready` | Version, session id, active document id. |
| `GET  /api/v1/state` | Full editor state (documents, selection, command, camera). |
| `GET  /api/v1/capabilities` | Feature/collection advertisement incl. `operations`. |
| `GET  /api/v1/openapi` | OpenAPI 3 description of this whole surface. |
| `GET  /api/v1/documents` · `POST /api/v1/documents` | List; open `{"path":…}`, start from a template `{"template":…}` (a `.dwt`/`.dwg` file; the active drawing is replaced), or `Add()` a fresh untitled document (empty body) → **201**. |
| `DELETE /api/v1/documents/{id}?discard=true` | Close a document; `discard=true` drops unsaved changes, otherwise a dirty document is refused (`document_dirty`). |
| `GET  /api/v1/entities` | Query — `type`, `layer`, `detail`, `offset`, `limit`, `fields`, `handles`, `bounds`, `near`, `contains_point`, `intersections` (comma-separated) and `where` (a JSON-encoded property filter array, e.g. `[{"path":"/radius","op":"gt","value":2}]`). |
| `GET  /api/v1/entities/{handle}` | One entity, `detail:"full"`. |
| `POST /api/v1/entities` | Create a typed batch → **201** with `handles`. |
| `DELETE /api/v1/entities?handles=a,b` | Erase by handle (body `{"handles":[…]}` also accepted). |
| `POST /api/v1/entities/transform` | move/copy/rotate/scale/mirror/array. |
| `POST /api/v1/entities/copy-to` | Copy entities into another open document: `{"handles":[…],"document_id":target}` → **201** with the new handles. |
| `GET  /api/v1/entities/{h}/xdata?app=` · `PUT …/xdata/{app}` · `DELETE …/xdata/{app}` | Read / replace / remove extended data. |
| `POST /api/v1/blocks` | Define a block from entities (+ Insert); `"replace":true` drops a same-named definition first. |
| `DELETE /api/v1/blocks/{name}` | Delete a block definition with its children, markers and every Insert referencing it. |
| `POST /api/v1/file-identity` | Stable per-document GUID (`{"renew":true}` mints a fresh one). |
| `POST /api/v1/groups` | Create a named group from handles → **201**. |
| `POST /api/v1/selection-sets` · `GET /api/v1/selection-sets/{name}` | Save a named selection set → **201**; recall it (`?select=true` also makes it the current selection). |
| `POST /api/v1/get_selection` | Read-only snapshot of the current selection (see [the GUI-hosted channel](#gui-hosted-rest-channel---http--file) below). |
| `GET  /api/v1/sysvars?names=a,b` · `POST /api/v1/sysvars` | Read sysvars (`{"get":[…]}` in the body also works) / set them (`{"set":{"ltscale":2.5}}`). |
| `POST /api/v1/layouts` | Create a layout with default page setup → **201**. |
| `PUT  /api/v1/layouts/{name}/page-setup` | Write a layout's plot configuration (paper, orientation, fit/scale, center, window, plot style). |
| `POST /api/v1/plot` · `/api/v1/wblock` · `/api/v1/images` | PDF export (`"layout":"all","per_page":true` writes one PDF per layout) · DWG/DXF export (`"template"` bases the new database on a file, `"normalize":true` shifts the export to the origin) · attach picture. |
| `POST /api/v1/commands` | Run one command line (`{"cmd":"LINE 0,0 10,10"}`). |
| `POST /api/v1/undo` · `/redo` · `/save` | Lifecycle (`/save` to a `*.dwt` path writes the template — DWG bytes). |
| `GET  /api/v1/layers` · `/header` · `/records?collection=…` | Database reads. |
| `POST /api/v1/{op}` | Generic passthrough for any automation op. |

Status codes: `200`/`201` on success; `400` validation; `404` unknown route
or handle (`code:"entity_absent"`); `409` stale state or GUI required;
`503` busy. Every failure body keeps the symbolic `code` and message, so
clients can branch on either. The server-generated `request_id` and the
retry-on-stale-state dance happen inside the REST layer — callers just do
HTTP.

### Walkthrough with curl

```sh
# 1. Create entities (the missing layer "FRAME" is created automatically;
#    the whole batch commits as one undoable step or not at all).
curl -s -X POST http://127.0.0.1:8090/api/v1/entities \
  -H "Content-Type: application/json" \
  -d '{"entities":[
        {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
        {"type":"LwPolyline","vertices":[[0,0],[100,0],[100,60],[0,60]],"closed":true},
        {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0},
        {"type":"Circle","center":[80,30],"radius":5}
      ]}'
# → 201 {"ok":true,"status":"completed","result":{"handles":["63","64","65","66"],…}}

# 2. Verify: query it back.
curl -s "http://127.0.0.1:8090/api/v1/entities?type=Line&detail=full"
# → {"ok":true,"entities":[{"handle":"63","start":[0,0,0],"end":[100,0,0],…},…]

# 3. Move the circle 10 units right.
curl -s -X POST http://127.0.0.1:8090/api/v1/entities/transform \
  -H "Content-Type: application/json" \
  -d '{"handles":["66"],"action":"move","vector":[10,0]}'

# 4. Mark the text with extended data (RegApp "SPM" registered implicitly).
curl -s -X PUT http://127.0.0.1:8090/api/v1/entities/65/xdata/SPM \
  -H "Content-Type: application/json" \
  -d '[{"code":1000,"value":"PAGE-01"},{"code":1070,"value":3}]'
curl -s "http://127.0.0.1:8090/api/v1/entities/65/xdata?app=SPM"

# 5. Turn the two frame lines into a block definition + Insert.
curl -s -X POST http://127.0.0.1:8090/api/v1/blocks \
  -H "Content-Type: application/json" \
  -d '{"name":"FRAME-MARK","base":[0,0,0],"handles":["63","67"]}'
# → 201 {"result":{"block":"FRAME-MARK","insert":"6B"}}

# 6. Save and prove persistence.
curl -s -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwg"}'

# 7. Filter entities by any property (RFC 6901 pointers, SQL-ish operators).
curl -s "http://127.0.0.1:8090/api/v1/entities?type=Circle&detail=geometry&where=%5B%7B%22path%22%3A%22%2Fradius%22%2C%22op%22%3A%22gt%22%2C%22value%22%3A2%7D%5D"

# 8. Set and read back drawing sysvars.
curl -s -X POST http://127.0.0.1:8090/api/v1/sysvars \
  -H "Content-Type: application/json" -d '{"set":{"ltscale":2.5}}'
curl -s "http://127.0.0.1:8090/api/v1/sysvars?names=ltscale,mirrtext"

# 9. Provision a sheet: create the layout, write its page setup, then
#    plot every layout to its own PDF (one file per entry in result.files).
curl -s -X POST http://127.0.0.1:8090/api/v1/layouts \
  -H "Content-Type: application/json" -d '{"name":"PLAN"}'
curl -s -X PUT http://127.0.0.1:8090/api/v1/layouts/PLAN/page-setup \
  -H "Content-Type: application/json" \
  -d '{"paper":"ISO_A4_(210.00_x_297.00_MM)","orientation":"landscape","fit":true,"center":true}'
curl -s -X POST http://127.0.0.1:8090/api/v1/plot \
  -H "Content-Type: application/json" \
  -d '{"path":"C:/out/plan.pdf","layout":"all","per_page":true}'

# 10. Save the drawing as a template (a .dwt is DWG bytes — same writer,
#     no lock held on the file) and start a fresh drawing from it.
curl -s -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwt"}'
curl -s -X POST http://127.0.0.1:8090/api/v1/documents \
  -H "Content-Type: application/json" -d '{"template":"C:/out/session.dwt"}'

# 11. Second document, then copy entities across documents.
curl -s -X POST http://127.0.0.1:8090/api/v1/documents -d '{}'   # a fresh document
curl -s -X POST http://127.0.0.1:8090/api/v1/entities/copy-to \
  -H "Content-Type: application/json" \
  -d '{"handles":["63","67"],"document_id":2}'

# 12. Group the copies, save them as a named selection set, recall it.
curl -s -X POST http://127.0.0.1:8090/api/v1/groups \
  -H "Content-Type: application/json" -d '{"name":"FRAME","handles":["63","67"]}'
curl -s -X POST http://127.0.0.1:8090/api/v1/selection-sets \
  -H "Content-Type: application/json" -d '{"name":"frame-set","handles":["63","67"]}'
curl -s "http://127.0.0.1:8090/api/v1/selection-sets/frame-set?select=true"

# 13. Close a document, discarding unsaved changes.
curl -s -X DELETE "http://127.0.0.1:8090/api/v1/documents/2?discard=true"
```

The same lifecycle — create → verify → transform → xdata → block →
save → reopen → verify persisted → erase → undo, then where filters,
sysvars, layout + page setup + per-page plotting, a `.dwt` template →
new-from-template round trip, a cross-document copy, groups and selection
sets, and a close-with-discard — runs as an automated black-box check:

```sh
python3 docs/automation/rest_smoke.py target/debug/OpenCADStudio
```

## Entity operations reference (protocol ops)

These are the underlying operations the REST routes call; every native
transport (stdio `--serve`, TCP, MCP, wasm) exposes them under the same
names and semantics. Angles are **degrees** on the wire; points accept
`[x,y]` (z=0) or `[x,y,z]`; handles are hexadecimal strings.

### `entities_create` — add entities

Batch of typed definitions, validated in full before anything commits; one
undo step; missing `layer`s are created and reported in `layers_created`.

| type | fields |
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

Every definition also accepts `layer` and `color` (ACI index). Response:
`{"handles":[…], "created":N, "layers_created":[…]}`. An invalid definition
(`unknown_entity_type`, `invalid_radius`, `invalid_vertices`, …) aborts the
whole batch with nothing committed.

### `entities_delete` — erase entities

`{"op":"entities_delete","handles":[…]}` — every handle must exist
(`entity_absent` lists the missing ones and nothing is erased). One undo
step; `result.erased` counts.

### `entities_transform` — move, rotate, scale, mirror

`{"op":"entities_transform","handles":[…],"action":…}` with, per action:

- `move` / `copy` — `vector:[dx,dy(,dz)]`; `copy` keeps the originals and
  returns the new handles in `result.created`.
- `rotate` — `center:[x,y]`, `angle_deg` (CCW positive).
- `scale` — `center`, `factor` (nonzero).
- `mirror` — `axis:[[x1,y1],[x2,y2]]` (points must differ); optional
  `"copy":true` to keep the originals.
- `array` — `rows`, `columns`, `row_spacing`, `column_spacing`; creates
  rows×columns−1 copies (the original counts as cell 0,0).

`result.affected` counts the addressed entities; `result.created` lists new
handles for copy/mirror-copy/array.

### `xdata_set` / `xdata_get` — extended data

`{"op":"xdata_set","handles":[…],"app":"SPM","data":[{"code":1000,"value":"PAGE-01"},{"code":1070,"value":7}]}` —
replaces that application's record on every handle and registers the APPID
table entry implicitly. Supported codes: `1000` string, `1003` layer name,
`1004` hex bytes, `1005` hex handle, `1010`–`1013` `[x,y,z]` (point,
position, displacement, direction), `1040`/`1041`/`1042` real, `1070` int16,
`1071` int32. An empty (or absent) `data` list removes the record — that is
the clear operation.

`xdata_get` is a **read**: `{"op":"xdata_get","handles":[…],"app":"SPM"}` →
`{"ok":true,"items":[{"handle":"65","xdata":{"SPM":["PAGE-01",7]}}]}` — no
`request_id`, no `document_id` needed.

### `block_define` / `block_delete` — block definition lifecycle

```json
{"op":"block_define","name":"MARK","base":[0,0,0],"handles":["63","67"],"insert_at":[0,0,0]}
{"op":"block_define","name":"MARK","base":[0,0,0],"handles":["6C"],"replace":true}
{"op":"block_delete","name":"MARK"}
```

BLOCK semantics: the sources move into the definition (flattened at
`base` = block origin) and one `Insert` is placed at `insert_at` (default
`base`), so the drawing looks unchanged while the entities became one
reusable block. Response: `{"block":"MARK","insert":"6B"}`. Names must be
unique and may not start with `*`. `"replace":true` drops a same-named
definition — its children, markers and inserts — inside the same undo step
before creating the new one: that is SPM's barcode re-import flow, so
`block_define` never fails with "already exists" on a re-import.
`block_delete` removes a definition completely (children, Block/BlockEnd
markers, every Insert referencing it and the table record) and reports
`result.erased`; unknown names are `block_missing`, `*`-prefixed layout
records are `block_protected`. Definitions survive save/open round trips
and are consumable by `wblock` (by block name) and `INSERT` creation via
`entities_create`.

### `file_identity` — per-document GUID

```json
{"op":"file_identity"}                     → {"identity":"<guid>","created":true}
{"op":"file_identity"}                     → {"identity":"<same guid>","created":false}
{"op":"file_identity","renew":true}        → {"identity":"<new guid>","created":true}
```

A stable RFC 4122 v4 GUID per drawing, minted on first call and rewritten
only with `"renew":true`. The identity lives on a single invisible marker
text (height 0.0001) at the origin carrying the XData application
`SPMDOCUNIQUE` — exactly how SPM.ACAD marks files — so it survives
save/open round trips and travels with clones that include the marker.
`GET /api/v1/state` also reports the document's `hand_seed` (the DWG handle
seed, hex).

### `view_focus` — GUI sessions

```json
{"op":"view_focus","handles":["65"],"highlight":true}
```

Fits the entities into the view and selects them. Headless servers refuse
with `code:"gui_required"`; REST clients get HTTP 409.

## Document, sheet and organization operations (protocol ops)

Same envelope conventions as the entity operations: `protocol:1`,
caller-generated `request_id`, `document_id` addressing, one undo step.
Angles stay **degrees** on the wire everywhere below.

### `close` — close a document

```json
{"protocol":1,"op":"close","request_id":"cl-1","document_id":1,"discard":true}
```

Closes the addressed document. A document with unsaved changes is refused
with `code:"document_dirty"` unless `"discard":true`; an unknown document id
is `document_closed`. The last tab is replaced by a fresh empty drawing, so
the session always keeps one document. `result.closed` reports the outcome.
`activate`, `close` and `entities_copy_to` are exempt from the
active-document guard — they may address any open document.

### `sysvar` — read and write system variables

```json
{"protocol":1,"op":"sysvar","request_id":"sv-1","document_id":1,"set":{"ltscale":2.5,"clayer":"Walls"}}
{"protocol":1,"op":"sysvar","request_id":"sv-2","document_id":1,"get":["ltscale","extmax"]}
```

`set` applies a JSON object of name → value pairs atomically (unknown names
→ `unknown_sysvar`, wrong values → `invalid_sysvar_value`, nothing partial).
`get` (also with an omitted list = every readable name) returns
`result.values`. The registry:

| var | meaning | writable |
|---|---|---|
| `ltscale` | global linetype scale (positive) | ✓ |
| `celtscale` | current-entity linetype scale | ✓ |
| `clayer` | current layer (must exist) | ✓ |
| `ctextstyle` | current text style (must exist) | ✓ |
| `textsize` | default text height | ✓ |
| `filletrad` | fillet radius (≥ 0) | ✓ |
| `mirrtext` | mirror-text flag (`0`/`1`) | ✓ |
| `insunits` | insertion units (integer) | ✓ |
| `osmode` | object-snap bitmask (integer) | ✓ |
| `pdmode` / `pdsize` | point display mode / size | ✓ |
| `extmin` / `extmax` | model-space extents `[x,y,z]` | read-only |

### `layout_create` / `page_setup_set` — layouts and page setups

```json
{"protocol":1,"op":"layout_create","request_id":"lc-1","document_id":1,"name":"PLAN"}
{"protocol":1,"op":"page_setup_set","request_id":"ps-1","document_id":1,"layout":"PLAN",
 "paper":"ISO_A4_(210.00_x_297.00_MM)","orientation":"landscape","fit":true,"center":true,
 "scale":"1:100","window":[0,0,210,297],"plot_style":"monochrome.ctb"}
```

`layout_create` adds a layout with the default page setup and a sheet
viewport; duplicates are refused (`layout_exists`). `page_setup_set` writes
the named layout's plot configuration — explicit fields win, the rest keeps
the stored setup. `paper` resolves through the paper catalog
(`invalid_paper` lists nothing — use a canonical catalog name);
`orientation` is `portrait`/`landscape` (degrees of sheet rotation);
sizing is `"fit":true` or `"scale":"paper:drawing"` like `"1:100"` (both
parts positive — `nonsense` is refused with `invalid_scale`, it does not
silently mean 1:1); `center` centers the plot; `window` sets a plot window;
`plot_style` names a CTB/STB. Unknown layouts are `layout_missing`. The
stored setup is what `plot` consumes when the op's own fields are absent.

### Templates — start from and save as `.dwt`

```json
{"protocol":1,"op":"new","request_id":"nt-1","template":"C:/tpl/base.dwt"}
{"protocol":1,"op":"wblock","request_id":"wb-1","document_id":1,"path":"C:/out/x.dxf","handles":["2A"],"template":"C:/tpl/base.dwg"}
```

`new` with `"template"` starts an untitled drawing from a `.dwg`/`.dxf`/
`.dwt` file: tables, styles and entities come from the template and the
document keeps no source path (`result.total` counts the imported
entities). A `.dwt` file is **DWG bytes** — the same binary DWG writer
produces it through a hidden scratch file that is renamed into place, so
no lock is held on the target and a save can never leave a half-written
template. `wblock` with `"template"` bases the exported database on the
template's tables/styles instead of the source drawing's.

### `entities_copy_to` — copy into another document

```json
{"protocol":1,"op":"entities_copy_to","request_id":"cp-1","document_id":2,"handles":["2A","31"]}
```

Copies the entities from the active document into the open document
addressed by `document_id`. Every handle must exist in the source
(`entity_absent` otherwise); the target must be open (`document_closed`).
Referenced (missing) layer definitions travel along. `result.created` lists
the new handles in the target, `result.count` their number. Combined with
an empty `POST /documents` (REST) this is the multi-document assembly
workflow.

### `group_create` — named groups

```json
{"protocol":1,"op":"group_create","request_id":"g-1","document_id":1,"name":"FRAME","handles":["2A","31"]}
```

Creates an anonymous-group-record-backed named group over existing handles
(`entity_absent` if any is missing). `result` reports the `group` name and
its object `handle`.

### `selection_set_save` / `selection_set_load` — SelectionSet reuse

```json
{"protocol":1,"op":"selection_set_save","request_id":"s-1","document_id":1,"name":"frame-set","handles":["2A","31"]}
{"protocol":1,"op":"selection_set_load","request_id":"s-2","document_id":1,"name":"frame-set","select":false}
```

Named handle sets are **session-scoped** by design (like a held
`SelectionSet` object id, they live while the session lives). `save` stores
and returns the handle list; `load` recalls it and, with `"select":true`
(the default), also makes the entities the current selection
(`result.selected` counts). Unknown names are `selection_set_missing`.

### `user_select` — ask the person at the screen to pick entities

```json
{"protocol":1,"op":"user_select","request_id":"us-1","document_id":1,"type":"CIRCLE","layer":"Equipment","prompt":"Pick the equipment circles","detail":"full","clear":true}
```

Hands the screen to the person operating the GUI: they pick entities with the normal gestures and **Enter confirms,
Escape cancels**. The operation resolves only after that answer — pollers
see `status:"running"` the whole time, and every other mutation waits, so
treat `running` as "the person is still picking", not as a hang. When they
confirm, `result` carries `cancelled:false`, the picked `handles` and the
matching `entities` with their attributes (`detail`: `summary`, `geometry`
or `full` — default `full`, the whole entity object). Escape answers
`cancelled:true` with empty lists; an immediate Enter with nothing picked
is an empty answer.

Optional filters: `type` / `layer` gate the *answer*, not the gestures —
picks outside the filter are deselected at confirm time and counted in
`result.ignored`. `prompt` replaces the on-screen instruction, `clear`
(default `true`) starts from an empty selection, and `detail` controls the
attribute depth. Requires the desktop GUI (headless `--http`/`--serve`
sessions answer `gui_required` — there is nobody at a screen to ask).

### `plot` per-page publishing

`{"op":"plot", …, "layout":"all","per_page":true}` writes **one PDF per
layout** instead of one multi-page document, returning `result.files`
(`[{path,…},…]`) — one entry per layout, ready for sheet-by-sheet delivery.
Without `per_page` the result stays `{path, pages, page_sizes}`.

### `query` `where` — property filters

```json
{"op":"query","type":"Circle","detail":"geometry",
 "where":[{"path":"/radius","op":"gt","value":2},{"path":"/common/layer","op":"eq","value":"Walls"}]}
```

`where` filters query results on serialized entity properties using RFC
6901 JSON Pointer paths (`/radius`, `/common/layer`, …) and the operators
`eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `contains`, `starts_with`,
`ends_with`, `in`, `exists`, `not_exists`. Filters combine with
AND and are validated up front (a bad pointer or unknown operator fails the
query before any entity is inspected). The same filter shape works over the
REST `GET /entities?where=<json-encoded>` parameter. Hatch queries expose
their loop definitions under `properties` so boundaries can be read back.
