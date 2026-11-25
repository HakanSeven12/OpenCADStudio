# Plugin architecture history: `ocs_plugin_api` versions, ABI and API changes

The version history of the stable add-on contract (`crates/ocs_plugin_api`):
every API major, what it appended to `HostApi` / `BuiltinPlugin` / the wire
protocol, and the compatibility rules that let a v3-era cdylib still load
today. The authoritative narrative lives in the `manifest.rs` module docs;
this file is the navigable overview.

## 1. The compatibility model (ABI rules)

- **The major version is the ABI compatibility unit.** A plugin compiled
  against major `N` runs on any host whose major is `>= N`, because new
  vtable entries and enum variants are **appended at the end** — the vtable
  layout of an older plugin is a prefix of the current one.
  `host_accepts_plugin_version` enforces the accepted range at load.
- **`API_VERSION`** (currently **30**) is the host's major.
  **`API_VERSION_MIN_SUPPORTED = 3`**: V2 is no longer loaded because its
  ribbon wire types predate the current ones — a V2 plugin passed the gate
  and then failed silently while registering its ribbon (#1085, #987). The
  floor exists so gate-passing plugins cannot fail mysteriously mid-load.
- **`OCS_PLUGIN_MAX_API_VERSION`** (`MAX_API_VERSION_ENV`) caps the accepted
  major at runtime, so an operator can disable a new API major (e.g. `= 5`
  to refuse V6+) without rebuilding the host.
- **Two change classes, two mechanisms:**
  - *New vtable slot* (a `HostApi` / `BuiltinPlugin` method or lifecycle
    callback): requires a **major bump**; the runner consults the plugin's
    declared major before calling (e.g. `on_dialog_action` is only invoked
    for v17+ plugins).
  - *Appended wire variant* (a `PluginRequest` / `PluginResponse` /
    `HostNotification` discriminant): rides the current major; a plugin that
    sends a variant an older host cannot decode **declares the newer major**,
    and the older host refuses it **at load** — never mid-session.
- **Notifications are gated per endpoint**: the runner drops any notification
  whose `notification_api_version` exceeds the plugin's declared major
  (`runner.rs`), so a v26 runner never sees the v27 delta stream or the v29
  panel-surface events. Unknown tags decode to an `Unknown(Vec<u8>)`
  fallback rather than failing the stream.
- **Since v14 the host matches wire shapes, not compilers**: the runner is
  self-driving (`export_plugin!` emits `ocs_plugin_run`; no Rust type crosses
  the library boundary) and compatibility is the codec types' wire shape
  (`version_info::wire_schema`), replacing the earlier
  compiler/codec-commit matching.
- **In-place reshapes happen only before release**: v14's parametric
  envelopes were reshaped in place with `api_version` kept at 14 because no
  deployed plugins pre-dated the reshaping.

## 2. Version timeline

Dates anchor to the upstream merge or the cited commit; pre-crate versions
(v2–v13) predate the June 2026 crate extraction (`72ec1146d`, 2026-06-15)
and are preserved here from the manifest narrative.

| Major | Anchor | `HostApi` / `BuiltinPlugin` surface | Wire / protocol | Compatibility notes |
|---|---|---|---|---|
| v2 | pre-crate | `start_interactive` | — | no longer loadable (see floor above) |
| v3 | pre-crate | `document()` / `document_mut()` become local cached copies; `document_reader` / `document_view` appended | — | **the load floor** (`API_VERSION_MIN_SUPPORTED = 3`); backward-compat gate landed 2026-06-24 (`d0003534a`) |
| v4 | pre-crate | — | full-duplex notifications on a multiplexed socket | V2/V3 ABI and protocol untouched |
| v5 | pre-crate | `BuiltinPlugin::on_load`; `document_path` | — | lifecycle begins |
| v6 | pre-crate | `system_variable` / `set_system_variable` | — | generic host variable access |
| v7 | pre-crate | atomic, undoable replacement of a set of existing entities | — | first set-atomic write |
| v8 | pre-crate | `create_xrecord` / `read_xrecord` / `update_xrecord` / `delete_xrecord` / `list_xrecords` | — | name-addressed XRecord CRUD |
| v9 | pre-crate | `set_reference_only` | — | display-only, plugin-owned geometry |
| v10 | pre-crate | `BuiltinPlugin::entity_classes` | — | XDATA class-identity display labels |
| v11 | pre-crate | `entity_grips`, `grip_drag`, `set_plugin_grips`; `on_custom_grip_event` | — | the grip surface: built-in + plugin-declared custom grips |
| v12 | pre-crate | `entities_batch` | live-geometry channel (`CustomGripResponse::live_entities`, capped) | consumer-validation hardening: bounded polls + settle watchdog, nested COMMAND frames refused in deliveries, deterministic boot retry |
| v13 | pre-crate | `constraint_create`; `read_xrecords` / `update_xrecords` / `list_xrecords_with_entries` | `RunQuiet`; `ChannelReady` ack; `DocumentChangedRegions` (dirty regions) | live geometry is VISUAL-ONLY vs the solver; plugin-labeled single undo steps |
| v14 | pre-crate (painted release) | `stage_parametric_chain`, `sample_parametric`, `register_plugin_class` / `stage_plugin_class_instance`, `plugin_class_instance_properties` (+ parameter schema), `CustomGripResponse::descriptor`, grip `drives` anchors | runner self-driving (`ocs_plugin_run`); `wire_schema` matching; ramp data vocabulary | parametric envelopes reshaped IN PLACE pre-release (version kept 14 — no deployed plugins existed); host re-derives display sets locally per move |
| v15 | pre-crate | — | quad-cursor menus + custom icons (`QuadPanelDef`, `RegisterQuadPanels`, `QuadAction`) | declarative menus, protocol-append only |
| v16 | pre-crate | — | `ValidateParametricChain` | custom grips snap and settle click-move-click like native grips |
| v17 | pre-crate | `InteractiveCommand::on_dialog_action` (**new vtable slot**) | `DialogValue::ChoiceOptions` / `Enabled` / `Visible`; `DialogControl::Label` | reactive modal dialogs |
| v18 | pre-crate | `panel_snapshot` | appended request | complete typed dock-panel snapshots; ownership authenticated from the IPC transport |
| v19 | pre-crate | — | `DialogControl::StyledText`, `DialogControl::Svg` | presentation controls mapped onto the host theme |
| v20 | pre-crate | — | `PluginClassInstance::selectable_custom` | selectable class instances refuse native editing |
| v21 | pre-crate | — | `PluginGrip::xy_plane` | grip events constrained to the original world-XY plane |
| v22 | pre-crate | `set_geo_location` / `remove_geo_location` | `HostNotification::SelectionChangedDigest` — routed INSTEAD of the legacy handle-list variants for v22+ endpoints | O(1) selection notification; legacy streams preserved per endpoint |
| v23 | pre-crate | — | `TableOperation::LayoutSetTabOrder`; `PanelDef::style` (`Palette` default / `Form`) | layout tab order in one undo step; older payloads decode via `deserialize_panel_style` |
| v24 | pre-crate | `InteractiveCommand::on_undo_step` | `HostRequest::UndoStep` / `HostResponse::UndoStep` | mid-command Ctrl+Z; host sends only to v24+ manifests |
| v25 | pre-crate | `BuiltinPlugin::tools` / `call_tool` | `HostRequest::ListTools` / `CallTool` | typed operations for AI clients over the automation bridge |
| v26 | pre-crate | — | `TableOperation::CustomPropertiesSet` | DWGPROPS Custom in one undo step |
| v27 | pre-crate | — | `HostNotification::DocumentChangedDelta` (wire tag 15): added/modified/deleted handles + record tuples + V4 snapshot correlation + `full_refresh_required` | routed INSTEAD of Regions for v27+ endpoints; `notification_api_version` gate drops it for v26 runners |
| v28 | pre-crate | `InteractiveCommand::input_kind` / `on_text_input` | `InteractiveInputKind`, `GetInputKind` / `InputKind`, `InteractiveEvent::TextInput` | command-line text prompts; v3–v27 unchanged |
| **v29** | **2026-11-24** (`efb03f9e3`, PR #3586 + the S01–S08 arc, #3642) | surface-publish overrides on `V4PluginHostApi` (on-load publishing attaches to the wire) | **the panel-surface arc** (see §3): `PanelSurfacePublish` / `Patch` / `Page` / `TablePatch`, the `PanelSurfaceEvent` notification (wire tag 16), `plugin_panels` / `plugin_panel_action` automation ops | scenes, widget trees, typed tables, event routing and MCP parity appended within one major |
| v30 | 2026-11-25 (`d46bd4a11`) | — | `ModuleEvent::PluginFolderDialog` | a ribbon tool asks for a native folder picker; `"<command> <folder>"` is dispatched back — a v29 host cannot decode it, so a sender declares v30 |

## 3. The v29 panel-surface arc in detail (S01–S08)

API v29 is the one major that carries the whole panel-surface system. All of
the following appended within v29 (wire discriminants appended, never
renumbered; each pinned by tests):

- **Data model** (`scene.rs`, `widget.rs`, `tables.rs`): `PanelScene`
  (Group/Rect/Path/Text, `StableId`, budgets), `WidgetSurface` (keyed rows,
  `WidgetPage` lazy paging), `TableSurface` (typed columns/cells,
  `TablePatch`); the `PanelSurface` carrier pins `Scene = 0`.
- **Publish/update wire**: `PanelSurfacePublish {panel_id, surface}` (one
  request, any tier), `PanelSurfacePatch` (scene ops), `PanelSurfacePage`
  (widget page), `PanelSurfaceTablePatch` (cell/row ops); typed responses
  (`PanelSurfacePublished`, `PanelSurfacePatchResult`,
  `PanelWidgetPublished`, `PanelSurfacePageResult`, `PanelTablePatchResult`)
  answering the stored revision or the tier's reject.
- **Events**: `SurfaceEvent` (click/drag/node/cell vocabulary) delivered as
  `HostNotification::PanelSurfaceEvent {panel_id, sequence, event}` — wire
  **tag 16**, v29-gated, unknown-tag fallback.
- **Automation parity**: the `plugin_panels` read op (tiered bounded reads
  with cursors) and the `plugin_panel_action` execute op (the agent gesture
  produces the same host `Message` as the GUI gesture), pinned into the
  tool-schema digest.
- **Follow-ups within v29**: `a3b00410a` (#3642) gave `V4PluginHostApi` the
  surface-publish overrides so on-load publishing attaches; `8ebfb0d7a`
  (#3649, reworked upstream as `c0b9e3830`) made dock plugin bodies fill
  their pallet width.

## 4. Versioning governance as practiced

The history above shows a consistent decision procedure:

1. **Append, never renumber** — vtable slots, enum variants, and
   discriminants only grow at the end; renames/renumbers are pre-release
   events (v14's in-place reshape is the recorded exception, justified by
   zero deployed plugins).
2. **Prefer declare-later wire additions over major bumps** — an appended
   request/response/notification rides the current major; only genuinely new
   host-called surfaces (vtable slots) take a major.
3. **Replace-by-routing, not break** — legacy notification streams
   (`SelectionChangedDigest`, `DocumentChangedDelta`) run per endpoint
   against the plugin's declared major, so old plugins keep their exact
   bytes.
4. **Fail at load, not mid-session** — an incompatible request moves the
   plugin's declared major so an older host refuses the load up front
   (v16/v18 are the explicit examples).
5. **The manifest doc-comment is the changelog of record** — every major's
   entry names its additions *and* the compatibility consequence; this file
   only restates it.

## 5. Sources

- `crates/ocs_plugin_api/src/manifest.rs` — module docs (v2–v30 narrative),
  `API_VERSION = 30`, `API_VERSION_MIN_SUPPORTED = 3`,
  `MAX_API_VERSION_ENV`.
- `crates/ocs_plugin_api/src/{host,host_v4,runner,ipc/protocol,ipc/client}.rs`
  — vtable surfaces, the notification gate (`runner.rs`), wire discriminants
  and pins.
- Git anchors: `72ec1146d` (crate extraction, 2026-06-15), `d0003534a`
  (backward-compat gate, 2026-06-24), `efb03f9e3` (v29 panel-surface train,
  2026-11-24), `d46bd4a11` (v30, 2026-11-25).
- Companion docs: `docs/plugin-host-model.md` (the runtime model),
  `docs/plugin-panel-scene-api.md` (the v29 surface system in depth).
