# Plugin panel surfaces

Plugin panels can publish three kinds of declarative SURFACE that the host renders with native widgets (api v29): a bounded declarative **scene** (vector snapshot on the canvas), a semantic **widget** tree/list (rows with lazy paging and optional BUTTON rows), and a **typed table** (scalar cells the user can edit; the plugin stays the data authority). `ocs_panels` reads them; `ocs_panel_action` acts on them exactly like the user's gesture. Both ride the same facade as every other intent tool: pass `ocs_session_id` when several sessions are open.

## Discover what is published

Without arguments you get one bounded line per surface — owner and per-tier revision plus counts, never content:

```
ocs_panels  →  {"ok":true,"panels":[
  {"panel_id":"acme:tree","owner":"acme.widgetpack",
   "tiers":{"widgets":{"revision":3,"rows":1042}}},
  {"panel_id":"acme:plot","owner":"acme.plotsmith",
   "tiers":{"scene":{"revision":7,"elements":214}}}
]}
```

## Read one surface, page by page

Pass `panel_id` for detail. The scene tier answers with COUNTS only (the plugin owns the geometry; use `ocs_render`/`ocs_capture` for pixels):

```
ocs_panels {"panel_id":"acme:plot"} →
  {"tier":"scene","revision":7,"elements":214,
   "by_type":{"group":4,"rect":120,"path":80,"text":10}}
```

Widget and table rows come back as keyed pages: at most `rows` entries (default 50, cap 200), starting strictly after the `after` cursor, with `next.after` set when more remain. Walk the cursor until `next` is null — every row appears exactly once, in a stable order:

```
ocs_panels {"panel_id":"acme:tree","tier":"widgets","rows":200}
ocs_panels {"panel_id":"acme:tree","tier":"widgets","rows":200,"after":1042}
```

Widget rows read `{"id","label","children","action"}`; `children` is `leaf`, `unloaded` (ask the plugin by activating the row — see below), or a list of child ids; `action` is set on BUTTON rows. Table pages list `columns` first and rows in the surface's own order with typed cells (`"text"`, `1.5`, `true`).

## Act: activate an element

`ocs_panel_action` is the semantic click. On a widget row it is exactly the event the user pressing the row (button or tree node) produces:

```
ocs_panel_action {"panel_id":"acme:tree","element":1042}
  →  {"ok":true,"panel_id":"acme:tree","element":1042,
      "kind":"node_activated","sequence":17,
      "delivery":"best_effort_owner_targeted"}
```

On a scene element pass `"kind":"element_clicked"`. `kind` defaults by tier (widget rows activate as nodes, scene elements click). The element must exist in that surface — a typo refuses and burns no sequence number.

## What the plugin sees (and what you should expect)

The activation is delivered to the OWNING plugin only, as the same `PanelSurfaceEvent` a GUI gesture produces, with the surface's monotonic `sequence` as its explicit identity: two identical-looking activations are DISTINCT events. Delivery is best-effort — a dead owner's event drops silently, and a plugin that wants to know about lazy children answers an `Unloaded` expansion by pushing its own page. The plugin stays the data authority: after any action, re-read the surface (the revision advances when the plugin publishes anew) instead of assuming a result. None of this touches records, geometry, or undo.

---

## For plugin authors: constructing the surfaces

Everything below is the plugin side of the same v29 story — the plugin declares data, the host renders it with native widgets, and the plugin is notified when the user acts. All three tiers can coexist on one panel (independent revision spaces).

### A widget tree/list with a BUTTON row

```rust
use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::widget::{WidgetChildren, WidgetRevision, WidgetRow, WidgetSurface};

let mut surface = WidgetSurface::new(WidgetRevision(1));

// A leaf row that ACTS as a button (buttons are leaf-only by validation).
surface.rows.insert(StableId(1), WidgetRow {
    label: "Recalculate".into(),
    children: WidgetChildren::Leaf,
    action: Some("recalc".into()),   // arrives back as NodeActivated
});
// A node with children the host will page in ONLY when expanded.
surface.rows.insert(StableId(2), WidgetRow {
    label: "Layers".into(),
    children: WidgetChildren::Unloaded,
    action: None,
});
surface.roots = vec![StableId(1), StableId(2)];
host.panel_widget_publish("acme:tree", surface)?;
```

When the user expands the `Unloaded` row the plugin receives `SurfaceEvent::NodeExpanded` and answers with a keyed page — installing a page NEVER replaces the surface:

```rust
let page = WidgetPage {
    base_revision: WidgetRevision(2),          // must match the stored head
    parent: Some(StableId(2)),                 // children of "Layers"
    after: None,                               // insert at the front; position with `after`
    rows: (0..50).map(|i| (StableId(100 + i), WidgetRow {
        label: format!("Layer {i}"),
        children: WidgetChildren::Leaf,
        action: None,
    })).collect(),
    next: Some(StableId(150)),                 // cursor for the NEXT page (or None)
};
host.panel_surface_page("acme:tree", page)?;
```

### A typed, editable table

```rust
use ocs_plugin_api::tables::{CellValue, TableColumn, TableRevision, TableSurface};
use ocs_plugin_api::scene::StableId;

let columns = vec![
    TableColumn { id: StableId(10), label: "Name".into(), .. },
    TableColumn { id: StableId(11), label: "Length".into(), .. },
];
let mut table = TableSurface::new(TableRevision(1), columns);
table.order = vec![StableId(100)];
table.rows.insert(StableId(100), vec![
    CellValue::Text("Wall A".into()),
    CellValue::Number(4.25),
]);
host.panel_table_publish("acme:schedule", table)?;
```

The user edits cells in the host's native grid (host-local drafts until a typed commit). Each commit arrives as `SurfaceEvent::CellEdited { row, column, value }`; the plugin canonicalizes by pushing its own atomic patch (all-or-nothing, revision-checked — `SetCell` touches exactly one cell):

```rust
let patch = TablePatch {
    base_revision: TableRevision(2),
    ops: vec![TableOp::SetCell {
        row: StableId(100), column: StableId(11), value: CellValue::Number(4.5),
    }],
};
host.panel_surface_table_patch("acme:schedule", patch)?;
```

### A scene surface

```rust
let scene = PanelScene { revision: SceneRevision(1), elements: vec![ /* Rect/Path/Text/Group */ ] };
host.panel_surface_publish("acme:plot", PanelSurface::Scene(scene))?;
```

### Menus: reuse the v15 quad panels — no new path

Plugin menus are the EXISTING v15 declarative quad panels (`QuadPanelDef`, registered once per session via `register_quad_panels`); they are not part of the surface tiers and this stage adds no second menu vocabulary:

```rust
let menu = QuadPanelDef::new("align_tools", "Align")
    .with_applies_to(vec!["LWPOLYLINE".into()]);
// items: QuadItemDef { control_id, label, icon: OwnedIconKind::Svg(..),
//                      action: QuadActionDef::Command("ALIGN".into()) }
host.register_quad_panels(plugin_id, &[menu])?;
// A pick arrives as HostNotification::QuadAction (or runs the command).
```

### The contract in one line

Publish data at a revision; the host renders and owns the interaction; you receive only the semantic events that need your authority (activation, expansion of `Unloaded` rows, committed cell edits) — owner-targeted, sequence-identified, best-effort. A working example plugin lives in the train repository's `example_plugin_panel/`.

