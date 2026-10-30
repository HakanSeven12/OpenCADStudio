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
| Read | `document()`, `entity_snapshot` | The full `Document` snapshot; a detached JSON view of any typed entity. |
| Entities | `add_entity`, `remove_entity`, `update_entities_transaction`, `set_reference_only` | Create, delete and replace entities. A transaction validates every replacement first and either commits all of them as one undo step or none. `set_reference_only` designates plugin-owned display geometry (API v9, see below). |
| Selection | `selection`, `set_selection` | Ordered, tab-scoped, exact (missing or duplicate handles reject the request). |
| Solids | `solid_operation` | Kernel-backed create, transform, booleans, extrude, region and surface from a profile, picture embedding. Every result is verified to lift back from its ACIS payload without loss; a refusal leaves the operands untouched. |
| Tables | `table_operation` | Layers, text and dimension styles, blocks and their contents, linetypes, layouts (see below). |
| Objects | `create_xrecord`, `read_xrecord`, `update_xrecord`, `delete_xrecord`, `list_xrecords` | Name-addressed XRecords under the drawing's named-object dictionary (API v8, see below): a non-graphical record carrier with strict create-vs-update semantics and one undo step per change. |
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

## XRecords (API v8)

`create_xrecord`, `read_xrecord`, `update_xrecord`, `delete_xrecord` and
`list_xrecords` address XRecords by name under a dictionary path walked from the
named-object dictionary root (an empty path is the root itself); missing
dictionaries are created on create only, so the first call seeds a plugin's
namespace. The payload is the codec's typed `XRecordEntry` pairs — the host puts
no opinion on the content. Names and path fragments are symbol-validated, and
dictionary addressing follows the codec's case-insensitive lookup. Create never
overwrites and update never creates; each refusal is decided before the undo
scope is touched, so a failure changes nothing, and each change joins a
plugin-opened `push_undo` scope — the entity-write contract — so an operation
composed of several writes (entities and XRecords) is a single undo step; with
no scope open a change is one undo step of its own, whose undo restores the
objects map byte-exactly, the auto-created dictionaries included.
`list_xrecords` reports only the keys that address XRecords, in the
dictionary's own order, so audits need no prior knowledge. Not yet mapped to
Python.

## Reference-only entities (API v9)

`set_reference_only` designates plugin-owned display geometry: a
reference-only entity renders and snaps normally, but the built-in edit paths
treat it like an entity on a locked layer — it is not grip-editable, not
click-pickable (a click passes through to the editable entity underneath),
and window, box, lasso and fence selections skip it. User-driven writes
(grip drags, modify commands, the properties panel, user erase, MCP
`set_properties`) refuse it; the owning plugin keeps full write access:
`add_entity`, `update_entity`, `update_entities_transaction`, `remove_entity`
and the XDATA record methods are unaffected, so a plugin can refresh derived
display geometry in place. The record-method policy applies first: absent
entities and locked layers refuse, and like them every refusal is decided
before the undo scope is touched, so a failure changes nothing. A
re-designation that would not change the state succeeds without recording an
undo step. Undo composition is the entity-write contract: a change inside a
plugin-opened `push_undo` scope joins it, so an operation composed of several
writes is a single undo step under the plugin's label; with no scope open a
change is one undo step of its own.

**Storage is a scene-scoped in-memory set, not a file format.** The host
writes nothing of its own into the saved DWG/DXF — no APPID registration, no
XDATA marker on the entity — so a saved drawing never depends on host state
to load correctly. The set lives and dies with the drawing tab: closing a
tab forgets its designations, and a freshly opened drawing starts
undesignated. Because the designation is per handle, an in-place refresh
(clone → edit → replace) keeps it by construction. Undo keeps the contract
through an explicit designation channel on each history step (the same
pattern the parametric-constraint directory uses): undo/redo restores the
designation together with the entity it belongs to — erasing a designated
entity inside a scope, then undoing, brings back the entity *and* its
designation. Out of scope today: entities nested inside block instances
(the block cache rebuilds instance wires without the flag) and a Python
mapping. The MCP `entities` read exposes the flag as a `reference_only`
boolean on every entity record.

**Re-marking on load: `HostNotification` dispatch (API v9).** The host
dispatches the existing `HostNotification`s to every loaded native plugin
through `BuiltinPlugin::on_notification`: `DocumentChanged` (carrying the
active tab's settled geometry epoch), `SelectionChanged` (the plain selection
set), and a new `DocumentOpened` — the counterpart of the existing
`DocumentTabClosed` — which fires once whenever a whole document becomes
resident in a tab (file open, template new, system open). With
`DocumentOpened`, the owning plugin re-marks its entities when a drawing
loads, which makes the in-memory designation fully equivalent to a persisted
one for its use case. Dispatch happens at the message boundary — after the
mutation settles and pending history scopes have closed — never mid-undo or
mid-pick; history (undo/redo) replay itself emits nothing, the boundary is
the only channel. Plugin-initiated writes publish immediately after they
settle (the same contract the drawing-changed event follows), and the
boundary's fingerprint guard deduplicates. Handlers may call HostApi write
methods through the same session (`set_reference_only` at minimum;
`update_entity` for reference-only refresh goes through the plugin write
paths): a handler's write bumps the epoch, the next boundary notifies once
more, and because a re-mark of already-designated geometry is a documented
no-op — no epoch bump, no undo step, no publish — the loop terminates. The
remaining gap stays open and documented: a plugin process restarted while
its drawing is open has no snapshot of the designations (the plugin's own
session persistence closes it; reopen from disk re-marks via
`DocumentOpened`). Not mapped to Python.

### Usage from a plugin

Build the derived geometry and designate it inside one `push_undo` scope, so
creating a preview plus marking it is a single undo step under the plugin's
label:

```rust
host.push_undo("CVI: refresh path preview");
let preview = host.add_entity(EntityType::LwPolyline(computed_path));
if let Err(error) = host.set_reference_only(preview, true) {
    host.push_error(&format!("set_reference_only failed: {error}"));
}
```

Refresh the display data in place through the same API — reads before the
scope, mutations inside it (the entity-write contract's ordering rule):

```rust
let found = host
    .document()
    .entities()
    .find(|entity| entity.common().handle == preview)
    .cloned();
if let Some(mut entity) = found {
    // edit the clone; the designation travels with the entity
    host.push_undo("CVI: recompute preview");
    host.update_entity(entity);
}
```

Notes for plugin authors:

- `add_entity` ignores layer locks; `set_reference_only` does not — it
  refuses absent entities and locked layers exactly like the XDATA record
  methods, every refusal changes nothing, and an idempotent re-mark records
  no undo step.
- The designation travels with the entity: a clone → edit → replace refresh
  keeps it, so a plugin marks its display data once and refreshes freely.
- The designation is scene state, not file state: it does not survive
  DWG/DXF save and reload. Subscribe to `on_notification` and re-mark on
  `DocumentOpened`; the idempotent re-mark guarantees the
  `DocumentChanged`-driven loop terminates.
- Rendering and object snap are unaffected; grips, picking and the built-in
  write paths are the only observers.
- Plugins that read the document through the IPC client (the
  out-of-process runner) resolve entities by iterating
  `document().entities()`: a received `DocumentSnapshot` does not serve
  `get_entity(handle)` lookups. This is a property of the snapshot path,
  not of this API — host-side lookups are unaffected.

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
