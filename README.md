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

| Feature | Use |
|---|---|
| Default | DTOs, facade, transport trait and binding schema |
| `host` | Kernel and entity adapters, executor, `DocApiBackend`, `InProcess` |
| `ipc` | `OcsPluginApiIpc` adapter for a plugin connected to the host |

See the [API reference](src/gen/api_reference.md) for constructors and methods,
[architecture](ARCHITECTURE.md) for backend rules, and
[binding guide](bindings/README.md) for the Python facade and transport contract.
