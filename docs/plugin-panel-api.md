# Plugin panel API — behaviour as implemented (API v21 base + v22 additions)

Behaviour documentation for the dockable-side-panel surface a plugin
exercises, verified against a live host (the pallet-menu extension ships on
its own review branch; the v22 additions from `selection-panel-v22` are in
[value](#api-v22--selection-digest-structured-panel-state-panel_action)).
Evidence: the `firetest-ocsgui` skill (native MCP bridge firetest,
2026-11-14 session) plus host source references.

## Registration

- `HostApi::register_panel(PanelDef)` docks the panel (default right side,
  new dock group), installs its control state, and registers BOTH its id and
  its `command` string as dynamic command-line completions.
- Re-registration of an unchanged `PanelDef` is **idempotent** and keeps the
  panel's mid-edit values, focus, and layout (`register_panel_for_plugin`):
  safe to call at every plugin load.
- `visible` starts **false**; a plugin shows a fresh panel only through
  `toggle_panel` or the user's pallet-menu pick.
- A panel id is owned by exactly one plugin: `register_panel_for_plugin`
  refuses another owner's id. One plugin's registration never displaces
  another's.
- The pallet (**+**) menu lists every registered panel: built-ins first in
  their historical order, then plugin panels title-sorted, capped at 32 rows
  per edge (overflow reported at registration time on stderr/gui.log).
  Toggling a menu row rides `EdgeMenuToggle` → the generic `PanelId::Plugin`
  dock arms; **works without any open drawing**.
- Commands routed THROUGH the command line (`EPL_*` prefixes) require an
  open drawing (`no_document` guard otherwise) — `op:"new"` from automation,
  or a user-created drawing, unblocks them.

## Panel control events

- The host broadcasts `PanelAction { panel_id, control_id, action, value }`
  to every loaded plugin:
  - `change` for text/number/checkbox/choice/color/file edits (live, per
    keystroke for text);
  - `click` for buttons **without** an `action` string;
  - built-in keywords (`close`/`hide`, `clear`, `reset`) are consumed by the
    host before reaching plugins.
- `on_notification` carries **no host reference**: plugins TRACK control
  state there; any host write-back (`update_panel`, entities) happens from
  `on_load` or `dispatch`.
- `update_panel(panel_id, values: HashMap<String, DialogValue>)` rewrites
  control drafts in an open panel (or modal dialog state). Engine-side it is
  an idempotent UI draft write — records and geometry never change
  implicitly.

## Icons

- `PanelId::Plugin(_)` uses a generic built-in asset (`PLUGIN_PANEL`, a
  docked-panel outline).
- Plugin-provided ribbon/QAT art (`IconKind::Svg`) must follow the OCS
  pixel grid: `width="16" height="16" viewBox="0 0 16 16"`, 1 px strokes on
  half-pixel coordinates, fills on whole pixels, OCS palette colours
  (`#ffffff` primary, `#888888` secondary, `#4a9eff` accent…). The host
  re-tints per active theme and refits the art to every slot with sharp
  line weights. `currentColor` is NOT filled in in this pipeline (renders
  black); glyph fallbacks (`IconKind::Glyph`) draw as text via
  the ribbon font stack — a glyph the stack lacks renders as NOTHING, so
  ship SVG art (the 8 KiB / unparsable fallback is the label abbreviation).
- Quad-menu (v15) art differs: 24×24 viewBox, final colours, no recolouring.

## Automation (MCP) surface — how a host session is firetested

- `ocs_read {op:"state"}` exposes `plugin_panels`: id, owner, dock
  location, `visible`, `on_screen`, and current per-field draft values
  (Text/Number structured; Checkbox/Choice/Color/Label gained typed
  `value` fields in v22 — see below).
- `ocs_execute` carries the same primitives a user clicks:
  `panel_input` (draft edits), `panel_button` (declared-action buttons
  only — action-less `click` buttons are UI-only by design), `panel_action`
  (v22 — drives clicks AND typed change values, below), `run` / `new`
  (document-bound command routes).
- `request_id` is MANDATORY and idempotency-keyed for the whole session:
  reuse with a changed payload is refused (`request_id_reused`); a retry
  with the SAME payload replays the original verdict — the mechanism that
  makes agent retries safe.

## API v22 — selection digest, structured panel state, `panel_action`

- `notification_api_version` is **22** (`API_VERSION` moved 21→22). The
  runner ABI gate is unchanged: manifests report their own `api_version`,
  and the host streams each plugin the event shapes it understands.
- **`SelectionChangedDigest`** (discriminant 14, ser+deser) is what an
  API-22+ plugin receives at every selection boundary instead of
  per-handle lists: only `{tab_id, revision, count}` — O(1) bytes even for
  tens of thousands of selected entities. `revision` increases
  monotonically **per tab** whenever the selection fingerprint actually
  changes (`bump_selection_revision` / `selection_revision_by_tab`,
  `src/app/update/mod.rs`; published via
  `publish_selection_changed_versioned` in `src/plugin/v4_support.rs`), so
  a plugin that already saw revision `N` can skip work entirely. Handles
  are fetched lazily via `HostApi::selection` at dispatch time — where a
  real host reference exists (`on_notification` has none).
- **Per-plugin routing** (`ProcessManager::broadcast_for_api` +
  `broadcast_selection_changed`, `crates/ocs_plugin_api/src/process/manager.rs`):
  every selection boundary fans out ONE notification chosen PER PLUGIN by
  its manifest API major:

  | Plugin API | Selection event |
  |------------|-----------------|
  | 22 and newer | `SelectionChangedDigest {tab_id, revision, count}` |
  | 4 ..= 21 | `SelectionChangedV4 {tab_id, handles}` (tab-tagged handle list — unchanged) |
  | older than 4 | `SelectionChanged {handles}` (plain, tab-less — unchanged) |

  No endpoint receives a variant twice; legacy streams stay byte-identical,
  with the same deferral/boot-window and best-effort error semantics as
  `broadcast_notification`.
- **Structured panel values** (additive; legacy `kind` debug strings kept):
  `Checkbox`/`Choice`/`Color`/`Label` each carry a typed `value` next to
  `kind` in state reads — `bool`, `{index, text}` plus `options` for
  Choice, `[r,g,b]` for Color, the label text for Label. Automation reads
  typed state without parsing Rust debug output (closes the v21-era
  debug-stringification gap).
- **`panel_action` MCP op** (`PANEL_ACTION` in `src/mcp_ops.rs`, armed on
  the plugin_panel control deed; `panel_control` is a recursive
  control-type finder, `panel_action` dispatches): fires ONE control's
  real GUI interaction without a window — `click` on an action-less
  button, or a typed `change` on Checkbox (boolean), Choice
  (`{index, text?}` — text defaults to `options[index]`), Text/Number
  (plain string/number) drafts; Color is picker-only (color drafts reach
  the plugin through `update_panel` or the picker itself):

  ```json
  {"op":"panel_action","panel_id":"epl_panel","control_id":"epl_button_ping","action":"click"}
  ```

  The same Message the rendered widget would send; records, geometry, and
  undo stay untouched. Complements `panel_input` (typed draft edits) and
  `panel_button` (declared-action buttons only).
- Plugin side (example panel, `schoeller.example_panel` v0.3.0): panel
  writes ride the thread-safe plugin request line
  (`plugin_request_sender` → `PluginRequest::UpdatePanel`) behind a 16 ms
  gate — newest state wins, one request per gate window, explicit actions
  bypass. Apply/Revert roundtrip through the host: one
  `UpdateEntitiesTransaction` undo group per direction, exact-state
  conflict detection on revert (external edits/deletes reported, never
  clobbered), selection restored via `set_selection`.
- Benchmarks (digest-vs-handle payloads, burst coalescing, apply/revert
  timings) live in the v22 addition's session notes and the table below.

## Benchmarks (v22 digest vs legacy selection events)

Wire payload = `bincode::serialize(&HostNotification)` bytes (fixed-int LE,
bincode 1.x — the exact bytes the host pipes to a runner; the protocol's
`Serialize` impl embeds the discriminant tag). Selection size `N`:

| N | `SelectionChangedDigest` (22+) | `SelectionChangedV4` (4..21) | `SelectionChanged` (<4) |
|---|--------------------------------|------------------------------|--------------------------|
| 1 | **33 B** | 33 B | 25 B |
| 10 | **33 B** | 105 B | 97 B |
| 256 | **33 B** | 2 073 B | 2 065 B |
| 10 000 | **33 B** | 80 025 B | 80 017 B |
| 50 000 | **33 B** | 400 025 B | 400 017 B |

The digest row is constant (tag + discriminant tag + three `u64`); the
legacy rows are `~8·N + 25` — at 50 000 entities the digest is a
**12 100×** smaller wire payload, and no per-handle clone/serialize work
happens on the host at the selection boundary (the tab-tagged handle list
rows also serialize/copy O(N) host-side, which the digest eliminates).

16 ms gate coalescing (plugin-side `PANEL_WRITE_MIN_GAP`, back-to-back
burst hitting the gate with newest-state-wins semantics): bursts of
100 / 1 000 / 10 000 notifications each collapse to **1** panel write per
gate window (100×–10 000× fewer host roundtrips; under load the gate
sustains at most one write per 16 ms window = a 62.5 writes/s ceiling per
panel, with each write carrying the newest draft values). Measured in the
`digest-bench` wall-clock simulation of the gate logic shipped with the
example plugin's unit tests.

Live-host roundtrip (`schoeller.example_panel` 0.3.0 on the v22 build,
2026-11-14 session, 3 entities: 2 Lines + 1 Circle, digest revision seen
climbing 1→7 across the run):

| Operation | Wall time (single automation roundtrip) | Result |
|-----------|------------------------------------------|--------|
| `EPL_APPLY_SELECTION` | 0.94 s | all 3 entities at `Rgb{46,125,50}` (#2E7D32 draft green), one undo group |
| `EPL_REVERT_SELECTION` | 0.93 s | untampered handles restored to ByLayer; a foreign `entities_set` recolor (`Rgb{10,10,10}`) survives as a reported conflict, never clobbered |
| digest → gated label write | immediate (≤16 ms window) | summary label tracked `sel 3 rev 2 … rev 7` newest-wins |
| host Undo/Redo | ≤2 ops | entity colours follow the host stack with NO plugin re-apply and no clobbering |

(A pure no-op re-apply on an already-green selection writes nothing —
"nothing to apply" and no journal — so a later revert honestly reports
"nothing to revert"; this was verified and initially misread as a restore
bug.)

## Host lifecycle on a developer machine

- The plugin runner holds the cdylib (`LoadLibrary`) — **the host must be
  stopped before the dll can be replaced** (`: install-plugin-windows.sh`).
- A freshly rebuilt host exe is AV-scanned synchronously on first launches:
  the process may sit silent for MINUTES before its control listener binds
  (this masquerades as "the build boot hangs"). Do not kill live boots; wait
  for the descriptor (`automation/<session_id>.json` = port + token), not
  the window.
- `automation/gui.log` is SHARED across instances with no instance marker;
  read it for content (wire-schema refusals, spawn lines, engine stderr),
  not for instance identity.
