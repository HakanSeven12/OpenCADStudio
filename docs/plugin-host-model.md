# Scripting host model (API v7)

This note describes how a script controls OCS and what the host guarantees. The
per-kind evidence is in the [coverage ledger](plugin-host-model-coverage-ledger.md),
the increment-by-increment record in the [completion plan](plugin-host-model-completion-plan.md),
and the script-facing reference in `plugins/opencad-python/PLUGIN.md`. A reviewer new to
the change should start with the [review guide](plugin-host-review-guide.md).

## Shape

Python runs in RustPython inside a separate plugin process
(`plugins/opencad-python`), so a script cannot crash the editor. It talks to the
host over the versioned IPC in `crates/ocs_plugin_api`. **The host owns the
drawing**: it validates every request, records the undo step, refreshes scene
caches and the shared document view, and marks the document dirty. The Python
adapter is thin: it turns dictionaries into typed values and back, and holds no
CAD rules of its own. Everything below is additive to API v7; a plugin built
without it keeps working, and the default `HostApi` methods refuse.

## What the host offers

| Area | `HostApi` entry | What it does |
|---|---|---|
| Read | `document()`, `entity_snapshot` | The full `CadDocument` snapshot; a detached JSON view of any typed entity. |
| Entities | `add_entity`, `remove_entity`, `update_entities_transaction` | Create, delete and replace entities. A transaction validates every replacement first and either commits all of them as one undo step or none. |
| Selection | `selection`, `set_selection` | Ordered, tab-scoped, exact (missing or duplicate handles reject the request). |
| Solids | `solid_operation` | Kernel-backed create, transform, booleans, extrude, region and surface from a profile, picture embedding. Every result is verified to lift back from its ACIS payload without loss; a refusal leaves the operands untouched. |
| Tables | `table_operation` | Layers, text and dimension styles, blocks and their contents, linetypes, layouts (see below). |
| Commands | `run_command` | Drives the real OCS command one step at a time (see below). |
| Settings | `system_variable`, `set_system_variable` | `CLAYER`, `SNAPANG`, and a read-only `CTAB` (the current layout). |

Entity kinds outside a generated schema still read (handle, kind, layer) and take
`layer` changes; nothing is writable that the host has not validated.

## Entity validation

The host embeds a coverage catalog generated from its traced `EntityType`
registry and `crates/ocs_plugin_api/entity_coverage_policy.json`. It classifies all
48 variants: 43 canvas kinds, three internal records (`Block`, `BlockEnd`,
`Seqend`) and two opaque fallbacks (`Extended`, `Unknown`), and records for each
property its source field, type, shape, snapshot readability, Python access
(`read_write`, `read_only`, `unmapped`) and validation status.

Creation and edits are validated per kind (finite coordinates, positive radii and
scales, unit directions, non-parallel vectors, in-range flags, reference targets
that exist and have the right kind, and so on). Creation must be valid; an edit
is refused only for problems it introduces, so already-invalid legacy values stay
editable. Derived state is computed by the host, never trusted from a script (MLine
geometry, Dimension measurement, Helix spline, Table merge dimensions, Spline
rational flag, SectionSymbol counts, Viewport id and scale).

Nested records are checked too. A dictionary for a vertex, edge or face may not
carry an unknown key, and the fields that define its geometry are required
(`entity_manifest.json`, `required_struct_fields`); a typo can no longer turn into
a zero coordinate. Everything else in a nested record defaults when absent.

Every kind is exercised through the real plugin runner by a per-kind audit that
checks create, read, edit, delete, undo, invalid input with an atomic rollback,
and a DWG and DXF round trip. A kind is `Complete` in the ledger only when all of
those pass.

## Tables

`TableOperation` is one additive request with typed variants, executed by
`HostSession::table_operation`. Every refusal is decided **before** the undo step is
recorded, so a failure changes nothing and leaves the history alone.

- **Layers:** create, modify, rename (entities follow), delete (refused while it
  holds objects unless erasing them is asked for), set current. Layer `0`,
  `Defpoints` and the current layer are protected.
- **Text and dimension styles:** create, modify, rename, delete, set current. Dimension
  style fields travel as a validated JSON object; handles, xref fields and the name are
  host-managed. Renaming a text style updates the dimension styles that name it.
  `Standard` and any style that is current or in use are protected.
- **Blocks:** create from entities (copied in shifted by the base point, optionally
  erasing the originals), modify, rename (inserts follow), delete (refused while
  referenced), and add an entity to a definition through the normal validation path.
  A block may not nest itself, directly or through other blocks.
- **Linetypes:** simple dash, gap and dot patterns; rename follows layers and
  entities; delete is refused while anything uses it.
- **Layouts:** create with the default page setup and sheet viewport, rename, delete,
  switch (through OCS's own layout switch), and page setup (paper size, rotation,
  custom scale). `Model` is protected.

Style and layout operations act on the active document, because they use the
application's shared style and view machinery.

## Commands

`run_command` takes a `CommandRequest` (`Run`, `Start`, `Point`, `Text`, `Token`,
`Entity`, `Selection`, `Enter`, `Cancel`) and answers with a `CommandOutcome`:
whether the command is done or waiting, its prompt, what input it accepts, its
keyword options, how many entities it added, tokens nobody asked for, and any
error. It uses the same primitives as the automation channel and finishes each
step synchronously on the host thread, so a step never waits for anything that has
to arrive from outside; an earlier fire-and-forget replay could hang, this does not.

Guards: refused while another command is active or off the active tab; refused for
commands that could end the session or re-enter Python (`QUIT`, `EXIT`, `CLOSE*`,
`NEW`, `QNEW`, `OPEN`, `SAVE*`, `RECOVER`, `SCRIPT`, `RUNSCRIPT`, `SCRIPTCOMMANDS`, `PY_*`); an editor or
dialog a command opens is reported and closed by `Cancel`. The user can turn the whole
facility off with the saved `SCRIPTCOMMANDS` preference (on by default; `SCRIPTCOMMANDS 0`
at the command line): while it is off, every request except `Cancel` is refused (so a command a script left
waiting can still be closed). A script cannot change
it, because the runner refuses that command. The runner does not consult the automation
on/off switch, which governs the external MCP and serve channels. The Python layer cancels
any step sequence that leaves a command waiting.

## Python surface

`ocs.active_document` exposes `entities`, `create_entity`, `delete_entity`,
`transaction`, `selection`, `solids`, `layers`, `text_styles`, `dim_styles`, `blocks`,
`linetypes`, `layouts`, `command`, `start_command` and `modify` (offset, trim, extend,
fillet, chamfer, move, copy, rotate, scale, mirror, erase, rectangular, polar, path and
3-D arrays, explode, join, break, stretch, lengthen and polyline edits). `doc.coverage()`
lists the catalog, and `doc.entities[handle].coverage` one entry, with the readable and
editable keys and the fields the typed snapshot carries but the model does not map.

Interactive picks use a token-based request and poll API because `PY_RUN` uses a
fresh interpreter per invocation and a script cannot suspend while the user clicks.
Drawing, selection and command notifications are kept in a bounded 256-event queue
per tab between runs and drained with `doc.poll_events()`; an overflow is reported
as `{"type": "overflow", "dropped": n}`. Tokens are bound to the tab that asked.

## Known limits

- Twelve of the 43 canvas kinds are not yet `Complete`; each has a named blocker in the
  ledger. Seven are opencadcodec DXF bugs whose fixes are submitted upstream but not yet adopted.
- The DXF codec also drops some table properties (text style generation flags, block
  descriptions, a dimension style's text-style name) and mis-scales one angle; these are
  pinned by canaries and listed in `cadcodec-reader-gaps.md`.
- Not available from a script: complex (text and shape) linetypes, plot devices and named
  page setups, PEDIT's fit and spline options, and interactive-only commands such as HATCH.
- A command step nests inside the editor's message handling. In a debug build a deep step
  (PEDIT converting a line to a polyline) needs between 2 and 4 MiB of stack; the native
  builds link with at least 8 MiB (16 MiB on Windows, `.cargo/config.toml`), the nesting depth
  is fixed rather than recursive (`PY_*` is refused), and a release build uses far less.
