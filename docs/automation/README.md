# OpenCADStudio automation

`--serve` remains available for isolated headless scripts. `live.py` connects to the real desktop application through a private per-process descriptor under the user's OCS configuration directory. The GUI owns every mutation: transport threads only validate and enqueue requests.

The live protocol requires protocol version 1, stable session/document IDs, an expected edit revision for mutations, and a unique request ID. Commands report `waiting_input` while interactive work remains. Asynchronous file and renderer work remains `running` until its real callback finishes. A bounded operation cache makes retries with the same payload idempotent; after a restart or an expired operation, clients must read state instead of replaying a mutation.

Operations include document state and activation, paged entity queries, layer/header/property inspection, command discovery, start/run/step/cancel, WCS/UCS/relative points, entity and structure picks, selection, property edits, semantic UI actions, undo/redo, protected save, event polling, and current-window PNG capture. The status bar includes a control switch whose green, yellow, and red states mean ready, busy, and off.

The Web build exposes `ocs_control_submit(json)` and `ocs_control_take(ticket)`. They enter the same bounded GUI queue and return the same semantic responses; browser code polls the ticket until a response is present.

`OpenCADStudio --mcp` is the common local entry point for every MCP client. It is built into the application, uses stdio, launches the editor when needed, and has no Python or package-manager dependency. Tool definitions and safety instructions are served by MCP itself, so the drawing behavior is identical in every client.

Run `./docs/automation/install.sh` once for a source build. It installs the release binary in `~/.local/share/ocs-control` and automatically connects installed Codex and Claude Code CLIs. Both registrations point to the same binary. Re-run an individual connection later with:

```sh
~/.local/share/ocs-control/connect.sh codex
~/.local/share/ocs-control/connect.sh claude-code
```

Any other local MCP client uses the same command and argument. `connect.sh config` prints the portable configuration object to paste into clients that do not provide a registration CLI:

```json
{"mcpServers":{"opencadstudio":{"command":"/path/to/OpenCADStudio","args":["--mcp"]}}}
```

Client-specific skills and extension manifests may improve discovery, but contain no drawing or protocol implementation. Adding another AI client therefore requires only registering this command; the OpenCADStudio executable and tools do not change.

The legacy client is unchanged:

```python
from ocs import Ocs
with Ocs(binary="OpenCADStudio") as app:
    app.new()
    app.run("LINE 0,0 100,0")
```

For direct live diagnostics:

```sh
python3 docs/automation/live.py --launch
python3 docs/automation/live.py '{"op":"state"}'
```
