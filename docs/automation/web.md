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

The page also loads `ocs-control.js`, which wraps the two functions in promises:

| API | Behaviour |
|---|---|
| `await window.ocsControl.ready` | Resolves once the control channel exists; the `ocs-ready` window event fires at the same moment. |
| `await window.ocsControl.request(request, {timeoutMs, pollMs})` | Submits one request and resolves with the final reply, following `accepted`/`running` replies through `op: "operation"`. Rejects only on timeout or a missing channel. |

Without the wrapper, poll `ocs_control_take` until it returns a reply, as the smoke test does:

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

## Getting the drawing back

`save` serializes the active drawing and returns it in the reply, without a download:

```json
{"op":"save","name":"plan.dwg","target_format":"dwg","target_version":"AC1032"}
```

The reply carries `name`, `target_format`, `target_version`, `dropped_on_save`, `bytes`
and `data_base64`, and the tab is marked clean. `target_format` and `target_version`
default to the drawing's own; a conversion that would drop unsupported records needs
`allow_lossy: true`, as on native.

## Supplying fonts

The web build has no font folder. A text style whose TrueType or OpenType file is not one
of the bundled Noto subsets falls back to them. The host page can hand the file over:

```json
{"op":"font_register","file_name":"Brand-Regular.ttf","data_base64":"…"}
```

`file_name` is the name the drawing's text styles use; it is matched case-insensitively and
without a directory, as for installed fonts. The reply carries the `family` inside the file
and the number of `faces` it added. Open drawings lay their text out again. The font stays
registered for the session and is never written into saved files. Bytes without a readable
face fail with `font_invalid`; a missing `file_name` or empty or invalid base64 with
`invalid_request`.

## Receiving saved files in the host page

A page that embeds the app can take every saved or exported file itself instead of a
browser download by defining a function on the app's window before saving:

```js
window.openCadStudioSaveFile = (name, bytes) => {
  // name: file name, bytes: Uint8Array
};
```

While it is defined, Save, Save As, the save-before-close prompt and the exports call it
with the file name and the bytes, and no file picker opens. Without it nothing changes.

## Startup dialogs

A dialog can be open when the page is ready. The donation prompt appears once per
application version in each browser profile, so a fresh or disposable profile always
sees it — unless the session is automated: the first control request drops a prompt
that is still queued, and a host that sets `localStorage["opencadstudio.automation"]`
to `"1"` before the page loads never gets one. A prompt already on screen stays. While a modal is open `state.modal` is set, and `{"op":"new"}` returns `ok`
without creating a tab until the modal is closed. Close it first:

```js
const state = await control({ op: 'state' });
if (state.modal) await control({ op: 'action', name: 'close_modal' });
await control({ op: 'new' });
```

Native builds can show a second dialog at launch (the file-association prompt), so check
`state.modal` again after closing one.
