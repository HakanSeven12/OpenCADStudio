# Document API architecture

`DocApi → Document → collections → typed handles` builds DTO requests.
`Transport::apply` carries one operation or a read-only query batch to the
executor. The native adapter binds each request to its existing `HostSession`.

The executor validates inputs and dispatches through `DocApiBackend`. Backends
must prepare fallible geometry and serialization before changing the document.
Bulk writes prepare all entities, begin one undo record, apply the prepared
changes, then finalize once. Cancellation releases a pending undo capture;
it is not a rollback mechanism for partially applied writes.

The native adapter reuses scene changes, document history, entity transforms
and profile conversion. Solid construction, sweeps, booleans, meshing and mass
properties belong to cadkernel. Profile planes preserve their source elevation
and orientation. Geometry revisions advance when the scene changes; a compound
operation may advance the revision more than once while recording one undo step.

`Operation` and `Query` are manually maintained wire enums. Append variants;
do not reorder them. The consistency tests verify their bincode discriminants.
`spec/entities.toml` supplies family, constructor and method mappings. `build.rs`
generates the API reference and binding schema. The layout snapshot is curated
vocabulary, not a reflected binary codec schema.

To extend the API, update the DTO, backend, executor, facade and spec together,
add a native behavior test, and rebuild the generated reference/schema. New
backend methods must reject unsupported behavior explicitly.
