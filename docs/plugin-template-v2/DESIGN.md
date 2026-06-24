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
| `MARK_FIRST_SURVEY_POINT` | Read the first `SURVEY` point and mark it with an XDATA record. |
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

The resulting cdylib plus `plugin.toml` can be installed into the host's
plugins folder (see `docs/plugin-architecture.md`).

---

## Compatibility

The template is built against the current `ocs_plugin_api` and declares
`ApiVersion::CURRENT`. Older plugins compiled against API v2 continue to load
on an API v3 host because new `HostApi` methods are appended at the end of the
trait and the host accepts plugin majors from `API_VERSION_MIN_SUPPORTED` up to
`API_VERSION`.
