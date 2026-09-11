This is acadrust 0.5.4 from HakanSeven12/cadcodec commit
`f23ffc873228e5f41b876485c741e4a8ac20a67d`, with its MPL-2.0 license retained.
Cargo's source patch uses this copy in both the application and the Web worker.

Local changes for large DWG loading:

- DGN component array counts are checked against the remaining encoded record
  before allocating. Unsupported encodings are preserved as raw DWG objects,
  including their handles and source version, for same-version saving.
- Entity and object reactor arrays cannot read beyond the handle stream.

Previously, a 35.1 MiB HVAC drawing produced hundreds of phantom 100,000-item
DGN stroke arrays: normal parsing reported no errors, but its serialized
document grew to 3,844,838,236 bytes and exhausted the browser's wasm memory.
The bounded readers prevent those allocations at their source.

Only source, Cargo metadata, README and license are vendored. Review local
changes against the commit above before upgrading; keep the stream-bound
regressions in the Web worker tests.
