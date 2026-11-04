# Quad Cursor Menu — Design & Implementation Strategy

Branch: `quad_menu` · Status: **implemented** — host P0/P1 + plugin API v15 core · Date: 2026-11-03

The strategy below is kept as history; the landed implementation's deviations
from it are listed in §9, and remaining work in §10.

References (BricsCAD V27 help):

- The Quad basics: https://help.bricsys.com/en-us/document/bricscad/getting-started-with-bricscad/exploring-the-interface/the-quad-basics?version=V27&id=165079147018
- The Quad cursor menu: https://help.bricsys.com/en-us/document/bricscad/getting-started-with-bricscad/exploring-the-interface/the-quad-cursor-menu?version=V27&id=165079146598

---

## 1. What we are building

A **Quad** is a command system anchored at the mouse cursor. It inspects what is
under the cursor (or what is selected) and shows only the commands that apply:

| Cursor situation (when triggering)        | Quad content                                                             |
| ----------------------------------------- | ------------------------------------------------------------------------ |
| Hovering an entity (quad-on-hover flag)   | Default command icon for that entity type; expands on entry into the quad |
| Selection set exists (quad-on-select flag)| Commands that operate on the selection (mixed types → common commands)    |
| Nothing under cursor (right-click flag)   | "No Selection Quad": recently used commands + entity-creation groups      |

Key BricsCAD behaviors we replicate (phase 1 unless marked otherwise):

- **Collapsed → expanded**: for a hover/select target the quad starts as a single
  default-command icon; moving the cursor onto it expands to the 6 most recently
  used commands for that entity type (`QUADWIDTH`), with category tabs below
  holding the remaining applicable commands; hovering a tab expands its items.
- **Rollover tips**: entity properties shown next to the default icon when
  `ROLLOVERTIPS` is on; the quad title bar toggles between commands and tips.
- **Sub-launching**: `QUADCOMMANDLAUNCH=1` lets a right-click *over an entity*
  (without entering the quad) launch the default command directly. `=0` requires
  clicking the icon.
- **No Selection quad**: right-click in void, or (over an entity) right-hold
  longer, opens the creation/recent quad instead of the legacy shortcut menu.
- **Toggles**: F12, a status-bar `QUAD` field (click toggles, right-click shows
  display options), `QUADDISPLAY` sysvar with flags: *on hover*, *on select*,
  *on right-click*, *suppress hover-quad when a selection exists*. The hover
  variant requires hover highlighting to be active (our `SELECTIONPREVIEW`
  analogue), which OpenCADStudio already has.
- **Tab-cycling** of several coincident entities under the cursor, and Ctrl-driven
  subentity targeting (dimension arrow/text, solid edges/faces) — **deferred**
  (phase 4).

Deliberate novelty: plugins contribute quad panels/actions. BricsCAD does this
via CUI/workspace customization; we do it via the plugin IPC layer (§4).

---

## 2. What OpenCADStudio already has (fits naturally)

| Need                       | Existing machinery (path)                                                                                     |
| -------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Pure popup menu model      | `src/ui/popup/context_menu.rs` — `build_context_menu(ctx)`, `MenuRow/MenuItem/MenuAction`, mnemonics, groups   |
| App glue + keyboard nav    | `src/app/update/context_menu.rs` — `context_menu_context()`, `on_context_menu_pick()`, `intercept_context_menu_key()` |
| Right-click trigger        | `src/app/update/mod.rs` (`ViewportRightRelease`, ~:5695) honoring `settings::RightClickMode` + `right_click_hold_ms` |
| Hover entity detection     | `src/app/update/viewport.rs` hover block (~:3860) — `interaction_hover_candidates_near()`, `set_hover_highlight()`; `src/scene/pick/hit_test.rs` (`click_hit`, `click_hits_all`, `pick_tolerance_px`) |
| Cursor-anchored overlays   | `src/app/view/overlay.rs` snap-override grid (cursor-anchored, click-catcher), `cycle_popup_overlay`, `anchored()` widget in `src/app/view/overlay/mtext_menu.rs`; stack push in `src/app/view/mod.rs` (~:2731) |
| Command launch             | `Message::Command(name)` → `dispatch_command()` (`src/app/commands/mod.rs:128`); PickFirst works because commands read `scene.selected` at dispatch |
| Icons / labels             | `src/ui/icons.rs`; `ribbon_commands()` map command → (label, icon) in `src/modules/registry.rs`; `ToolDef`/`IconKind` shared with plugins |
| Settings + persistence     | serde `UserSettings` (`src/app/settings.rs`), load/save in `src/app/update/file.rs`, Options UI `src/ui/window/options.rs`, sysvar mapping in `src/app/commands/styleprops.rs` |
| Plugin extension pattern   | End-to-end pattern to copy: `PanelDef` (crates/ocs_plugin_api/src/dialog.rs) → `HostApi::register_panel` → `PluginRequest::RegisterPanel` → host session state → `HostNotification::PanelAction` broadcast |

**Gaps:** (1) no per-entity-type "applicable commands" metadata anywhere
(`ToolDef` has only `id/label/icon/event`); (2) no per-type most-recently-used
(MRU) command memory; (3) no quad UI; (4) no plugin surface for cursor-anchored
UI. All four are additive.

**Verification (2026-11-03):** a full-codebase search for the term confirms the
host contains **no quad feature** — every hit is incidental (SDF text quads,
`osnap::QUAD` = quadrant snap, `QuadTree` benchmarks, `QuadraticBSpline` codec,
"quadrant" prompts). There is no `QUAD*` sysvar, no quad toggle, and no
quad-shaped state; the existing right-click ShortcutMenu is the only
cursor-adjacent menu and is not entity-typed, not MRU-driven, and has no tabs —
the quad is net-new, built on the same pipeline listed above. The plugin API
(v14) similarly has **no menu-of-any-kind surface besides ribbon/panels**
(verified over `crates/ocs_plugin_api`), and `HostRequest::CursorMove`
(`ipc/protocol.rs:100`) is a command-scoped preview event for an active
`InteractiveCommand` — it carries no entity-under-cursor context and gives
idle plugins no hover UI hook.

---

## 3. Host design

### 3.1 State and model

New file `src/ui/popup/quad_menu.rs` — a pure sibling of `context_menu.rs`:

```rust
pub struct QuadMenu { anchor: Point, mode: QuadMode, expanded: QuadExpand,
                      active_tab: usize, panels: Vec<QuadPanel>, tips: bool }
pub enum QuadMode { Hover, Selection, NoSelection }
enum QuadExpand { Collapsed, Expanded }
pub struct QuadPanel { id: String, label: String, items: Vec<QuadItem> } // panel = a bottom tab
pub struct QuadItem { label: String, command: String, icon: MenuIcon, hint: String, enabled: bool, plugin: bool }
```

State lives in `SelectionMenu` (extend `src/scene/pick/selection_state.rs`) as
`quad: QuadUi { open_at: Option<Point>, mode, expanded, active_tab }`, keeping
the same "selection boundary owns transient popup state" convention the context
menu already uses. App fields added only for hover-timing:

- `last_hover_pos / hover_dwell_since: Option<Instant>` (drives hover-quad onset).

### 3.2 Context resolution

A `quad_context()` in `src/app/update/context_menu.rs` classifies the quad into
`QuadTarget { None, Hovered{ handle, type_key }, Selection{ handles, kinds } }`
from: current `hover_handle`, `scene.selected`, and last cursor position.
`type_key` uses the existing stable names (`entity_type_name()` →
`"line"`, `"insert"`, …; `src/entities/traits.rs`, display name via
`names::ui_name()`). Model building must be **cheap**: resolve panels once per
(target-kind, selection-hash) change, not per frame.

### 3.3 Panels and applicability data

No applicability metadata exists today, so phase 1 ships a curated side table in
`src/modules/registry.rs` (same style as `COMMAND_ICON_ALIASES`, zero impact on
`ToolDef` wire shape):

```rust
static QUAD_ENTITY_COMMANDS: &[(&str, &[&str])] = &[
  ("Line",       &["MOVE","COPY","ROTATE","SCALE","TRIM","FILLET","LENGTHEN","BREAK","ARRAY","DELETE", ...]),
  ("Insert",     &["EXPLODE","COPY","ROTATE","SCALE","ENUMINSERT","MOVEREFEDIT","DELETE", ...]),
  ("Hatch",      &["HATCHEDIT","HATCHSETBOUNDARY","MOVE","SCALE","DELETE", ...]),
  // Line, LwPolyline, Circle, Arc, Ellipse, Spline, Point, MText, Text,
  // Dimension, Leader/Multileader, Table, Viewport, ... — keys are exactly
  // the canonical entity_type_name() strings (src/entities/traits.rs:174)
];
static QUAD_NO_SELECTION_COMMANDS: &[&str] = &["LINE","CIRCLE","ARC",...]; // creation groups as panels
```

Keys are the canonical `entity_type_name()` strings ("Line", "LwPolyline",
"CenterMark", "Revcloud", "Hatch", "Insert", …) — the same identifiers Quick
Select / Select Similar already use — so hover typing and panel binding can
never drift apart. Plugin-declared entity classes participate equally under
their stable `class_id` (§4).

Panel/label/icon resolution goes through `ribbon_commands()` (single source of
truth with ribbon+search). Plugin plain commands (exact-name `try_dispatch`
path, `command_prefixes`) stay command-line / auto-complete only — no generic
"Plugins" tab (removed by user decision); fine-grained plugin panels come
with API v15 (§4).
Later phases can replace hand curation with (a) ties into
`SelectObjectsCommand::filtered(..., keep: fn(&EntityType)->bool)` selection
predicates and (b) a declarative `applies_to` field on `ToolDef` (plugin-safe:
append-only).

**MRU service**: `QuadMru` (per-`type_key` `VecDeque<String>`, cap 16) in
`src/ui/popup/quad_menu.rs`. Recorded when a command is launched *from the quad*
(`Message::QuadRun(cmd)` wrapper) and seeded from `QUAD_ENTITY_COMMANDS` order.
In-memory first; persistence optional behind a `quad_persist_mru` flag.

### 3.4 Dispatch semantics (hover vs selection)

Quad clicks route through the existing action router (`on_quad_pick` mirroring
`on_context_menu_pick` in `src/app/update/context_menu.rs`):

- **Selection quad**: pick runs `Message::Command(name)` with the selection as
  is (PickFirst already reads `scene.selected` at dispatch).
- **Hover quad, target not in selection**: replace selection with the hovered
  entity (`deselect_all` + `select_entities`) before dispatch. This keeps one
  code path and matches BricsCAD's hover→act flow.
- **Hover quad, target in selection**: dispatch with the existing selection.
- **`QUADCOMMANDLAUNCH=1` and right-click over an entity**: launch default
  command instead of opening anything (also skip legacy menu). Time-sensitive
  right-click interplay: quick right-click = default command; hold = No
  Selection quad; void right-click = No Selection quad (flag) else legacy menu.
- Right-click during an **active command** keeps current behavior (Enter /
  option context menu); the quad does not open mid-command in phase 1.
- Undo parity for free: quad launches are ordinary `dispatch_command` calls.

### 3.5 Rendering

New `quad_overlay(...)` in `src/app/view/overlay.rs`, pushed into the viewport
stack in `src/app/view/mod.rs` next to `viewport_context_menu_overlay`:

- Collapsed mode: one icon button (default command) at anchor + rollover tip.
- Expanded: `QUADWIDTH` columns (default 6) of the MRU commands, tab strip at
  bottom edge, tab content expands on hover (BricsCAD layout), click-catcher
  background closes on outside press (pattern: `cycle_popup_overlay` /
  snap-override overlay), `anchored()` widget for tip toggle.
- Reuse `MenuIcon` and `icons.rs`/statics; theme via `icons.rs` helpers.

### 3.6 Keyboard

- `Tab` (phase 2): cycles coincident hover candidates using existing
  `click_hits_all` + hover-highlight; the quad re-resolves to the cycled entity.
- Keytips (phase 2): per-item codes via the `Tip` pattern
  (`src/app/update/keytips.rs`), `intercept_context_menu_key`-style interception
  before other key handling when quad is open.
- `F12` master toggle: `src/app/shortcuts.rs`.

### 3.7 Settings & sysvars

`UserSettings` additions (serde-default, so old `settings.json` still loads):
`quad: bool`, `quad_on_hover: bool`, `quad_on_select: bool`,
`quad_on_right_click: bool`, `quad_suppress_hover_when_selection: bool`,
`quad_width: u8 = 6`, `quad_command_launch: bool`. Wire into
`src/app/update/file.rs` persistence, an Options section
(`src/ui/window/options.rs`), and status-bar toggle + right-click options menu
(modeled on the OSNAP status-bar popup pattern `src/ui/popup/snap_popup.rs`).
Sysvar exposure via the `styleprops.rs` SETVAR table: `QUAD` (int/bool),
`QUADWIDTH` (int), `QUADCOMMANDLAUNCH` (int). (Exact BricsCAD `QUADDISPLAY` bit
values to be confirmed at implementation time; OCS may store 4 bools and expose
`QUADDISPLAY` as a mapped int.)

### 3.8 i18n

The strategy's `quad-labels` ftl block (`quad-labels = Quad` +
`-recent`-style children in `locales/*/opencadstudio.ftl`, resolved via
`t!` + regenerated `src/locale_catalog.rs`) was appended to all 23 locale
dirs and then removed again as dead keys — no `src` code ever consumed
these keys. The quad's UI strings are the English source strings instead:
the "Recent" No-Selection tab label is the quad model's hardcoded English
literal (the `quad.recent` panel, `src/ui/popup/quad_menu.rs`), built-in
quad items take their labels from the existing English command catalog
(`src/modules/registry.rs`), and the locale catalog itself is untouched.
Full localization of the quad UI is queued in §10 (locale parity). Panel
titles are data, not code.

### 3.9 Icons — OCS-only, resolved from the command registry

The standard quad carries **OCS-style icons exclusively** — the same set the
ribbon, menu bar, QAT, and command-line suggestions already use. No icon is
introduced under a second naming scheme:

- **Single resolution path**: every quad item's icon is looked up via
  `registry::command_icon(command)` (`src/modules/registry.rs:970`), which is
  alias- and variant-aware (`RECTANG→RECT`, `ARC→ARC_3P`, `ARRAY→ARRAYRECT`,
  `GCCOINCIDENT→CCONSTRAINT`, `-LAYER→LAYERS`, leading `'` transparent
  commands) and returns `MenuIcon::Tool(bytes)` from the `IconKind::Svg`
  statics of `src/ui/icons.rs`. Quad MRU entries resolve through the same path,
  so a command keeps one icon everywhere it appears.
- **Asset source**: the existing OCS-owned `assets/icons/**` SVGs —
  per-command icons (`line.svg`, `chamfer.svg`, `modify_hatchedit.svg`,
  `move.svg`, …), variant families (`circle/`, `ellipse/`, `gradient/`),
  chrome (`sysvar.svg`, `table_cell_*`, layout/model sets). The availability of
  per-command SVG coverage for the phases' curated tables is checked at P1
  startup; only genuinely missing icons are candidates for new OCS assets.
- **Fallback**: commands registered as `IconKind::Glyph` (no SVG) render the
  existing glyph tile used by ribbon search / status popups — never a
  hand-invented quad icon. A "missing SVG" list is a tracked deliverable, not a
  quad-local patch.
- **Quad chrome**: chevrons for tab expand, the title-bar tips toggle, the
  "Shortcut Menu…" row, and the status-bar QUAD field reuse existing chrome
  SVGs (`assets/icons/quickcalc/chevron_*.svg` pattern) or are authored as new
  OCS-own icons under `assets/icons/ui/` only where nothing fits — drawn in
  the same OCS icon style. (The authored `quad_field.svg` for the status bar
  was added and then removed as unused: the pill ships as text-pill chrome,
  §9.)
- **Rendering**: items draw above context-menu row height; the same
  `icons.rs` `themed()`/`tool()` helpers scale the statics at tool-button size
  with theme tinting (ribbon `widgets.rs` precedent).
- **Plugin items** (§4): plugins may supply **custom icons** for their quad
  items — the same `IconKind::Svg(Vec<u8>)` byte mechanism ribbon tools already
  use (`OwnedIconKind`, `crates/ocs_plugin_api/src/ribbon/owned.rs:18`); see
  the custom-icon contract in §4.



---

## 4. Plugin API (API v15)

Follow the grip/dialog end-to-end pattern; no new host concept, just a new
declarative UI family. Plugins already surface their commands globally
through the command line (exact-name `try_dispatch` + `command_prefixes`);
only explicitly registered quad panels reach the quad itself.

Additions (append-only, default-safe for older plugins):

| Layer                    | Change                                                                                                   |
| ------------------------ | --------------------------------------------------------------------------------------------------------- |
| Types (`dialog.rs`)      | `struct QuadPanelDef { panel_id, title, applies_to: Vec<String> /* entity_type_name() keys + plugin class_ids, "*" ok */, items: Vec<QuadItemDef> }`; `struct QuadItemDef { control_id, label, icon: OwnedIconKind, action: QuadActionDef }` — authored with the ribbon's borrowed `IconKind`, converting with `Into` to `OwnedIconKind` exactly like ribbon tools; `enum QuadActionDef { Command(String), Callback }` |
| IPC (`protocol.rs` v4)   | `PluginRequest::RegisterQuadPanels { panels: Vec<QuadPanelDef> }` / `UnregisterQuadPanels { panel_ids: Vec<String> }`; `PluginResponse::QuadPanelsRegistered { ok: Vec<bool>, errors: Vec<String> }` (one entry per panel/id, order); the owning plugin's id is transport identity, never a wire field; host→plugin `HostNotification::QuadAction { panel_id, control_id, handles: Vec<Handle>, type_keys: Vec<String>, pick_point: [f64; 3] }` — a **flat** notification, discriminant 13; no `QuadTargetInfo` wrapper |
| Host vtable (`host.rs`)  | `HostApi::register_quad_panels(&mut self, plugin_id: &str, panels: &[QuadPanelDef]) -> Vec<bool>` / `unregister_quad_panels(&mut self, plugin_id: &str, panel_ids: &[&str]) -> Vec<bool>`; `BuiltinPlugin::on_quad_action(&mut self, host, panel_id: &str, control_id: &str, handles: &[Handle], type_keys: &[&str], pick_point: [f64; 3])` — all with default no-op impls; the runner calls the callback only for v15+ plugins (old cdylibs never see the variant: unknown notifications fall back to `Unknown`) |
| Host state               | New `src/plugin/quad.rs`: per-plugin registered panels (session-owned like `src/plugin/grip_points.rs`; budgets: **16 panels per plugin** (`MAX_QUAD_PANELS_PER_PLUGIN`), **64 items per panel**; one owner per `panel_id`, panel ids namespaced `plugin:<plugin_id>:<id>`) |
| App plumbing             | `HostSession::register_quad_panels` impl in `src/app/plugin_host.rs` (:3931, pattern: `register_panel` :4786); merge order = builtin panels → plugins by registration order; `publish_quad_action` in `src/plugin/v4_support.rs` (owner-targeted, not a fan-out); click routing in `src/app/update/context_menu.rs` / `update/mod.rs` |
| Dispatch                 | `Command(String)` items reuse the ordinary command path (undo/interactive handled by host); `Callback` items emit `QuadAction` to the owning plugin, wrapped in a host undo group by the plugin's own `push_undo` convention. Plugins fetch context at action time via `selection()` / `entities_batch()` — no stream push |
| Python parity            | Landed: `ocs_module.rs` pyfunctions `register_quad_panels(...)` / `unregister_quad_panels(...)` — a panel is a `{"panel_id", "title", "applies_to": [...], "items": [...]}` dict, an item's `icon` is `{"glyph": "X"}` or `{"svg": "<svg…>"}` (raw bytes, ≤ 8 KiB) and its `action` is `{"command": "VERB"}` or `{"callback": True}`; the call returns one bool per panel. Callback picks reach the *owning* plugin's `QuadAction` (owner-routed, unlike `PanelAction`'s fan-out) |
| Docs                     | `docs/plugin-architecture.md` new "Quad surface (API v15)" section + roadmap entry; `docs/plugin-template-v2/` example |

### Capability contract: entity-specific plugin quads

 Plugins must be able to enable entity-specific quad menus — verified as a
 hard requirement. Today (API v14) this is impossible: plugins can only ship
 ribbon groups, global panels, and grips; there is no menu surface and no
 cursor context outside an active command. API v15 closes it with these
 guarantees:

- **Binding**: `QuadPanelDef.applies_to` lists entity-type keys; the host shows
  the panel's tab only when the quad target matches. Bindings are resolved by
  the host at quad-open time — the plugin registers once, no per-hover traffic.
- **My own entity classes**: `applies_to` also accepts plugin-declared
  `class_id`s (`register_plugin_class` / `PluginClassDeclaration`,
  `src/plugin/plugin_classes.rs`), so a plugin that introduces new entity types
  can add a quad for exactly those types — first-class, like the grips story.
- **Acting on the hovered entity, not just the selection**: for
  `Command(String)` items the host aligns `scene.selected` to the quad target
  before dispatch (§3.4), so an unselected hovered insert is still correctly
  fed to a plugin command. `Callback` items instead receive the flat
  `HostNotification::QuadAction` fields — the quad-target `handles` and
  `type_keys` even when they were not selected (the router aligns
  hover-to-selection before the `quadplugin:` dispatch, and the pick delivers
  with the current selection as its context). Plugins can additionally read or
  adjust context through the existing `selection()` / `set_selection()`
  (`host.rs`).
- **No streaming**: context evaluation is host-side at hover/open; every quad
  action ships its own context in the flat `QuadAction` fields.
  `pick_point` is context-only and today always ships `[0.0; 3]` — the quad's
  anchor is a screen position and no world point is reconstructed on this
  path yet (the host-supplied zero is the honest placeholder;
  `src/app/commands/mod.rs`). `CursorMove` stays a command-scoped preview
  event and is left untouched.

### Custom plugin icons

`QuadItemDef.icon: OwnedIconKind` supports plugin-authored artwork with **no
new wire mechanism**: the bytes ride the existing
`OwnedIconKind::{Glyph(String), Svg(Vec<u8>)}` encoding
(`crates/ocs_plugin_api/src/ribbon/owned.rs:18`) — you author with the
ribbon's borrowed `IconKind` (`Svg(&'static [u8])`, `include_bytes!`) and the
`Into` conversion moves them into the owned shape — proven end-to-end by
ribbon tools, whose SVG bytes already cross IPC and render through the host's
iced SVG stack (resvg; monochrome and multi-colour both work) at the 64 MiB
frame cap (`ipc/transport.rs:18`), far above any icon.

Contract for plugins (enforced + documented in `plugin-template-v2`):

- **Format**: raw `<svg>` bytes; recommended 24×24 viewBox matching OCS icon
  conventions; use `currentColor` for strokes/fills so the icon participates in
  light/dark theming (multicolour artwork allowed, but fixed colours will not
  re-theme).
- **Budget** (grip-cap style, honest and capped): ≤ 8 KiB of SVG per icon
  (`MAX_QUAD_ICON_BYTES`, `src/plugin/quad.rs`), at most 16 panels per plugin
  and 64 items per panel. The host keeps the declared bytes at registration
  and converts them to the UI's `QuadIcon::SvgOwned` when the quad builds
  (per target change, never per frame).
- **Defensive registration**: an oversized or malformed-intent icon (see
  "malformed-intent": `looks_like_svg` in `src/app/plugin_host.rs`) does not
  abort registration — the conversion degrades it to the glyph tile (first
  letter of the item's label) and the panel survives. Refused *panels* (caps,
  duplicate control ids, a foreign panel id) each return one `false` in the
  registration response, with a short reason on the command line. The host
  never crashes on plugin artwork, and unknown-host/old-plugin combinations
  degrade exactly as ribbon icons do.
- **Ownership**: icons are plugin-owned bytes; the host never names, localizes,
  or rewrites them beyond theme-aware rendering.

Compat rules: bincode discriminants append-only; `API_VERSION 14 → 15`
(`api_version=15` in plugin.toml for panels-aware plugins; v14 plugins keep
working — their plain commands are reachable through the command line only
(no auto "Plugins" tab is synthesized, removed by user decision), and the
runner never calls `on_quad_action` on a pre-v15 vtable; a `QuadAction`
notification falling to an older plugin decodes as `Unknown`). Note
`wire_schema` is **unchanged**: it fingerprints the *codec* types the wire
carries, and v15 adds only app-level protocol variants
(`PluginRequest`/`PluginResponse`/`HostNotification`) plus HostApi vtable
appendages — no codec shape changed, so the codec-fingerprint hash stays put
(`crates/ocs_plugin_api/src/version_info.rs`).

---

## 5. Implementation phases (each independently shippable)

- **P0 — switches**: settings fields, Options section, status-bar QUAD field +
  mini menu, F12, sysvar table (`QUAD`, `QUADWIDTH`, `QUADCOMMANDLAUNCH`).
- **P1 — host MVP**: quad model/state/overlay; Selection quad + No Selection
  quad (right-click trigger); hover collapsed/expanded quad with dwell; curated
  applicability table; MRU; dispatch semantics (§3.4); legacy "Shortcut Menu…"
  escape item; locales; OCS-icon resolution (§3.9). Acceptance: over a hatch
  right-click-less hover shows HATCHEDIT-first MRU with the same OCS icon the
  ribbon uses, click edits it; void right-click opens creation quad when flag
  set; every quad panel icon resolves via `command_icon()` with at most a
  glyph-fallback whitelist; EnterFirst/TimeSensitive right-click modes
  unaffected when quad off.
- **P2 — depth**: rollover tips + title-bar toggle; Tab cycling; keytips; MRU
  persistence; predicate-derived applicability for the ~40 existing filter
  functions; command-active restrictions formalized.
- **P3 — API v15 plugin surface** (§4) + contract-test plugins (+Python).
  Acceptance: a plugin registers an entity-bound panel (built-in key or its own
  `class_id`) whose tab appears only on matching targets, and its command item
  edits a hovered-but-unselected entity (host-aligned selection); a plugin item
  ships a custom SVG icon (rendered, with glyph fallback on malformed bytes and
  over-budget icons); a v14 plugin still loads (plain commands stay command-line only,
  no synthesized quad tab).
- **P4 — later/advanced**: subentity quad (dimension arrow/text ↔ e.g. flip
  arrow; solid edge/face tools), quad-aware `CUSTOMIZE` page, IF terrain
  (touch/constrained UI), `applies_to` on `ToolDef` for third parties.

## 6. Testing

- Unit: pure `build_quad_menu` contexts (empty/hovered/selection/mixed/
  plugin-fallback), MRU ordering/caps, settings serde roundtrip, sysvar
  int⇄flags mapping (styleprops-adjacent tests), icon coverage: extend the
  `command_aliases_and_variants_resolve_target_icons` pattern so every command
  named in `QUAD_ENTITY_COMMANDS` / `QUAD_NO_SELECTION_COMMANDS` resolves an
  icon (SVG or whitelisted glyph fallback).
- Interaction: existing app-input test harness (find the pattern in
  `src/app/update/*_tests.rs`, e.g. context-menu/unsaved-changes tests) to
  simulate right-release, dwell, quad pick; assert `scene.selected` mutation +
  dispatch for hover-not-selected.
- Plugin contract: template plugin registers a panel bound to `"line"`, host
  roundtrips `RegisterQuadPanels`/`QuadAction`; old-API plugin still loads.
- Locales: `scripts/test_locales.py` + catalog regen check.

## 7. Risks & open questions

- **Perf**: hover candidates are re-evaluated per move — gate quad resolution on
  hover-state change + dwell, cache panels per `(type_key, selection-hash)`.
- **RMB semantics**: quad interacts with `RightClickMode` (3 modes) and
  Shift+RMB snap popup; keep Shift+RMB untouched; document the precedence
  matrix in §3.4 and cover each mode in tests.
- **Reconciling quad and rich context menu**: BricsCAD *replaces* right-click
  menus; OCS's context menu is richer. Decision above: quad wins on flag when
  a target exists; legacy menu stays reachable inside the No Selection quad
  until a customization story exists. Revisit after dogfooding.
- **Immediate-mode churn**: keep quad model cached; rebuild only on
  expand/tab-switch/target change.
- **Editor churn**: `update/mod.rs` and `view/mod.rs` are hot files — land quad
  as new files with minimal diffs into the routers.
- Open: MRU persistence default, keytip codes, whether plugin `Command` items
  should also record MRU, `QUADWIDTH` clamp, paper-space viewport items.

## 8. Out of scope (explicitly)

Customize-dialog quad editing, workspace profiles, touch, the automation/MCP
path (quad is pointer-only), and any change to existing right-click defaults
until P2 dogfooding says otherwise.

---

## 9. Deviations landed vs strategy

Each decision below was verified against the landed code while the host
P0/P1 waves and the plugin API v15 were merging; file:line references are to
that state of the tree. The plugin wave has since landed (API v15: quad
panels, the flat `QuadAction` notification, `QuadIcon::SvgOwned`, and the
Python pyfunctions); entries below describe the landed state.

- **Settings defaults — the quad is opt-in**: the master switch `quad`
  defaults to `false`; every piece after it is on (`quad_on_hover`,
  `quad_on_select`, `quad_on_right_click`,
  `quad_suppress_hover_when_selection`), with `quad_width = 6`, dwell 300 ms
  and `quad_command_launch = false` (`src/app/settings.rs:1389-1395`, test
  `quad_defaults_opt_in_and_old_configs_load_unchanged`). The strategy never
  pinned this default; on by default would have changed right-click behavior
  overnight.
- **Dedicated message enums**: the quad runs on `QuadMsg` / `QuadOptionMsg`
  (`src/ui/popup/quad_menu.rs`, `src/ui/popup/quad_status.rs`; routed in
  `src/app/shortcuts.rs:393`, handled in
  `src/app/update/context_menu.rs:710`), instead of the strategy's
  hypothetical `Message::QuadRun(cmd)` wrapper — `QuadMsg::Toggle` mirrors
  the popup-model convention of `context_menu.rs`.
- **Keybinding — `SHIFT+F12`, not F12**: F12 is already Dynamic Input
  (`shortcuts.rs:50: F12 → DYNINPUT`), so the quad's toggle takes the nearest
  free seat, default `("SHIFT+F12", "QUADTOGGLE")` with `QUADTOGGLE` as the
  command alias (`src/app/shortcuts.rs:50-54`, `:390-393`). The strategy's
  §3.6 said F12.
- **Selection quad trigger — RMB-with-selection**: with a selection present
  and `quad_on_select` on, the *right-click takeover* opens the Selection
  quad; the quad never opens on plain select events
  (`src/app/update/context_menu.rs:866-870`,
  `quad_right_click_takeover`:847). The strategy's trigger table implied
  select-driven opens; landing inside the RMB path reuses the trigger
  precedence and the Enter-first right-click cycle tests.
- **Target classification while hovering**: the selection set is classified
  using the mouse-present resolution (`quad_target_for`,
  `src/app/update/context_menu.rs:595`) — the hovered entity wins with `None`
  / `Hovered{handle, type_key}` / `Selection` classified at right-click time,
  not cached on the selection events.
- **Tab semantics — `active_tab` indexes panels overall**:
  `QuadUi.active_tab` (`src/ui/popup/quad_menu.rs:144`) addresses the whole
  panel list, 0 = primary grid (`quad_panels[0]`, the hover/Recent grid) and
  every non-zero index the following panels' tabs — including plugin panels —
  rendered from that flat index (`src/ui/popup/quad_overlay.rs:48-71`), rather
  than a tab-local index.
- **Plugin actions run through the quad pick path only**: plugin quad items
  (Command and Callback alike) dispatch a synthetic command string under
  the `quadplugin:` scheme —
  `quadplugin:<plugin_id>:<panel_id>:<control_id>` (`QUAD_PLUGIN_PREFIX`,
  `src/plugin/mod.rs:14`, mirrored at
  `src/app/update/context_menu.rs:540`) — but the pick router routes it to
  `App::quad_plugin_action` INSTEAD of the ordinary command dispatcher:
  `try_dispatch_command` answers `quadplugin:` with the plain
  unknown-command path (and the string is *not recorded into the MRU*,
  `src/app/update/context_menu.rs:808`), so typed/automation/script input
  can never trigger quad callbacks. At the pick path a `Command` item
  dispatches its verb exactly like a plain quad item (the review's
  dead-tile fix) and a `Callback` item publishes the owner-targeted
  `QuadAction` under the `grip_points::visibility` guard — a
  stale/invisible triple answers like any unknown command. There is no
  separate plugin-pick channel.
- **MRU recording — global + per type**: every quad-launched plain command is
  recorded: hovered/selection targets record under the target's type key
  (`record_for`, last key for a mixed selection) and the No Selection quad
  records globally (`record_global`)
  (`src/app/update/context_menu.rs:792-815`);
  `QuadMru` caps at 16 per key (`src/ui/popup/quad_menu.rs:18`), resolving
  the strategy's open question ("whether plugin Command items should also
  record MRU") as *yes, plain commands only*.
- **Plugin icon variant — `QuadIcon::SvgOwned`**: plugin-authored quad
  artwork renders through a `QuadIcon::SvgOwned(Arc<[u8]>)` variant of the
  quad model's icon enum (owning bytes, so the host can drop the plugin's
  `&'static` lifetime demand). **Landed**: `QuadIcon::SvgOwned` exists
  (`src/ui/popup/quad_menu.rs:43`), the conversion applies the ≤ 8 KiB
  budget and degrades to the glyph tile
  (`src/app/plugin_host.rs` `quad_item_icon`), and a disabled render dims the
  artwork instead (`src/ui/popup/quad_overlay.rs:139-142`). The ribbon
  `OwnedIconKind` encoding stays the wire format for the bytes.
- **UI metrics**: tile size 38 px with a 4 px gap (the 36–40 px tile band),
  `QUADWIDTH` columns per expanded row, 20 px tab strip
  (`QUAD_TILE`/`QUAD_TILE_GAP`/`QUAD_TAB_H`, `src/ui/popup/quad_overlay.rs:17-25`).
- **Status bar + options surfacing**: the `QUAD` status-bar pill toggles via
  main-press and opens the display-options popup from a caret
  (`quad_pill`, `src/ui/statusbar/mod.rs:442`, `:983-1033`; flag mirror +
  popup in `src/ui/popup/quad_status.rs`), and Options gained a
  "Quad Cursor Menu" section (`src/ui/window/options.rs:1562-1658`) — the
  status-bar sibling pattern from `snap_popup.rs` as planned, with text-pill
  chrome instead of an SVG asset.
- **Sysvars**: `QUAD`, `QUADWIDTH` and `QUADCOMMANDLAUNCH` are exposed in the
  SETVAR table with clamping (`src/app/commands/styleprops.rs:918-921`,
  `:2336-2368`); `QUADDISPLAY` as a mapped bit-int was not landed — the four
  bools surface individually in the pill's popup instead, as the strategy
  allowed ("OCS may store 4 bools").
- **Review-fix round**: `HostApi::unregister_quad_panels` gained the
  `plugin_id: &str` argument with the ownership law enforced both ways
  (a caller retires only its own panels; over IPC the host fixes the id
  from the transport), matching `docs/plugin-architecture.md`. `QuadStore`
  teardown rides the plugin lifecycle pump's reaper next to grips/classes
  (`retain_alive`, `src/plugin/quad.rs`, wired in
  `src/app/update/plugin_grips.rs`). The §3.3/§4 pre-v15 "Plugins" tab was
  landed as a synthesized universal auto panel per plain-command plugin
  (`plugin_auto_panels`) and then removed by user decision: only
  explicitly-registered quad panels contribute, and plain plugin commands
  stay reachable through the command line / auto-complete only. And the §3.8 `quad-labels` ftl block was
  appended to all 23 locale dirs and removed again as dead keys (§3.8) —
  quad UI strings are English source strings with full localization queued
  in §10.

## 10. P2 / follow-ups

The remaining quad backlog, in no particular order: rollover tips (entity
properties next to the default icon, title-bar toggle) are declared but do
not render yet; `Tab`-cycling coincident hover candidates (phase-2 §3.6) is
not wired into the quad the way `click_hits_all` supports it; per-item keytip
codes (the `Tip` pattern) are absent, so keyboard-only quad use is
`QUADTOGGLE`-less; MRU persistence (the optional `quad_persist_mru` flag) is
still memory-only per session; and the legacy-menu closes that
still live inline in unrelated command paths —
`src/app/update/qselect.rs:104`, `src/app/update/command.rs:1473`,
`src/app/update/command.rs:2413` (hard-coded `menu.open_at = None`) — want
the refactor to a shared close helper once the quad replaces the legacy
shortcut menu for good; and full localization of the quad's UI strings —
the `quad-labels` ftl keys were added and then removed as dead (§3.8), so
reintroduce them together with the consuming code and keep all 23 locale
dirs in parity.
