# Language bindings

`src/gen/binding_schema.json` describes handle families, constructors, methods,
arguments and operation/query mappings. `build.rs` derives it from
`spec/entities.toml`. Its `layouts` section is manually maintained vocabulary;
it does not contain everything needed to generate a bincode codec.

Generate and test the Python facade from the repository root:

```sh
python3 crates/ocs_doc_api/bindings/python/generate.py > /tmp/ocs_doc.py
python3 -m unittest discover -s crates/ocs_doc_api/bindings/python
```

The generated `DocApi(transport, active_tab)` expects a caller-supplied
`transport.apply(envelope) -> Receipt`. It does not open a plugin connection.
An envelope has `version` and a Python tuple `("Op", operation_dict)` or
`("Queries", [query_dict])`. Enum payloads use externally tagged dictionaries,
for example `{"CreateSolid": {"Sphere": {"centre": [0, 0, 0], "radius": 2}}}`.
The bridge converts these objects to the Rust DTOs and bincode 1.3 wire format,
sends `DocApiRequest { tab_id, bytes }`, and decodes `Result<Receipt, ApiError>`.
Host plugin API version 6 and document envelope version 1 are required.

Decode receipt outcomes as `{"NewId": 42}` or `{"NewIds": [42, 43]}` and query
results as externally tagged dictionaries such as `{"Volume": 12.5}`. The facade
unwraps these into typed handles or values. The bridge must raise an exception
for an error response. Transport binding determines the tab; requesting another
tab or combining typed handles from different documents raises an error.
