# OpenCADStudio Automation API — Specification

Specification version **1.1** · API protocol **1** · applies to fork builds
`2026.41` and later (`2026.40` for everything except §3.5 and §6.12).

This document is the complete, self-contained reference for the OpenCADStudio
automation API. Everything a client needs — conventions, transports, the
operation catalogue, validation rules, error codes and worked examples — lives
here. The API is transport-neutral: one operation dispatcher serves every
client, so a request has identical semantics whether it arrives over a local
HTTP port, a stdio pipe, a TCP socket or an AI-tool (MCP) session.

MCP session set-up and a hands-on REST walkthrough live in
[`docs/automation/README.md`](README.md); this file links to it rather than
repeating either.

---

## 1. Overview

| Concept | Meaning |
|---|---|
| **Session** | One running editor process. Owns a session id, an event cursor and 1…N open documents. |
| **Document** | One drawing (a tab). Identified by a stable `document_id`. Exactly one document is **active** at a time. |
| **Handle** | Identity of every database object — a hexadecimal string (e.g. `"2A"`). Handles are stable across save/open and are the key for all read, edit, erase and clone operations. |
| **Operation** | One atomic step. Data mutations are undoable unless identified as view state; reads are passive. A rejected request changes nothing. |
| **Envelope** | The mutation wrapper: `"protocol":1`, a caller-generated `request_id`, and the `document_id` under edit. |
| **Pick** | A human-in-the-loop operation (`user_select`, `getpoint`, §6.12): the operation stays pending while a *person* answers at the screen. |

Design rules, in priority order:

1. **Idempotency** — a mutation replayed with the *same* `request_id` returns
   the cached result instead of executing twice. Generate one fresh
   `request_id` per logical operation (≤ 128 bytes) and reuse it only to
   retry after a timeout.
2. **Optimistic state** — read `state` first, pass `document_id` (and
   `revision`/`geometry_revision` when you hold them) back on mutations. A
   refusal whose code is in the retryable set (§5.2) means "your snapshot is
   behind": refresh state and retry once with a *new* `request_id`.
3. **Atomicity** — every mutation is all-or-nothing. Document-data mutations
   produce exactly one undo step; exceptions such as `layout_activate` only
   change non-undoable view state. Multi-step transactions are the client's
   `batch` op or a sequence of ops against a held `revision`.
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
| Names | layer / block / style / layout names are case-insensitively unique; `*`-prefixed names are reserved |
| Booleans | JSON `true`/`false` |
| Templates | a `*.dwt` file is written and read as DWG bytes — same container, template extension |
| Paper sizes | catalog names, e.g. `"ISO_A4_(210.00_x_297.00_MM)"` (canonical name is echoed back in results) |
| Scales | `"fit"` or `"paper:drawing"` ratio like `"1:100"` (both parts > 0) |

---

## 3. Transports

All transports speak to the same dispatcher; pick one per client. REST and
MCP advertise the whole surface; feature detection is `capabilities` (§4).

### 3.1 HTTP REST — headless (`--http <port>`)

`OpenCADStudio --http 8090` runs a private, window-less session behind a
resource-oriented REST surface at `http://127.0.0.1:<port>/api/v1` (loopback
bind only, JSON bodies, one request per connection, no CORS headers). A
machine-readable OpenAPI 3 description of the whole surface is served at
`GET /api/v1/openapi`.

Every request carries `Authorization: Bearer <token>`: the value of
`OCS_API_TOKEN` when the launcher set it, otherwise a random token printed on
stderr at startup. A request without it is answered **401**
`{"code":"unauthorized"}`. Loopback is shared by every process and user on
the machine, so the port alone is no credential.

| Status | Meaning |
|---|---|
| `200` | Success (read or update) |
| `201` | A resource was created (document, entities, block, group, selection set, layout) |
| `400` | Validation failure — the body keeps the symbolic `code` and message |
| `401` | Missing or wrong bearer token (`code:"unauthorized"`) |
| `404` | Unknown route, unknown handle (`code:"entity_absent"`), or unknown layout (`code:"layout_missing"`) |
| `409` | State conflict (`stale_state`, `document_not_active`) or GUI required (`gui_required`) |
| `503` | Dispatcher busy |

The REST layer generates `request_id`s and caches the active `document_id`
server-side, retrying once on retryable refusals — plain HTTP clients do not
need envelope bookkeeping. Power clients may instead POST any operation name
to `/api/v1/{op}` with an explicit body — including `operation` (poll a
pending op) and `cancel`.

Connections are served one thread each: a slow op (`plot`, opening a large
drawing) or a client that connects and stalls holds only its own connection,
never the other callers. The ops themselves still execute one at a time on
the private session (the same one-dispatcher pipeline as every transport), so
the concurrency buys responsiveness, not parallel mutation. A connection that
sends nothing for **15 s** is dropped.

### 3.2 HTTP REST — GUI-hosted (`--http <port>` **with a drawing file**)

`OpenCADStudio.exe "<file>" --http 8090` boots the **normal editor** and hosts
the same REST surface on this very process, aimed at the drawing the person at
the screen is working on: identical routes, identical semantics, plus the
human-in-the-loop picks of §6.12 (which need the window this process owns).
Differences worth knowing:

- `GET /state` primes a server-side `document_id` cache; later mutations
  address the live drawing without the caller repeating the id. A stale cache
  (the person switched documents) costs one state refresh and one retry, not
  an error.
- `session_id` is not needed (the channel has no descriptor handshake; a
  guessed value is stripped).
- One thread per connection: a parked pick never blocks other calls. A pick
  may park up to **30 minutes**; any other op answers within **300 s** or the
  bridge replies `response_timeout`.
- A parked pick watches its own connection: the moment the client hangs up
  (its own timeout, a crash, a killed probe), the pending pick is retracted
  with `cancel` and the channel is free again — the next pick starts clean
  instead of dying on `interactive_pending` until the person happens to press
  Esc. The same retraction fires if the caller waits out the full 30 minutes.
- Every pick logs its lifecycle to stderr as `[pick] <op> <request_id>` lines
  (start, cancelled-by-disconnect, settled), one per state, so a client-side
  log can be cross-referenced line for line.
- Unknown POST paths are treated as op attempts and forwarded (any op of the
  surface works — including `operation` to poll a pending pick and `cancel`
  to retract it); truly unrouted paths answer `404 {"code":"unknown_route"}`.
- The same bearer token as §3.1 is required on every request.

### 3.3 stdio JSONL (`--serve`)

One JSON value per line over stdin/stdout. The **first** stdout line is the
ready greeting: `{"ok":true,"ready":true,"version":"…","session_id":"…"}`.
Every subsequent line is one request/response pair (legacy reads need no
envelope; mutations use the envelope of §1).

### 3.4 TCP (`--serve --port N`)

The same JSONL protocol as §3.3 over `127.0.0.1:<N>`, one client at a time.
Every request line carries `"token":"<token>"` (the §3.1 token); a line
without it is answered `{"ok":false,"code":"unauthorized"}` and the
connection is closed.

### 3.5 AI-tool session (`--mcp`)

Exposes 20 tools with JSON-Schema described arguments. Four are general-purpose
facades: `ocs_sessions`, `ocs_read`, `ocs_execute` (all 57 mutation operations,
§6), and `ocs_capture`. Sixteen are intent tools over the same operation paths:
`ocs_inspect`, `ocs_query`, `ocs_find_text`, `ocs_audit`, `ocs_commands`,
`ocs_measure`, `ocs_render`, `ocs_run`, `ocs_new`, `ocs_open`, `ocs_save`,
`ocs_save_verified`, `ocs_select`, `ocs_cancel`, `ocs_undo`, and `ocs_redo`.

Intent tools expose operation arguments directly. `ocs_session_id` is optional
when exactly one session is available and required to disambiguate several.
Execute intents make `request_id` optional but pass through a caller-supplied
value; reuse that value with the identical request for a safe retry. Generated
IDs do not make autonomous retries safe. `ocs_run` maps `command` to the
operation's `cmd`. `ocs_render` reuses capture behavior with `view: "extents"`
as its default. Session discovery, idempotency, validation, task behavior, and
capability advertisement remain identical to the facade path. Set-up
instructions: see the MCP section of [`README.md`](README.md).

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
| `block_editor` | The definition open in the block editor, `{name, handle}`, or `null` — while set, created geometry joins that definition |
| `command` | An interactive command's state, when one is running: `accepts[]`, `options[]`, `input_example` |
| `operation` | The `request_id` of the operation currently pending, when one is |
| `capabilities` | Feature list (also available as the dedicated `capabilities` read) |

`hello` is an alias of `state`. `file_identity` (§6.1) additionally gives
every drawing a stable GUID.

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
| `stale_state`, `session_changed` | The caller's snapshot is behind | yes — nothing ran, so refresh `state` and retry once; the same `request_id` is safe. Do not echo `camera_revision` on edits |
| `document_required`, `document_not_active`, `document_closed` | Envelope addressing problem | yes (refresh state) |
| `document_dirty` | Closing would lose unsaved changes | no — resend with `"discard":true` to confirm |
| `entity_absent` | A listed handle does not exist (the message names them) | refresh and re-read |
| `command_busy` | An interactive command is active; the reply names it (`command`, `document_id`) and the client holding it (`owner_client_id`, null when the user holds it) | finish or `cancel` first |
| `gui_required` | Operation needs the editor window (`view_focus`, and the picks of §6.12 over headless transports) | headless sessions only |
| `busy` | Async work still running | poll `operation` |
| `interactive_pending` | Another person-pick (§6.12) is already waiting for the user | wait for it, or retract it with `cancel` |
| `user_input_busy` | The person is mid-command or in a dialog; their keystrokes own Enter/Esc | wait, then retry |
| `invalid_detail` | `user_select` `detail` outside `summary`/`geometry`/`full` | no |
| `selection_changed` | Precondition `selection[]` no longer matches | re-read and confirm |
| `name_required`, `window_required`, `entities_required`, `selection_required`, `app_required`, `path_required`, `handles_required`, `block_required` | Missing mandatory field | no |
| `unknown_entity_type`, `invalid_radius`, `invalid_vertices`, `invalid_point`, `invalid_text`, `invalid_scale`, `invalid_paper`, `invalid_handle`, `invalid_xdata`, `invalid_sysvar_value`, `invalid_layout_name`, `invalid_orientation`, `invalid_area`, `invalid_offset`, `invalid_window` | Validation failures — nothing commits | no |
| `unknown_sysvar` | Variable outside the registry (§6.7) | no |
| `layout_exists`, `layout_missing`, `layout_protected`, `layout_name_unchanged`, `layer_failed`, `block_failed`, `block_missing`, `block_protected`, `block_cycle`, `selection_set_missing`, `template_missing`, `template_failed`, `save_failed` | Domain refusals, self-explanatory | no |

Unknown routes answer `code:"unknown_route"` (HTTP 404). A timed-out parked
request answers `code:"response_timeout"`.

---

## 6. Operations reference

Every mutation below is an envelope op (§1). Reads (`query`, `records`,
`layers`, `layouts`, `xrefs`, `page_setup_get`, `header`, `xdata_get`,
`capabilities`, …) need no envelope and are passive. All 57 mutations are
listed in the MCP schema; `capabilities` advertises feature availability,
including the layout and xref operations. The REST passthrough accepts every
mutation at `POST /api/v1/{op}`.

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
and `color` (index); any other field is refused (`unknown_entity_field`).

| Type | Fields |
|---|---|
| `Line` | `start`, `end`, optional `thickness` |
| `Circle` | `center`, `radius` (≥ 0) |
| `Arc` | `center`, `radius` (> 0), `start_angle_deg`, `end_angle_deg` |
| `Spline` | `control_points` (more than `degree`) or `fit_points` (≥ 2), optional `degree` (1–10, default 3), `knots` (control points + degree + 1, non-decreasing), `weights` (one positive value per control point), `closed`, `periodic`, `rational`, `planar`, `linear` |
| `LwPolyline` | `vertices` (≥ 2), `closed`, optional `constant_width` |
| `Point` | `location` |
| `Text` | `value`, `position`, optional `height` (default 2.5), `rotation_deg`, `style` |
| `MText` | `value`, `position`, optional `height`, `width`, `rotation_deg`, `style` |
| `Insert` | `block` (must exist), `position`, optional `scale` (number or `[x,y,z]`, nonzero), `rotation_deg`, `attributes` (`{"TAG":"value"}`; tags are case-insensitive, the ones left out keep their default; unknown or constant tags are refused) |
| `Solid` | `corners` (3 or 4 `[x,y,z]`) |
| `Hatch` | `boundary` (≥ 3 points, closed), `solid`, or `pattern` (catalog name, default `ANSI31`), `pattern_scale`, `pattern_angle_deg` |

While `state.block_editor` names a definition, the whole batch is authored
inside it, on the editor's own undo stack, exactly like `run`; an `Insert`
that would nest the open definition inside itself is refused
(`block_cycle`) before anything commits.

Result: `{"handles":[…],"created":N,"layers_created":[…]}` → HTTP 201.

### 6.3 Entity modification

| Op | Fields | Notes |
|---|---|---|
| `entities_set` | `handles` plus any of `layer`, `color`, `linetype`, `linetype_scale`, `invisible`, optional `label` | One validated flat patch and one undo step for all unique handles. Existing case-insensitive layer/linetype names; colors are ByLayer/ByBlock, ACI 0..256, `{Index:N}`, `{Rgb:{r,g,b}}` or `{rgb:[r,g,b]}`. Result `{requested,written,unchanged}`; no-op writes create no history. Unknown fields, missing/locked/non-native targets and invalid values are atomic refusals. |
| `entities_delete` | `handles` | Every handle must exist; nothing is erased otherwise (`entity_absent` names them) |
| `entities_transform` | `handles`, `action`, per-action params | `move`/`copy`: `vector:[dx,dy(,dz)]` (copy returns new handles in `result.created`); `rotate`: `center`, `angle_deg`; `scale`: `center`, `factor` (≠ 0); `mirror`: `axis:[[x1,y1],[x2,y2]]`, optional `copy:true`; `array`: `rows`, `columns`, `row_spacing`, `column_spacing` |
| `entities_copy_to` | `handles`, `document_id` (target) | Cross-document clone; the dependencies the interactive copy carries travel along (layers, linetypes, styles, and the block definitions the source holds for the copied references); result `created[]` + `count` |

### 6.3a Node graph — `node_graph`

`{"op":"node_graph","action":…}` drives the active drawing's node graph; every
action answers with the graph state (`nodes` with their `sections`, ports,
values, `params`, `outputs` and owned `handles`; `links`; `visible`).

| Action | Fields | Notes |
|---|---|---|
| `library` | — | Node types: `objects` (drawing object nodes; `creates:false` for `Object`, which takes any linked object) and `operations` (`type`, `category`, `inputs`, `outputs`) |
| `state` | — | Default action |
| `show` / `hide` | — | Panel visibility |
| `add` | `type`, optional `kind` (`object`/`operation`, needed when a name is both, e.g. `Point`), optional `position:[x,y]` | `result.node` is the new id |
| `remove` | `node` | Erases the entities an object node made |
| `connect` | `from:{node,field}`, `to:{node,field}` | `graph_cycle` when it would close a loop; `unknown_port` when a side has no such port |
| `disconnect` | `to:{node,field}` | |
| `set` | `node`, `field`, `value` | An input's own value. On object nodes it is written to the entity on every evaluation |
| `toggle` | `node`, `field` | Folds a section (title or its `section:` port) or a coordinate group |
| `clear` / `run` | — | Clear starts an empty graph; run re-evaluates |
| `save` / `load` | `path` | `.ocg` file |

Object node ports: each Properties row by its field, each coordinate group
(`start_x|start_y|start_z`, a point `{x,y,z}`) and each section header
(`section:<first field>`, an object of field values). Linking two groups or two
section ports copies every value across.

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

`where` filters use JSON Pointer paths into the returned entity JSON:
`/layer` at every detail, geometry fields such as `/radius` with
`detail:"geometry"`, and `/properties/common/color/Index` with
`detail:"full"`. Legacy raw-property paths such as `/common/layer` remain a
fallback. Operators are `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `contains`,
`starts_with`, `ends_with`, `in`, `exists`, and `not_exists`. Filters are
validated up front; a bad pointer or unknown operator fails the query before
any entity is inspected. On REST the same filter rides
`GET /entities?where=<json-encoded>`.

Text entities return the raw string in `value` plus a formatting-free
rendering in `text`; degenerate-width text bounds are widened with a
documented estimate (height × 0.8 × character count) so region filters stay
usable.

Every returned entity, at every detail level including `summary`, carries
`owner` (the owning block record's handle, uppercase hex; `0` when unowned) and
`space` (that record's name:
`*Model_Space` for drawing geometry, a layout's record on a sheet, a
definition such as `*U1` for its children), so definition geometry can be
filtered out of a drawing result. The capture's `visible_entities` /
`annotations` records carry the same two fields.

Every entity returned above `summary` detail also carries `constraints`: one
entry per constraint the solver holds on it — `kind` (the constraint's
canonical kind name, shared by the constraint commands and reports),
`participants` (every distinct entity handle the constraint relates, in
reference order), `enabled` (`false` for a constraint kept but not solved),
`conflict` (`null`, or `Redundant` when the solver found the constraint
already implied by the rest, `Conflicting` when removing it would let the rest
solve without satisfying it) and `form` (`Dynamic` or `Annotational` for a
dimensional constraint's dimension; `null` when the record is not a
dimensional constraint's dimension — a geometric constraint, or a dimension
that does not resolve). An
empty array states that the entity carries no
constraint. A `fields`
projection that does not name `constraints` drops it, like any other property.
The same array is on each `records` `entities` row, each `get_selection`
entity, each `user_select` hand-over and each capture candidate, so the
association the solver enforces is enumerable from every read surface. The
`user_select` hand-over follows its `detail` too: `summary` leaves the array
out.

Companion reads: `entities` (alias), `layers`, `layouts`, `xrefs`, `header`,
`records` (paged, filterable database records), `record_schema` (generated type
registry with write rules), `properties`, `measure` (length/area/bounds),
`xdata_get`, `get_selection` (§6.12).

### 6.5 Blocks

| Op | Fields | Semantics |
|---|---|---|
| `block_define` | `name`, `base:[x,y,z]`, `handles`, optional `insert_at`, optional `"replace":true` | The sources move into the new definition (flattened at `base` = block origin) and one `Insert` is placed at `insert_at` (default `base`), so the drawing looks unchanged. `"replace":true` drops a same-named definition — children, markers and inserts — inside the same undo step before creating the new one (idempotent re-import). Result `{"block":name,"insert":"<handle>"}`. Names are unique, `*` is reserved. |
| `block_delete` | `name`, optional `"force":true` | Removes the definition completely: child entities, block markers and the table record. One undo step (`Delete block <name>`). A referenced block is refused (`block_referenced`); `"force":true` erases the referencing INSERTs in the same step and reports `result.references`. Result `erased`. Unknown → `block_missing`; layout/anonymous/xref/dynamic records → `block_protected`; open BEDIT session → `block_in_edit`; invalid/taken names → `block_invalid_name` / `block_exists`. |
| `wblock` | `path`, `handles` *or* `block`; optional `"template":"<file>"` (the export inherits that file's tables/styles), optional `"normalize":true` (the export is shifted so its overall bounds minimum lands on the origin) | Writes a standalone DWG/DXF (extension decides). The source document is untouched. Result `{path, entities, normalized}`. |

REST: `POST /blocks`, `DELETE /blocks/{name}` (add `?force=true` to erase the referencing INSERTs instead of being refused), `POST /wblock`.

### 6.6 Layouts and plotting

| Op | Request | Result and semantics |
|---|---|---|
| `layouts` (read) | none | `{current,count,layouts[]}`; every entry has `name`, `handle`, `current`, `model`, `tab_order`. Includes Model and paper layouts. |
| `layout_create` | `name` | Adds a default page setup and sheet viewport, plus the normal initial model viewport when `LAYOUTCREATEVIEWPORT` is enabled; result has `layout` and the layout-name list. Does **not** activate the new layout. |
| `layout_rename` | `name`, `new_name` | Renames a paper layout; result has canonical `name`, `new_name`, `current`, `layouts`. One undo step. |
| `layout_activate` | `name` | Activates Model or a paper layout; result has canonical `layout`, `current`. Non-undoable view state. |
| `layout_delete` | `name` | Deletes a paper layout; result has canonical `deleted`, `current`, `layouts`. Deleting the current layout falls back to Model. One undo step. |
| `page_setup_get` (read) | `layout` | Returns canonical `layout` and the stored `page_setup` fields below, including for Model. |
| `page_setup_set` | `layout` plus optional fields below | Updates the stored setup and returns its canonical layout, paper, scale, and plot style. One undo step. |
| `plot` | `path` (`.pdf` or `.svg`), `layout` (`"Model"` default, a name, or `"all"`), `area` (`"extents"` default, `"display"`, `"limits"`, `"window"` + `window:[…]`, `"layout"`), `paper`, `orientation`, `shade`, `fit`/`scale`, `center`, `offset_x/offset_y`, `upside_down`, `plot_style` (CTB/STB), output toggles `apply_plot_styles`, `transparency`, `lineweights`, `scale_lineweights`, `paperspace_last`, `merge_lines`, `stamp`, SVG-only `non_scaling_stroke`, `text_as_outlines` and `layer_attributes` (all default `false`; outlines replace searchable text with glyph paths, layer attributes tag each drawn item with a `data-layer` group), and `"per_page":true` (with `layout:"all"` writes **one output file per layout**, returning `result.files`) | Writes PDF or SVG synchronously with no device dialogs; result reports `{path,pages,page_sizes}` (millimetres). |

The `page_setup` read object contains `initialized`, `paper`,
`paper_width_mm`, `paper_height_mm`, `rotation` (0/90/180/270 degrees),
`orientation`, `scale:{numerator,denominator,fit}`, `center`, `plot_style`,
`area`, `view`, `window`, `offset_x`, and `offset_y`. `window` is
`[x0,y0,x1,y1]` for a window plot and `null` otherwise.

`page_setup_set` retains its compatible request shape: `layout`, `paper`
(catalog name), `orientation` (`portrait`/`landscape`), `fit:true` or
`scale:"1:100"`, `center`, `offset_x`/`offset_y` (finite millimetre values),
`area` (`extents`/`display`/`limits`/`layout`/`window`),
`window:[x0,y0,x1,y1]`, and `plot_style`. Explicit fields win; every omitted
field keeps its stored value, so an orientation-only update preserves paper,
scale, offsets, area, and all other settings. Orientation is the effective sheet
orientation: a natively landscape medium uses rotation 0 for landscape and a
quarter turn for portrait. `area:"window"` requires a finite, positive-size
`window`; malformed windows are rejected on every transport. Creation and
page-setup writes include any normal first-viewport initialization in their
undo step, so activation never creates entities. The stored setup is what `plot`
consumes when its own fields are absent.

Names resolve case-insensitively and every response returns canonical spelling.
Create names and `new_name` are trimmed and limited to 255 characters; blank
values return `name_required`, while control characters, reserved punctuation,
or excessive length return `invalid_layout_name`. Unknown names return
`layout_missing` and duplicates return `layout_exists`. Model can be listed and
activated, but rename/delete return `layout_protected`. Both reads are passive.

Use structured operations instead of `-LAYOUT` command scripting. In
particular, create and activate are separate so callers choose whether to
change the current view:

```json
{"op":"layouts"}
{"protocol":1,"op":"layout_create","request_id":"lc-1","document_id":1,"name":"PLAN"}
{"protocol":1,"op":"layout_activate","request_id":"la-1","document_id":1,"name":"plan"}
{"protocol":1,"op":"page_setup_set","request_id":"ps-1","document_id":1,"layout":"PLAN",
 "paper":"ISO_A4_(210.00_x_297.00_MM)","orientation":"landscape","scale":"1:50",
 "offset_x":12.5,"offset_y":-4,"area":"window","window":[0,0,210,297]}
{"op":"page_setup_get","layout":"PLAN"}
{"protocol":1,"op":"layout_rename","request_id":"lr-1","document_id":1,"name":"PLAN","new_name":"Floor Plan"}
{"protocol":1,"op":"layout_delete","request_id":"ld-1","document_id":1,"name":"floor plan"}
```

Over MCP, `ocs_read` supports `layouts` and `page_setup_get` (the latter takes
`parameters.layout`), and `ocs_execute` supports `layout_create`,
`layout_rename`, `layout_activate`, `layout_delete`, and `page_setup_set`.
`capabilities.operations` and the MCP schemas advertise these operations.

REST exposes `GET /api/v1/layouts`, `POST /api/v1/layouts`,
`PATCH /api/v1/layouts/{name}`, `DELETE /api/v1/layouts/{name}`,
`POST /api/v1/layouts/{name}/activate`, `GET /api/v1/layouts/{name}/page-setup`,
and `PUT /api/v1/layouts/{name}/page-setup`. Percent-encode each `{name}` path
segment: `Floor Plan` becomes `Floor%20Plan`. GET routes return the read object
directly; mutations use the normal REST `{ok,status,result,…}` envelope.

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
| `constraintstatus` | constraint state of the active space: `{scope, constraint_count, enabled_count, dof}` — `scope` is the name of the block record in force (`*Model_Space` outside an editor), the two counts are constraint totals (all vs still solved), and `dof` is the solver's remaining degrees of freedom, `null` until it has solved that space | read-only |

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

### 6.9a DGN import — `dgn_import`

`{path,model?,units?,mapping?,codepage?}` imports synchronously through the
same conversion/placement path as `-DGNIMPORT`, with no prompt replay. Model
lookup is case-insensitive and defaults to the first design model; units are
`Master`/`Sub`, mapping is `Standard` with optional `;codepage=N`, and numeric
`codepage` overrides suffix/stored settings (0/1252 select the default).
`result.entities` counts top-level converted model entities, excluding block
members and subsequent reference attachments; `model` is canonical and `path`
echoes the resolved source spelling.
Errors retain stable codes including `model_missing` (plus `available_models`),
`invalid_units`, `invalid_mapping`, `invalid_codepage`, and `empty_dgn_model`.
Target, text-node, naming and reference behavior inherit the command's saved
import settings. The command/dialog fallback remains available. REST uses
`POST /api/v1/dgn_import`; MCP uses `ocs_execute`.

### 6.9b External references — `xrefs` and `xref_*`

The passive `xrefs` read returns a fresh `{count,xrefs[]}` report from the same
reference model as the Reference Manager. Each row contains `id`, `name`,
`kind` (`dwg_xref`, `image`, `underlay`, or `point_cloud`), `format`,
`reference_type` (`attach` or `overlay` where applicable), `status`, the raw
stored `saved_path`, `resolved_path`, `parent_id`, `nested`, `size_bytes`, and
`modified_unix_ms`. Direct rows have `parent_id:null` and `nested:false`;
nested rows identify their direct host in `parent_id`. Embedded images are not
external references and are omitted.

Every mutation addresses one direct row by hexadecimal `id` and runs in the
active document with one undo step when something changes:

| Op | Request (beyond envelope) | Result and semantics |
|---|---|---|
| `xref_reload` | `id` | Reloads a direct DWG/DXF xref, underlay, or point cloud. A failed reload preserves prior content and returns diagnostics; image reload is unsupported. |
| `xref_unload` | `id` | Unloads without deleting the definition. Result includes `session_only` and `unchanged`; repeating an unloaded row creates no history. |
| `xref_detach` | `id` | Detaches the definition, its insertion, and owned dependent records. Result includes `detached`, `id`, and `name`. |
| `xref_bind` | `id` | Converts a direct DWG/DXF xref into a local block; result includes `block` and `unremapped`. Other kinds are refused. |
| `xref_set_path` | `id` plus exactly one of `path` or `path_type` (`full`, `relative`, `none`) | Updates the stored path without implicitly reloading. Result includes `old_path`, `saved_path`, `reload_required`, `unchanged`, and the fresh `reference`. |
| `xref_set_type` | `id`, `reference_type` (`attach` or `overlay`) | Switches a direct DWG/DXF xref between attachment and overlay; result includes `reference` and `unchanged`. |

Unknown IDs return `xref_missing`; nested rows return
`xref_nested_read_only`; unsupported kind/action combinations return
`xref_kind_unsupported`. Native protocol and REST requests reject unknown
operation fields with `unknown_xref_field`; MCP preflight rejects them as tool
validation errors before dispatch. Source and conversion failures use stable
`xref_source_missing`, `xref_load_failed`, `xref_path_across_drives`,
and `xref_path_unsaved_host` codes. No-op and rejected requests create no
history or dirty state.

Structured attach is intentionally not exposed yet because the existing
XATTACH commit cannot roll back atomically if final INSERT creation fails.
Use the existing `XATTACH` command to create a reference, then these operations
to manage it. REST exposes `GET /api/v1/xrefs` and the six generic
`POST /api/v1/{op}` routes; MCP uses `ocs_read` for `xrefs` and `ocs_execute`
for the mutations.

### 6.10 Commands, batches, events

| Op | Purpose |
|---|---|
| `run` | Execute one command line headlessly (`"cmd":"LINE 0,0 10,10"` — command name + space-separated prompt answers; points `x,y[,z]`, options by displayed token). Response `status` is `completed` or `waiting_input`. Requires `document_id`. |
| `start` / `input` | Interactive dialog: `start` opens a command, `state.command.accepts` lists the valid input kinds (`text`, `token`, `point`, `entity`, `structure`, `selection`, `enter`), `input` feeds one. |
| `cancel` | Aborts the running command — or retracts this client's own pending pick (§6.12). A command another client holds is refused with `command_busy` unless the request carries `"force":true`; `state.command_owner_client_id` names the holder (null when the user holds it). |
| `batch` | `steps[]` — sequential ops (≤ 64) where each step sees the previous step's state; stops at the first failure and reports `completed_steps` / `next_step`. |
| `select` | Change the current selection (`handles`, `clear`, filters). |
| `events` | Cursor-paged event stream (command lifecycle, document and selection changes, 128-event ring, `resync` flag). Polling by design. |
| `history` | Command-line history (also the person's view of what clients did — see §6.12). |
| `capture` | PNG snapshot of the viewport or full window (`scope`, `max_dimension`). |
| `view_focus` | Zoom-to-fit + highlight the listed handles (GUI sessions; headless answers `gui_required`). |
| `action` | UI toggles (grid, ortho, …) by name, and the open dialogs' fields: one `name` per call with `value="key=value"` (see the per-dialog notes). |
| `action` + `dgn_import_dialog` | Drives the DGN Import window (it must be open — `DGNIMPORT` with the dialog, or the Import File window picking a DGN): `path=<file>`, `model=<index>`, `target=new\|current`, `conflict=prefix\|ignore`, `units=master\|sub`, `explode=on\|off`, `code_page=<n>` (the picker's page: `1252` or `default`, `1250`, `1251`, `1253`-`1258`, `874`, `932`, `936`, `949`, `950`, `65001`), `references=translate\|ignore\|underlay`, `existing=prompt\|overwrite\|keep`, `mapping=layer\|linetype\|lineweight\|color`, then `ok` (imports; returns `status`) or `cancel`. |

### 6.11 Complete REST endpoint table

| Method & path | Purpose |
|---|---|
| `GET /ready` · `/state` · `/capabilities` · `/openapi` | Readiness · full state · feature advertisement · OpenAPI 3 document |
| `GET /documents` · `POST /documents` · `DELETE /documents/{id}?discard=` | List · open/template/new · close |
| `GET /sysvars?names=` · `POST /sysvars` | Read · set |
| `GET /layouts` · `POST /layouts` | List structured layout entries · create without activating |
| `PATCH /layouts/{name}` · `DELETE /layouts/{name}` · `POST /layouts/{name}/activate` | Rename · delete · activate (percent-encode `{name}`) |
| `GET /layouts/{name}/page-setup` · `PUT /layouts/{name}/page-setup` | Read · write plot configuration (percent-encode `{name}`) |
| `GET /xrefs` · `POST /xref_{reload,unload,detach,bind,set_path,set_type}` | Fresh structured reference report · direct-reference lifecycle writes |
| `GET /entities` · `GET /entities/{h}` · `POST /entities` · `DELETE /entities?handles=` | Query (incl. `where`) · one entity · create · erase |
| `POST /entities/transform` · `POST /entities/copy-to` | Modify · cross-document copy |
| `GET/PUT/DELETE /entities/{h}/xdata[/{app}]` | Extended data |
| `POST /blocks` · `DELETE /blocks/{name}` | Define (+`replace`) · delete |
| `POST /groups` · `POST /selection-sets` · `GET /selection-sets/{name}` | Groups · selection sets |
| `POST /getpoint` · `POST /user_select` | Person-in-the-loop picks (§6.12; the connection parks) |
| `POST /plot` · `/wblock` · `/images` · `/commands` · `/undo` · `/redo` · `/save` | Publishing, export, command line, lifecycle |
| `GET /layers` · `/header` · `/records` | Database reads |
| `POST /file-identity` | Stable drawing GUID |
| `POST /{op}` | Passthrough for any envelope op, including `cancel` and `history` |

### 6.12 Human-in-the-loop picks — `user_select` & `getpoint`

These two operations hand the screen to the person at the desk. They are
answerable only where a person exists: on the GUI-hosted channel (§3.2) or
MCP sessions attached to a desktop build, a headless session answers
`gui_required`. Both stay pending — pollers see `running` — until that person
answers, and both can be retracted programmatically with
`{"op":"cancel","request_id":"<a fresh id>"}`.

**`user_select`** — interactive selection sampling. The person picks entities with the
normal gestures (click, window, crossing), **Enter** confirms, **Escape**
cancels.

```json
{"protocol":1,"op":"user_select","request_id":"sample-1","document_id":1,
 "type":"INSERT","layer":"TITLE","prompt":"Chọn các block cần lấy props",
 "detail":"full","clear":true}
```

| Field | Meaning |
|---|---|
| `type`, `layer` | optional filters for what counts as picked; matching is case-insensitive against the display name (`"Block Reference"`) and the DXF name (`"INSERT"`) |
| `prompt` | optional; replaces the default hint on the command line |
| `detail` | `summary` / `geometry` / `full` (default) — `full` returns each entity's complete serialized properties |
| `clear` | default `true`: the request starts a fresh selection; `false` keeps whatever is selected |

Confirm resolves with:

```json
{"ok":true,"status":"completed","result":{
  "cancelled":false,"count":1,"ignored":1,
  "handles":["6F"],
  "entities":[{"handle":"6F","type":"Block Reference","block":"ANCHOR_A",
               "position":[0,0,0],"properties":{…}}]}}
```

`ignored` counts picks the `type`/`layer` filter deselected at confirm time
(the person's gestures are never blocked; only the answer is filtered).
Escape — or a `cancel` retraction, or the document closing — resolves with
`status:"cancelled"` and `{"cancelled":true,"count":0}`. Starting a pick
while a command or dialog is active is refused (`user_input_busy`); a second
concurrent pick is refused (`interactive_pending`).

**`getpoint`** — one snapped world point from the next left-click.

```json
{"protocol":1,"op":"getpoint","request_id":"qr-spot","document_id":1,
 "prompt":"Chọn vị trí đặt QR code"}
→ {"ok":true,"status":"completed","result":{"point":[125.5,64.25,0.0]}}
```

Escape cancels with `{"cancelled":true}`. `detail` does not apply.

**What the person sees while a pick is pending** (so integrators know the
request is visible even before anyone clicks):

- the command line pins a labeled, non-fading request line:
  `[user_select · sample-1] Chọn các block cần lấy props  (Enter confirms, Esc cancels)`;
- the automation pill on the command line turns blue — *"MCP is waiting for
  you to pick — Enter confirms, Esc cancels"*;
- the viewport cursor trades its crosshair arms for a blue pickbox;
- the answer is printed back into the message list with the same identity
  (`user_select sample-1: handed 1 object(s) to the client.`), and the full
  trail of every client operation — including failures — stays reviewable in
  the command-line history.

On the GUI-hosted HTTP channel (§3.2) the whole exchange rides one parked
connection: the request holds until the person answers (up to 30 minutes,
then `response_timeout`), so no polling loop is needed. Callers that prefer
polling may treat an `accepted` reply as the answer and follow the operation
with `{"op":"operation","request_id":…}` reads; a parked pick that loses its
connection (or its caller) is retracted automatically, and the server-side
trail of every state change — start, answer, cancel — lands on stderr as
`[pick] <op> <request_id>: …` lines for cross-referencing against the
client's own log.

---

## 7. Worked examples

### 7.1 Draw, verify, publish (REST)

```sh
# Create (missing layers auto-created; the batch is atomic).
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities -H "Content-Type: application/json" -d '{
  "entities":[
    {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
    {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0}
  ]}'
# → 201 {"result":{"handles":["63","64"],"created":2,"layers_created":["FRAME"]}}

# Filter by property.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" "http://127.0.0.1:8090/api/v1/entities?type=Line&where=%5B%7B%22path%22%3A%22%2Flayer%22%2C%22op%22%3A%22eq%22%2C%22value%22%3A%22FRAME%22%7D%5D"

# Mark, wrap into a reusable block, plot every sheet, save as a template.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X PUT http://127.0.0.1:8090/api/v1/entities/64/xdata/SPEC \
  -H "Content-Type: application/json" -d '[{"code":1000,"value":"PAGE-01"}]'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/blocks \
  -H "Content-Type: application/json" -d '{"name":"MARK","base":[0,0,0],"handles":["63","64"]}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/plot \
  -H "Content-Type: application/json" -d '{"path":"C:/out/pages.pdf","layout":"all","per_page":true}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwt"}'
```

### 7.2 Sample entities with the person at the screen (GUI-hosted)

```sh
OpenCADStudio.exe "C:/drawings/plan.dwg" --http 8090   # the editor stays interactive

# Ask the person to pick block references; the connection parks until they
# press Enter (or Esc). No polling loop needed.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -m 1800 -X POST http://127.0.0.1:8090/api/v1/user_select \
  -H "Content-Type: application/json" \
  -d '{"request_id":"sample-1","type":"INSERT","detail":"full","prompt":"Pick the title blocks"}'
# → {"ok":true,"status":"completed","result":{"cancelled":false,"count":2,…}}

# Read the same entities back, then continue driving the live drawing.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/get_selection -d '{}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities/transform \
  -H "Content-Type: application/json" \
  -d '{"handles":["6F"],"action":"move","vector":[5,0]}'
```

### 7.3 Clone a region into a new, normalized drawing

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

### 7.4 Stable identity + idempotent retry

```json
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ {"ok":true,"result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true}}
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ same response served from the idempotency cache — never executed twice
```

---

## 8. Surface status and upstreaming map

This spec documents the fork's surface, which is a strict superset of
upstream `main`. Everything in the **core** column exists upstream today and
is documented here unchanged; each **piece** row is a self-contained,
independently reviewable addition — the matching section of this spec is
meant to travel with its piece's PR, so no section ever runs ahead of the
code it describes.

| Scope | Contents | Spec sections |
|---|---|---|
| **Core (upstream `main`)** | Transports `--serve` (stdio JSONL), `--serve --port` (TCP), `--mcp`; ops `new`, `open`, `save`, `activate`, `select`, `run`, `start`, `input`, `cancel`, `stop`, `action`, `property`, `set_properties`, `embed_image`, `capture`, `undo`, `redo`; reads `state`, `hello`, `capabilities`, `query`/`entities`, `records`, `record_schema`, `layers`, `header`, `properties`, `measure`, `history`; envelope conventions and the core error codes | §1–§5, §6.1, §6.4, §6.10 |
| Piece A — entity CRUD | `entities_create`, `entities_set`, `entities_delete`, `entities_transform`, `entities_copy_to` | §6.2, §6.3 |
| Piece B — blocks & export | `block_define`, `block_delete`, `wblock` | §6.5 |
| Piece C — publishing | `layouts`, `layout_create`, `layout_rename`, `layout_activate`, `layout_delete`, `page_setup_get`/`page_setup_set`, `plot` (incl. `per_page`) | §6.6 |
| Piece D — metadata | `sysvar`, `xdata_set`/`xdata_get`, `file_identity` | §6.7 |
| Piece E — collections | `group_create`, `selection_set_save`/`load` | §6.8 |
| Piece F — control plane | `batch`, `events`, `view_focus`, `close`, `where` filters | §6.4 (`where`), §6.10 |
| Piece G — REST transport | `--http` headless server, `/api/v1` routes, OpenAPI 3 document | §3.1, §6.11, §7.1 |
| Piece H — person-in-the-loop | GUI-hosted `--http` channel, `user_select`, `getpoint`, the on-screen pending signals | §3.2, §6.12, §7.2 |
| Piece I — DGN import | `dgn_import` named options over the shared `-DGNIMPORT` conversion pipeline | §6.9a |
| Piece J — external references | `xrefs`, `xref_reload`/`unload`/`detach`/`bind`/`set_path`/`set_type` | §6.9b |

Sections §3.2/§6.12 (piece H) additionally require the desktop build; over a
headless session every documented request still answers truthfully
(`gui_required`), so a client written against this spec behaves consistently
on both.

---

## 9. Versioning and compatibility

- The wire protocol version is **1**; it is declared in every envelope and in
  the greeting line. Additions (new ops, new optional fields, new error
  codes) are non-breaking; clients should ignore unknown fields and treat
  unknown op names in `capabilities` as "not available in this build".
- Feature detection: read `capabilities` before unfamiliar work — every op in
  this document is listed there when the running build supports it.
- Drawing files: DWG and DXF readers/writers are version-tolerant; templates
  (`*.dwt`) are byte-compatible DWG files.

*Companion documents: the automation walkthrough and MCP set-up
([`README.md`](README.md)) and the OpenAPI 3 description served at
`GET /api/v1/openapi`.*
