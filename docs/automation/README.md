# OpenCADStudio MCP control

Every native OpenCADStudio build contains the same MCP server as the editor. `OpenCADStudio --mcp` starts it over stdio, opens the desktop editor when needed, and exposes the live document without Python, a package manager, a sidecar service, or client-specific code.

MCP lets an AI client inspect the open drawing, execute editor commands, and verify the result through a shared protocol. Install OpenCADStudio, then add a local MCP server in the client and set its command to:

```sh
OpenCADStudio --mcp
```

The client must start that command over standard input/output. If it asks for the executable and arguments separately, select the installed `OpenCADStudio` executable and enter `--mcp` as its only argument. The exact registration screen or configuration location belongs to the client. Once connected, the four tools below should appear in the client's MCP tool list.

The server provides four tools:

- `ocs_sessions` finds running editor sessions and opens OpenCADStudio when none exists.
- `ocs_read` reads document state, commands, entities, layers, properties, measurements, history, events, and operation status.
- `ocs_execute` performs semantic commands and input against the real editor.
- `ocs_capture` returns a PNG of the current window for visual verification.

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

For unfamiliar or conditional commands, use `start`, then inspect `state.command` in every response. Its `accepts` array gives the valid MCP input kinds, `options` gives the current tokens, and `input_example` gives the next request shape. Add the current state fields and a new `request_id` to each step.

Every `ocs_execute` request requires a caller-generated `request_id`. Reuse that ID only to retry the identical request after a timeout, together with the same session ID, document ID, expected revision, and selection. Commands report `waiting_input` while more input is required and asynchronous work remains `running` until its real callback finishes. Geometry stays in OpenCADStudio and its geometry kernel.

The source-tree protocol smoke test treats the executable as a black box and uses only the Python standard library. It checks both supported protocol styles, the published schemas, structured errors, and the four-tool surface:

```sh
cargo build
python3 docs/automation/mcp_smoke.py target/debug/OpenCADStudio
```
