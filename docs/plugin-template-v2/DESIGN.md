# `plugin-template-v2` — Shared-memory `DocumentReader` example

This crate is a runnable plugin template that demonstrates the V2 host/plugin
contract:

- **Zero-copy reads** through [`ocs_plugin_api::host::DocumentReader`].
- **Validated writes** through the existing `HostApi` RPCs (`add_entity`,
  `write_record`).
- **Read→write round-trips**: read an entity handle from the shared document
  view and attach XDATA to it.
- **Quad panel (API v15)**: register a cursor-menu panel bound to `Insert`
  entities — one host-dispatched command item, one plugin callback item, and a
  custom SVG icon (`quad_icon.svg`).

It lives under `docs/` (not `crates/`) because it is documentation/example code
rather than a runtime dependency of the host.

---

## Commands

| Command | Purpose |
|---|---|
| `COUNT_SURVEY_POINTS` | Count point entities on the `SURVEY` layer via `document_reader()`. |
| `ADD_SURVEY_POINT` | Add a new point entity on the `SURVEY` layer via `host.add_entity()`. |
| `MARK_FIRST_SURVEY_POINT` | Read the first `SURVEY` point and mark it with an XDATA record (including the `SURVEY-CLASS:POINT` identity key). |
| `COUNT_MARKED_SURVEY_POINTS` | Count `SURVEY` points that carry the `SURVEYMARK` XDATA record. |

---

## Design notes

### Reads are zero-copy; writes are RPCs

`document_reader()` returns a read-only view backed by host-owned shared memory
(for out-of-process plugins) or by `&CadDocument` (for in-process plugins). The
plugin iterates entities without copying the model.

All mutations still cross the validated RPC boundary. This preserves the
host's crash-safety boundary: a buggy plugin cannot corrupt the host document
because it never holds a mutable reference to host memory.

### Entity handles bridge reads and writes

Each [`ReaderEntity`](ocs_plugin_api::host::ReaderEntity) exposes the entity
`handle`. A plugin can therefore:

1. Find an entity of interest with `document_reader().for_each_entity(...)`.
2. Use that handle with `host.write_record(handle, record)` to attach XDATA.
3. Read the record back later with `host.read_record(handle, app_name)`.

This is the primary read→write round-trip pattern the template demonstrates.

### Entity class display labels

The template also demonstrates the entity-class label convention (API v10):

- the plugin declares `ENTITY_CLASSES` and exposes it through
  `BuiltinPlugin::entity_classes()`: app `SURVEYMARK`, prefix
  `SURVEY-CLASS:`, and key → label mappings;
- `MARK_FIRST_SURVEY_POINT` stamps a `SURVEY-CLASS:POINT` string value into
  the point's XDATA alongside the existing marker value.

While the plugin is loaded, the host shows marked points as
"Survey Point (Point)" in the Properties panel title and in the
`display_type` field of the records/automation APIs; without the plugin they
are plain "Point" again. Only the stable key round-trips the drawing — the
label text lives in the plugin and can be reworded or localized without
touching saved files. The label is display-only: it never governs snapping,
selection, or any other behavior.

### Ribbon integration

The plugin registers one ribbon module (`SurveyTools`) with one group
(`Survey`). Each tool emits a `ModuleEvent::Command(...)` carrying the command
name. The host routes the command to `BuiltinPlugin::dispatch`.

### Quad panel (API v15)

The template registers one quad panel, `insert-ext` ("Insert Extras"), bound to
`Insert` entities via `applies_to` and registered once in
`BuiltinPlugin::on_load` through `HostApi::register_quad_panels` (passing the
plugin's own manifest id as the `plugin_id` argument):

| Item / control_id | Icon | Action | Behaviour |
|---|---|---|---|
| Explode — `explode` | `quad_icon.svg` (custom 24×24 `currentColor` SVG) | `QuadActionDef::Command("EXPLODE")` | The host dispatches `EXPLODE` itself with the selection aligned to the quad target — a hovered-but-unselected Insert is force-selected first, so the command edits what was pointed at. The plugin never sees the pick. |
| Inspect — `inspect` | `IconKind::Glyph("I")` | `QuadActionDef::Callback` | The pick reaches the plugin in `BuiltinPlugin::on_quad_action` with the target's handles, type keys and pick point: it logs a line to the command line and stamps a `QUADINSPECT` XDATA record on each handle inside one `push_undo` step. (`pick_point` ships `[0.0; 3]` today — the quad is screen-space only, no world point is attached yet — so the echoed coordinates read `0.000`.) |

What you see when it runs: hover an Insert with the quad enabled (Options →
"Quad Cursor Menu" section; the master `QUAD` switch is **opt-in/off by
default**), and the Insert's quad shows the "Insert Extras" tab next to the
generic panels. Its two tiles sit under the panel title; `EXPLODE` runs the
host command from the quad, while "Inspect selection" echoes
`quad inspect: 1 entity(ies) [Insert] at x, y, z` on the command line and
attaches `QUADINSPECT` XDATA (round-tripped on the drawing, APPID declared in
`xdata_apps`). Unloading the plugin removes the panel; re-registering
`insert-ext` replaces it in place.

Notes on the locked API surface (read [`docs/quad-menu.md`](../quad-menu.md)
§4 for the full contract):

- **Landed**: the v15 surface is in `crates/ocs_plugin_api` (`API_VERSION`
  is 15), so the names above are the landed signatures, not pre-landed
  targets.
- `QuadPanelDef` / `QuadItemDef` / `QuadActionDef` live in
  `ocs_plugin_api::dialog`. `QuadItemDef.icon` is an
  `OwnedIconKind` field (the owned wire shape, exactly like ribbon tools);
  you author it with the ribbon's borrowed `IconKind` (`Glyph(&'static str)`
  / `Svg(&'static [u8])`) and it converts with `Into` — the same
  borrowed → owned conversion the ribbon path performs
  (`crates/ocs_plugin_api/src/ribbon/owned.rs`).
- Registration: `HostApi::register_quad_panels(
  &mut self, plugin_id: &str, panels: &[QuadPanelDef]) -> Vec<bool>` — pass
  the plugin's own manifest id as `plugin_id`; over the plugin IPC the host
  fixes ownership from the transport (one plugin can never register in
  another's name). Registration is per-panel and replace-if-same-owner: a
  re-registered `insert-ext` replaces the owner's earlier declaration in
  place, and each returned bool answers one panel in order. Panel ids are
  namespaced by the host, visible as `plugin:<plugin_id>:<panel_id>`.
  Budgets: 16 panels per plugin, 64 items per panel, ≤ 8 KiB of SVG per icon
  (an oversized or malformed icon degrades to a glyph tile instead of
  refusing).
- Unregistering: `HostApi::unregister_quad_panels(
  &mut self, plugin_id: &str, panel_ids: &[&str]) -> Vec<bool>` removes
  named panels owned by `plugin_id`. The ownership law runs both ways: a
  `plugin_id`/`panel_id` another plugin owns is refused, not removed (one
  bool per id), and over the plugin IPC the `plugin_id` comes from the
  transport (the protocol's `UnregisterQuadPanels` request carries only the
  ids), exactly like `register_quad_panels`. Cleanup beyond
  replace-on-reregister: a plugin's panels also disappear automatically
  when the plugin unloads or its process dies (the session store reaps
  dead owners, the same law as grips/classes) — the explicit
  `unregister_quad_panels` call is for retiring panels while running.
- Callback delivery: `BuiltinPlugin::on_quad_action(
  &mut self, host: &mut dyn HostApi, panel_id: &str, control_id: &str,
  handles: &[Handle], type_keys: &[&str], pick_point: [f64; 3])` — the
  landed signature takes `type_keys` as `&[&str]` (the wire's owned
  `Vec<String>`s are borrowed for the call by the runner). The example call
  site in `src/lib.rs` matches these landed shapes: it passes
  `MANIFEST.id` as `register_quad_panels`'s `plugin_id` and declares
  `&[&str]` for `on_quad_action`'s `type_keys`.

---

## Building and running

From the workspace root:

```bash
cargo build -p plugin-template-v2
```

The resulting cdylib plus `plugin.toml` (checked in alongside this file) can
be installed into the host's plugins folder (see `docs/plugin-architecture.md`).
From API v14 the plugin runs its own runner loop, so it does not have to be
built with the host's compiler or codec commit.

---

## Compatibility

The template is built against the current `ocs_plugin_api` and declares
`ApiVersion::CURRENT`. Older plugins compiled against API v2 continue to load
on an API v3 host because new `HostApi` methods are appended at the end of the
trait and the host accepts plugin majors from `API_VERSION_MIN_SUPPORTED` up to
`API_VERSION`.
