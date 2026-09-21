# Driving the web build

The web build exposes the same control core as `--serve` and `--mcp`, through two
functions on the WebAssembly module. This page covers only what is specific to the
browser; the operations, request envelope and replies are the ones in the
[README](README.md).

## Reaching the control channel

The Trunk loader publishes the wasm-bindgen module as `window.wasmBindings`. The
control channel is ready once `window.wasmBindings?.ocs_control_submit` is defined
(the [save smoke test](web_save_smoke.cjs) waits for exactly that).

| Function | Behaviour |
|---|---|
| `ocs_control_submit(request: string) → string` | Queues one request (a JSON string) and returns a ticket immediately. |
| `ocs_control_take(ticket: string) → string \| undefined` | Returns the reply (a JSON string) once it is ready, `undefined` before that. A reply is removed when taken. |

Poll `ocs_control_take` until it returns a reply, as the smoke test does:

```js
async function control(request) {
  const api = window.wasmBindings;
  const ticket = api.ocs_control_submit(JSON.stringify(request));
  for (let i = 0; i < 600; i++) {
    const reply = api.ocs_control_take(ticket);
    if (reply) return JSON.parse(reply);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error('control timed out');
}
```

Limits: at most 64 requests wait in the queue (a further `submit` is answered with the
`busy` error), and only the newest 64 replies are kept, so take a reply soon after it is
ready. A request that is not valid JSON is answered with `op: "invalid"` and a
`parse_error`.

## Opening a drawing from bytes

The web build has no file system, so `open` takes the file's bytes instead of a path:

```json
{"op":"open","name":"plan.dwg","data_base64":"…"}
```

`name` is reduced to its file name and labels the tab and the recent-files entry;
`data_base64` must be valid base64 and non-empty (`invalid_request` otherwise). While an
earlier drawing is still opening the request is answered with `busy`; retry it once
`state` shows the tab. A `path` is resolved against the browser's "recent files" store
rather than a disk, so use `data_base64` for a file the page has not seen.
`data_base64` is rejected on native builds; use `path` there.

## Startup dialogs

A dialog can be open when the page is ready. The donation prompt appears once per
application version in each browser profile, so a fresh or disposable profile always
sees it. While a modal is open `state.modal` is set, and `{"op":"new"}` returns `ok`
without creating a tab until the modal is closed. Close it first:

```js
const state = await control({ op: 'state' });
if (state.modal) await control({ op: 'action', name: 'close_modal' });
await control({ op: 'new' });
```

Native builds can show a second dialog at launch (the file-association prompt), so check
`state.modal` again after closing one.
