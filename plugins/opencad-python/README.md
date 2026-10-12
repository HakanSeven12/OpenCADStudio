# opencad-python

Python scripting for Open CAD Studio, shipped as a bundled first-party plugin. Embeds
[RustPython](https://github.com/RustPython/RustPython) and exposes an `ocs`
module wrapping `HostApi`, so `PY_`-prefixed commands can run Python against
the open document.

The crate remains an independent process plugin under `plugins/` so Python
cannot crash the editor process and all writes still cross the versioned
`HostApi`. Its API dependency is repository relative, and the macOS packaging
flow builds and places it in the application bundle automatically.

Full design and phased plan: `ROADMAP.md` in this directory.
Command reference, the `ocs` module's functions, and every real gap/limit
found while building this: `PLUGIN.md` in this directory.

## Status

The API v7 build provides `PY_EVAL`/`PY_RUN`, the read/write `ocs` module, and
the ribbon integration. On top of that, the `experimental-host-model` feature
(the name stays until a later API cleanup; the staged bundled plugin is built with
it) adds a general Python document model over the host API:

- **Entities:** create, read, edit, delete and undo for all 43 canvas kinds
  (41 creatable), validated by the host, with per-kind DWG/DXF round-trip audits.
- **Drawing tables:** `doc.layers`, `text_styles`, `dim_styles`, `blocks` (and their
  contents), `linetypes` and `layouts`, plus kernel-backed solids (`doc.solids`).
- **Commands:** `doc.command`, `doc.start_command` and `doc.modify` (offset, trim,
  extend, fillet, chamfer, move, copy, rotate, scale, mirror, arrays, explode, join,
  break, stretch, lengthen, PEDIT polyline edits) drive the real OCS commands.

The design and guarantees are in `../../docs/plugin-host-model.md`, the per-kind
evidence in `../../docs/plugin-host-model-coverage-ledger.md`, and the script-facing
reference in `PLUGIN.md`. The older experimental `ocs.command()` replay is
superseded and not in the default build.

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | `cdylib` crate depending on `ocs_plugin_api` (`host` feature) and `rustpython-vm` (stdlib disabled — see `PLUGIN.md`'s sandboxing section) |
| `src/lib.rs` | manifest + `CadModule` ribbon + `BuiltinPlugin` + `export_plugin!` + `PY_EVAL`/`PY_RUN` dispatch |
| `src/ocs_module.rs` | the `ocs` Python module (`selection`, `get`, drawing access; experimental `command`/`select` are feature-gated) |
| `src/host_ctx.rs` | thread-local bridge from `dispatch()`'s `HostApi` into the `#[pymodule]` native functions |
| `src/selection_cache.rs` | process-wide selection cache, populated via `on_notification` |
| `src/document_model.py` | API v7 Python document view and transaction context manager |
| `src/event_cache.rs`, `src/input_cache.rs` | bounded event queue and interactive-pick results |
| `tools/stage-bundled.sh` | build and stage the matching API v7 package |
| `plugin.toml` | metadata read by the host (mirrors the manifest) |
| `examples/example.py` | a script demonstrating shapes plus two real, persistent constraints |
| `../../.github/workflows/release.yml` | bundles the cdylib into the macOS application |
| `PLUGIN.md` | command reference, the `ocs` module, and every real gap/limit found |

## Build

```sh
cargo build --release --locked --features experimental-host-model \
  --manifest-path plugins/opencad-python/Cargo.toml
```

## Stage the bundled plugin

The staging script builds against the in-tree `ocs_plugin_api`, writes exact
compiler and CAD codec metadata, and creates the directory layout consumed by
bundled plugin discovery.

```sh
bash plugins/opencad-python/tools/stage-bundled.sh /path/to/stage-directory
```

Add `--debug` for a faster local build. Build with the same Rust toolchain as
the OCS binary; the host checks the compiler version when loading plugins.

```python
doc = ocs.active_document
line = doc.create_entity("Line", start={"x": 0, "y": 0, "z": 0},
                         end={"x": 10, "y": 0, "z": 0})
line = doc.entities[handle]
print(line.start, line.end, line.layer, line.coverage)
with doc.transaction("Move line"):
    line.start = (0, 0, 0)
    line.end = (10, 0, 0)

doc.selection = [line]
token = doc.request_point("Pick destination")
# After the host has collected the click, run another script:
result = doc.poll_input(token)
events = doc.poll_events()
doc.delete_entity(line)
```

`doc.entities` iterates every document entity. The generated Python schema
currently edits Point, Line, Circle, Arc, Ellipse, Polyline, Polyline2D,
Polyline3D, LwPolyline, Spline, Text, MText, Ray, XLine, Solid, Face3D,
Insert, Tolerance, Shape, AttributeDefinition, AttributeEntity, and Hatch.
Insert block references support edits to placement and transform
properties; their block name is readable and creation is not yet exposed.
Other canvas kinds expose
handle, kind, and layer; their layer can be changed in a transaction. Internal
and opaque kinds remain read-only. Each
mapped entity reports its actual readable/editable keys through `coverage`.
The host-owned catalog classifies all 48 entity variants and tracks every
traced field, including fields marked `unmapped`. Query it with
`doc.coverage()` or `doc.coverage("Line")`; each row includes source field,
type, access, and current validation status. Geometry edits remain limited to
the 43 mapped kinds.
For any entity, `entity.snapshot` returns a detached dictionary of its raw
CAD fields, including `snapshot["common"]` and kind-specific nested values.
It can inspect an unmapped kind; editing the dictionary does
not commit changes to the drawing.
Point and entity requests return a token because `PY_RUN` uses a fresh
interpreter for each invocation; `poll_input` reports pending, picked, or
cancelled on a later run in the same drawing tab. Another tab cannot consume
that token. `doc.selection` preserves the exact requested handle order and
rejects duplicate or absent handles. `poll_events` drains tab-scoped drawing, selection,
and active-command state changes. Each tab retains up to 256 events; an
`{"type": "overflow", "dropped": n}` event means older events were lost, so
refresh the document and selection before acting on cached state.

## Test locally as a bundled plugin

```sh
bash plugins/opencad-python/tools/stage-bundled.sh \
  work/bundled-plugins/opencad.python --debug
OCS_BUNDLED_PLUGINS_DIR="$PWD/work/bundled-plugins" cargo run -- --new
```

The macOS release and signed packaging scripts perform this staging inside
`OpenCADStudio.app/Contents/Resources/plugins/opencad.python` automatically.
