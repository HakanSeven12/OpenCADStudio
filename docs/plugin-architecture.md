# Open CAD Studio — Plugin Architecture

**Status:** Accepted
**Author:** Open CAD Studio contributors
**Date:** June 2026

This document is the **authoritative spec** for how add-on packages integrate
with Open CAD Studio. The model follows [QGIS](https://plugins.qgis.org/)-style
extensibility: a small metadata file, a single entry point, an optional separate
engine crate, and user-installable packages from a curated index.

> **Open CAD Studio ships no built-in plugins.** Every add-on is an **external
> dynamic library** (`cdylib`) the host loads at runtime from the user plugins
> folder. The host source only contains the generic plugin *runtime*
> (`src/plugin/`, `src/app/plugin_host.rs`) and the stable contract crate
> (`crates/ocs_plugin_api`). Add-ons live in their own repositories and
> **consume** that contract.

---

## Design goals

| Goal | Rationale |
|------|-----------|
| **One package, one entry point** | Manifest, ribbon tab and commands ship together in the plugin crate; no edits to the host. |
| **Stable contract** | Authors target the semver-versioned `ocs_plugin_api` crate, not `OpenCADStudio` internals. |
| **Out-of-tree by default** | A plugin is its own repo + crate; the host never recompiles to gain one. |
| **DWG round-trip** | Domain data lives on entities as XDATA, not in an opaque side database. |
| **Engine reuse** | A headless `std`-only engine crate can run in WASM/CLI without the CAD host. |

## Non-goals

- Signature verification (installing a plugin runs native code; the user trusts
  the repos they install from). Process isolation limits the blast radius of a
  buggy or malicious plugin, but it is not a security sandbox.
- Cross-toolchain binary compatibility — see [Compatibility](#compatibility--abi).
- Sandboxed scripting (Python/Lua); replacing the `acadrust` entity model.

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
│  hydraulics / COGO / … — `std` only, no iced/acadrust              │
└────────────────────────────────────────────────────────────────────┘
```

| Layer | Lives in | May depend on |
|-------|----------|---------------|
| **A — Host** | this repo: `src/`, `crates/ocs_plugin_api` runtime | everything |
| **B — Plugin** | a separate repo (cdylib), spawned by the host in runner mode | `ocs_plugin_api` + optional engine |
| **C — Engine** | the plugin's own crate or crates.io | `std` only (WASM/CLI-capable) |

**Hard rules**

1. The host (`src/plugin/`) imports no plugin code — it only knows the contract.
2. Engine crates import neither `iced`, `acadrust`, nor `OpenCADStudio`.
3. A plugin never edits host source; it runs entirely from its own crate.

---

## The contract crate — `ocs_plugin_api`

[`crates/ocs_plugin_api`](../crates/ocs_plugin_api) is the semver-versioned API a
plugin compiles against. Two tiers:

- **Dependency-free core** (default): `PluginManifest` / `ApiVersion` and the
  ribbon vocabulary — `CadModule`, `ToolDef`, `RibbonGroup`, `RibbonItem`,
  `IconKind`, `ModuleEvent`, `StyleKey`. Engine crates and tooling depend on this
  cheaply.
- **`host` feature** (pulls `acadrust`): the runtime surface — the `HostApi`
  trait, the `BuiltinPlugin` entry-point trait, the `export_plugin!` macro, and
  the out-of-process plugin runtime (`PluginProcess`, `runner`).

A plugin enables the `host` feature.

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
}
```

### `HostApi` — the plugin-facing runtime surface

`dispatch` receives `&mut dyn HostApi`, so a plugin never touches the host's
concrete types:

| Category | Methods |
|----------|---------|
| Document | `document()` / `document_mut()` return a **local cached copy** of the document (API v3); the first access in a dispatch clones the full `CadDocument` over IPC. Use `add_entity()` / `write_record()` for host-visible mutations. `add_entity()`, `bump_geometry()` |
| XDATA | `read_record(handle, app)`, `write_record(handle, record)`, `remove_record(handle, app)` — keyed by entity handle; `write_record` registers the APPID so data round-trips through DWG/DXF |
| Tab state | object-safe `plugin_state_any*` helpers exist for in-process use; out-of-process plugins should keep state inside the plugin crate because `dyn Any` is not serializable |
| Command line | `push_info`, `push_output`, `push_error` |
| Undo / dirty | `push_undo`, `set_dirty` |
| Tab | `tab_index()` |

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

# Match the host's acadrust so the loaded library is binary-compatible.
[patch.crates-io]
acadrust = { git = "https://github.com/HakanSeven12/acadrust", branch = "main" }
```

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
    fn ribbon_groups(&self) -> Vec<RibbonGroup> {
        vec![RibbonGroup { title: "Demo", tools: vec![RibbonItem::LargeTool(ToolDef {
            id: "EX_HELLO", label: "Hello", icon: IconKind::Glyph("◆"),
            event: ModuleEvent::Command("EX_HELLO".to_string()),
        })]}]
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
api_version = 3
ribbon_order = 50
command_prefixes = ["EX_"]
xdata_apps = []
```

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
and uploads these on a `v*` tag.

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

`<config>` is `%APPDATA%` (Windows), `~/Library/Application Support` (macOS), or
`$XDG_CONFIG_HOME` / `~/.config` (Linux).

## Failure handling

| Failure | Behavior |
|---|---|
| Plugin panics | Caught inside the plugin runner child; an error response is returned to the host and stays alive. |
| Plugin crash / hang / malformed message | The host detects a dead process via `try_wait` on the next dispatch or ribbon rebuild; the tab is dropped and an error is logged. |
| Spawn failure | Reported per-plugin during startup and surfaced in the Plugin Manager / command line. |
| Oversized message | The length-framed transport rejects messages larger than 64 MiB. |

---

## Marketplace

The **Plugin Manager** (`PLUGINS` / `PLUGINMANAGER`, or the Start-page button)
installs plugins from GitHub Releases:

- **Curated registry** — [`plugins/registry.json`](../plugins/registry.json) in
  this repo lists discoverable plugins. The host fetches it from `main` at
  runtime and shows each entry under *Available plugins*. To list a plugin, open
  a PR adding `{ "repo", "name", "description" }` (see
  [`plugins/README.md`](../plugins/README.md)); merged PRs reach every user with
  no app update.
- **Manual link** — *Add a repository* (`owner/repo`) for unlisted or private
  dev repos; linked repos persist in `settings.txt` (`plugin_repos=`).
- **Install / upgrade / reinstall** — pick a release from the dropdown and
  *Install*; the host downloads the platform asset + `plugin.toml` into the
  plugins folder, checking `api_version` first. Reinstalling overwrites and
  clears any stale library; picking a newer release upgrades. Changes take effect
  on the next restart (the running library stays resident).
- **Uninstall** — removes the package folder (effective next restart).
- **Enable/disable** — toggles a loaded plugin's ribbon tab + dispatch without
  uninstalling.

---

## Compatibility & ABI

Plugins are loaded as `cdylib`s by a plugin-runner child process. The host spawns
this child from its own executable (`--ocs-plugin-runner` mode), so the runner
and host always share the same `ocs_plugin_api` build. The runner checks
`ocs_plugin_api_version` before any plugin code runs. Each plugin runs in its
own OS process, so the host is protected from plugin crashes and memory
corruption. Process isolation removes the need for the host and plugin to share
a Rust toolchain ABI beyond the stable `ocs_plugin_api` contract.

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

Next:

- [ ] Incremental document snapshots instead of cloning `CadDocument` over IPC.
- [ ] `#[repr(C)]` vtable / strict handshake for cross-toolchain binaries.
- [ ] Trust: checksums / signatures before spawning plugin processes.
- [ ] Interchange (LandXML / SWMM) and live `on_entity_committed` hooks.
- [ ] External automation API (drive OCS headless from a process) — issue #29.

---

## Reference

| Piece | Location |
|-------|----------|
| Contract crate + runtime | [`crates/ocs_plugin_api`](../crates/ocs_plugin_api) |
| Plugin runner implementation | [`crates/ocs_plugin_api/src/runner.rs`](../crates/ocs_plugin_api/src/runner.rs) |
| Host spawn logic | [`crates/ocs_plugin_api/src/process.rs`](../crates/ocs_plugin_api/src/process.rs) |
| Host plugin integration | `src/plugin/`, `src/app/plugin_host.rs` |
| Core module registry generator | `build.rs` (writes to `OUT_DIR`, included by `src/modules/registry.rs`) |
| Marketplace + registry | `src/plugin/marketplace.rs`, [`plugins/registry.json`](../plugins/registry.json) |
| Template scaffold | [`docs/plugin-template/`](plugin-template) |
| Live example plugin | [`opencad-example-plugin`](https://github.com/HakanSeven12/opencad-example-plugin) |
