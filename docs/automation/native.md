# Driving the native builds

Every native build has three entry points besides the editor window: the MCP
server described in the [README](README.md) (`OpenCADStudio --mcp`), a headless
JSON-line channel for scripts and CI (`OpenCADStudio --serve`), and a one-shot
converter (`OpenCADStudio --export IN OUT`). The browser entry is described in
[web.md](web.md).

## Choosing a channel

| | `--mcp` | `--serve` |
|---|---|---|
| Transport | MCP (JSON-RPC over stdio) with the four tools of the README | one JSON request per line on stdin, one reply per line on stdout; `--port N` listens on `127.0.0.1:N` instead |
| Editor | attaches to a running desktop editor, or starts one when there is none (`ocs_sessions`, `launch_if_none`) | no window; a document session of its own that lives as long as the process |
| Needs a display | yes (`xvfb-run` works on a headless machine; software rendering may raise the GPU warning dialog) | no |
| Needs a writable `HOME` / `XDG_CONFIG_HOME` | yes — the session descriptors live in the user configuration directory; without one `ocs_sessions` answers `No user configuration directory` | no |
| Startup dialogs | those of the editor (below) | none |
| `capture` | `ocs_capture` | not available (`gui_required`): there is no window to capture |
| Good for | an MCP client driving a real editor | batch jobs, CI, agents without a display |

## `--serve`

The first line written is a readiness banner, before any request is read:

```json
{"ok":true,"ready":true,"version":"2026.38"}
```

Over `--port N` the banner opens every connection; the listener serves one
client at a time and the document session persists across reconnects.

Two request styles share the channel, told apart by the `protocol` field:

| Style | Shape | Operations |
|---|---|---|
| protocol 1 | `{"protocol":1,"request_id":"…","document_id":N,"op":"…"}` — the same requests and replies as `ocs_execute` / `ocs_read`, described in the README | `new`, `open`, `save`, `activate`, `select`, `run`, `start`, `input`, `cancel`, `stop`, `action`, `property`, `set_properties`, `embed_image`, `undo`, `redo`; reads `state`, `capabilities`, `query`, `entities`, `records`, `record_schema`, `layers`, `header`, `properties`, `measure`, `history`, `commands`, `events`, `operation` |
| legacy | `{"op":"…"}` without `protocol` | `new`, `open`, `run`, `entities`, `query`, `records`, `record_schema`, `capabilities`, `layers`, `header`, `select`, `save`, `undo`, `redo` |

The two styles differ in how a waiting command is treated. A protocol-1 `run`
or `start` while a command is still active is refused with `command_busy`:
continue it with `input` or drop it with `cancel`. A legacy `run` line ends the
waiting command and starts the new one.

### The legacy `run` line

`{"op":"run","cmd":"LINE 0,0 10,10"}` feeds the tokens after the command name
to the prompts in order and then presses Enter. The reply is

```json
{"ok":true,"cmd":"CIRCLE 0,0 5 9","status":"completed","blocked_by":null,"entities":1,"added":1,"unconsumed":["9"]}
```

| Field | Meaning |
|---|---|
| `status` | `completed`, or `waiting_input` when the line left something open |
| `blocked_by` | what *this line* left open: `"command"`, `"text_editor"`, `"mtext_editor"`, `"modal:<Kind>"`, or `null`. A surface that was already open when the line started is not attributed to it |
| `entities`, `added` | entity count after the line, and the difference to before |
| `unconsumed` | tokens no prompt asked for, in order (`["9"]` above: the radius step ended the command); `[]` when everything was consumed |

`waiting_input` means input is still required, not that nothing happened:
`SPLINE` and `ARC` keep accepting input after the entity exists, and
`zoom_extents` reports it after the view has already moved.

`TEXT 0,0 5 0 hello world` creates one `Text` entity holding `hello world`: the
tail of the line is typed into the in-place editor, committed, and the command
is closed so it does not stay armed for a second line. A `TEXT` line without
content answers `waiting_input` with `blocked_by: "text_editor"`.

Known gap (#1390): `MTEXT`, `HATCH` and `POLYGON` answer `completed` with
`added: 0` on this path — they need interactive surfaces (the rich-text editor,
a pattern choice, a boundary pick) that a single line has no tokens to fill.

## `--mcp`

The MCP server is a client of the desktop editor: `ocs_sessions` lists the live
editor sessions found in the configuration directory and starts one when none
is running, so several MCP connections can share one editor and its state.

A fresh profile opens two dialogs before any drawing exists, and both block
document creation:

| Step | `ocs_sessions` reports |
|---|---|
| right after launch | `modal: "AssocPrompt"` |
| after `close_modal` | `modal: "DonationPrompt"` |
| after a second `close_modal` | `modal: null` |

While a dialog is open, `{"op":"new"}` answers `ok` but no tab is created.
Close the dialogs first:

```python
while (info := sessions())["modal"]:
    execute({"op": "action", "name": "close_modal", "document_id": info["document_id"]})
```

The donation prompt appears once per application version per profile, so an
automated profile meets it once per version rather than on every run.

`new` makes the new drawing the active document. Requests carry `document_id`;
take it from the reply's state and pass it on the following requests, as the
README describes.

## `--export`

`OpenCADStudio --export IN OUT` loads `IN` and writes `OUT` without a window or
a session. The output format follows the extension: `.dxf` writes DXF, any other
extension writes DWG at the document's own version. The exit code is 0 on
success and 1 when the input cannot be read or the output cannot be written.

## Interactive steps

For commands whose answers cannot be written on one line, use `start` and then
`input`. `start` replies with `state.command`, whose `accepts`, `options` and
`input_example` say what the current step wants:

| Step | Request | Note |
|---|---|---|
| a point | `{"op":"input","kind":"point","point":[x,y,0],"space":"wcs"}` | the point is an array; `space` is `wcs` (default), `ucs` or `relative` |
| a typed value (height, rotation) | `{"op":"input","kind":"text","text":"5"}` | digits go through `text`, not `token`; `{"op":"input","kind":"enter"}` accepts the default |
| a keyword (`J`, `ST`, `C`) | `{"op":"input","kind":"token","text":"C"}` | the payload field is named `text` |
| an object | `{"op":"input","kind":"entity","handle":"HANDLE","point":[x,y,0]}` | `kind: "structure"` for a sub-entity pick, `kind: "selection"` to hand over the current selection |
| the content of `TEXT` | `{"op":"action","name":"text_input","value":"hello"}` then `{"op":"action","name":"text_commit"}` | the command has already ended; the in-place editor owns the input |
| the content of `MTEXT` | `{"op":"action","name":"mtext_insert","value":"hello"}` then `{"op":"action","name":"mtext_commit"}` | the rich-text editor has its own actions; the `TEXT` ones answer `editor_closed` |

## Native-only operations

| Operation | `--serve` (protocol 1) | `--mcp` |
|---|---|---|
| measure | `{"protocol":1,"op":"measure","handles":["2A"]}` — kernel-computed length, area, bounds and mass properties; no window needed | `ocs_read` with `op: "measure"` and `parameters.handles` |
| capture | `{"protocol":1,"op":"capture","path":"out.png"}` — needs the editor window, so it answers `gui_required` here | `ocs_capture` with `scope` (`viewport` or `window`) and `max_dimension` |
| `set_properties` | requires `collection`; colours are the serialised enum — `{"Index": 1}` or `"ByLayer"`; a colour name such as `"Red"` is rejected with `invalid_value` | same request through `ocs_execute` |
