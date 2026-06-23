# Out-of-Process Plugin Architecture — `ocs_plugin_api`

**Status:** Design proposal  
**Scope:** `crates/ocs_plugin_api` only (minimal host wiring, no plugin changes, no new crate)

---

## 1. Problem

The host currently loads external add-ons as `cdylib` libraries into its own process via `libloading`. `panic::catch_unwind` catches Rust panics, but a plugin can still corrupt host memory, segfault, or deadlock the UI thread. The fix is to run each plugin as a separate OS process and mediate all interaction through IPC.

---

## 2. Design

`ocs_plugin_api` becomes a dual-use library:

- **Plugin side:** unchanged API surface (`BuiltinPlugin`, `HostApi`, `CadModule`, `export_plugin!`).
- **Host side:** runtime that spawns plugin processes and handles their IPC requests.

Plugins remain `cdylib`s. The host spawns **itself** in runner mode (`--ocs-plugin-runner <socket> <cdylib>`) to load each cdylib in a child process and bridge to the host over `interprocess::local_socket`. The runner implementation lives inside `ocs_plugin_api` as a library module; no separate helper binary is needed, so the runner and host are always the same build and cannot get out of sync at deployment time.

---

## 3. Constraints

| Constraint | Handling |
|---|---|
| Plugin API unchanged | Trait/type signatures preserved. `document()` / `document_mut()` keep their signatures but return a local cached copy, so `API_VERSION` bumps to 3. |
| Only `ocs_plugin_api` modified | All new code lives here. The host needs only minimal call-site wiring in `src/plugin/external.rs`, `src/plugin/registry.rs`, and `src/app/plugin_host.rs`. |
| No new crate | Runner code lives inside `ocs_plugin_api`; the host executable serves as the runner process. |
| Platform-independent | `interprocess::local_socket` uses named pipes on Windows and Unix domain sockets elsewhere; self-spawning works on every host target. |

---

## 4. Architecture

```text
Host process                          Plugin process
┌─────────────────┐  local socket   ┌─────────────────┐
│ HostSession     │◄───────────────►│ HostApi proxy   │
│ (document, UI)  │  bincode frames │ (sends RPCs)    │
└─────────────────┘                 └─────────────────┘
        ▲                                    │
        │                                    ▼
 PluginManager                          cdylib loaded
 (spawn / kill /                        by host in
  supervise)                             runner mode
```

### 4.1 IPC protocol

All messages are length-framed and serialized with `bincode`.

**Host → plugin:**

- `GetManifest`, `GetRibbon`
- `Dispatch { cmd: String }`
- `InteractiveEvent { command_id, event }`
- `Shutdown`

**Plugin → host:**

- `PushInfo` / `PushOutput` / `PushError`
- `AddEntity(SerializedEntity)` → `Handle`
- `BumpGeometry`, `PushUndo`, `SetDirty`
- `ReadRecord`, `WriteRecord`, `RemoveRecord`
- `StartInteractive`, `PollInteractive`
- `DocumentSnapshot` → `SerializedDocument`

`SerializedEntity`, `SerializedRecord`, and `SerializedDocument` are `acadrust` types with `Serialize` / `Deserialize` derived.

### 4.2 Plugin-side runtime

`OpenCADStudio --ocs-plugin-runner <socket_name> <cdylib_path>`:

1. Loads the cdylib, validates `ocs_plugin_api_version`, calls `ocs_plugin_register`.
2. Connects to the host socket and answers `GetManifest` / `GetRibbon`.
3. Runs a request loop: dispatch commands, forward interactive events.

`PluginHostApi` implements `HostApi` by sending RPCs. `document()` / `document_mut()` return a local copy fetched from `DocumentSnapshot`; mutations are **not** automatically synced back. Plugins use `add_entity`, `write_record`, etc. for host-visible changes.

### 4.3 Host-side runtime

`ocs_plugin_api::process` provides:

- `PluginProcess::spawn(cdylib_path)` — creates a socket, launches the host executable in runner mode, accepts its connection.
- `PluginManager` — spawn all discovered plugins, supervise, kill.
- `serve_plugin_connection(stream, &mut dyn HostApi)` — host-side request handler.

The host creates a `HostSession` and passes it to `serve_plugin_connection`; existing document/undo logic is reused.

### 4.4 Ribbon

Ribbon types use `&'static str`, which cannot cross a socket. Define owned equivalents in `ocs_plugin_api::ribbon::owned` and convert for IPC. The host reconstructs `RibbonGroup` once per load (e.g., by leaking the owned strings), avoiding changes to ribbon rendering code.

### 4.5 Failure handling

| Failure | Behavior |
|---|---|
| Plugin crash / hang / malformed message | Host marks plugin dead, drops its ribbon tab, logs the error, and continues running. |
| Plugin panics | Caught inside the runner; an error response is returned to the host. |
| Spawn failure | Reported through `PluginManager` and shown in the Plugin Manager. |

---

## 5. Host Integration Points

1. **`src/plugin/external.rs`** — replace `libloading`-based `LoadedPlugin` with `PluginProcess::spawn`.
2. **`src/plugin/registry.rs`** — use `PluginProcess` for ribbon collection and command dispatch.
3. **`src/app/plugin_host.rs`** — add an IPC request bridge that maps incoming messages to `HostSession` calls.

No changes to `docs/plugin-template` or any other plugin.

---

## 6. Crate Changes

### 6.1 New files inside `crates/ocs_plugin_api`

```text
src/
  ipc/
    protocol.rs      # HostRequest / PluginRequest / PluginResponse
    transport.rs     # framed read/write over local_socket
    client.rs        # plugin-side IpcClient + PluginHostApi
    server.rs        # host-side serve_plugin_connection
  process.rs         # PluginProcess / PluginManager
  runner.rs          # plugin runner logic invoked by host in runner mode
```

### 6.2 Dependencies

Add under the existing `host` feature:

```toml
[dependencies]
interprocess = { version = "2", optional = true }
serde = { version = "1", features = ["derive"], optional = true }
bincode = { version = "1", optional = true }
thiserror = { version = "1", optional = true }
libloading = { version = "0.8", optional = true }

[features]
host = ["dep:acadrust", "dep:interprocess", "dep:serde", "dep:bincode", "dep:thiserror", "dep:libloading"]
```

---

## 7. API Version

Bump `API_VERSION` to `3` because `document()` / `document_mut()` semantics change from direct host references to local cached copies. v2 plugins are refused as usual; plugin authors recompile with `ApiVersion::CURRENT`.

---

## 8. Implementation Plan

1. Add dependencies to `Cargo.toml`.
2. Implement framed transport and protocol messages.
3. Implement runner logic in `runner.rs`.
4. Implement `PluginHostApi` proxy.
5. Implement host-side server and `PluginManager`.
6. Add owned ribbon conversions.
7. Wire the three host call sites and add `--ocs-plugin-runner` dispatch in `src/main.rs`.
8. Bump `API_VERSION` and update `docs/plugin-architecture.md`.

---

## 9. Testing

- **Unit:** protocol round-trip, ribbon conversion, proxy request emission.
- **Integration:** spawn a test plugin, verify dispatch and interactive command round-trip, kill the process and confirm the host survives.
- **Host:** update registry tests once `LoadedPlugin` is replaced by `PluginProcess`.

---

## 10. Compliance with `AGENT.md`

| Requirement | Status |
|---|---|
| `ocs_plugin_api` is a library, not a plugin | Yes |
| Plugin API source-compatible | Yes; signatures are unchanged. `document()` semantics change is gated by v3. |
| Separate processes + failure management | Yes |
| Platform-independent `interprocess` IPC | Yes |
| Only `ocs_plugin_api` modified | Code: yes. Host needs minimal wiring; unavoidable because `OpenCADStudio` / `HostSession` are host-private. |
| No new crate | Yes |
| Memory / process isolation | Yes |

---

## 11. Summary

`ocs_plugin_api` absorbs the plugin runtime: the host spawns itself in runner mode to load each cdylib in its own process, and all host/plugin interaction is serialized over `interprocess` local sockets. Plugin API signatures stay intact, host changes are limited to a few call sites, and no new crate is introduced.
