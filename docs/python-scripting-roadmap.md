# Open CAD Studio — Python Scripting & AutoLISP Migration Plan

**Status:** Proposed
**Author:** Open CAD Studio contributors
**Date:** September 2026

This document plans embedded Python scripting for Open CAD Studio as an
**external plugin** (not a host change), followed by a best-effort AutoLISP →
Python transpiler built on top of it. It supersedes the host-embedded design
sketched in an earlier draft of this document: scripting fits OCS's existing
plugin model better than it fits inside the host. `plugin-architecture.md`
explicitly lists "sandboxed scripting (Python/Lua)" as a non-goal of the
external-plugin *system* itself — this plan doesn't touch that system, it
just builds a plugin that happens to embed an interpreter, same as any plugin
embeds whatever dependency it needs.

**Why a plugin, not a host feature:**

| | Host-embedded | Plugin (this plan) |
|---|---|---|
| Requires OCS core changes / Hakan's buy-in | Yes | No — ships from an independent repo, installed via the existing Plugin Manager |
| Crash isolation | Relies on RustPython's memory safety alone | Gets OS-process isolation for free (plugins already run out-of-process) |
| Distribution | Bundled into every OCS release, forever | Opt-in install, versioned independently, can iterate fast |
| Cost | New maintenance burden on the host | Same ABI-pinning cost every plugin already has (see Phase 1, Risk) |

---

## Phase 1 — Python scripting plugin

**Goal:** a working `opencad-python` plugin (own repo, `cdylib`) that embeds
RustPython and exposes an `ocs` module wrapping `HostApi`, installable today
through the existing Plugin Manager with zero OCS core changes.

### 1.1 — Scaffold

- [x] New repo from [`docs/plugin-template/`](plugin-template); `plugin.toml`
      + `Cargo.toml` pinning `ocs_plugin_api` (`host` feature) and matching the
      target host's `acadrust_source` / `rustc_version` (see Risks). Scaffolded
      locally at `~/Documents/MacApps/opencad-python` (own git repo, no
      remote yet — not pushed anywhere).
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

- [ ] `ocs.selection()` → list of entity handles.
- [ ] `ocs.get(handle)` → read-only geometry/property access.
- [ ] `PY` command (`command_prefixes = ["PY_"]`): evaluate one inline
      expression, print the result via `host.push_output`.
- [ ] Errors: map RustPython tracebacks to `host.push_error` — a script bug
      must never look like a silent no-op.

### 1.3 — Write access + scripts

- [ ] `ocs.add_line/add_circle/...`, wrapped so a whole script is **one**
      `host.push_undo` group, not one per call.
- [ ] `PY_RUN <path.py>` to run a file; reuse the same path for the CLI/
      automation surface (`src/app/control/transport.rs`'s `run` op can already
      invoke `PY_RUN "..."` with no transport changes needed).
- [ ] `ocs.command("ALIGNLEFT")` to invoke existing built-in commands by name
      — lets a script compose primitives instead of reimplementing them.

### 1.4 — Usability

- [ ] Script manager UI inside the plugin's own ribbon tab: list / run / save
      scripts, bind one to a ribbon button. (A dockable REPL console is a
      stretch goal — confirm first whether `HostApi`/`CadModule` can host a
      persistent custom panel today, or whether that needs a new `HostApi`
      method, i.e. an `ocs_plugin_api` version bump. Treat as an open question,
      not an assumption.)
- [ ] `ModuleEvent::PluginFileDialog` (already exists) for "Run Script…" /
      "Save Script As…".

### 1.5 — Sandboxing

- [ ] Audit: the `ocs` module is the *only* thing a script can call — no
      ambient `import os`/`socket`/`subprocess`. RustPython's default stdlib
      surface needs an explicit allow-list decision, not an assumption that
      "no C extensions" already means "safe."

### 1.6 — Distribution

- [ ] Publish binaries per platform on GitHub Releases (matches every other
      plugin's release shape already documented in `plugin-architecture.md`).
- [ ] Manual repo link works immediately (`owner/repo`, no OCS PR needed);
      listing in the curated [`plugins/registry.json`](../plugins/registry.json)
      is a separate, optional later step once the plugin is stable.

**Risk to track continuously:** a plugin must match the host's exact
`rustc_version` and `acadrust_source` (API v4+ enforces this at load time).
Every OCS host release potentially requires a matching rebuild + republish of
this plugin, or users get a version-mismatch refusal. Budget for this as an
ongoing CI job (build matrix triggered off OCS's release tags), not a one-time
cost.

---

## Phase 2 — AutoLISP → Python parser/translator

**Goal:** a best-effort, offline source-to-source translator from `.lsp` to
the Phase 1 plugin's Python dialect. It is explicitly **not** an interpreter —
output is a draft a human reviews and finishes, not a guaranteed-correct
program. That's what makes this tractable where a live AutoLISP runtime isn't
(see the earlier discussion: the language core is easy, AutoCAD's builtin
library is the hard, open-ended part — a translator only needs a lookup table
for the builtins it recognizes, not a faithful live implementation of all of
them).

### 2.1 — Corpus & scoping

- [ ] Pull a representative sample from [Lee Mac's routines](https://lee-mac.com/)
      and the [Whole-Spec catalog](https://whole-spec.com/en/cad-tips/lisp-catalog/)
      (157 curated routines) to find the actual distribution of constructs
      used in real scripts — don't guess coverage priorities, measure them.
- [ ] From that sample, explicitly decide and document what's out of scope:
      **`vla-*` (COM/ActiveX)** — doesn't run in AutoCAD for Mac either, so
      this isn't a new limitation — and **DCL dialogs** (no renderer in OCS).
      Both get left as commented-out LISP with a `# TODO: manual port needed`
      marker in the generated output, never silently dropped.

### 2.2 — Reader & core-language transform

- [ ] S-expression tokenizer/parser (standalone, no OCS runtime dependency —
      this can and should be developed/tested completely independently of the
      plugin).
- [ ] AST-to-AST transform for the actual Lisp core: `setq`, `defun`
      (`C:CMDNAME` → a Python function registered as a script command),
      `if`/`cond`, arithmetic, string/list operations, `foreach`/`while`.

### 2.3 — Builtin mapping table

- [ ] `entget`/`entmod` → `ocs.get(handle)` / entity property writes.
- [ ] `ssget` → `ocs.selection()` / a query helper matching common `ssget`
      filter patterns.
- [ ] `command` sequences → `ocs.command("...")` calls (the common case);
      flag sequences that depend on AutoCAD's *interactive prompt* behavior
      mid-command as needing manual review, since that's not a 1:1 mapping.
- [ ] `getvar`/`setvar` → whatever OCS system-variable surface exists (check
      current state before assuming parity; this may itself be a gap to file
      separately).
- [ ] Anything not in the table: preserved as a commented LISP fragment plus a
      TODO, never guessed at.

### 2.4 — Validation loop

- [ ] Run translated output through the Phase 1 plugin's interpreter against
      the same corpus; every failure either fixes the mapping table or gets
      added to the documented unsupported list. This is the actual acceptance
      criterion for "done" — not 100% translation, but every gap being a known,
      documented one.

### 2.5 — Packaging

- [ ] Ship as a standalone CLI (`ocs-lisp2py file.lsp > file.py`) — needs
      nothing from the OCS runtime, useful even to someone not running OCS yet.
- [ ] Optionally surface as an "Import AutoLISP Macro…" button in the Phase 1
      plugin's script manager, calling the same conversion logic, once Phase 1
      has a script manager to import into.

---

## Phase 3 — polish (not yet scoped in detail)

- [ ] Decide the open question from Phase 1.4: dockable console REPL vs.
      run-only. Depends on what `HostApi` can support without a version bump.
- [ ] Versioning story for the `ocs` module surface as it grows: same strict
      `ApiVersion` gate as `ocs_plugin_api`, or a looser contract since scripts
      aren't compiled artifacts and can tolerate softer compatibility breaks?
- [ ] Whether scripts should ever be able to register permanent ribbon tools
      (leaning: no — keep that exclusive to compiled plugins, per
      `plugin-architecture.md`'s "one package, one entry point" goal).

---

## Reference

| Piece | Location |
|-------|----------|
| Existing plugin architecture (non-goal note) | [`docs/plugin-architecture.md`](plugin-architecture.md) |
| Plugin host API this plugin would wrap | `crates/ocs_plugin_api` (`HostApi` trait) |
| Plugin scaffold to start from | [`docs/plugin-template/`](plugin-template) |
| Existing automation/headless surface | `src/app/control/transport.rs` |
| RustPython | https://github.com/RustPython/RustPython |
| AutoLISP corpus — Lee Mac | https://lee-mac.com/ |
| AutoLISP corpus — Whole-Spec catalog (157 routines) | https://whole-spec.com/en/cad-tips/lisp-catalog/ |
