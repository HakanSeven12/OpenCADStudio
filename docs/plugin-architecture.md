# Open CAD Studio — Plugin Architecture

**Status:** Accepted
**Author:** Open CAD Studio contributors
**Date:** June 2026

This document is the **authoritative spec** for how add-on packages integrate
with Open CAD Studio. The model follows [QGIS](https://plugins.qgis.org/)-style
extensibility: a small metadata file, a single entry point, an optional separate
engine crate, and user-installable packages from a curated index.

> Plugins are **out-of-process dynamic libraries** (`cdylib`). The host loads
> first-party packages from its application resources and third-party packages
> from the user plugins folder. Both use the generic runtime
> (`src/plugin/`, `src/app/plugin_host.rs`) and the stable contract crate
> (`crates/ocs_plugin_api`). A user package with the same id explicitly
> overrides a bundled package.

---

## Design goals

| Goal | Rationale |
|------|-----------|
| **One package, one entry point** | Manifest, ribbon tab and commands ship together in the plugin crate; no edits to the host. |
| **Stable contract** | Authors target the semver-versioned `ocs_plugin_api` crate, not `OpenCADStudio` internals. |
| **Out-of-tree by default** | Third-party plugins remain independent; selected first-party plugins may build and ship with OCS. |
| **DWG round-trip** | Domain data lives on entities as XDATA, not in an opaque side database. |
| **Engine reuse** | A headless `std`-only engine crate can run in WASM/CLI without the CAD host. |

## Non-goals

- Signature verification (installing a plugin runs native code; the user trusts
  the repos they install from). Process isolation limits the blast radius of a
  buggy or malicious plugin, but it is not a security sandbox.
- Cross-toolchain binary compatibility — see [Compatibility](#compatibility--abi).
- Sandboxed scripting (Python/Lua); replacing the `opencadcodec` entity model.

---

## Three layers

```
┌────────────────────────────────────────────────────────────────────┐
│  Layer A — Host (OpenCADStudio)                                     │
│  iced UI · Scene · Document · Undo · Command line                   │
│  Core ribbon tabs: Home, Model, View, … (NOT plugins)              │
│  Generic plugin runtime: discovery, spawn, dispatch                │
└───────────────────────────────┬────────────────────────────────────┘
                                │ IPC over local socket (ocs_plugin_api)
┌───────────────────────────────▼────────────────────────────────────┐
│  Layer B — Plugin process  (external repo, cdylib)                  │
│  host spawns itself in runner mode · Cargo.toml · plugin.toml       │
│  PluginManifest · CadModule ribbon · BuiltinPlugin · export_plugin! │
└───────────────────────────────┬────────────────────────────────────┘
                                │ pure Rust API
┌───────────────────────────────▼────────────────────────────────────┐
│  Layer C — Domain engine crate (optional)                          │
│  hydraulics / COGO / … — `std` only, no iced/opencadcodec              │
└────────────────────────────────────────────────────────────────────┘
```

| Layer | Lives in | May depend on |
|-------|----------|---------------|
| **A — Host** | this repo: `src/`, `crates/ocs_plugin_api` runtime | everything |
| **B — Plugin** | a separate repo (cdylib), spawned by the host in runner mode | `ocs_plugin_api` + optional engine |
| **C — Engine** | the plugin's own crate or crates.io | `std` only (WASM/CLI-capable) |

**Hard rules**

1. The host (`src/plugin/`) imports no plugin code — it only knows the contract.
2. Engine crates import neither `iced`, `opencadcodec`, nor `OpenCADStudio`.
3. A plugin never edits host source; it runs entirely from its own crate.

---

## The contract crate — `ocs_plugin_api`

[`crates/ocs_plugin_api`](../crates/ocs_plugin_api) is the semver-versioned API a
plugin compiles against. Two tiers:

- **Dependency-free core** (default): `PluginManifest` / `ApiVersion` and the
  ribbon vocabulary — `CadModule`, `ToolDef`, `RibbonGroup`, `RibbonItem`,
  `IconKind`, `ModuleEvent`, `StyleKey`. Engine crates and tooling depend on this
  cheaply.
- **`host` feature** (pulls `opencadcodec`): the runtime surface — the `HostApi`
  trait, the `BuiltinPlugin` entry-point trait, the `export_plugin!` macro, and
  the out-of-process plugin runtime (`PluginProcess`, `runner`).

A plugin enables the `host` feature.

### Compatibility from API v14

A plugin built for API v14 or later runs the runner loop in its own code: the
host's runner process loads the library and calls its C entry point
`ocs_plugin_run` (emitted by `export_plugin!`) with the socket name, and the
plugin talks to the host over IPC from there. No Rust type crosses the library
boundary, so **the plugin's compiler and codec commit no longer have to match
the host's**, and `plugin.toml` needs neither `rustc_version` nor
`acadrust_source`.

What both sides still share is the shape of the codec types they exchange.
The host passes its fingerprint of those shapes (`version_info::wire_schema`,
a hash of the embedded type registry) to the runner in `OCS_WIRE_SCHEMA`; a
plugin whose own fingerprint differs stops with a message naming both. Codec
commits that do not change those shapes, which is most of them, leave a
plugin working. Rebuild against a newer `ocs_plugin_api` when the message
says so.

The gates below apply to API v4 to v13 plugins only.

### Build-time ABI metadata (API v4 to v13)

For API v4 to v13 the host records two pieces of build-time metadata that
affect whether a prebuilt plugin can safely be loaded:

- `acadrust_source` — the exact git source of the `opencadcodec` crate the host was
  built against.
- `rustc_version` — the output of `rustc --version` for the compiler that built
  the host.

A plugin's `plugin.toml` declares the same values under `[opencad]`. For API v4
and later, `rustc_version` is required and the host refuses to load the plugin
unless it matches exactly — omitting it is treated as a mismatch, not skipped.
This turns the current silent runner crash into a one-line diagnosis such as:

```
Plugin built with rustc 1.96.0, host requires 1.98.0 - rebuild required
```

`acadrust_source` is also required for API v4 and later. Missing, empty, or
mismatched metadata makes the package incompatible.

### `PluginManifest`

```rust
pub struct PluginManifest {
    pub id: &'static str,              // reverse-DNS: "opencad.example"
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub api_version: ApiVersion,       // host ABI major; must match the host
    pub ribbon_order: i32,             // sort key among add-on tabs
    pub xdata_apps: &'static [&'static str],
    pub command_prefixes: &'static [&'static str],
}
```

### `BuiltinPlugin` — the entry point

```rust
pub trait BuiltinPlugin: Send + Sync {
    fn manifest(&self) -> &'static PluginManifest;
    fn ribbon(&self) -> Box<dyn CadModule>;            // the ribbon tab
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool;
    fn on_load(&mut self, host: &mut dyn HostApi) {}
    fn entity_classes(&self) -> Option<&'static EntityClasses> {} // API v10
    fn on_custom_grip_event(                                           // API v11
        &mut self,
        host: &mut dyn HostApi,
        event: CustomGripEvent,
    ) -> CustomGripResponse {}
}
```

### `HostApi` — the plugin-facing runtime surface

`dispatch` receives `&mut dyn HostApi`, so a plugin never touches the host's
concrete types:

| Category | Methods |
|----------|---------|
| Document | `document()` / `document_mut()` return a **local cached copy** of the document (API v3); the first access in a dispatch clones the full `Document` over IPC. Use `add_entity()` / `write_record()` for host-visible mutations. `add_entity()`, `bump_geometry()` |
| Query (V14) | `query_entities(json)` returns one page (`EntityPage { entities, count, next_offset }`) of the entities matching a control-API `query` filter: `layer` with `*`/`?` wildcards, `type`, `bounds`, `xdata_app`, `where`, `offset`/`limit`. One IPC message carries at most 64 MiB, so size pages and write batches below it. |
| XDATA | `read_record(handle, app)`, `write_record(handle, record)`, `remove_record(handle, app)` — keyed by entity handle; `write_record` registers the APPID so data round-trips through DWG/DXF |
| Tab state | object-safe `plugin_state_any*` helpers exist for in-process use; out-of-process plugins should keep state inside the plugin crate because `dyn Any` is not serializable |
| Command line | `push_info`, `push_output`, `push_error` |
| Undo / dirty | `push_undo`, `set_dirty`; `begin_undo_group(label)` / `end_undo_group()` (V14) make one undo step of edits spread over several messages, worker-thread writes included |
| Tab | `tab_index()` |
| Document identity (V5) | `document_path(tab_id)` returns the saved path for a document tab. |
| Notifications (V4) | `on_notification` receives `HostNotification::SelectionChangedV4 { tab_id, handles }` when the active tab's selection changes, plus `DocumentChangedV4` / `DocumentTabClosed`. |

### `export_plugin!` — the C-ABI export

```rust
ocs_plugin_api::export_plugin!(MyPlugin);
```

emits the two symbols the loader looks for:

- `ocs_plugin_api_version() -> u32` — checked **first**, so an API-incompatible
  build never runs its code.
- `ocs_plugin_register() -> *mut Box<dyn BuiltinPlugin>` — constructs the plugin
  and hands ownership to the host.

---

## Writing a plugin

A plugin is a standalone crate that builds a `cdylib`:

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
ocs_plugin_api = { git = "https://github.com/HakanSeven12/OpenCADStudio", features = ["host"] }

# Match the host's opencadcodec so the loaded library is binary-compatible.
[patch.crates-io]
opencadcodec = { git = "https://github.com/HakanSeven12/opencadcodec", branch = "main" }
```

Your release build must also use the same `rustc` as the host. Record it in
`plugin.toml` (see below) and pin the toolchain in your CI matrix so every
published asset matches the host's compiler.

```rust
// src/lib.rs
use ocs_plugin_api::host::{BuiltinPlugin, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

static MANIFEST: PluginManifest = PluginManifest {
    id: "opencad.example", name: "Example Plugin", version: "0.1.0",
    description: "…", api_version: ApiVersion::CURRENT,
    ribbon_order: 50, xdata_apps: &[], command_prefixes: &["EX_"],
};

struct ExampleModule;
impl CadModule for ExampleModule {
    fn id(&self) -> &'static str { "example" }
    fn title(&self) -> &'static str { "Example" }
    // Returns a slice, not an owned Vec — build once into a static and hand
    // back a reference, rather than rebuilding (and dropping) it every call.
    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| vec![RibbonGroup { title: "Demo", tools: vec![RibbonItem::LargeTool(ToolDef {
            id: "EX_HELLO", label: "Hello", icon: IconKind::Glyph("◆"),
            event: ModuleEvent::Command("EX_HELLO".to_string()),
        })]}]})
    }
}

struct ExamplePlugin;
impl BuiltinPlugin for ExamplePlugin {
    fn manifest(&self) -> &'static PluginManifest { &MANIFEST }
    fn ribbon(&self) -> Box<dyn CadModule> { Box::new(ExampleModule) }
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        match cmd { "EX_HELLO" => { host.push_info("Hello"); true } _ => false }
    }
}

ocs_plugin_api::export_plugin!(ExamplePlugin);
```

```toml
# plugin.toml — shipped beside the binary; values mirror MANIFEST
[plugin]
id = "opencad.example"
name = "Example Plugin"
version = "0.1.0"
description = "…"

[opencad]
api_version = 14
ribbon_order = 50
command_prefixes = ["EX_"]
xdata_apps = []
```

(API v4 to v13 plugins also declare `rustc_version` and `acadrust_source`,
filled at release-build time.)

The full, buildable scaffold is in [`docs/plugin-template/`](plugin-template);
the live reference is the
[`opencad-example-plugin`](https://github.com/HakanSeven12/opencad-example-plugin)
repository.

### Commands

A plugin owns its `command_prefixes` (e.g. `EX_`). The host's command router
calls `try_dispatch` first; a returning `true` consumes the command. A plugin
tool fires `ModuleEvent::Command("EX_FOO")`, which round-trips to
`dispatch(host, "EX_FOO")`.

`ModuleEvent::PluginFileDialog { command, title, filter_name, extensions }` lets a
tool request a native file picker; on selection the host dispatches
`"<command> <path>"` back with the path's original case preserved.

A leading `>` or the persistent literal-input toggle preserves both spaces
and case. Automation and `ModuleEvent::PluginFileDialog` already preserve
case without this prefix.

### Interactive (click-to-place) commands

For tools that collect points — placing a structure, drawing a pipe — call
`host.start_interactive(Box::new(my_cmd))` from `dispatch`, where `my_cmd`
implements `ocs_plugin_api::host::InteractiveCommand`:

```rust
fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
    // … return NeedPoint, Commit(entity), CommitAndEnd(entity), Done, or Cancel
}
```

The host drives it through its normal point-collection flow, so the **same**
command works by clicking in the viewport and by feeding coordinates over the
`--serve` automation API (`run "MY_CMD 0,0 10,10"`). This is what bumped the
API to **v2** — the added `HostApi` method changes the contract's vtable, so v1
binaries are refused at load.

To reference **existing** geometry (e.g. connect a pipe between two structures),
set `needs_object_pick() -> true`; the host then calls
`on_object_pick(handle, pt)` with the clicked entity's handle (read its
XDATA/geometry via `HostApi`). Over `--serve` the pick is supplied as a hex
handle: `run "MY_CMD 2F 30"`.

### XDATA — domain persistence

Store domain data on entities as XDATA (under your `xdata_apps` ids), not in a
side database, so it round-trips through DWG/DXF. `write_record` also registers
the APPID. Document your schemas in the plugin's own `PLUGIN.md`.

### Entity class display labels (API v10)

A plugin can give its XDATA-stamped entities human type names in the host UI.
The split between *identity* (in the drawing) and *labels* (in the plugin) is
the point:

- **Identity** — the plugin stamps one string value into an entity's XDATA
  record: `"<class_prefix><KEY>"`, e.g. `"CVIFC-CLASS:LINE"`. The key is
  stable, meaningless to the host, and round-trips the DWG/DXF verbatim.
- **Labels** — the plugin declares a table and exposes it through
  `BuiltinPlugin::entity_classes()`:

```rust
use ocs_plugin_api::manifest::EntityClasses;

static ENTITY_CLASSES: EntityClasses = EntityClasses {
    app_id: "CVIFC",                    // one of the manifest's xdata_apps
    class_prefix: "CVIFC-CLASS:",
    labels: &[
        ("LINE", "Linear Element"),
        ("CIRCULARARC", "Circular Arc Element"),
    ],
};

impl BuiltinPlugin for MyPlugin {
    fn entity_classes(&self) -> Option<&'static EntityClasses> {
        Some(&ENTITY_CLASSES)
    }
    // ...
}
```

The host resolves them at display time only: the single-entity Properties
panel title becomes `"Linear Element (Line)"`, and the records/automation
`display_type` field takes the same form. The rule is display-only — a label
never governs behavior, and the raw schema fields (`type`, `dxf_name`,
`record_type`, the command-line `LIST` output) stay native so existing scripts
and filters keep working.

**Gating.** Declarations are consulted through the live plugin manager, never
cached: a plugin that is not installed, is disabled, is excluded by
`OCS_PLUGIN_MAX_API_VERSION`, failed at load, has been removed, or whose
process has died contributes nothing — its entities display as their bare
native type ("Line"), immediately, with no re-open needed. An `app_id` claimed
by more than one loaded plugin resolves through the first-loaded declaration;
later duplicates are ignored (debug-logged). A class key that is empty,
missing from the table, or mapped to an empty label falls back to the native
type. Because labels are display text resolved from the loaded plugin, they
are plugin-localizable and fixable without rewriting drawings — and
`plugin.toml` (the pre-load discovery file) deliberately does not carry them.

### Plugin grip surface (API v11, hardened in v12)

Three additive surfaces, one rule: **plugin DECLARED, plugin OWNED**. The host
renders, hit-tests and drags plugin-declared grip points, but it never
interprets them — no entity change, constraint or record of its own ever
follows from a custom grip. All semantics live in the declaring plugin.

**1. Reading built-in grips.** `HostApi::entity_grips(handle)` enumerates an
entity's grips with full viewport parity — the same per-entity enumeration the
selection cache is built from (`shape` and field semantics match the
automation `grips` read; `moves_entity` is the translate-grip flag). It is
generic over every entity type: an arc answers its center / endpoints / its
mid-arc radius grip, a spline its control points, and so on. Reference-only
entities and locked layers answer an empty list — the same answer the
viewport gives.

**2. Programmatic grip edit.** `HostApi::grip_drag(handle, grip_id, to)`
performs the same edit an interactive drag performs: the entity-typed grip
modes, snaps/ortho/polar off for the move, exactly one undo step labeled like
the interactive grip step ("GRIP"), byte-exact restore on undo, and the
interactive path's refusals (unknown entity/grip, reference-only, locked
layer, point out of view). The viewport, the automation `grip_drag` op and
this method all drive ONE shared pipeline.

**3. Plugin-declared custom grip points.**
`HostApi::set_plugin_grips(plugin_id, grips)` declares (or, with an empty
batch, clears) the calling plugin's grips: `grip_id` (a plugin-chosen stable
id), `anchor` (any live entity handle — ownership scoping and hit-test
legibility only), `point` (**absolute world** — the host never moves, offsets
or auto-follows it when the anchor entity moves), `axis` (an optional
hard-constrained drag direction), and `shape`. The declaration lifecycle:

- **Session-only display state.** Nothing of a custom grip is persisted in
  the drawing file; the plugin re-declares after its own notification
  boundaries (the pull model — `on_notification` marks the safe refresh
  points). Whole batches REPLACE the plugin's previous set, applied as a
  diff keyed by `grip_id`: an unchanged re-declare costs nothing.
- **The one-undo-step contract.** Drags ride the viewport's existing grip
  drag lifecycle: press engages, the cursor streams move events, mouse-up
  settles, Esc cancels. Move events are coalesced (at most one in flight,
  superseded positions dropped); the plugin answers each with a
  `CustomGripResponse` whose `preview` REPLACES its live preview wires — the
  only feedback the host shows (no host ghost; the anchor entity never
  moves by host action mid-drag). The settle event carries the final point
  and gives the plugin its ONLY write window: from inside
  `on_custom_grip_event` the plugin can read the document and write —
  including opening its own `push_undo` scope — and a settled drag totals
  exactly the plugin's one labeled undo step. The host records nothing of
  its own; cancel means zero writes, zero undo steps, byte-identical
  drawing. Every callback is panic-guarded: a buggy handler is contained and
  reported, never fatal.
- **Reference-only and locking.** Custom grips never render on reference-only
  designated entities, never mutate entities through built-in paths, and
  never bypass layer locks — the plugin's own settle writes go through the
  same HostApi as any other plugin write. A grip coincident with
  click-through (reference-only) geometry stays fully hittable and wins the
  hit, because grips are tested before any wire pick.
- **Visibility.** Always visible while the declaring plugin is loaded, alive
  and not disabled (the same live-manager gating as the entity-class
  labels), with hover highlighting through the same overlay the built-in
  grips use, in a distinct accent. Death/unload removes the grip — and
  nothing else. Hit-testing scales through a per-tab spatial grid with
  viewport culling (never a linear scan per cursor move); per-plugin and
  per-tab caps (4 096 / 16 384) keep the budget machinery honest — custom
  grips are not subject to the selection grip budget.
- **Ownership.** Over the plugin IPC the host attaches the speaking
  process's own manifest id and refuses mismatches, so a plugin cannot
  declare grips in another plugin's name — and only the declaring plugin
  ever receives its grips' events. The declaration requires the V4
  transport (the settle's nested writes ride the multiplexed link).
- **Live geometry (v11.2).** A begin/move answer may also carry
  `live_entities: Vec<(handle, entity)>` — the plugin-directed visual
  channel, computed by the plugin exactly as the settle will commit it.
  The host swaps those entities in through the same visual-only semantics
  its own grip drags show: the elements visibly move, ZERO history steps
  mid-drag, the originals are restored byte-identically before the settle,
  and a cancel discards the phase untouched. The payload is capped at 256
  entities per response (a larger set is refused with an error line); a
  response carrying only `preview` behaves exactly as before.
- **A plugin can never freeze the host (v11.1).** Every delivery — begin,
  move, settle, cancel — is issued WITHOUT waiting and collected at pump
  ticks (coalescing unchanged: at most one move in flight, newest point
  delivered next). Nested COMMAND frames (`run_command` /
  `execute_command`) are REFUSED inside grip deliveries with an honest
  `Err`: their execution would need the host's message loop, which a
  delivery on the UI thread cannot pump — the freeze the consumer's
  settle-replay wedge exhibited. Every other nested request (reads,
  `push_undo`, direct writes, `entities_batch`) keeps its full inline
  serving, so the settle callback stays the plugin's complete write
  window. A delivery that outlives its settle deadline is abandoned: the
  plugin's connection is torn down, any partially-open scope is closed
  (no dangling history), the drag ends as a cancel, and an error line
  names the plugin. The UI loop keeps painting throughout — it never
  waits on plugin answers.
- **Cheap refresh reads (v11.1).** `entities_batch(&[handle])` answers
  live per-entity reads so a write-then-reread refresh pass never re-clones
  the invalidated full-document snapshot (the per-boundary quadratic the
  consumer's live PI-marker pass hit). Prefer it in every refresh path.
- **Live geometry never re-solves constraints (v13).** The visual phase
  swaps entities through the restyled bump policy: no parametric solve, no
  associative re-resolve — the plugin returns every affected entity
  (markers included) exactly as the settle will commit, and bonded
  partners never move by host action mid-drag. The consumer's captured
  defect (radius 15 → 14.39 mid-drag, then its own settle drift gate
  refused) is structurally gone; the elements follow the drag again.
- **Constraints as HostApi (v13).** `constraint_create(PluginConstraint)`
  creates coincident/tangent constraints through the interactive commands'
  own machinery — no command line, no prompt echo, the solver behaves
  identically — and joins the caller's open `push_undo` scope like every
  other write (the driver's `begin_undo` no longer closes a plugin scope:
  a re-bond composed of writes + constraints + command replays is the
  plugin's ONE labeled step). `CommandRequest::RunQuiet` is the interim
  twin for command replays that must stay silent: the same machinery as
  `Run`, with the command-line echo suppressed (errors still surface).
- **Deterministic notification attach (v13).** The runner acks
  `PluginNotification::ChannelReady` at main-loop entry; the host defers a
  process's boot-window broadcasts until the ack and flushes them then
  (a grace window covers older runners that never ack, logged once). A
  fresh boot's `DocumentChanged` can no longer race the runner's startup
  and vanish — the silent 4-in-5 attach flake is closed by the same
  determinism the dispatch table already has.
- **Dirty regions (v13).** `DocumentChangedRegions` rides every boundary
  notification next to `DocumentChanged`: the touched entity handles and
  (entity, app) record pairs since the previous boundary. Boundary work is
  O(changed), not O(store); empty sets mean "too much changed / unknown —
  walk everything" (an honest degradation, never a wrong claim).
- **Batched record walks (v13).** `read_xrecords`, `update_xrecords` and
  `list_xrecords_with_entries` collapse the list-then-read/write-per-record
  store walk into one frame per pass — the record-carrier twin of
  `entities_batch`. With dirty regions, a boundary pass is 2–3 frames
  instead of dozens.

**MCP parity.** Plugin-declared grips appear in the automation `grips` read
(each entry marked with its owning `plugin`), and `grip_drag` takes a `plugin`
parameter to drag one: `{"op":"grip_drag","plugin":"opencad.civil_ifc",
"grip_id":7,"point":[10,0,0]}` routes through the same live machinery — the
consumer can drive declare → read → drag → settle end-to-end. MCP stays a
thin view over the same internals; plugins themselves speak HostApi and IPC
frames only, never the automation bridge.

**Custom grips never carry host-side semantics.** The host stores a point, a
shape and an axis — nothing else. What a grip *means* (an alignment's implied
PI, a pipe endpoint, a widening handle) is entirely the declaring plugin's,
resolved in its own `on_custom_grip_event` from the event's grip id and point.

## Derived display, parametric chains, and plugin class objects (API v14)

The parametric display split established the pattern the class objects
generalize. One law governs both: **the record layer is the file's only
truth, the host derives the display, and the plugins never run the
derivation math.**

**Parametric chains (v14, the v2 shapes).** `stage_parametric_chain(reference, chain,
tolerance)` carries the record layer's declared parameters only — curve
definitions (`length`, the driving-quantity `ramp`, optional `cross_slope`
data) plus the continuity fields (start point, start direction radians,
±2π-legal) — never evaluated geometry. The driving quantity is the plan
curvature in the plan plane and the profile slope in the profile plane, and
the ramp is declared DATA (`Ramp::Polynomial` at most cubic, `Ramp::Piecewise`
of up to six ascending knots in [0,1], or `Ramp::Sampled` of up to 4096
slots — the vocabulary any alignment library restates its own laws with).
The host knows no species and validates the STRUCTURE only; the record-model
consistency checks are the declaring plugin's own. The host evaluates, then
audits seams *at the instance's own declared tolerance* (the evaluated
end of segment i must restate segment i+1's declared start — point,
direction/gradient, where the profile's declared start slope is the next
ramp's own start value — and the same `PluginClassInstance.tolerance`
number bounds the chord carriers: point seams read it as meters, the
plan's bearing seam as radians, the profile's slope seam as the
gradient family); a refusal names the audited pair and changes nothing (no entities,
no undo step). Passing designs stage as exact carriers derived GENERICALLY
(a constant plan curvature as `Line` (zero) or `Arc`; a profile ramp that
is an at-most-quadratic polynomial as an exact clamped Hermite spline of
degree up to 3 — one from-end-states builder whose degree-2 mid control
rides the start-tangent line's mid value, exactly the mean-gradient law) or
as `LwPolyline` chords refined to the declared tolerance (every table law
and every law without an exact generic carrier: the plan law integrates via
a hard-coded 16-node Gauss–Legendre table, the Fresnel series is the tests'
independent oracle). The staged set is **reference-only** (picks pass
through, box selections skip it, no grips/edges; the designation applies
through the same pending-scope machinery as `set_reference_only`) and the
undo composition is the entity-write contract: the plugin's open `push_undo`
scope is joined, otherwise one labeled step ("Stage parametric chain").
`sample_parametric(chain, s)` exposes the same engine as a pure read
(point/direction at chain position *s*; validates every segment per call —
the seam audit belongs to staging, not to the pure read; no
document state, no undo, no publish). Regulations: `MAX_PLUGIN_LIVE_ENTITIES`
caps the staged set; an empty chain refuses; a declared tolerance above
`MAX_DECLARED_TOLERANCE` (1e-2) refuses naming the ceiling — the honors
scheme trusts the plugin's declaration, the ceiling is the host law.
The lane's bench
(`cargo bench --bench parametric_display_staging`) pins the budgets:
50-segment staging far under 5 ms, sampling at sub-µs scale, and the
Sampled-arm reads ride the stage-time cumulative-integral table (a lookup
plus the moved tail's exact trapezoid per re-integration).

**Plugin class objects.** `register_plugin_class(plugin_id,
declaration)` declares a plugin-owned entity class: a stable identity key
(the XDATA class key staged instances carry), a monotone schema version, a
class-scoped grip schema (grip id → the parameter name of the class the grip
drives), and the parameter set lives in the plugin's own record layer. The
registry is tab-scoped display-session state — **the store dies with its
plugin's process** (re-declared per load; nothing persists) — and is keyed
by declaring plugin: the plugin id never travels the wire (the runner
supplies the serving process's own id, the grip anti-impersonation rule).
Re-registration is idempotent for an equal schema and refuses a mismatch: a
schema change is a *re-stage of the instances*, never a hot swap.
`stage_plugin_class_instance(plugin_id, instance)` stages one instance whose
payload is identity only — class id, schema version, layer, `design_record`
linkage — plus a **host-evaluated design descriptor**
(`PluginDisplayDescriptor`): the v14 engine is the first host-known kind
(`ParametricChain`), so a chain design stages through exactly the
audited-seam / cap / refusal / undo contract above; the descriptor enum
appends kinds at the end (an unknown — newer — kind refuses at the contract
instead of guessing). The staged set is reference-only and XDATA-stamped
with the class identity key (the stamp carries the schema version and the
record linkage; `write_record` auto-registers the APPID so DXF/DWG round
trips keep the stamp byte-identical).

**Grip editing on the derived path.** Custom-grip drags stream per-move
geometry when the declaration is unserved (the v11/v12 lane: the plugin
answers `CustomGripEvent`s with `preview` wires and `live_entities`, the
host coalesces moves, settle stays the plugin's one write window).
`CustomGripResponse.descriptor` (a serde-default field — the
`live_entities` precedent) LANDED the descriptor path the branch's drag
arc (the squash of the v15/v16 layers): a Begin answer hands the host a
live *design* plus the per-move tolerance, and when the whole declaration
is serviceable (the profile chain, the END-side drive row, the segment in
range, the class stamp resolving a non-empty display set) the host
re-derives the display set LOCALLY per coalesced cursor move along the
record model's own laws — **zero Move frames crossing the IPC**; a
refused move keeps the last good design with one headline, and every
unserved declaration keeps the streaming path. Measured on the bench's
grip-edit section: the per-move re-derivation (a clothoid's Fresnel
re-integration + chord refinement per move) sits in the tens of
microseconds against a 16.7 ms frame (the staging bench's grip-edit
section measures it: ~18 µs/move for the declared-ramp carrier, sub-µs
for the bezier-carrier cubic), so host-local re-derivation wins on both
latency and traffic. The
drag's settle keeps the plugin's one write window: the byte-identical
restore of the live set BEFORE the settle lands, records first, then the
re-stage — exactly one plugin-labeled undo step. Editing class parameters
travels the records, not the descriptor: a parameter write goes through
`update_xrecords`, the plugin sees the record-dirty notification and
re-stages from its own schema. The grip-edit correctness contract is
pinned engine-side and session-side (no new code — the tests): a grip
drag revises one DECLARED parameter, the plugin's honest re-declare
propagates the moved joint from `sample_parametric` (never a duplicated
constant), the audited seam law holds on the edited chain, and the
display set re-applies over the SAME handles in place — count-stable,
stamps and reference-only designations intact, one undo step
round-tripping the original. The session pair crosses the wire where the
reader is concerned: `PluginClassInstanceProperties` frames (additive,
API v14) serve a plugin's read, and the python `ocs` surface names it
`plugin_class_instance_properties`.

**Class parameter schema and the properties pair.** A declaration can
now declare its parameters in the schema (`PluginClassParameter`: name +
value kind — `Real`/`Integer`/`Text`, appended at the end — + `editable`;
serde default, so declarations without the field read as an empty schema;
empty/duplicate names refuse at registration). The declaration says WHAT
the parameters are; the values stay on the instance's design record row —
the record layer keeps the truth, the schema only makes it addressable. Two
session methods ride the same core the host's properties panel uses:
`plugin_class_instance_properties(handle)` reads a stamped entity's
declared rows (the design record resolves under the NOD root, then one
sub-dictionary level — the identity stamp names the record, never its
path), and `update_plugin_class_instance_parameter(handle, name, value)`
writes ONE declared, editable parameter's (name, value) pair in place: the
layout is alternating (String name, value) entries — the pair layout the
declaration owns — the value entry's group code and everything else on the
record stay untouched, the write composes as one undo step through the
existing XRecord update path, and the record's dirty notification asks the
declaring plugin to re-stage. Refusals — unknown name, read-only row, an
unparseable or non-finite value, a stale stamp, an unresolvable record —
happen before any change and record nothing. The properties panel (a user
surface, not a plugin call) reuses the core for selections whose XDATA
carries a registered class key: a "Plugin class parameters" section
renders the declared rows and the owning-record source line; an editable
row commits through the same one-undo-step write; host-staged
(reference-only) instances render read-only; the host never invents
semantics for a name or a value kind the declaration does not spell.

**Ramps as declared data — the generic kernel (v2).** The host carries no
per-species math wall: the plan's transcendental path is one generic
Gauss–Legendre executor over the declared ramp, and the three ramp arms are
the whole vocabulary — every alignment library's laws restate into them as
DATA (the civil-ifc plugin restates the IFC blend lexics: the spirals as
linear/polynomial curvature ramps, the cosine/sine/bloss/helmert/viennese
half-waves as dense sampled or low-degree polynomial tables, bespoke blend
shapes as piecewise tables; the record-model consistency checks for those
readings are the plugin's own). The driving ramp IS the law: kappa = ramp
for the plan plane, gradient = ramp for the profile; a law's deflection is
its own ramp's integral W(1), nothing normalized behind the declaration.
Bank/cant corrections do not bend the plane curve — the secondary sheet
rides along as declared `cross_slope` data. New blend shapes — the cant
family, custom transitions — join as declared data, never as per-shape
host code.

**The descriptor-backed grip drag.** The drag pipeline's host-local tier
(api 14): a class's grip schema may declare a drive —
`PluginGripSchemaEntry.drives`, a `PluginGripAnchor` naming the segment,
its end and the record-model anchor law the grip re-anchors — and the
Begin answer may hand the host the live design plus the same
tolerance the plugin staged its instance with. When the whole declaration
is serviceable (the profile chain, the END side), the host re-derives the
display set LOCALLY per coalesced cursor move through the declared anchor
law's own generic solve: the ramp's end-sensitive value re-derives per its
arm's closed integral gain, the EndCurvatureScale route bisects with the
same 16-doubling + 60-bisect discipline, the downstream joints re-declare
from the engine's own sampling (each arm's shape preserved), and the
chain's own audit gates every move's design. No Move frames cross
the IPC; the per-move restage patches the staged handles in place
through the visual-only dynamic phase (originals stashed once, zero
history steps, the byte-identical restore before the settle); the
plugin's settle callback stays the only write window — records first,
then the re-stage, exactly one plugin-labeled undo step. Unserved
declarations (the plan plane, the START side, an out-of-range segment, a
missing tolerance) fall back to the v12 streaming path with one headline
in the error queue; older hosts read the new fields as the serde
defaults and never change behavior.

## HostApi vs MCP — the two surfaces

The host exposes exactly two programmatic surfaces, and they are separate
protocols over one shared pipeline: plugins speak **HostApi** (in-process or
over the plugin IPC frames) and **never** the automation bridge; automation
clients speak **MCP** (sessions, `session_id`, `request_id`) and never the
plugin transport. Where both touch the same machinery — grip enumeration, the
drag apply, hit-testing, the preview channel — they call the same internals,
so their answers agree structurally (a drift test pins the grip read).

Which surface carries what:

| Capability | MCP op | HostApi |
|---|---|---|
| Built-in grip read / drag | `grips` / `grip_drag{handle,…}` | `entity_grips` / `grip_drag` |
| Custom grip read / drag | `grips` → `plugin_grips` / `grip_drag{plugin,…}` | via `set_plugin_grips` + `on_custom_grip_event` |
| Custom grip declaration | — (a view; only the owner declares) | `set_plugin_grips` |
| Entity CRUD, blocks, text | `entities_*`, `block_*`, `text_replace` | `add/update/remove_entity`, `solid/table_operation` |
| Selection | `select` (handles **and** type/vertex modes), selection sets | `selection` / `set_selection` (explicit handles) |
| Undo | `undo`, `redo` — drives the user's stack | `push_undo` scopes, `update_entities_transaction` |
| Commands | `run`, `start`, `input`, `cancel` | `run_command` (incl. the echo-free `RunQuiet`) |
| Constraints | only command replays (with echo) | `constraint_create` (no command line, joins the caller's scope) |
| Entity XDATA | `xdata_get` / `xdata_set` | `read_record` / `write_record` / `remove_record` |
| NOD XRecords | — | `create/read/update/delete/list_xrecord` + the v13 batches |
| Bulk data | `batch` (batched **ops**) | `entities_batch`, record batches (batched **data**) |
| Notifications | `events` (the session journal) | `on_notification` (`DocumentChanged`, `DocumentChangedRegions`, …) |
| Viewport automation | `click`, `pointer`, `getpoint`, `user_select`, `view_focus`, `capture` | — (a plugin's grip events are its gestures) |
| Reference-only designation | — | `set_reference_only` |
| Parametric chain staging / sampling (v14) | — (`query` reads the staged entities) | `stage_parametric_chain` / `sample_parametric` |
| Plugin class objects | — (future: a class/instance browser view) | `register_plugin_class` / `stage_plugin_class_instance`; the instance's declared rows read across the wire (`plugin_class_instance_properties`, the python `ocs` surface names it too) |
| Files | `new/open/save/save_verified/wblock/plot` | `document_path`, tab lifecycle |

Semantic traps worth knowing before writing a consumer on either side:

- **MCP `records` is not records.** It is the automation session's operation
  journal; entity data lives in `entities`/`query`/`properties`/`xdata_get`.
  A plugin's NOD XRecords (its real carrier) are HostApi-only — automation
  must assert them through undo/audit/file, not direct reads.
- **Undo is deliberately asymmetric.** Automation drives the user's stack
  (`undo`/`redo`); plugins compose labeled `push_undo` scopes. A settled
  custom-grip drag totals exactly the plugin's one labeled step, and a later
  MCP `undo` pops it like any user step.
- **`grip_id` namespaces differ but are labeled.** Built-ins are entity-local
  indices; custom grips are plugin-chosen ids routed by `(plugin, id)`. The
  MCP payload marks the owner so the two can never be confused.
- **Selection scoping differs.** The MCP `grips` read reflects the current
  selection's cached grips; `entity_grips` answers per handle, generically
  over every entity type (boxed variants included), no selection needed.
- **The never-interpret rule holds on both surfaces.** Even where MCP touches
  custom grips it only routes; the sole writer of meaning is the owning
  plugin, at settle, in its own scope.

---

## Building & distribution

Build per platform and publish to **GitHub Releases**:

```
cargo build --release        # → target/release/lib<crate>.so | <crate>.dll | lib<crate>.dylib
```

A release attaches one binary per platform plus `plugin.toml`, with the platform
in the asset name so the host can pick the right one:

```
opencad.example-linux-x86_64.so
opencad.example-windows-x86_64.dll
opencad.example-macos-aarch64.dylib
plugin.toml
```

A GitHub Actions matrix workflow (see the example repo / template) cross-builds
and uploads these on a `v*` tag. It also fills `plugin.toml`'s `rustc_version`
and `acadrust_source` placeholders from the CI run's actual toolchain and
resolved dependency, rather than hand-editing them per release — see
`docs/plugin-template/.github/workflows/release.yml` for the exact `sed`
substitutions, including reading `acadrust_source` out of `Cargo.lock`'s
resolved `opencadcodec` package entry (`source = "git+...#<hash>"`) since that's
the same value the host embeds at its own build time. The template's CI pins
`rust-toolchain@stable`, which is a floating target — a real release pipeline
should pin the exact toolchain version the host was built with (check
`crates/ocs_plugin_api/src/version_info.rs`'s embedded value, or the host's
own CI config) rather than trusting "whatever's current stable today" to
keep matching it.

---

## Loading

On startup the host scans `<config>/OpenCADStudio/plugins/<id>/` for a
`plugin.toml` + native library (`src/plugin/external.rs`):

```
<config>/OpenCADStudio/plugins/
  opencad.example/
    plugin.toml
    libocs_example_plugin.so      # any name with the platform extension
```

For each compatible package the host spawns **itself** in runner mode
(`--ocs-plugin-runner <socket> <cdylib>`). The child process loads the `cdylib`
in its own address space and connects back to the host over an `interprocess`
local socket. The runner checks `ocs_plugin_api_version` and refuses on
mismatch, then calls `ocs_plugin_register` to obtain the boxed `BuiltinPlugin`.
Each plugin runs in a separate OS process, so a plugin crash or memory
corruption cannot affect the host or other plugins. Plugin processes stay
**resident for the session**; external plugins merge into the same ribbon and
`try_dispatch` path the host uses and honour the enable/disable set
(`disabled_plugins` in `settings.txt`).

Two timeouts protect the host from a stuck runner:

| Timeout | Env var | Default | Floor |
|---|---|---|---|
| Spawn (connection) | `OCS_PLUGIN_SPAWN_TIMEOUT_SECS` | 30 s | — |
| Per-call | `OCS_PLUGIN_CALL_TIMEOUT_SECS` | 30 s | `GetManifest`/`GetRibbon` ≥ 5 s, `Dispatch` ≥ 10 s, interactive events/prompt/pick ≥ 2 s |

A call timeout covers the full round-trip, including any nested plugin→host
requests handled inline. When it fires the host kills the runner, marks the
plugin dead, and reports a `CallTimeout` error via the normal plugin error path.

`<config>` is `%APPDATA%` (Windows), `~/Library/Application Support` (macOS), or
`$XDG_CONFIG_HOME` / `~/.config` (Linux).

## Failure handling

| Failure | Behavior |
|---|---|
| Plugin panics | Caught inside the plugin runner child; an error response is returned to the host and stays alive. |
| Plugin crash / hang / malformed message | The host detects a dead process via `try_wait` on the next dispatch or ribbon rebuild; the tab is dropped and an error is logged. |
| Slow or non-responsive call | The per-call timeout (`OCS_PLUGIN_CALL_TIMEOUT_SECS`) fires, kills the runner, and surfaces a `CallTimeout` error. |
| Spawn failure | Reported per-plugin during startup and surfaced in the Plugin Manager / command line. |
| Oversized message | The length-framed transport rejects messages larger than 64 MiB. |

---

## Marketplace

The desktop **Plugin Manager** (`PLUGINS` / `PLUGINMANAGER`, or the Start-page
button) installs plugins from GitHub Releases. In the browser, the same entry
points show a desktop-download notice because marketplace packages are native
dynamic libraries:

- **Curated registry** — [`plugins/registry.json`](../plugins/registry.json) in
  this repo lists discoverable plugins. The host fetches it from `main` at
  runtime and shows each entry under *Available plugins*. To list a plugin, open
  a PR adding `{ "repo", "name", "description" }` (see
  [`plugins/README.md`](../plugins/README.md)); merged PRs reach every user with
  no app update.
- **Manual link** — *Add a repository* (`owner/repo`) for unlisted or private
  dev repos; linked repos persist in `settings.json`.
- **Install / upgrade / reinstall** — pick a release from the dropdown and
  *Install*; the host downloads the platform asset + `plugin.toml` into the
  plugins folder, checking `api_version` first. Reinstalling overwrites and
  clears any stale library; picking a newer release upgrades. Changes take effect
  on the next restart (the running library stays resident).
- **Uninstall** — removes the package folder (effective next restart).
- **Enable/disable** — toggles a loaded plugin's ribbon tab + dispatch without
  uninstalling.

One linked repository is one marketplace entry. A release can carry more
plugins than its own: every `<anything>-<platform>.zip` asset (e.g.
`open-infra-civiel-windows-x86_64.zip`) is a bundle holding one `plugin.toml`
plus library per plugin, at the zip's root or in one folder per plugin, and
*Install* installs them along with the release's own library and
`plugin.toml`. Each plugin keeps its own id, version and API check, and gets
its own entry under the installed plugins. The release's own `plugin.toml`
asset is what the release list reads, so keep it on every release.

---

## Compatibility & ABI

Plugins are loaded as `cdylib`s by a plugin-runner child process. The host spawns
this child from its own executable (`--ocs-plugin-runner` mode), so the runner
and host always share the same `ocs_plugin_api` build. The runner checks
`ocs_plugin_api_version` before any plugin code runs. Each plugin runs in its
own OS process, so a plugin crash or memory-corruption bug cannot take down the
host process itself. The runner loads the plugin library in-process, however,
so API v4+ requires matching `rustc_version` and `acadrust_source` metadata.

A future hardening step is a `#[repr(C)]` vtable (a true C ABI) so binaries built
by any toolchain interoperate — required before trusting prebuilt binaries from
arbitrary build environments.

---

## Roadmap

Done:

- [x] Stable `ocs_plugin_api` crate — dependency-free core + `host` feature
      (`HostApi` / `BuiltinPlugin` / `export_plugin!`).
- [x] Runtime discovery + out-of-process loading (host spawns itself in runner
      mode) with an `api_version` gate and `interprocess` local-socket IPC.
- [x] XDATA helpers, `ModuleEvent::PluginFileDialog`, per-tab plugin state.
- [x] Marketplace — curated registry + manual repo link, install / upgrade /
      reinstall / uninstall, enable/disable.
- [x] Interactive command round-trip over IPC (prompt, point/enter/object-pick).
- [x] Spawn and per-call IPC timeouts so a stuck runner cannot freeze the host.
- [x] External automation (`--mcp` — local stdio MCP/JSON-RPC server for AI
      clients; `--serve` — headless JSON automation server; both drive OCS
      from a separate process without the GUI, per `src/cli.rs`/`src/mcp.rs`/
      `app::serve`). This closes the "drive OCS headless from a process" goal
      of issue #29; this line previously and incorrectly listed it as not yet
      started.
- [x] Batched record walks + dirty regions + deterministic attach (v13).
- [x] Parametric display staging + sampling: ramps declared as data
      (polynomial/piecewise/sampled), audited seams, exact carriers, 
      reference-only derived display (v14; the species vocabulary lives in
      plugins — the v2 reshaping of the v14 envelope).
- [x] Plugin class objects: tab-scoped class registry, host-evaluated design
      descriptors (the chain kind first), XDATA identity stamps (squashed
      into the API v14 arc); the grip-edit descriptor hook rides
      `CustomGripResponse` (inert default).

Landed in this branch:

- [x] Descriptor-backed grip drags: the Begin descriptor consumed per
      coalesced move (zero Move frames; the declared `drives` row — the
      v2 `PluginGripAnchor` with its anchor law — plus the response's
      per-move tolerance engage the profile-chain END re-anchor through
      the generic solver; unserved declarations keep the v12 streaming
      path) — the drag's settle stays the plugin's one write window and
      the session tests mirror the grip-edit restage arc.
- [x] Properties-panel parameter editing for plugin class entities: a
      `PluginClassParameter` schema on the declaration, the panel section
      bound to selected class-stamped entities, edits through the session's
      `update_plugin_class_instance_parameter` (XRecord update; one undo
      step), the plugin re-stages from the record-dirty notification.
- [x] The generic ramp kernel (v2): the ramp arms are the whole declared
      vocabulary (polynomial/piecewise/sampled); the exact carriers derive
      generically (the constant plan classes as Line/Arc, the profile
      polynomial-slope class as the one from-end-states Hermite), table
      laws stage measured chords, the Sampled arm carries the stage-time
      cumulative-integral table, and the plan integrator's swing budget
      (65 536 half-radian sub-intervals) keeps the surface hang-free past
      legal-but-absurd curvature data.

---

## Reference

| Piece | Location |
|-------|----------|
| Contract crate + runtime | [`crates/ocs_plugin_api`](../crates/ocs_plugin_api) |
| Internal API architecture | [`crates/ocs_plugin_api/ARCHITECTURE.md`](../crates/ocs_plugin_api/ARCHITECTURE.md) |
| Plugin runner implementation | [`crates/ocs_plugin_api/src/runner.rs`](../crates/ocs_plugin_api/src/runner.rs) |
| Host spawn logic | [`crates/ocs_plugin_api/src/process.rs`](../crates/ocs_plugin_api/src/process.rs) |
| Host plugin integration | `src/plugin/`, `src/app/plugin_host.rs` |
| Core module registry generator | `build.rs` (writes to `OUT_DIR`, included by `src/modules/registry.rs`) |
| Marketplace + registry | `src/plugin/marketplace.rs`, [`plugins/registry.json`](../plugins/registry.json) |
| Template scaffold | [`docs/plugin-template/`](plugin-template) |
| Live example plugin | [`opencad-example-plugin`](https://github.com/HakanSeven12/opencad-example-plugin) |
