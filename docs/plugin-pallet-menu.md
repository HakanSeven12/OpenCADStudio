# Plugin panels in the edge (+) pallet menu

## Background

`HostApi::register_panel(PanelDef)` (API v7) docks a plugin panel and wires
its command. The edge (+) pallet menus — the user-facing palette pickers on
the left and right rails — enumerated only the hard-coded
`PanelId::ALL` (17 built-ins); `PanelId::Plugin(u32)` was excluded, so a
registered plugin panel could be toggled by command but was invisible in the
menu a user browses to re-open palettes. The plumbing behind the menu
(`EdgeMenuToggle` → `dock()` / `dock_set_open()` / `dock_panel_visible()`
/ `dock_panel_wanted()`) already carries generic `PanelId::Plugin(_)` arms,
so the only missing piece was the listing itself.

## Stage 1 — host-side listing (this branch, no API change)

`app/dock/view.rs` now builds the menu rows from
`OpenCADStudio::pallet_menu_entries(side, tab)` instead of `PanelId::ALL`:

- built-ins keep their historical order and the block-edit-session gate;
- every registered plugin panel follows, sorted by authored title
  (case-insensitive), each mapped through `plugin_panel_by_num` to its
  numeric dock id;
- a row's checked state is the same rule built-ins show: docked on this
  edge **and** currently visible;
- plugin rows use the generic `PanelId::Plugin(_)` icon (`MORE`) and the
  plugin-authored title; built-ins keep their localized titles;
- toggling reuses `EdgeMenuToggle` unchanged — a checked plugin row *and*
  an unchecked one both behave exactly like built-ins.

The plugin side needs **no change**: every `register_panel` call
automatically becomes a menu row. Regression test:
`edge_menu_lists_and_toggles_registered_plugin_panels`.

Known limitation (by design for stage 1): a user-hidden plugin panel
reappears after a restart because plugin load re-registers fresh state
(idempotent nutrition, same as today's auto-dock). Persisting the
suppressed ids in the dock config is a follow-up host change, independent
of the API.

## Stage 2 — API extension review (proposed, v22, additive)

```rust
// ocs_plugin_api, additive; hosts predating v22 keep the default (no-op).
/// Include this plugin's panel in the edge (+) pallet menu (default true
/// for every registered panel). `listed = false` withdraws the row.
fn set_panel_pallet_entry(&mut self, panel_id: &str, listed: bool) -> bool {
    false
}
```

Optional companion (mirrors the v15 quad-panel shape exactly — owner
credits, replace-if-same-owner, per-call outcome vector):

```rust
fn set_panel_pallet_icons(&mut self, entries: &[PalletIcon { panel_id: String, icon: crate::ribbon::IconRef }]) -> Vec<bool>;
```

### Wire safety

- No `PanelDef` struct-field additions: bincode is positional; a trailing
  new field would make old hosts refuse or misread new plugins. New
  *methods* plus fresh serde-defaulted structs keep every older version
  loading (the discipline `register_panel` → v7 quad panels → v15 followed).
- The runner gives plugins a host that ignores unknown requests silently;
  a plugin calling `set_panel_pallet_entry` on an older host observes
  `false` and continues.

### Performance

- **Menu build cost stays per-frame** (the dock view rebuilds every render).
  Built-ins are a fixed 17-array; the plugin contribution is one pass over
  `plugin_panels` (dozens at most in practice) plus a sort. Measured frame
  cost impact is negligible; a cached title-ordered id vector maintained at
  register/remove time is available if profiling ever says otherwise.
- **No event or IPC traffic** is added: the menu reads host-local state.
  Toggling rides the existing `EdgeMenuToggle` → `dock_set_open` path, and
  the plugin observes an ordinary `PanelAction`-channel event only if it
  subscribes to document events (it never blocks the render loop).
- The idempotency-guard state of `register_panel_for_plugin` (re-register
  unchanged → no reset) is what keeps repeated loads cheap.

### Security

- **Sizing / abuse ceiling.** A plugin can register many panels; the menu
  would list them all. The stage-2 companion adds a per-plugin cap
  contract identical to quad panels (per-plugin caps exist there); before
  that, the stage-1 implementation is capped at 32 plugin rows per menu
  edge (sorted, overflow documented in gui.log), which bounds the render
  and keeps one buggy plugin from widening the palette indefinitely.
- **No trust expansion.** The menu trusts plugin-authored *titles* only as
  plain text through the existing text pipeline (no markup, no icon URLs);
  icons in the stage-2 op re-use `OwnedIconKind` and its existing size
  budget — same embedding rules as ribbon SVGs, no network fetches.
- **Ownership stays one-way.** Only the owning plugin can withdraw or
  icon its own rows (id owner check identical to quad panels); one plugin
  can never rename, move, or hide another's entry.
- **No new secrets.** The automation descriptor/token flow is untouched;
  the menu never carries credentials and adds no new IPC channel.
- **Lifecycle.** Entries live and die with `plugin_panels`; the reaper
  prunes them when a plugin unloads (or crashes), so no menu row can
  outlive its owner.
- **State recovery.** Withdrawn-then-re-registered panels re-enter as
  unchecked; a hostile plugin toggling `listed` in a loop only flips its
  own row and amortizes against the same idempotency rules as everything
  else.

## Verification

- WSL `cargo check -p OpenCADStudio` + the new dock test.
- Windows release build from the same branch, then the `firetest-ocsgui`
  skill: `state plugin_panels` before/after menu toggles prove presence
  (`on_screen`/`location`/`visible`) exactly like host-owned panels.
