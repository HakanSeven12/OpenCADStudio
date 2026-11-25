# Plugin panel surface API (api v29): scene, widget, table tiers

How a plugin puts content on one of its dock panels and interacts with it.
One publish vocabulary — `PanelSurface` — carries three tiers, stored side by
side per panel, each on its **own independent revision space**:

| Tier | Carrier | Host renders as | Update primitive |
|---|---|---|---|
| **Scene** | `PanelScene` | a 2-D canvas (paint prims, pointer seam, drag preview) | whole-snapshot publish or atomic `ScenePatch` |
| **Widgets** | `WidgetSurface` | native tree/list widgets (expansion/selection are host-local) | whole-surface publish or keyed `WidgetPage` installs |
| **Tables** | `TableSurface` | a native editable grid (edits are host-local drafts) | whole-surface publish or atomic `TablePatch` (cell/row ops) |

A panel can carry all three at once; the dock shows the richest tier first
(**table → widget → scene → legacy view**, `src/app/dock/view.rs`), and the
automation facade reads/writes every tier with the same two ops.

Layers involved:

| Layer | File(s) | Role |
|---|---|---|
| Data model | `crates/ocs_plugin_api/src/{scene,widget,tables}.rs` | pure data: surfaces, patches, events, budgets — serde + std only |
| Host API trait | `crates/ocs_plugin_api/src/host.rs` | `panel_*_publish/patch/page` defaults + `*_for_plugin` authenticated forms |
| Wire | `crates/ocs_plugin_api/src/ipc/protocol.rs` (+ `ipc/client.rs`, `ipc/v4/`) | `PluginRequest`/`PluginResponse` frames; `HostNotification` stream |
| Host store | `src/app/panel_surfaces.rs` | `PanelSurfaceStore`: ownership, revision rules, atomic applies |
| Host renderer | `src/ui/window/plugin_{scene,widgets,tables}.rs`, `src/app/dock/view.rs` | native projections; the dock picks the tier |
| Event router | `src/app/panel_events.rs` | pointer/activation transitions → per-panel sequenced `SurfaceEvent`s |
| Automation facade | `src/app/automation.rs` | `plugin_panels` read op, `plugin_panel_action` execute op (MCP parity) |

The plugin owns the semantic model (what the surfaces mean); the host owns
the projection, the editing sessions, and interaction routing. The host never
interprets scene vocabulary beyond geometry and paint.

---

## 1. Scene tier: the data model

### 1.1 Scene, elements, identity

```rust
pub struct PanelScene {
    pub revision: SceneRevision,     // monotonic per panel (u64)
    pub elements: Vec<SceneElement>, // roots in paint order (first painted first)
}

pub enum SceneElement {
    Group { id: StableId, name: Option<String>, children: Vec<SceneElement> },
    Rect  { id: StableId, name: Option<String>, pos: Vec2, size: Vec2,
            fill: Option<SceneColor>, stroke: Option<Stroke> },
    Path  { id: StableId, name: Option<String>, points: Vec<Vec2>,
            closed: bool, stroke: Option<Stroke> },
    Text  { id: StableId, name: Option<String>, content: String,
            pos: Vec2, size: f32 /* default 13.0 */, color: SceneColor },
}
```

- **`StableId(u64)`** — plugin-assigned, unique within one scene. The host
  uses it to keep element identity stable across revisions and to address
  events, patches, and automation actions.
- **Groups carry no transform** — children stay in scene coordinates. The
  scene space equals the canvas-local space in the host's projection.
- Every variant leads with `id`, then the optional display `name` (shares the
  text byte budget). `name` is an accessory: it never affects geometry.

### 1.2 Paint

```rust
pub enum SceneColor { Theme(ThemeColor), Rgba(Rgba8) }

pub enum ThemeColor {
    Normal, Strong,          // regular / high-emphasis foreground
    Accent,                  // theme accent (primary)
    Muted,                   // de-emphasised secondary
    Success, Warning, Danger // state colors
}

pub struct Stroke { pub color: SceneColor, pub width: f32 } // width > 0, finite
```

Theme tokens state **intent, not colors**; the host resolves them against its
theme at paint time (§5.3). Exact `Rgba` is available when a plugin must match
its own brand. `Rgba8([r, g, b, a])` is one byte per channel.

### 1.3 Scene budgets (wire + validation)

| Budget | Value | Refusal |
|---|---|---|
| Nesting depth | `MAX_SCENE_DEPTH = 64` | `DepthBudgetExceeded` (reports first crossing, cap + 1) |
| Total elements | `MAX_SCENE_NODES = 10_000` | `NodeBudgetExceeded` |
| Text run / name bytes | `MAX_TEXT_BYTES = 4_096` | `TextBudgetExceeded` |
| Patch ops per patch | `MAX_PATCH_OPS = 10_000` | `PatchTooLarge` |

Additional structural rules: all coordinates/sizes/text sizes finite
(`NonFiniteCoordinate`), rect sizes strictly positive (`InvalidRect`), stroke
widths finite and strictly positive, paths carry ≥ 2 points (`TooFewPoints`),
ids unique across the whole tree (`DuplicateId`).

Decoding is bounded too: the recursive element lists deserialize under a
thread-local per-frame budget (`scene_decode_frame_begin`), so a hostile
payload cannot overflow the decoding thread's stack.

### 1.4 Wire stability

- Bincode discriminants are **pinned** by a test: `Group=0, Rect=1, Path=2,
  Text=3`. New variants **append at the end**, never renumber.
- JSON is externally tagged PascalCase (`{"Rect": {...}}`); `name` and Text
  `size` decode with defaults when missing.
- `PanelSurface` (the publish carrier) pins `Scene = 0`; `Widgets` and
  `Tables` appended after.

---

## 2. Scene tier: building and updating

### 2.1 `SceneBuilder`

The builder is fluent, never on the wire, and eager about id uniqueness (a
duplicate refuses **at add time**). Typed adds append in paint order;
`group(id)` / `end_group()` open and close the insertion container; paint
setters (`fill`, `stroke`, `color`, `name`) re-point at the **active** element
(the most recently added one). A setter the active variant does not take is a
documented no-op, never an error. `finish()` validates and drains the builder.

Example — a small node graph (two labeled nodes, one edge, all in a named
group), exactly the shape the API's own test fixture uses:

```rust
use ocs_plugin_api::scene::{SceneBuilder, SceneColor, Stroke, ThemeColor, Vec2};

let outline = |w: f32| Stroke { color: SceneColor::Theme(ThemeColor::Strong), width: w };
let node_a = [80.0, 120.0];
let node_b = [280.0, 200.0];

let mut b = SceneBuilder::new(7);            // revision 7
b.group(1)?.name(Some("fixture graph"));
b.rect(2, node_a, [64.0, 32.0])?             // node a
    .fill(SceneColor::Theme(ThemeColor::Accent))
    .stroke(outline(1.5));
b.text(3, "node a", node_a, 13.0, SceneColor::Theme(ThemeColor::Normal))?;
b.rect(4, node_b, [64.0, 32.0])?.stroke(outline(1.5));
b.text(5, "node b", node_b, 13.0, SceneColor::Theme(ThemeColor::Normal))?;
b.path(6, vec![Vec2(node_a), Vec2(node_b)], false)?.stroke(outline(1.0));
b.end_group();
let scene = b.finish()?;                      // validates, returns PanelScene
```

**Possible contents** — the vocabulary is deliberately small; complex UIs are
compositions:

- **Node graphs / flowcharts** — rects as nodes, polylines as edges, text
  labels, a group per cluster (the fixture above).
- **Status tiles / dashboards** — filled rects with `Theme` state colors
  (`Success`/`Warning`/`Danger`), text captions, one group per row.
- **Mini-maps / legends** — a rect per drawing frame, polylines for
  polylines, `Muted` strokes for de-emphasis.
- **Gauges and bars** — background rect + fill rect sized proportionally;
  republish or patch the fill rect on updates.
- **Interactive pickers** — rects with `name`s as hit zones; the plugin binds
  semantics to `StableId`s on `ElementClicked`.
- **Diagrams layered over host data** — since groups carry no transform,
  overlays align in scene coordinates 1:1 with the canvas.

The automation facade reports per-type counts (`by_type`: `group`, `rect`,
`path`, `text`) for the scene tier (§7.2).

### 2.2 Publishing and patching (plugin side)

```rust
// Publish or replace the panel's whole scene:
HostApi::panel_surface_publish_for_plugin(plugin_id, panel_id, scene)
    -> Result<SceneRevision, SurfaceReject>

// Atomic revision-checked update:
HostApi::panel_surface_patch_for_plugin(plugin_id, panel_id, patch)
    -> Result<SceneRevision, SurfaceReject>
```

(V4 transports: the same methods are overridden on `V4PluginHostApi`, so a
plugin can publish **during `on_load`** and the surfaces attach to the wire
instead of trait-defaulting to `Unattached`.)

Refusals are typed and change nothing — the same four codes guard every tier:

| Reject code | Meaning |
|---|---|
| `Unattached` | No surface-supporting attach / panel not registered by this plugin / blank ids |
| `OwnedByOther` | Another verified plugin owns the panel's surface |
| `InvalidSurface` | Failed structural validation |
| `NonMonotonicRevision` | Revision did not strictly advance (carries the stored head) |

Revision rules (per tier, on each tier's independent revision space):

- A publish must **strictly advance** the stored head. An **identical**
  republish at the head is idempotent (`Ok(head)`); same revision with
  different content refuses.
- A restarted plugin starts fresh (its dead registration was reaped), so
  revision 1 again is accepted after the reap/re-register.

### 2.3 Atomic scene patches: `ScenePatch`

```rust
pub enum SceneOp {
    Add     { parent: Option<StableId>, element: SceneElement }, // None = root append
    Replace { id: StableId, element: SceneElement },            // same StableId required
    Remove  { id: StableId },                                    // removes the subtree
}

pub struct ScenePatch { pub base_revision: SceneRevision, pub ops: Vec<SceneOp> }
```

Example — move a node, recolor it, and drop its old edge in one atomic step:

```rust
let patch = ScenePatch {
    base_revision: SceneRevision(7),          // must equal the stored head
    ops: vec![
        SceneOp::Replace {
            id: StableId(2),
            element: SceneElement::Rect {
                id: StableId(2), name: None,
                pos: Vec2([100.0, 130.0]), size: Vec2([64.0, 32.0]),
                fill: Some(SceneColor::Theme(ThemeColor::Success)), stroke: None,
            },
        },
        SceneOp::Remove { id: StableId(6) },
    ],
};
let new_revision = api.panel_surface_patch_for_plugin(plugin_id, panel_id, patch)?;
// success: revision is exactly 7 + 1 = 8
```

Guarantees (enforced in `ScenePatch::apply_to` and the store's `patch`):

- **All-or-nothing** — the candidate is staged and swapped only when every op
  succeeds; a failure leaves the stored scene byte-for-byte unchanged.
- On success the revision advances by **exactly one** and the whole result
  re-validates (budgets, id uniqueness, finite floats).
- A stale `base_revision` refuses with `NonMonotonicRevision` **carrying the
  current head**, so the publisher can rebase. `Replace` naming an id with a
  different `StableId` refuses; `Add` under a non-group parent refuses
  (`UnknownParent`); ops naming missing ids refuse (`UnknownElement`).
- Only a previously published surface can be patched (a patch never creates
  one: `Unattached`).

---

## 3. Widget tier: semantic tree/list, native rendering

The widget tier is a **keyed row map with display order** — the plugin states
*what* the rows are, the host renders them with its own native widgets.
Expansion and selection are host-local state (not notified), except the one
case where the plugin is the data source: expanding an `Unloaded` row.

```rust
pub struct WidgetSurface {
    pub revision: WidgetRevision,
    pub rows:  BTreeMap<StableId, WidgetRow>, // every row, keyed by StableId
    pub roots: Vec<StableId>,                 // display order of the top rows
    // ... plus the loaded child order per row (in `WidgetChildren::Loaded`)
}

pub struct WidgetRow {
    pub label: String,                 // bounded by MAX_TEXT_BYTES
    pub children: WidgetChildren,      // Leaf | Unloaded | Loaded(Vec<StableId>)
    pub action: Option<String>,        // set => the row is a BUTTON (leaves only)
}

pub enum WidgetChildren {
    Leaf,                    // cannot expand
    Unloaded,                // children exist but must be paged in on expansion
    Loaded(Vec<StableId>),   // ordered child ids present in this surface
}
```

Budgets: `MAX_WIDGET_NODES = 10_000` rows per surface; `MAX_PAGE_ROWS =
1_000` rows per single page install (a page is a bounded window, not a whole
subtree — install more pages for more rows). Refusal vocabulary
(`WidgetError`): `StaleBase`, `PageTooLarge`, `SurfaceTooLarge`,
`UnknownParent`, `ParentCannotExpand` (a `Leaf` cannot take children),
`UnknownAfter`, `UnknownElement`, `DuplicateRow`, `LabelTooLong`,
`ActionTooLong`, `ActionWithChildren` (buttons are leaves), `OrphanRows`
(every row must stay reachable from the roots).

### 3.1 Publish and lazy paging

```rust
// Publish or replace the whole surface (same revision rules as scenes):
HostApi::panel_widget_publish_for_plugin(plugin_id, panel_id, surface)
    -> Result<WidgetRevision, WidgetReject>

// Install a keyed page atomically (lazy expansion / paging primitive):
HostApi::panel_surface_page_for_plugin(plugin_id, panel_id, page)
    -> Result<WidgetRevision, WidgetReject>

pub struct WidgetPage {
    pub base_revision: WidgetRevision,   // must equal the current revision
    pub parent: Option<StableId>,        // None = install as roots
    pub after: Option<StableId>,         // position after this existing sibling
    pub rows: Vec<(StableId, WidgetRow)>,// ids must be NEW (DuplicateRow otherwise)
    pub next: Option<StableId>,          // cursor for the NEXT page's `after`
}
```

The lazy-expansion loop: publish the tree with an `Unloaded` child state →
the user expands the row → the host sends `SurfaceEvent::NodeExpanded { node
}` → the plugin pushes a `WidgetPage` under that node → the host renders the
children natively (revision advances by the page install). Only a previously
published surface can be paged (`Unattached` otherwise), and pages apply
atomically exactly like scene patches.

### 3.2 Events and buttons

- A row with `action` set is a **button**: activating it (enter/double-click
  — the host's activation gesture) sends `SurfaceEvent::NodeActivated { node
  }` to the plugin, which maps the row to its behavior. Buttons cannot also
  declare children.
- Expansion/selection changes of `Loaded` rows stay host-local and are NOT
  notified (the plugin's data is unaffected); `NodeExpanded` on `Unloaded`
  rows is the one expansion notification.

Worked example (the firetest plugin's tree, read back over MCP — see §7.2):
root `1 "Train menu"` with children `[2, 3]`; row `2` is a button with action
`tsp_ping`; row `4 "Details"` declares `Unloaded`, and after the plugin pages
in rows `200–202` the surface reads back with `children: [200, 201, 202]`.

---

## 4. Table tier: typed grid, host-local editing

The table tier is a typed scalar grid — stable row keys, ordered typed
columns, atomic patches. The host renders it as its native editable grid;
**edits are host-local drafts until the plugin canonicalizes them** by
pushing its own patch (the plugin remains the data authority).

```rust
pub struct TableSurface {
    pub revision: TableRevision,
    pub columns: Vec<TableColumn>,              // ordered; cell index derived
    pub rows:  BTreeMap<StableId, Vec<CellValue>>, // keyed by StableId
    pub order: Vec<StableId>,                   // display order
}

pub struct TableColumn { pub id: StableId, pub label: String }

pub enum CellValue { Text(String), Number(f64), Bool(bool) }  // finite; bounded text
```

Budgets: `MAX_TABLE_ROWS = 10_000` rows, `MAX_TABLE_OPS = 10_000` ops per
patch. Refusal vocabulary (`TableError`): `StaleBase`, `PatchTooLarge`,
`TableTooLarge`, `UnknownRow`, `UnknownColumn`, `DuplicateRow`,
`ArityMismatch` (cells must match the column count), `NonFiniteNumber`,
`TextTooLong`.

### 4.1 Publish and atomic table patches

```rust
HostApi::panel_table_publish_for_plugin(plugin_id, panel_id, surface)
    -> Result<TableRevision, TableReject>
HostApi::panel_surface_table_patch_for_plugin(plugin_id, panel_id, patch)
    -> Result<TableRevision, TableReject>

pub enum TableOp {
    SetCell    { row: StableId, column: StableId, value: CellValue },
    InsertRow  { after: Option<StableId>, row: StableId, cells: Vec<CellValue> },
    RemoveRow  { row: StableId },
}

pub struct TablePatch { pub base_revision: TableRevision, pub ops: Vec<TableOp> }
```

Patches apply atomically against the surface whose revision equals
`base_revision`: any refusal leaves the table byte-for-byte unchanged;
success advances the revision by exactly one and re-validates the whole
table (budgets, arity, order reachability). `SetCell` is the single-cell edit
path — updating one cell **never republishes the table**.

### 4.2 The edit flow: `CellEdited` → canonicalization

1. The user edits a cell; the host's editing session commits the draft under
   the typed-commit rule (the grid shows the draft immediately).
2. The host sends `SurfaceEvent::CellEdited { row, column, value }` (best
   effort, like every surface event).
3. The plugin validates the value against **its** model and answers with its
   own `TablePatch` (e.g. `SetCell` with the canonical value — possibly a
   corrected one). Until that patch lands, the draft is host-local; the
   plugin's patch is the authority.

Worked example (the firetest plugin's table, read back over MCP): columns
`Item`/`Value`; typed rows `alpha → Text("alpha")/Number(1.5)` and `beta →
…/Bool(true)` rendered in the native grid.

---

## 5. Surfaces in the host: store, router, renderer

### 5.1 Store (`PanelSurfaceStore`)

One authoritative record per tier per panel (`scenes`, `widgets`, `tables`
maps keyed by `panel_id`), each carrying `owner_plugin_id`. The store
re-checks ownership on **every** operation (defense in depth — the caller has
already authenticated the transport). Reads are owner-checked for plugins
(`scene()`/`widget()`/`table()`); the automation facade reads
owner-agnostically (`*_any_owner()`) because the operator's app owns the whole
store — plugin-to-plugin isolation is untouched. A dead owner's records are
reaped (`retain_owners`); a panel re-registration starts without surfaces.

### 5.2 Event routing (`PanelEventRouter`)

Raw canvas pointer transitions (`Press`/`Move`/`Release`/`Cancel`) become
semantic `SurfaceEvent`s with a **per-panel monotonic sequence**:

- **Click** (press + release without movement) → `ElementClicked { element,
  pos }` with absolute scene-space position.
- **Drag** — press on an element arms a drag; the first move emits
  `DragStarted { element, origin }` (+ the coalesced move); further moves
  **coalesce** (absolute positions lose nothing); release emits
  `DragSettled { element, pos }` — exactly once per drag, never dropped.
  `Escape`/focus loss/teardown → `DragCancelled { element, origin }` carrying
  the **pre-drag** origin for lossless restore. A second press settles the
  in-flight drag first (the host has one pointer). Moves never re-target the
  drag to another element.
- Widgets: `NodeActivated { node }` (activation gesture or
  `plugin_panel_action`), `NodeExpanded { node }` (page-in request).
- Tables: `CellEdited { row, column, value }` (committed draft).

Events are **best-effort notifications**: never acked by re-publishing, and
identical-looking activations are distinct events (the sequence is the event
identity). The full vocabulary lives in `crate::scene::SurfaceEvent` and
reaches the plugin as `HostNotification::PanelSurfaceEvent { panel_id,
sequence, event }` (§6).

### 5.3 Scene rendering pipeline (`plugin_scene.rs`)

1. **Projection** — `project_scene` flattens the validated tree depth-first
   into paint-order prims (`Rect`, `Polyline`, `Label`), semantic colors
   unresolved. Groups flatten (never paint).
2. **Retained cache** — one bounded entry keyed by `(owner, panel, revision)`.
   Repaints of an unchanged scene are free; a patch costs exactly one
   re-projection; a theme change invalidates nothing (theme applies at paint).
   Lifecycle callers (`forget_all_projections`) clear it on re-registration
   and dead-plugin reap so a dead revision never serves a reused revision.
3. **Paint** — `SceneCanvas` (an iced canvas `Program`) paints the prims,
   resolving `SceneColor` against the live theme, and shifts the dragged
   element's prims by the preview delta (host-local **drag preview**: the
   stored scene and cache stay untouched until the plugin settles with its
   own patch).
4. **Pointer seam** — the canvas translates raw events into
   `Message::PanelSurfacePointer` with the hit-tested element and scene-space
   position; the router owns semantics.

Theme mapping is deterministic and pinned by tests:

| Token | Theme slot |
|---|---|
| `Normal`, `Strong` | background base text |
| `Accent` | primary base color |
| `Muted` | readable muted text |
| `Success` / `Warning` / `Danger` | palette state colors |
| `Rgba` | exact color |

Hit-testing is coarse by design — the **topmost (last-painted) prim** whose
bounding box contains the point wins; polylines hit their bounding box (not
per-segment), labels use a `0.6 × size` per-character width heuristic and
`1.4 × size` height. The plugin owns any finer semantics. The scene canvas
fills the panel body (`Length::Fill` both axes).

### 5.4 Tier precedence in the dock

For a `Plugin(n)` dock panel, the body resolves per frame through the
owner-checked store reads: the table view wins when the panel owns a table
surface, else the widget view, else the scene canvas, else the legacy
dialog-style view (`src/app/dock/view.rs`). Per-tier UI state (editing
sessions, expansion state) lives host-side next to the surfaces.

---

## 6. Information flow: host ↔ plugin

### 6.1 The two transports, one API

Plugins talk to the host through the `HostApi` trait. **In-process** plugins
get the host's direct implementation; **out-of-process (v4)** plugins get
`V4PluginHostApi` (in `ipc/v4/`), which marshals the same calls onto the wire.
The `*_for_plugin(plugin_id, ...)` forms carry the **transport's
authenticated identity**, and the store re-checks ownership anyway — a plugin
can only ever address panels it registered and its own surfaces.

```
 PLUGIN (v4, out-of-process)                     HOST
 ─────────────────────────────                   ─────────────────────────────
 on_load / commands                              PluginHost (process manager)
   │ PanelSurfacePublish {panel_id, surface}          │ authenticate transport id
   │ PanelSurfacePatch  {panel_id, ScenePatch}        ▼
   │ PanelSurfacePage   {panel_id, WidgetPage}   PanelSurfaceStore
   │ PanelSurfaceTablePatch {panel_id, patch}      publish/patch/page → Result<Revision, Reject>
   └────────── request ───────────────────────►     │ stored surfaces (validated)
                                                    ▼
   ◄────────── response ──────────────────────    typed result
     PanelSurfacePublished/PatchResult/             (refusals change nothing)
     PanelWidgetPublished/PageResult/
     PanelTablePatchResult(Ok(rev) | Err(reject))

   ◄────────── notification ──────────────────    router + store
     HostNotification::PanelSurfaceEvent          pointer/activation/cell-edit
       {panel_id, sequence, event}  (wire tag 16) transitions → SurfaceEvent
```

### 6.2 Wire requests and notifications

- **Plugin → host** (`PluginRequest`, api v29 additive): `PanelSurfacePublish
  {panel_id, surface: PanelSurface}` — **one request carries any tier** via
  the `PanelSurface` enum (`Scene = 0`, `Widgets`, `Tables` appended);
  `PanelSurfacePatch` (scene ops), `PanelSurfacePage` (widget page),
  `PanelSurfaceTablePatch` (table ops). Each answers with a typed
  `PluginResponse`: `Ok(revision)` or the tier's reject carrying the code and
  the stored head where relevant. Discriminants are pinned by tests; new
  requests append.
- **Host → plugin** (`HostNotification`): `PanelSurfaceEvent { panel_id,
  sequence, event }` is **wire tag 16**, encoded by hand in the notification
  stream. It is routed only to plugins whose manifest declares v29 (a v28
  runner cannot decode tag 16 — the `notification_api_version` gate);
  unknown tags decode to `Unknown(Vec<u8>)` so older peers ignore future
  events without failing.
- **Delivery contract**: events are best-effort; the plugin never
  acknowledges them by re-publishing values. Sequence numbers are per panel
  and strictly monotonic — they are the event identity.

### 6.3 Host-internal flow (automation requests)

Automation ops enter through the control envelope
(`src/app/control/mod.rs`): read ops are owner-agnostic store reads;
mutations validate `request_id` (mandatory, unique per session — idempotency
keys live until restart) and the document envelope. `plugin_panel_action` is
document-exempt (an interactive op on the active tab); command-run ops need a
drawing (`no_document` otherwise). Accepted actions become the SAME host
messages the GUI gestures produce, so the panel state, the store, and the
plugin's notifications cannot tell the paths apart.

### 6.4 Render pull, not push

The dock view is a pure function of the store: each frame it resolves the
panel's owner through owner-checked reads (`panel_surfaces.table(owner,
panel_id)` → widget → scene) and projects the stored surface. There is no
render-side cache invalidation protocol — the revision in the store key is
the invalidation.

---

## 7. MCP automation (operator/agent parity)

The host exposes the store through the native MCP stdio bridge
(`OpenCADStudio.exe --mcp`); tools: `ocs_sessions`, `ocs_read`, `ocs_execute`.
`request_id` is mandatory and must be **unique for the whole session**. The
surface tiers are automation-first: every agent gesture produces the **same**
event the GUI gesture produces.

### 7.1 Discover sessions and open a drawing

```json
// ocs_sessions -> [{session_id, pid, bridge: {build_rev, tool_schema, ...}, ...}]
{"jsonrpc": "2.0", "id": 1, "method": "tools/call",
 "params": {"name": "ocs_execute", "arguments": {"ocs_session_id": "<sid>",
   "request": {"request_id": "ft-1", "op": "new"}}}}
```

A drawing must exist before command-run ops (`no_document` refusal otherwise);
`plugin_panel_action` itself is document-exempt (interactive op on the active
tab).

### 7.2 `ocs_read` op `plugin_panels` — bounded surface reads

**Overview** (no `panel_id`): one summary line per surface — owner plus
per-tier `(revision, count)` presence. Nothing materializes rows.

```json
{"ok": true,
 "panels": [{
   "owner": "schoeller.train_surfaces", "panel_id": "tsp_panel",
   "tiers": {
     "scene":   {"elements": 3, "revision": 1},
     "widgets": {"revision": 2, "rows": 7},
     "tables":  {"columns": 2, "revision": 1, "rows": 2}
   }}],
 "next": "Pass panel_id for one surface's bounded detail; plugin_panel_action activates an element."}
```

**Scene tier detail** — `{"op":"plugin_panels","panel_id":"tsp_panel",
"tier":"scene"}`: counts only, never an element dump (bounded reads):

```json
{"ok": true, "panel_id": "tsp_panel", "tier": "scene",
 "revision": 1, "elements": 3,
 "by_type": {"group": 0, "rect": 3, "path": 0, "text": 0},
 "note": "Scene surfaces expose counts; activate an element with plugin_panel_action."}
```

**Widget tier detail** — `tier:"widgets"` pages rows in StableId order:
`rows` ≤ 200 (default 50), `after` = StableId cursor, `next: null` when
exhausted. Each row reads back as
`{"id", "label", "children": "leaf"|"unloaded"|[ids], "action"}` — the
10 000-row surface pages in 200-row windows cursor-continuably (this exact
walk is a facade test).

**Table tier detail** — `tier:"tables"`: `revision`, `columns`, and the typed
rows (bounded pages like widgets).

**Events** — `ocs_read` `{"op":"events","limit":N}` returns the command
notification log; surface events reach the plugin as `PanelSurfaceEvent`
notifications and are reflected on the panel itself (see the summary label
below), which is the operator-visible witness.

### 7.3 `ocs_execute` op `plugin_panel_action` — activate an element

```json
{"request_id": "ft<unique>",
 "op": "plugin_panel_action",
 "panel_id": "tsp_panel",
 "element": 2,
 "kind": "element_clicked"}
```

- `kind`: `"element_clicked"` (the element must exist in the panel's **scene**
  surface) or `"node_activated"` (must exist in the **widget** surface).
- The element is validated against the addressed surface **before** anything
  fires: a typo activates nothing and **no sequence number burns**. Unknown
  `kind` is refused (`node_activated | element_clicked`).
- Success answer carries the event's sequence; the panel's summary label
  updates exactly as it would for the GUI gesture — the parity proof:

```
"tsp_label_summary" -> "NodeActivated node StableId(3) seq N (events seen: N)"
```

Worked climb (scene click, then two widget activations; sequences strictly
monotonic regardless of path — each request carries a fresh `request_id`):

```json
{"request_id": "ft-2", "op": "plugin_panel_action",
 "panel_id": "tsp_panel", "element": 2, "kind": "element_clicked"}
{"request_id": "ft-3", "op": "plugin_panel_action",
 "panel_id": "tsp_panel", "element": 1, "kind": "node_activated"}
{"request_id": "ft-4", "op": "plugin_panel_action",
 "panel_id": "tsp_panel", "element": 3, "kind": "node_activated"}
```

> Routing note: `plugin_panel_action` is an S08 automation op executed
> through `ocs_execute`'s request envelope. It is distinct from the v1 panel
> control ops (`panel_button` / `panel_input`), which address
> dialog-style `controls` and refuse unknown panels with `panel_not_found`.
> If a helper maps surface ops onto those v1 ops, the refusal is
> `panel_not_found` even though `plugin_panels` reads succeed.

Table cells have no execute op by design — table interaction automates as
read + the plugin's own commands, with the plugin's patch remaining the data
authority (§4.2).

### 7.4 Automation contract at a glance

| Op | Shape |
|---|---|
| `plugin_panels` (overview read) | per-panel owner + `{revision,count}` per tier |
| `plugin_panels` + `tier:"scene"` | `revision`, `elements`, `by_type` counts |
| `plugin_panels` + `tier:"widgets"` | paged rows `{id,label,children,action}`, `after` cursor |
| `plugin_panels` + `tier:"tables"` | `revision`, `columns`, typed rows, paged |
| `plugin_panel_action` (execute) | `{panel_id, element, kind}` → sequence; panel state reflects the event |
| `events` (read) | command notification log (filter for surface events; see §8.2) |

---

## 8. Automated panel testing

### 8.1 Where the proofs live (in-repo suites)

| Suite | What it pins |
|---|---|
| `crates/ocs_plugin_api/src/scene.rs` tests | wire roundtrips (bincode + JSON), discriminant pinning, duplicate/depth/node/text budget refusals (first-crossing semantics), builder id ledger, atomic patch semantics |
| `crates/ocs_plugin_api/src/widget.rs` tests | page installs (stale base, duplicate ids, orphan rows, button-with-children), page budgets, reachability |
| `crates/ocs_plugin_api/src/tables.rs` tests | patch atomicity, arity/finite/text refusals, row order integrity |
| `src/app/panel_surfaces.rs` tests | publish revision rules (idempotent resurface, stale advance, invalid-surface no-mutation), ownership walls, failed-patch atomicity, stale-base rebasing |
| `src/ui/window/plugin_scene.rs` tests | paint order + semantic colors, deterministic theme mapping, cache re-projection **counts** (repaints free; a patch costs exactly one) |
| `src/app/panel_events.rs` tests | click vs drag classification, coalescing, settle/cancel authority, per-panel sequence monotonicity |
| `src/app/automation.rs` tests | facade equivalence (MCP action ≡ GUI gesture), refusals, the 10 000-row 200-page cursor walk, tool-schema digest pin |

CI gate discipline: the **full unfiltered** host lib suite is the final gate
(scoped filters are not — suite-wide pins broke once outside every scoped
filter), plus all-features builds and a wasm32 check that catches UI/renderer
modules reaching into the desktop-gated v4 support layer.

### 8.2 In-process seams (host tests without a real plugin process)

- **`install_in_process_sink(plugin_id, |host, event| ...)`**
  (`crate::plugin::grip_points`) attaches a notification sink for one plugin
  id, so surface/event notifications land in the test without a runner
  process. Liveness rule: a surface's notifications keep flowing while the
  sink is installed **or** the manager is alive — tests that publish then
  assert must install the sink first, or the reaper drops the surfaces
  between publish and read (the four-panel `quad_panels` idiom exists for
  exactly this).
- **`Message::DrainPluginRequests`** is the automation pump: automation ops
  drive it so queued plugin requests/notifications flush deterministically
  inside the test's update loop (no async waiting).
- **`record_test_dispatch`** (`src/plugin/v4_support.rs`) records every
  `HostNotification` into a dispatch log tests can assert against. Two
  lessons are baked into the idiom: **drain before each gesture**, and
  **filter the log for `PanelSurfaceEvent` entries** — post-operation
  document broadcasts fire even on refused ops, so unfiltered logs make
  refusals look like events.

### 8.3 Live roundtrip over the bridge (the firetest loop)

The automation surface is itself the test driver for a built host + plugin:

1. Boot the host plainly (`--new-instance`); it writes the session descriptor
   the bridge discovers (`ocs_sessions` → check `bridge.build_rev` matches
   the built commit, and the tool-schema digest).
2. `new` (a drawing), then drive the plugin's publish command (here:
   `TSP_SHOW` / `TSP_TABLE`).
3. Assert the store through bounded reads: overview tier counts, scene
   `by_type`, widget paging (`after` cursor), table rows — all without
   materializing beyond the page.
4. Fire the parity climb: `plugin_panel_action` `element_clicked` /
   `node_activated` in sequence; assert the panel's reflected summary
   (`seq N (events seen: N)`) and monotonicity. Every MCP activation must be
   indistinguishable from the GUI gesture — that IS the parity proof.
5. Capture a screenshot of the host window (`PrintWindow` +
   `PW_RENDERFULLCONTENT` — captures GPU-composited surfaces of an occluded
   window; a plain screen copy grabs whichever window is on top and is
   wrong) and compare: native grid, scene rects, panel summary.

Gotchas the loop encodes: `request_id` reuse → `request_id_reused` until
restart; a stale/wrong-tree host rejects plugins at the wire schema
(`plugin reads codec types of wire schema X, but the host writes Y`) and
shadows it as slow boot loops; test against a freshly built exe, not a stale
one.

### 8.4 What to automate vs. what to hand-test

- **Automatable**: store/state assertions, tier reads and paging, action
  parity and sequence monotonicity, refusal shapes, schema digest pins,
  screenshots for layout regressions.
- **Hand-test only**: pure-plugin click events (an action-less button's
  `click` cannot be produced over MCP — `panel_action_not_found`), focus /
  drag-feel / keyboard nuances, theme aesthetics.

---

## 9. Performance model

The tiers are built so that the **common cases are cheap and the worst case
is bounded**. This chapter states where the costs actually are, in the order
plugin authors meet them.

### 9.1 Cost model at a glance

| Operation | Cost | Notes |
|---|---|---|
| Whole-surface publish | O(surface) — validate + wire bytes + insert | the "ship everything" path; revision must strictly advance |
| Scene patch / table patch / widget page | O(surface) clone + O(ops) apply + full re-validate | all-or-nothing staging; wire frames carry O(ops), not O(surface) |
| First render of a scene revision | O(elements) projection | depth-first flatten to paint prims |
| Scene repaint (same revision) | O(prims) paint only | **zero** re-projections — retained cache (§9.2) |
| Pointer move over the scene canvas | O(prims) hit-test + one host message | only when the cursor is inside the canvas bounds |
| A drag (any length) | **exactly 2 events**: `DragStarted` + `DragSettled`/`DragCancelled` | intermediate moves coalesce away silently (§9.3) |
| Theme switch | free | theme resolves at paint; never invalidates the cache |
| Automation overview read | O(surfaces) count-only summaries | nothing materializes rows |
| Automation tier read | O(page ≤ 200 rows) | `after`-cursor paging; 10 000 rows read as 200-row windows |

### 9.2 The retained projection cache

`plugin_scene.rs` keeps **one** projection entry per thread, keyed by
`(owner, panel, revision)` and shared as `Arc<Vec<ScenePrim>>`:

- Repaints of an unchanged scene are **free** (cache hits, zero
  re-projections) — a repaint re-records the canvas geometry from the flat
  prim list, which is the unavoidable per-frame work; the semantic projection
  runs once per revision.
- A patch — even one touching a single subtree — costs **exactly one**
  re-projection at the new revision, never one per frame.
- A panel switch **evicts** the entry and the previous panel re-projects once
  on return. That is the deliberate trade-off: the cache is O(1) memory per
  thread and **never grows with panel count or history** — patched-away
  revisions are dead state, so nothing accumulates. Projections are cheap
  relative to paint (a bounded flatten vs a full canvas re-record), so the
  eviction costs less than an unbounded cache would.
- `forget_all_projections()` clears the entry on panel re-registration and
  dead-plugin reap, so a dead revision can never serve a re-registered panel
  that reuses the revision number.

The behavior is pinned by a test that counts projections: 5 repaints → 1
projection; a revision bump → exactly 2; a switch away and back → 3 and 4.

### 9.3 The interaction hot path

- **Pointer seam**: every mouse move inside the canvas runs one coarse
  hit-test (reverse paint order, first bounding-box hit wins, early exit) and
  publishes one `PanelSurfacePointer` message. Bounding boxes only —
  polylines hit their rectangle, labels use a linear-width heuristic — so the
  per-event cost is O(prims) with a tiny constant, independent of path
  geometry.
- **Drags are two events, period.** The router emits `DragStarted` on the
  first move and then **coalesces every intermediate position** (absolute
  positions carry all the information — the current policy never streams
  `DragMoved`); release emits `DragSettled` with the final position,
  superseding any pending move; cancel emits `DragCancelled` carrying the
  position to restore. Notification traffic is constant per drag, not
  proportional to distance or event count.
- **The drag preview is paint-time only.** While dragging, the host shifts the
  dragged element's prims by the preview delta at draw time: no scene
  mutation, no cache invalidation, **no plugin traffic** during the gesture.
  The plugin sees exactly one authoritative `DragSettled` and answers with
  one patch.

### 9.4 Update economics: patch, don't republish

The patch primitives exist so incremental updates do not ship whole
surfaces:

- A scene patch's wire frame carries O(ops); the store-side cost is one
  candidate clone (bounded by 10 000 nodes) plus a full re-validate — the
  price of the all-or-nothing guarantee.
- `TableOp::SetCell` is the proof-of-obligation path: a single-cell edit is
  one patch, one revision bump, O(1) cells on the wire — **never** a table
  republish. The canonicalization flow (draft commit → `CellEdited` → plugin
  patch) is two small messages per edit.
- Widget `WidgetPage` installs are bounded windows (`MAX_PAGE_ROWS = 1_000`);
  growing a tree costs pages, not republications.
- An **identical republish at the head is a no-op** (`Ok(head)`), but it
  still costs a wire roundtrip and an O(surface) equality compare — plugins
  should publish on state change, not on timers.
- Batch independent updates into **one** patch: one clone, one revision
  bump, one wire frame — instead of N patches (N clones, N revisions, N
  frames).

Plugin-author rules of thumb: update **settle-driven** (after
`DragSettled`, never during the drag); prefer `Replace`/`SetCell` over
remove-and-re-add (identity stays stable for hit-testing and automation);
keep `name`/text strings short (the 4 096-byte cap is a ceiling, not a
target); page large widget trees behind `Unloaded` + `NodeExpanded` instead
of publishing ten thousand rows up front.

### 9.5 Budgets as denial bounds

Every budget doubles as a worst-case bound on CPU and memory magnification:

- **Decode** is bounded per wire frame (thread-local node counter reset at
  frame boundaries), so a hostile payload cannot accumulate across frames or
  overflow the decoding stack before refusing.
- **Validation walks are iterative** (explicit stack, never recursion), so
  validating a scene cannot overflow the validating thread's stack either.
- The patch/page **candidate clone** is bounded by the same node/row caps
  (10 000), which bounds the all-or-nothing staging cost.
- The result re-validation on every successful apply means a stored surface
  is a validated surface — the store never spends render time re-checking.

### 9.6 Automation reads are bounded by design

The facade never materializes more than a page: the overview emits one
summary line per surface (counts only); scene reads walk the tree once for
`by_type` counts and never dump elements; widget/table reads page at ≤ 200
rows with an `after` cursor and a `next: null` termination. Reading a full
10 000-row surface is 200 cursor-continued windows — this exact walk is a
facade test — so an operator dashboards a huge surface without ever paying
for it all at once.

---

## 10. Guarantees summary

- **Bounded by construction**: every tier has node/row/ops/bytes budgets,
  enforced at decode, validate, and apply; a stored surface is a validated
  surface (patches re-validate the whole result).
- **Atomic**: refused publishes/patches/pages change nothing; patches and
  pages are all-or-nothing with exactly-one revision advance.
- **Owned**: every store operation re-checks the authenticated plugin
  identity; foreign plugins can neither write nor read; automation reads are
  explicitly owner-agnostic (the operator's app owns the store).
- **Monotonic**: per-tier revisions strictly advance (idempotent identical
  resurface allowed); event sequences per panel strictly advance.
- **Wire-stable**: discriminants pinned; new variants/requests/notifications
  append; unknown notifications degrade to `Unknown` instead of failing.
- **Live-renderer hygiene**: the renderer holds one cached projection keyed
  by revision; UI/renderer modules never reach into the desktop-gated v4
  support layer (the wasm gate catches violations).
