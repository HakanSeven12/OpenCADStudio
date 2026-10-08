# Open CAD Studio — Python Scripting & AutoLISP Migration Plan

**Status:** Phase 1 is done and host-verified, and the general document model that
followed (entities, drawing tables, block contents and the command runner) is described in
[`../../docs/plugin-host-model.md`](../../docs/plugin-host-model.md). Phase 2 (an AutoLISP
translator) is a separate project that is not part of this repository. This file is kept as
the original planning document and Phase 1 build log.

**Author:** Open CAD Studio contributors
**Date:** September 2026

This document plans embedded Python scripting for Open CAD Studio as an
**external plugin** (not a host change), followed by a best-effort AutoLISP →
Python transpiler built on top of it. It supersedes the host-embedded design
sketched in an earlier draft of this document: scripting fits OCS's existing
plugin model better than it fits inside the host. OpenCADStudio's
`docs/plugin-architecture.md` explicitly lists "sandboxed scripting
(Python/Lua)" as a non-goal of the external-plugin *system* itself — this
plan doesn't touch that system, it just builds a plugin that happens to
embed an interpreter, same as any plugin embeds whatever dependency it
needs.

**Why a plugin, not a host feature:**

| | Host-embedded | Plugin (this plan) |
|---|---|---|
| Requires OCS core changes and maintainer sign-off | Yes | No — ships from an independent repo, installed via the existing Plugin Manager |
| Crash isolation | Relies on RustPython's memory safety alone | Gets OS-process isolation for free (plugins already run out-of-process) |
| Distribution | Bundled into every OCS release, forever | Opt-in install, versioned independently, can iterate fast |
| Cost | New maintenance burden on the host | Same ABI-pinning cost every plugin already has (see Phase 1, Risk) |

---

## Phase 1 — Python scripting plugin

**Goal:** a working bundled `opencad-python` plugin (`cdylib`) that embeds
RustPython and exposes an `ocs` module wrapping `HostApi` while retaining the
out-of-process runner boundary.

### 1.1 — Scaffold

- [x] New repo from OpenCADStudio's `docs/plugin-template/`; `plugin.toml`
      + `Cargo.toml` pinning `ocs_plugin_api` (`host` feature) and matching the
      target host's `acadrust_source` / `rustc_version` (see Risks). Scaffolded
      locally at `~/Documents/MacApps/opencad-python` (this repo — no remote
      yet at the time, not pushed anywhere).
- [x] Add RustPython as a dependency; confirm it builds cleanly as part of a
      `cdylib` on **macOS** (arm64) — Windows/Linux still unverified. Used
      crates.io `rustpython-vm = "0.5"` (not the `rustpython` facade crate;
      matches how the upstream `examples/hello_embed.rs` embeds it). Builds
      clean, no warnings: `libopencad_python.dylib`, 17MB, ~80s from a cold
      `cargo clean` release build. Exports the expected
      `ocs_plugin_register` / `ocs_plugin_api_version` C-ABI symbols. A
      `PY_HELLO` command runs a real RustPython smoke test (compiles and
      evaluates `1 + 1` via `Interpreter::without_stdlib`) — confirms the
      interpreter actually runs inside the cdylib, not just that it links.
      One API gap vs. the upstream example: 0.5.0's `Vm::compile` takes an
      owned `String` filename (not `&str`), and `CompileError` has no
      `into_pyexception` method — worked around by matching on the `Result`
      directly instead of using `?` with `PyResult`.

### 1.2 — Minimal `ocs` module (read-only)

- [x] `ocs.selection()` → list of entity handles. Implemented as a cache, not
      a live query — `HostApi` has no synchronous "current selection" call
      (only a best-effort `SelectionChanged`/`SelectionChangedV4`
      notification). Populated via `BuiltinPlugin::on_notification` into a
      process-wide static (`selection_cache.rs`) — see the host-runtime
      verification note below for why `dispatch`-time polling doesn't work.
      Documented as a real limitation in `PLUGIN.md`, not silently papered
      over.
- [x] `ocs.get(handle)` → read-only geometry/property access, via
      `HostApi::document_reader()` (zero-copy) + `for_each_entity`, matching
      by handle. Returns `{handle, kind, layer, point}` or `None`.
- [x] `PY_EVAL <expr>` command (`command_prefixes = ["PY_"]`): evaluate one
      inline expression, print the result via `host.push_output`. Runs a
      fresh, stdlib-free `Interpreter` per call with only the `ocs` native
      module registered (`Interpreter::builder(...).add_native_module(...)`)
      — no `import os`/`socket`/etc. is even possible since no stdlib is
      loaded, which happens to satisfy most of §1.5's sandboxing goal already
      as a side effect of keeping this phase minimal, not a deliberate
      Phase 1.5 pass yet.
- [x] Errors: both compile errors (`CompileError`, which implements
      `Display` directly — no `into_pyexception` needed) and runtime
      exceptions (formatted as `"<TypeName>: <message>"`, no traceback yet)
      go to `host.push_error`.
- [ ] Bridging `host: &mut dyn HostApi` into the `#[pymodule]` native
      functions needed a thread-local raw-pointer scratch slot
      (`host_ctx.rs`) with a documented safety argument, since
      `add_native_module` wants `&'static PyModuleDef` — there's no closure
      capture available to a native pyfunction. Worth a second pair of eyes;
      it's sound as written (guard clears the slot before the real borrow
      ends, single-threaded, non-reentrant) but is the one genuinely
      load-bearing `unsafe` block in the plugin so far.
- [x] Verified: `cargo build`/`--release` clean, zero warnings, against the
      real `ocs_plugin_api`/`rustpython-vm` 0.5.0 APIs (not guessed — traced
      through the actual crate sources for `HostApi`, `DocumentReader`,
      `InterpreterBuilder`, `ToPyObject`/`DictKey` impls, etc.).
- [x] **Host-runtime verified**, not just "compiles": built the actual OCS
      host (debug, ~1m43s), installed the plugin, drove it through the real
      `--mcp` automation API with an isolated `$HOME` (so the test couldn't
      collide with an already-running instance — see below, that mistake cost
      real time). Confirmed working end-to-end:
      - `PY_EVAL 6 * 7` → `"42"`.
      - `PY_EVAL 1 / 0` → `"ZeroDivisionError: division by zero"` via
        `host.push_error` — not a silent no-op.
      - `ocs.get(handle)` on a drawn `LINE` → correct
        `{'handle': 98, 'kind': 'line', 'layer': '0', 'point': None}`.
      - Along the way found and fixed a real bug: `PY_EVAL`'s `ocs` name was
        never bound in scope (scripts would need `import ocs`, impossible
        under `Mode::Eval`, which only accepts one expression). Fixed by
        `vm.import("ocs", 0)` + `scope.globals.set_item` before compiling —
        `ocs` is now pre-bound, no import needed or possible.
      - **Found and fixed a second real bug, this one plugin-side, not the
        host's**: `ocs.selection()` always returned `[]`. Root cause (found by
        temporarily instrumenting the host, then reverted — no host changes
        were needed): `ocs_plugin_api::runner`'s own V4 event loop (`run_v4`
        in `runner.rs`) drains `HostApi::try_recv_notification()` on every
        iteration and forwards each notification to
        `BuiltinPlugin::on_notification` — *before* a `Dispatch` request ever
        reaches `dispatch()`. Polling `try_recv_notification()` from inside
        `dispatch` (what the plugin did) therefore always sees an empty
        queue; the host's `SelectionChangedV4` broadcast
        (`notify_plugins_selection_changed` /
        `publish_selection_changed_v4` in `src/plugin/v4_support.rs`) was
        firing correctly the whole time. Fix: override `on_notification` and
        stash the selection in a process-wide cache instead
        (`opencad-python`'s `selection_cache.rs`); `dispatch` and
        `ocs.selection()` just read that. Verified end-to-end: select an
        entity, then `PY_EVAL ocs.selection()` returns its handle.
      - Worth folding into this doc's guidance for anyone writing a V4+
        plugin: **notifications must be consumed via `on_notification`, not
        by polling `try_recv_notification()` from `dispatch`** — the stock
        runner loop already drains that queue for you.
      - **Testing pitfall worth remembering**: `--mcp` doesn't run headless —
        it's a thin JSON-RPC client that reuses any already-running OCS
        instance it finds under `$HOME/Library/Application Support/
        OpenCADStudio/automation` (`descriptors()` in `src/mcp.rs`), spawning
        a fresh one only if none exists. First test run silently talked to
        the *user's own already-running production app* (which obviously had
        never seen this plugin — plugins load once at startup) instead of the
        freshly built debug binary. Fix: override `$HOME` for the test
        subprocess to a scratch directory, which isolates `config_dir()`,
        `plugins_dir()` and the automation discovery directory all at once.
      - macOS only, per current scope; Windows/Linux still unverified.

### 1.3 — Write access + scripts

- [x] `ocs.add_line(x1, y1, x2, y2)` / `ocs.add_circle(x, y, radius)` — 2D
      only for now (z=0, layer `"0"`), wrapped so a whole script is **one**
      `host.push_undo` group, not one per call (`host_ctx::ensure_undo_started`,
      called lazily by the first write). Verified end-to-end: a `PY_RUN`
      script that calls `ocs.add_line` twice undoes both lines in one `undo`.
      Take `rustpython_vm::function::ArgIntoFloat`, not a bare `f64` — found
      by actually running `ocs.add_line(0, 0, 10, 10)` (plain ints, the
      natural way to write it) and hitting a `TypeError`: unlike CPython's
      C-function argument parsing, a bare `f64` `#[pyfunction]` parameter in
      RustPython does not implicitly coerce an `int`.
- [x] `PY_RUN <path.py>` to run a file (`Mode::Exec`, so statements are
      allowed, unlike `PY_EVAL`'s `Mode::Eval`). Same error reporting as
      `PY_EVAL`: a missing file or a raised exception is never a silent
      no-op. Reusing the automation `run` op needed no transport changes, as
      expected — `{"op":"run","cmd":"PY_RUN <path>"}` just works.
- [x] `ocs.command(cmd)` to invoke existing built-in commands by name —
      **implemented, required a host change**. Added `HostApi::run_command`
      (default `Err` impl, no forced version bump) by reusing
      `run_command_line` + `drive_headless_task` — the exact machinery
      `--serve`/`--mcp` automation's `"run"` op already uses — via a new
      `PluginRequest::RunCommand` and one match arm in
      `handle_plugin_request`. Full detail: OpenCADStudio's
      `docs/plugin-architecture.md`'s `HostApi` table, and this plugin's own
      `PLUGIN.md`.
      - **Found and fixed a real deadlock along the way**: the naive version
        routed back through normal plugin dispatch, which — since this call
        is a nested plugin→host request arriving *while* the calling
        plugin's `dispatch()` is still running on its one runner thread —
        sends that same plugin a second `Dispatch` it has no free thread to
        answer. Both sides wait forever. Fixed by skipping plugin dispatch
        entirely for this one call path
        (`dispatch_command_no_plugin_reentry` /
        `run_command_line_no_plugin_reentry`); confirmed via the full
        `cargo test --lib` suite (1040 passed, 1 pre-existing unrelated
        failure) that normal command-line/automation behavior is unchanged.
        Consequence, by design: `ocs.command()` can reach built-ins only,
        never another plugin's command.
      - **Verified against a real, non-trivial case, not just `LINE`**:
        pre-selecting two non-parallel lines then `ocs.command("PCONSTRAINT")`
        ran the actual geometric solver and adjusted both lines to be
        genuinely parallel — checked numerically (direction-vector cross
        product ≈ 0 after), a real persistent constraint object.
      - **Gap found, then closed same session**: constraint commands read a
        prior *selection*, not picks fed as command tokens — feeding handles
        as trailing `PCONSTRAINT` tokens starts the command but ends in
        "Command cancelled", not a constraint. Added `HostApi::set_selection`
        (same shape as `run_command`: new `PluginRequest::SetSelection`,
        default-`Err` trait method, no forced version bump; validates every
        handle exists before changing anything) and `ocs.select(handles)` on
        top of it. Re-verified **fully scripted, no automation-API help**: a
        plain `.py` file via `PY_RUN` calling `ocs.select([h1, h2])` then
        `ocs.command("PCONSTRAINT")` produces the same genuinely-parallel
        result as before, now reachable entirely from a script. Confirmed no
        regressions via the full `cargo test --lib` suite both times (1040
        passed, 1 pre-existing unrelated font-fallback failure).
- [x] **`ocs.add_points(points)` and XDATA CRUD
      (`ocs.read_record`/`write_record`/`remove_record`), ported from
      reviewing `schoeller/ocs_python_repl`** — a separate, independently
      maintained out-of-process PyO3-based Python plugin for OCS (real
      CPython + IPython in a terminal, generated typed bindings) reviewed for
      reusable ideas. Both needed **no host change**: `HostApi::add_entities`
      (batch add) and `read_record`/`write_record`/`remove_record` (XDATA)
      already existed in `ocs_plugin_api`, unused by this plugin until now —
      unlike `run_command`/`set_selection` in §1.3, which needed new `HostApi`
      methods. `add_points` batches many points into one
      `host.add_entities()` call instead of looping `add_entity`, so the host
      pays for undo/dirty/geometry-rebuild bookkeeping once per batch, not
      once per point — the in-process analogue of `ocs_python_repl`'s
      `ocs.doc.add_many(...)`, which batches to avoid one IPC round trip per
      point instead. The XDATA functions deliberately reuse
      `ocs_python_repl`'s exact `{app_name, values}`/`{kind, value}` record
      shape (see this plugin's `PLUGIN.md`'s XDATA section) so a script's
      XDATA handling ports between the two plugins unchanged. Verified with
      unit tests that run a real RustPython interpreter (no live `HostApi`)
      and assert that every new function gets all the way through its own
      argument/dict parsing — down to every `XDataValue` kind — before
      failing on the expected "not running inside a PY_ command" sentinel;
      `cargo test`/`cargo clippy --all-targets` both clean. Not yet verified
      against a real running host (the "host-runtime verified" bar the rest
      of this phase holds itself to) — that still needs a live OCS build, as
      noted for `ocs.command()`/`ocs.select()` above.

### 1.4 — Usability

- [x] **Open question resolved by reading `ribbon.rs` end to end, not
      assumed**: a script manager UI (or a dockable REPL console) is **not
      possible** with today's `ocs_plugin_api`. `CadModule` exposes exactly
      one hook, `ribbon_groups()`, returning a *static* set of
      buttons/dropdowns (`RibbonItem`) — there is no API for a plugin to host
      a persistent custom panel or render dynamic content (a script list, a
      REPL buffer, ...). That needs a new `HostApi`/`CadModule` capability —
      an `ocs_plugin_api` version bump, a host feature request. Out of scope
      for a plugin repo; documented as a real, checked answer in this
      plugin's own `PLUGIN.md`, not left open.
- [x] `ModuleEvent::PluginFileDialog` (already exists) for "Run Script…":
      opens a native file picker filtered to `.py`, and on selection the host
      dispatches `PY_RUN <path>` back to the plugin — no new plugin code
      needed beyond the ribbon entry, since `PY_RUN` already exists from
      §1.3. Verified the plugin still loads and dispatches correctly with the
      new ribbon entry; the file-picker interaction itself isn't automatable
      headlessly over `--mcp` (no way to drive a native OS dialog over
      JSON-RPC), so that specific interaction is unverified beyond "doesn't
      crash". "Save Script As…" was dropped — with no in-app editor (which
      needs the same missing custom-panel capability above), there's no
      script content to save.

### 1.5 — Sandboxing

- [x] Audit: the `ocs` module is the *only* thing a script can call — done,
      and **the earlier assumption was wrong, which is exactly why this was
      worth auditing instead of assuming**. Phase 1.2/1.3 claimed "no stdlib
      is loaded, so `import os`/`socket`/`subprocess` is already impossible"
      — true for those three names, but RustPython 0.5.0's `host_env` Cargo
      feature is **on by default** and bakes `posix`/`_ctypes` into every
      interpreter's core module registry regardless of `rustpython-stdlib`.
      Confirmed by actually running it: with `host_env` on,
      `PY_EVAL __import__('posix').getcwd()` and
      `PY_EVAL open('/etc/passwd').read()` both worked — full filesystem
      read/write from any script. Fixed: `rustpython-vm` now builds with
      `default-features = false, features = ["compiler", "gc"]`.
      Re-verified the full matrix after: `os`/`posix`/`nt`/`subprocess`/
      `socket`/`ctypes`/`_ctypes`/`_socket`/`_subprocess`/`_signal`/`pwd`
      all blocked, `open()` blocked for both read and write (its `FileIO`
      needs `host_env` internally too, even though `_io` itself is always
      registered). No functional regression — `ocs.add_line` and the full
      `examples/example.py` (both real constraints) still run correctly.
      Audited what remains exhaustively rather than assuming "fewer is
      enough": 15 `sys.modules` entries, all core language/import/codec
      plumbing plus `ocs`. One accepted, documented residual gap: `_thread`
      is real and functional (verified callable) and has no feature guard
      at all in this RustPython version — no dependency-level way to
      remove it without patching RustPython. Grants no filesystem/network/
      process access; worst case is a resource/stability issue local to
      the already crash-isolated plugin process. Full detail in this
      plugin's own `PLUGIN.md`.

### 1.6 — Distribution

- [ ] Publish binaries per platform on GitHub Releases (matches every other
      plugin's release shape already documented in OpenCADStudio's
      `plugin-architecture.md`).
- [ ] Manual repo link works immediately (`owner/repo`, no OCS PR needed);
      listing in OpenCADStudio's curated `plugins/registry.json` is a
      separate, optional later step once the plugin is stable.

**Risk to track continuously:** a plugin must match the host's exact
`rustc_version` and `acadrust_source` (API v4+ enforces this at load time).
Every OCS host release potentially requires a matching rebuild + republish of
this plugin, or users get a version-mismatch refusal. Budget for this as an
ongoing CI job (build matrix triggered off OCS's release tags), not a one-time
cost.

---

## Phase 2 — AutoLISP → Python parser/translator

**This phase lives in a separate project that is not part of this repository.** The sub-goals below are the original
scoping; §2.2/2.3 shipped with substantially wider builtin coverage than
first planned (measured from a real AutoLISP corpus sample, not just the
constructs listed here).

**Goal:** a best-effort, offline source-to-source translator from `.lsp` to
the Phase 1 plugin's Python dialect. It is explicitly **not** an interpreter —
output is a draft a human reviews and finishes, not a guaranteed-correct
program. That's what makes this tractable where a live AutoLISP runtime isn't
(see the earlier discussion: the language core is easy, AutoCAD's builtin
library is the hard, open-ended part — a translator only needs a lookup table
for the builtins it recognizes, not a faithful live implementation of all of
them).

### 2.1 — Corpus & scoping

- [x] Pull a representative sample from [Lee Mac's routines](https://lee-mac.com/)
      and the [Whole-Spec catalog](https://whole-spec.com/en/cad-tips/lisp-catalog/)
      (157 curated routines) to find the actual distribution of constructs
      used in real scripts — don't guess coverage priorities, measure them.
- [x] From that sample, explicitly decide and document what's out of scope:
      **`vla-*`/`vlax-*` (COM/ActiveX)** — doesn't run in AutoCAD for Mac
      either, so this isn't a new limitation — and **DCL dialogs** (no
      renderer in OCS). Both get left as commented-out LISP with a
      `# TODO: manual port needed` marker in the generated output, never
      silently dropped.

### 2.2 — Reader & core-language transform

- [x] S-expression tokenizer/parser (standalone, no OCS runtime dependency).
- [x] AST-to-AST transform for the Lisp core: `setq`, `defun`
      (`C:CMDNAME` → a Python function), `if`/`cond`, `lambda`/`function`,
      arithmetic, string/list operations, `foreach`/`while`.

### 2.3 — Builtin mapping table

- [x] `entget` → `ocs.get(handle)`; `ssget` (no-arg) → `ocs.selection()`.
- [x] `command` sequences → `ocs.command("...")`.
- [x] `getvar`/`setvar` — confirmed no host system-variable surface exists;
      documented as unsupported rather than guessed at.
- [x] Widened well past this original list via measured corpus frequency —
      see that project's own README for the full table.

### 2.4 — Validation loop

- [ ] Run translated output through the Phase 1 plugin's interpreter against
      the same corpus; every failure either fixes the mapping table or gets
      added to the documented unsupported list. This is the actual acceptance
      criterion for "done" — not 100% translation, but every gap being a known,
      documented one.

### 2.5 — Packaging

- [x] Ship as a standalone CLI (`ocs-lisp2py file.lsp > file.py`) — needs
      nothing from the OCS runtime, useful even to someone not running OCS yet.
- [ ] Optionally surface as an "Import AutoLISP Macro…" button in the Phase 1
      plugin's ribbon, calling the same conversion logic in-process.

---

## Phase 3 — polish (not yet scoped in detail)

- [ ] Decide the open question from Phase 1.4: dockable console REPL vs.
      run-only. Depends on what `HostApi` can support without a version bump.
- [ ] Versioning story for the `ocs` module surface as it grows: same strict
      `ApiVersion` gate as `ocs_plugin_api`, or a looser contract since scripts
      aren't compiled artifacts and can tolerate softer compatibility breaks?
- [ ] Whether scripts should ever be able to register permanent ribbon tools
      (leaning: no — keep that exclusive to compiled plugins, per
      OpenCADStudio's `plugin-architecture.md`'s "one package, one entry
      point" goal).

---

## Reference

| Piece | Location |
|-------|----------|
| Existing plugin architecture (non-goal note) | OpenCADStudio's `docs/plugin-architecture.md` |
| Plugin host API this plugin wraps | OpenCADStudio's `crates/ocs_plugin_api` (`HostApi` trait) |
| Plugin scaffold this was started from | OpenCADStudio's `docs/plugin-template/` |
| Existing automation/headless surface | OpenCADStudio's `src/app/control/transport.rs` |
| RustPython | https://github.com/RustPython/RustPython |
| AutoLISP corpus — Lee Mac | https://lee-mac.com/ |
| AutoLISP corpus — Whole-Spec catalog (157 routines) | https://whole-spec.com/en/cad-tips/lisp-catalog/ |
