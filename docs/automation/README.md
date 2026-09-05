# OpenCADStudio MCP control

Every native OpenCADStudio build contains the same MCP server as the editor. `OpenCADStudio --mcp` starts it over stdio, opens the desktop editor when needed, and exposes the live document without Python, a package manager, a sidecar service, or client-specific code.

MCP lets an AI client inspect the open drawing, execute editor commands, and verify the result through a shared protocol. Install OpenCADStudio, then add a local MCP server in the client and set its command to:

```sh
OpenCADStudio --mcp
```

The client must start that command over standard input/output. If it asks for the executable and arguments separately, select the installed `OpenCADStudio` executable and enter `--mcp` as its only argument. The exact registration screen or configuration location belongs to the client. Once connected, the four tools below should appear in the client's MCP tool list.

The server provides four tools:

- `ocs_sessions` finds running editor sessions and opens OpenCADStudio when none exists.
- `ocs_read` reads document state, command manifests, entities, layers, properties, kernel measurements and spatial relationships, history, events, and operation status.
- `ocs_execute` performs one operation or a sequential batch against the real editor.
- `ocs_capture` returns a bounded PNG of the drawing viewport or complete window.

The normal flow is to call `ocs_sessions`, read the chosen session with `ocs_read`, then pass the returned document state into `ocs_execute`. For example, an undo request has this shape:

```json
{
  "session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "undo",
    "request_id": "undo-1",
    "document_id": 1,
    "revision": 12
  }
}
```

To discover command syntax without reading application source, call `ocs_read` with `op: "commands"`. The unfiltered response lists commands and actions. Add `parameters: {"name": "PLINE"}` for a command's batch examples and interactive guidance.

A complete command can be sent with `run`. Prompt answers are separated by spaces, points use `x,y` or `x,y,z`, and option answers use their displayed token:

```json
{
  "session_id": "SESSION_FROM_OCS_SESSIONS",
  "request": {
    "op": "run",
    "request_id": "polyline-1",
    "document_id": 1,
    "revision": 12,
    "cmd": "PLINE 0,0 10,0 10,10 C"
  }
}
```

Use `batch` when the steps are already known. OCS supplies each step with the state produced by the previous one and stops on the first failure. `completed_steps` and `next_step` show exactly what committed:

```json
{
  "session_id": "SESSION_FROM_OCS_SESSIONS",
  "response_detail": "changed_entities",
  "request": {
    "op": "batch",
    "request_id": "shape-1",
    "steps": [
      {"op": "run", "cmd": "LINE 0,0 10,0"},
      {"op": "run", "cmd": "CIRCLE 5,5 2"}
    ]
  }
}
```

Execute responses use `response_detail: "compact"` by default and return only the state needed for the next edit. Use `changed_entities` to receive the current geometry of affected handles in the same response, or `full` when the complete editor state is needed.

`ocs_read` query accepts exact handles, type/layer filters, field projection, world-XY bounds, nearest-curve ranking, closed-curve containment, and exact intersections between two planar curves. Nearest points, containment, curve length, area, and intersections are calculated by the geometry kernel:

```json
{
  "session_id": "SESSION_FROM_OCS_SESSIONS",
  "op": "query",
  "parameters": {"intersections": ["2A", "31"]}
}
```

For unfamiliar or conditional commands, use `start`, then inspect `state.command` in every response. Its `accepts` array gives the valid MCP input kinds, `options` gives the current tokens, and `input_example` gives the next request shape. Add the current state fields and a new `request_id` to each step.

Every `ocs_execute` request requires a caller-generated `request_id`. Reuse that ID only to retry the identical request after a timeout, together with the same session ID, document ID, expected revision, and selection. Commands report `waiting_input` while more input is required and asynchronous work remains `running` until its real callback finishes. Geometry stays in OpenCADStudio and its geometry kernel.

`ocs_capture` defaults to the drawing viewport and a longest edge of 1600 pixels. Set `scope` to `window` for the full interface or change `max_dimension` between 256 and 4096.

Clients using MCP 2026-07-28 can advertise `io.modelcontextprotocol/tasks`. OCS then returns a standard task handle when an operation is still running and accepts `tasks/get`, `tasks/update`, and `tasks/cancel`. Other clients continue to receive the existing operation status and can read it with `ocs_read`.

The source-tree protocol smoke test treats the executable as a black box and uses only the Python standard library. It checks both supported protocol styles, the published schemas, structured errors, and the four-tool surface:

```sh
cargo build
python3 docs/automation/mcp_smoke.py target/debug/OpenCADStudio
```

The repeatable live-editor evaluation draws three isolated entities in one batch, verifies exact intersections, nearest geometry and kernel measurements, removes the entities, and reports call count, elapsed time and wire bytes:

```sh
python3 docs/automation/mcp_eval.py target/debug/OpenCADStudio
```
