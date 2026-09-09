# ocs_doc_api

Typed document operations and queries for OpenCADStudio plugins and hosts.
The same facade runs over an in-process backend or the plugin IPC channel.

```rust
use ocs_doc_api::{ApiResult, DocApi};

fn intersect_solids(api: &DocApi) -> ApiResult<f64> {
    let doc = api.document(api.active_tab());
    let a = doc.solids().create_cuboid([0.0; 3], [10.0; 3])?;
    let b = doc.solids().create_cuboid([5.0; 3], [10.0; 3])?;
    a.intersect(&b)?.volume()
}
```

Each successful write marks the document changed and records one undo step.
A failed operation leaves the document unchanged. Bulk creation and transforms
prepare every entity before applying changes. Separate calls commit separately;
`OpGroup` offers best-effort cleanup of creations, not rollback.

Boolean `intersect`, `union` and `subtract` replace the first solid and erase the
second. `intersects` only tests bounding-box overlap. Volume and centroid are
computed by the kernel from a tessellated solid (chord tolerance 0.1 drawing units,
angular tolerance 0.0001 radians), then cached per geometry revision. A request
may compute at most 32 uncached mass properties; excess work returns an error.
Bulk operations and query batches are capped at 100,000 items.

Transports bind to one document tab. Typed handles from another transport and
requests for another tab are rejected. Raw `ObjectId` values are relative to the
receiving document. Errors are serialized as `ApiError` variants over IPC.

Layer CRUD is exposed through `Document::layers()`, `create_layer(info)`,
`update_layer(name, info)`, `delete_layer(name)` and through the `layer()` /
`set_layer(...)` methods on every entity handle (`Line`, `Circle`, `Solid`,
`Entity`, etc.). Layer properties are carried as the plain-data `LayerInfo` DTO
with `LayerFlags`, `Color` and `LineWeight`. The host enforces CAD-style rules:
layer "0" and the current layer cannot be deleted, duplicate names are rejected,
and a layer still referenced by entities cannot be removed.

XDATA and XRECORD payloads are carried as plain-data, serde-compatible DTOs
(`XDataRecord`, `XRecordSpec`, and their value/entry types) and roundtripped
verbatim by the host. Every `Entity` handle can read/write one XDATA record per
registered application name via `xdata(application_name)` and `set_xdata(...)`,
and standalone XRECORD objects can be created with `doc.entities().create_xrecord(...)`
and updated through the `XRecord` handle.

| Feature | Use |
|---|---|
| Default | DTOs, facade, transport trait and binding schema |
| `host` | Kernel and entity adapters, executor, `DocApiBackend`, `InProcess` |
| `ipc` | `OcsPluginApiIpc` adapter for a plugin connected to the host |
| `doc_api_host` | Convenience `doc_api_for_host(&dyn HostApi) -> Option<DocApi>` for out-of-process / worker-thread plugins |

### Connecting from a plugin

For out-of-process or worker-thread plugins that receive `&dyn HostApi` (or any
`ocs_plugin_api::host::HostApi` implementor), enable the `doc_api_host` feature
and use the helper instead of manually serializing `DocApiEnvelope`s:

```rust
use ocs_doc_api::doc_api_for_host;
use ocs_plugin_api::host::HostApi;

fn on_dispatch(host: &mut dyn HostApi) -> Option<()> {
    let doc_api = doc_api_for_host(host)?;
    let doc = doc_api.document(host.tab_id());
    let cube = doc.solids().create_cuboid([0.0; 3], [10.0; 3]).ok()?;
    Some(())
}
```

`doc_api_for_host` returns `None` for hosts that do not expose a
`PluginRequestSender` (for example the in-process `HostSession`). In-process code
should use `DocApi::in_process(backend, tab_id)` with a concrete
`DocApiBackend`.

See the [API reference](src/gen/api_reference.md) for constructors and methods,
[architecture](ARCHITECTURE.md) for backend rules, and
[binding guide](bindings/README.md) for the Python facade and transport contract.
