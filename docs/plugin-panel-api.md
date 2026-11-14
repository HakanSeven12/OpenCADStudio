# Plugin panel API — behaviour as implemented (API v21 era)

Behaviour documentation for the dockable-side-panel surface a plugin
exercises, verified against a live host (see `plugin-pallet-menu.md` for the
pallet-menu extension this branch adds and its performance/security review).
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
  black); glyph fallbacks (`IconKind::Glyph`, U+25A4 类) draw as text via
  the ribbon font stack — a glyph the stack lacks renders as NOTHING, so
  ship SVG art (the 8 KiB / unparsable fallback is the label abbreviation).
- Quad-menu (v15) art differs: 24×24 viewBox, final colours, no recolouring.

## Automation (MCP) surface — how a host session is firetested

- `ocs_read {op:"state"}` exposes `plugin_panels`: id, owner, dock
  location, `visible`, `on_screen`, and current per-field draft values
  (Text/Number structured; other kinds debug-stringified — a cosmetics gap).
- `ocs_execute` carries the same primitives a user clicks:
  `panel_input` (draft edits), `panel_button` (declared-action buttons
  only — action-less `click` buttons are UI-only by design), `run` /
  `new` (document-bound command routes).
- `request_id` is MANDATORY and idempotency-keyed for the whole session:
  reuse with a changed payload is refused (`request_id_reused`); a retry
  with the SAME payload replays the original verdict — the mechanism that
  makes agent retries safe.

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
