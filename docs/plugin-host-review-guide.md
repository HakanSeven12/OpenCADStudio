# Reviewer guide: scripting host model and the bundled Python plugin

This is written for a reviewer who has not followed the work and may not have a Mac.
It says what the change is, how to check it on Linux or Windows in a few minutes, where
it touches existing code, what to read first, and what is deliberately left out.

## What it is

The bundled `opencad.python` plugin (RustPython, in its own process) can now control OCS
through the versioned plugin API (v7), not only add lines and circles:

- **Entities.** Create, read, edit, delete and undo for all 43 canvas kinds (41 creatable),
  validated by the host, each with a DWG and DXF round-trip audit.
- **Drawing tables.** Layers, text and dimension styles, blocks and their contents,
  linetypes and layouts, and kernel-backed solids.
- **Commands.** Scripts can run OCS's own commands step by step (offset, trim, fillet,
  arrays, PEDIT and so on), with guards.

**The host owns the drawing.** It validates every request, records the undo step and
refuses without changing anything when it cannot honour a request. The Python adapter is a
thin translation layer. The design and its guarantees are in
[plugin-host-model.md](plugin-host-model.md).

**Size.** 72 files, +30,705 / −178, against current `main`. About 15,000 added lines are the
new plugin crate (including a 3,000-line lockfile and a generator), 5,600 are the API crate
(3,500 of them the entity coverage validator), and 8,800 are in `src/`, of which roughly 6,200
are tests in `plugin_host.rs`. Existing OCS code changes by **178 deleted lines in total** (151 of them outside docs, plugin and API);
everything else is additive.

## Check it without a Mac

Nothing here needs macOS. The plugin tests are headless: they build the plugin and the
application, then spawn the application as the plugin runner and talk to it over the real IPC.
`stage-bundled.sh` already handles Linux (`.so`) and Windows (`.dll`), and the host looks for the
plugin in `plugins/opencad.python/` next to the executable.

```bash
bash plugins/opencad-python/tools/stage-bundled.sh work/plugin --debug
cargo build --bin OpenCADStudio
OCS_TEST_PYTHON_PLUGIN=work/plugin/libopencad_python.so \
OCS_PLUGIN_RUNNER_EXE=target/debug/OpenCADStudio \
cargo test --lib -- --test-threads=1 python
```

(`opencad_python.dll` and `OpenCADStudio.exe` on Windows.) Expect 60 passed. **Without the two
environment variables these tests are skipped and still report success**; `python-host-check.yml` fails
instead of skipping.

Already run for you:

| Check | Where | Result |
|---|---|---|
| `python-host-check.yml` on the pull request | Linux and Windows | 115 plugin API tests, 20 plugin tests, 16 Python model tests, 60 host tests over real IPC, on each |
| Release build with the plugin staged (a fork-only workflow, not in this PR) | Linux and Windows | 60 of 60 host tests on each; unsigned portable packages were produced and can be shared on request |
| Local | macOS | the same, plus the wasm check |

`main` now has its own `Tests` workflow (`cargo test --workspace --locked` on Linux), which will also run on
this pull request. Run locally on macOS against the merged tree with the same command and `LC_ALL=C` and
`--no-fail-fast`, the whole workspace gives **1,805 passed and 1 failed**. The one failure,
`scene::text::lff::tests::fonts_parse_and_resolve`, also fails on pristine `main` in that environment (it
needs font data) and is not part of this change; I expect it passes on the Linux runner but have not seen that.

What has **not** been done: launching the GUI on Linux or Windows (the runners have no display), running the
whole workspace suite on Windows, and building an AppImage or MSI that contains the plugin.

## Reading order

Read the contract first, then the host, then the adapter. The commits are in dependency order but
several kinds and features are interleaved, so read by area rather than commit by commit.

1. **The contract: `crates/ocs_plugin_api`.**
   - `src/host.rs`: the new types and default-refusing methods: `SolidOperation`, `TableOperation`,
     `CommandRequest` and `CommandOutcome`, `update_entities_transaction`, `selection`, `set_selection`,
     `solid_operation`, `table_operation`, `run_command`.
   - `src/ipc/**`: one additive request and response variant per method. The new variants are appended
     after the existing ones so existing wire positions do not move. `v4/protocol.rs` has a bincode round-trip
     test for each.
   - `src/entity_coverage.rs`, `entity_coverage_policy.json`, `build.rs`: the per-kind validation and the
     generated coverage catalog. This is the largest file and the most mechanical; the tests in it are the
     rules in executable form.
2. **The host: `src/app/plugin_host.rs`**, lines 1 to about 2,700 (the rest is tests). Sections in order:
   scripted-entity normalization (host-derived state), solid operations, `table_operation` (layers, then
   styles, blocks, linetypes, layouts), `run_command`, and the `HostApi` delegation at the end.
3. **The adapter: `plugins/opencad-python`.** `entity_manifest.json` and `build/generate.rs` generate the
   dict-to-entity conversions from the host's type registry; `src/ocs_module.rs` is the `ocs.*` bindings;
   `src/document_model.py` is the script-facing document model; `PLUGIN.md` is the reference.
4. **The small edits to existing OCS files** (this is where behaviour of existing code could change):

   | File | Change |
   |---|---|
   | `src/plugin/external.rs`, `src/ui/window/plugin_manager.rs` | Bundled-plugin discovery (`bundled_plugins_dir`, `OCS_BUNDLED_PLUGINS_DIR`, a "Bundled with Open CAD Studio" label). A per-user plugin with the same id still overrides the bundled one. |
   | `src/plugin/v4_support.rs`, `src/app/update/mod.rs`, `src/app/automation.rs` | New `DrawingChanged` and `CommandStateChanged` notifications; a drawing-changed notification after automation ops. |
   | `src/entities/{mline,table,helix,dimension}.rs` | `normalize_scripted_*`: derived state a script must not set (MLine geometry, Table merges, Helix spline, Dimension measurement). Called only from the plugin host. |
   | `src/scene/selection.rs`, `src/scene/entity.rs`, `src/app/layers.rs` | `replace_selection_exact`, preserving storage data on entity replacement, `set_current_layer_name`. |
   | `src/app/style_ops.rs`, `src/app/control/mod.rs` | Four methods and one function widened to `pub(super)` so the host reuses OCS's own style rename, delete and in-use logic. |
   | `src/app/settings.rs`, `src/app/mod.rs`, `src/app/update/file.rs`, `src/app/commands/styleprops.rs` | The `SCRIPTCOMMANDS` preference and command (see the decisions below). |
   | `packaging/build_macos_signed.sh`, `.github/workflows/release.yml` | Stage (and, in the signed script, sign) the plugin in the macOS bundle; one added step in `release.yml`. |

## What each area proves

| Area | Evidence |
|---|---|
| 43 entity kinds | One real-IPC audit per kind (create, read, edit two properties, delete, undo and redo, invalid input with an atomic rollback, canvas geometry, DWG and DXF reopen and re-edit). [The ledger](plugin-host-model-coverage-ledger.md) records each kind's exact evidence or its blocker. |
| Solids | Kernel primitives, moves, booleans, extrusions and regions, each verified to lift back from its ACIS payload; refusals leave operands untouched. |
| Tables | `audit_python_layer_table`, `_text_and_dim_styles`, `_blocks`, `_block_contents`, `_linetypes_and_layouts`: dozens of refusals each, live and DWG and DXF persistence, one-step undo. |
| Commands | `audit_python_modify_wrappers`: exact geometry for every wrapper (offset, trim, extend, fillet, chamfer, arrays, explode, join, break, stretch, lengthen, PEDIT edits), guards, an interactive session, undo. |
| Nested input | `audit_python_nested_input_is_validated`: a mistyped vertex key or a missing location is refused instead of becoming a zero coordinate. |
| Guardrail | `script_commands_setting_gates_the_runner` and a settings round-trip test. |

## Decisions that are yours

1. **`run_command` and the automation switch.** Script-driven commands do not consult the automation on/off
   switch, which governs the external MCP and serve channels; a script is something the user chose to run. Say
   if you would rather they respected it.
2. **`SCRIPTCOMMANDS`.** A saved preference (on by default) toggled by a command turns script-driven commands off
   entirely. It has no Options-page control; adding one needs locale and layout work outside this change.
3. **Denylist.** A script cannot run `QUIT`, `EXIT`, `CLOSE*`, `NEW`, `QNEW`, `OPEN`, `SAVE*`, `RECOVER`,
   `SCRIPT`, `RUNSCRIPT`, `SCRIPTCOMMANDS` or any `PY_*`, and only one command may be active. Tell me about any
   other command that should be refused.
4. **Nesting depth.** A command step runs inside the editor's message handling. It is not recursive, but a debug
   build of one step (PEDIT converting a line to a polyline) needs 2 to 4 MiB of stack; the native builds link
   with at least 8 MiB (16 MiB on Windows).
5. **Splitting.** If 79 commits is too much, the natural seams are: the API and host core, the entity kinds,
   the tables, and the command runner. Tell me and I will cut it that way.

## What is deliberately not done

- **Twelve of the 43 kinds are not `Complete`** (see the ledger). Seven are blocked by DXF bugs in
  opencadcodec, whose fixes are submitted as [opencadcodec#48](https://github.com/HakanSeven12/opencadcodec/pull/48) and
  [#51](https://github.com/HakanSeven12/opencadcodec/pull/51). **This change still pins the unfixed opencadcodec revision**,
  so its canary tests assert the old behaviour; once those merge, the pin moves, the canaries flip and those kinds
  can be marked complete. The other five are held by the DWG format, a kind change on DXF save, an image-reactor
  structure that cannot be verified, an update-only decision, and a missing creation path.
- Not available from a script: complex (text and shape) linetypes, plot devices and named page setups, PEDIT's
  fit and spline options, and interactive-only commands such as HATCH.
- Two DXF gaps are not filed upstream because they need the exact XDATA layout AutoCAD expects (the TrueType
  family name and the block description); they are listed in [cadcodec-reader-gaps.md](cadcodec-reader-gaps.md).
- The AppImage, snap and MSI still ship without the plugin: `release.yml` stages it only in the macOS bundle
  (one added step, part of this change). Bundling it for Linux and Windows means adding the same staging step
  (`stage-bundled.sh`) to those jobs, which is small but I could not test the installers here.
