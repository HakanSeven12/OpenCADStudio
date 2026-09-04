# OpenCADStudio MCP control

Every native OpenCADStudio build contains the same MCP server as the editor. `OpenCADStudio --mcp` starts it over stdio, opens the desktop editor when needed, and exposes the live document without Python, a package manager, a sidecar service, or client-specific code.

The server provides four tools:

- `ocs_sessions` finds running editor sessions and opens OpenCADStudio when none exists.
- `ocs_read` reads document state, commands, entities, layers, properties, measurements, history, events, and operation status.
- `ocs_execute` performs semantic commands and input against the real editor.
- `ocs_capture` returns a PNG of the current window for visual verification.

Mutations carry a session ID, document ID, expected revision, selection, and idempotent request ID. Commands report `waiting_input` while more input is required and asynchronous work remains `running` until its real callback finishes. Geometry stays in OpenCADStudio and its geometry kernel.

Release builds also produce one standard MCP Bundle containing the native binaries for supported desktop platforms. The release workflow publishes its `server.json` metadata to the official MCP Registry. Compatible AI hosts can therefore discover and install the same server without an OpenCADStudio-specific adapter.

Registry discovery and permission prompts belong to the AI host. Hosts that implement MCP Registry and MCP Bundle support need no OpenCADStudio-specific setup; other hosts cannot be enabled silently by the application.

The source-tree protocol smoke test uses only the Python standard library:

```sh
cargo build
python3 docs/automation/mcp_smoke.py target/debug/OpenCADStudio
```
