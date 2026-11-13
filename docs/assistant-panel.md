# Assistant panel

`ASSISTANT` (alias `AI`, or **View ▸ Palettes ▸ Assistant**) opens a dockable
chat pallet for an AI agent that drives the open drawing. The pallet is only
the user interface: the agent itself is an external service running on the
same machine, usually an LLM client that operates OCS through the existing
`--mcp` endpoint. No model, API key or vendor is built into OpenCAD Studio.

What the user sees:

* their requests as chat bubbles and the agent's replies as Markdown;
* one card per tool step the agent takes (title, running / ✓ / ✗, duration),
  folded by default, opened automatically when a step fails, with the step's
  input and result and any viewport captures inline;
* a short "thinking" line while the agent plans;
* quick-request chips, **Stop** while it works, **New chat**, and a header
  with the drawing, the model, the tool count and the running cost the
  service reports.

Every change the agent makes goes through the normal command and undo paths,
so Ctrl+Z undoes it like any other edit.

## Connecting a service

The pallet connects to `127.0.0.1:8766` (`OCS_ASSISTANT_PORT` changes it).

If nothing is listening when the pallet opens and `OCS_ASSISTANT_COMMAND` is
set, OCS starts that command hidden and keeps retrying for up to 40 seconds:

| Variable | Meaning |
| :--- | :--- |
| `OCS_ASSISTANT_COMMAND` | Command line that starts the service; double quotes group a path with spaces. |
| `OCS_ASSISTANT_DIR` | Working folder for that command (optional). |
| `OCS_ASSISTANT_PORT` | Loopback port (default `8766`). Also passed to the service it starts. |

A service started this way also receives `OCS_EXE`, the path of the running
OpenCAD Studio executable, so it can spawn `"%OCS_EXE%" --mcp` and drive this
very build. Its output goes to `ocs-assistant-service.log` in the temp folder.

Example (Windows):

```
setx OCS_ASSISTANT_COMMAND "\"C:\Tools\ocs-agent\.venv\Scripts\python.exe\" -m app.server"
setx OCS_ASSISTANT_DIR "C:\Tools\ocs-agent"
```

## Protocol

UTF-8 JSON, one object per line (`\n`) in each direction, over a plain TCP
connection on the loopback interface. Each connection is one conversation.
Unknown message types and unknown fields are ignored on both sides.

### Panel → service

| `type` | Fields | Meaning |
| :--- | :--- | :--- |
| `chat` | `text` | A user request. |
| `stop` | | Cancel the running request after the current step. |
| `reset` | | Forget the conversation (the drawing is unchanged). |
| `refresh` | | Send a fresh `status`. The pallet sends it every 10 s while idle. |

### Service → panel

| `type` | Fields | Meaning |
| :--- | :--- | :--- |
| `status` | `ocs` (bool), `drawing`, `path`, `model`, `effort`, `tools` (int), `api_key` (bool), `running` (bool), `cost_usd` (number) | Header state; send on connect, on `refresh` and after `done`. |
| `text_start` | | A reply begins. |
| `text` | `text` | Reply text, streamed in pieces (Markdown). |
| `progress` | `text` | Planning / thinking summary, streamed. |
| `tool_pending` | `id`, `name` | A tool call is being prepared. |
| `tool_start` | `id`, `name`, `title`, `args` | The step runs; `title` is a short human label, `args` its input as text. |
| `tool_end` | `id`, `ok` (bool), `ms` (int), `preview`, `images` (data URLs) | The step finished; `preview` is its result as text, `images` any PNG/JPEG captures as `data:` URLs. |
| `error` | `text` | A message to show as an error. |
| `cancelled` | | The request was stopped. |
| `done` | `cost_usd` (optional) | The request is over; the pallet accepts input again. |

A minimal session:

```
→ {"type":"chat","text":"Select the doors"}
← {"type":"tool_start","id":"t1","name":"ocs_select","title":"Select by layer","args":"{\"layer\":\"DOORS\"}"}
← {"type":"tool_end","id":"t1","ok":true,"ms":41,"preview":"12 selected","images":[]}
← {"type":"text_start"}
← {"type":"text","text":"Selected **12** doors on layer DOORS."}
← {"type":"done","cost_usd":0.03}
```

## Security

The pallet only connects to the loopback interface, and only starts a program
the user named in `OCS_ASSISTANT_COMMAND`. Whatever the service does in the
drawing, it does through `--mcp`, with the same permissions as any other
automation client.
