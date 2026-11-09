# CAD automation workflow

Call ocs_sessions, then pass its session_id as ocs_session_id to ocs_read, ocs_execute and ocs_capture. Read capabilities to discover the complete CAD automation surface. Call record_schema to discover every record type, property path, JSON type, enum, unit, constraint and write rule before editing unfamiliar data. Use records to inspect every serializable entity, object, table, header and document record; filter with RFC 6901 JSON Pointer paths. Use set_properties for atomic, type-checked record edits and preserve document_id, revision and request_id.

Use commands with parameters.name for a command manifest. Use batch when several steps are known, and request changed_entities when resulting geometry is needed.

Before creating an unfamiliar entity, use `ocs_read` with `op: entity_schema` and
`parameters.type` (for example, `Circle`) to get its creation fields, required
values, units, defaults and example. Omit type for the catalog. Creation schemas
describe the inputs to `entities_create`; `record_schema` describes stored records.

Use `ocs_read` with `op: validate` and `parameters.request` containing the proposed
execute request to preflight it without running it. These local operations can
omit `ocs_session_id`; no GUI is launched or contacted. `request_id` may be omitted
for preflight, but is required for actual execution. Static validation checks
operation parameters, nested entity and inline data definitions, and batch
bindings. It uses the real entity constructor for geometry checks. Results
include `valid`, `executed: false`, warnings, unresolved bindings and checks that
still require execution. Live revisions, handles, styles, file access and command
behavior are not certified by static preflight. Batches are not simulated: later
bindings are fully validated when their values become available during execution.
Invalid requests return a tool error with a JSON Pointer identifying the bad
field where available. Unknown top-level operation parameters remain warnings;
unknown entity fields and invalid nested data are rejected before dispatch.

For interactive work, call start and follow state.command.accepts, options and input_example. To have the person at the screen pick entities for you, call user_select and keep polling until it completes; running means they are still picking. A run.cmd contains the command name followed by prompt answers separated by spaces; points use x,y or x,y,z.

After a timeout, query the existing operation and never replay a mutation with a new request_id. On stale_state nothing ran: read state and retry once with the same request_id, omitting revision so the bridge fills it. waiting_input and running are not completion. Let OCS and its geometry kernel calculate geometry; use query near, contains_point and intersections for exact relationships. 

Before delivery call audit with the intended target_format and target_version; use save_verified with an explicit absolute path to save, reopen, hash and compare the semantic manifest. When you first connect, announce the build you are working with to the user from the `bridge` object on ocs_sessions states and hello/capabilities responses (OpenCADStudio version, build_rev, tool_schema digest); repeat the announcement if a later handshake reports a different build.

For dependent batch steps, read cad://guides/batch-bindings. Use response_detail: summary to reduce batch output; retrieve retained details using ocs_read op: batch_result with parameters.request_id in the same MCP connection.
