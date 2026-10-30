# `plugin-template-v2` — Shared-memory `DocumentReader` example

This crate is a runnable plugin template that demonstrates the V2 host/plugin
contract:

- **Zero-copy reads** through [`ocs_plugin_api::host::DocumentReader`].
- **Validated writes** through the existing `HostApi` RPCs (`add_entity`,
  `write_record`).
- **Read→write round-trips**: read an entity handle from the shared document
  view and attach XDATA to it.

It lives under `docs/` (not `crates/`) because it is documentation/example code
rather than a runtime dependency of the host.

---

## Commands

| Command | Purpose |
|---|---|
| `COUNT_SURVEY_POINTS` | Count point entities on the `SURVEY` layer via `document_reader()`. |
| `ADD_SURVEY_POINT` | Add a new point entity on the `SURVEY` layer via `host.add_entity()`. |
| `MARK_FIRST_SURVEY_POINT` | Read the first `SURVEY` point and mark it with an XDATA record (including the `SURVEY-CLASS:POINT` identity key). |
| `COUNT_MARKED_SURVEY_POINTS` | Count `SURVEY` points that carry the `SURVEYMARK` XDATA record. |

---

## Design notes

### Reads are zero-copy; writes are RPCs

`document_reader()` returns a read-only view backed by host-owned shared memory
(for out-of-process plugins) or by `&CadDocument` (for in-process plugins). The
plugin iterates entities without copying the model.

All mutations still cross the validated RPC boundary. This preserves the
host's crash-safety boundary: a buggy plugin cannot corrupt the host document
because it never holds a mutable reference to host memory.

### Entity handles bridge reads and writes

Each [`ReaderEntity`](ocs_plugin_api::host::ReaderEntity) exposes the entity
`handle`. A plugin can therefore:

1. Find an entity of interest with `document_reader().for_each_entity(...)`.
2. Use that handle with `host.write_record(handle, record)` to attach XDATA.
3. Read the record back later with `host.read_record(handle, app_name)`.

This is the primary read→write round-trip pattern the template demonstrates.

### Entity class display labels

The template also demonstrates the entity-class label convention (API v10):

- the plugin declares `ENTITY_CLASSES` and exposes it through
  `BuiltinPlugin::entity_classes()`: app `SURVEYMARK`, prefix
  `SURVEY-CLASS:`, and key → label mappings;
- `MARK_FIRST_SURVEY_POINT` stamps a `SURVEY-CLASS:POINT` string value into
  the point's XDATA alongside the existing marker value.

While the plugin is loaded, the host shows marked points as
"Survey Point (Point)" in the Properties panel title and in the
`display_type` field of the records/automation APIs; without the plugin they
are plain "Point" again. Only the stable key round-trips the drawing — the
label text lives in the plugin and can be reworded or localized without
touching saved files. The label is display-only: it never governs snapping,
selection, or any other behavior.

### Ribbon integration

The plugin registers one ribbon module (`SurveyTools`) with one group
(`Survey`). Each tool emits a `ModuleEvent::Command(...)` carrying the command
name. The host routes the command to `BuiltinPlugin::dispatch`.

---

## Building and running

From the workspace root:

```bash
cargo build -p plugin-template-v2
```

The resulting cdylib plus `plugin.toml` (checked in alongside this file) can
be installed into the host's plugins folder (see `docs/plugin-architecture.md`).
From API v14 the plugin runs its own runner loop, so it does not have to be
built with the host's compiler or codec commit.

---

## Compatibility

The template is built against the current `ocs_plugin_api` and declares
`ApiVersion::CURRENT`. Older plugins compiled against API v2 continue to load
on an API v3 host because new `HostApi` methods are appended at the end of the
trait and the host accepts plugin majors from `API_VERSION_MIN_SUPPORTED` up to
`API_VERSION`.
