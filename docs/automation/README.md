# OpenCADStudio automation

`--serve` remains available for isolated headless scripts. `live.py` connects to the real desktop application through a private per-process descriptor under the user's OCS configuration directory. The GUI owns every mutation: transport threads only validate and enqueue requests.

The live protocol requires protocol version 1, stable session/document IDs, an expected edit revision for mutations, and a unique request ID. Commands report `waiting_input` while interactive work remains. Asynchronous file and renderer work remains `running` until its real callback finishes. A bounded operation cache makes retries with the same payload idempotent; after a restart or an expired operation, clients must read state instead of replaying a mutation.

Operations include document state and activation, paged entity queries, layer/header/property inspection, command discovery, start/run/step/cancel, WCS/UCS/relative points, entity and structure picks, selection, property edits, semantic UI actions, undo/redo, protected save, event polling, and current-window PNG capture. The status bar includes a control switch whose green, yellow, and red states mean ready, busy, and off.

The Web build exposes `ocs_control_submit(json)` and `ocs_control_take(ticket)`. They enter the same bounded GUI queue and return the same semantic responses; browser code polls the ticket until a response is present.

Run `./docs/automation/install.sh` once. It installs the release binary and pinned MCP environment in `~/.local/share/ocs-control`, registers the `opencadstudio` stdio server in the user's Codex configuration, and leaves source-independent launch paths. The personal `opencadstudio-control` skill tells later Codex sessions how to use it safely.

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
