---
name: opencadstudio-control
description: Inspect and control the user's live OpenCADStudio desktop session through its persistent MCP tools. Use for drawing, editing, selecting, measuring, changing properties or layers, opening/saving/exporting files, reviewing the current OCS UI, or visually verifying OCS results. Do not use for source-code changes to the OpenCADStudio repository.
---

# OpenCADStudio Control

Use the `ocs_*` MCP tools to act on the actual running application. Call `ocs_sessions` first; it opens the installed application when none is running. If multiple sessions exist, select the intended `session_id` from the listed documents instead of guessing.

Read `state` before an edit. Carry its `session_id`, `document_id`, `revision`, and selection into mutations. Treat `request_id` as the idempotency key: after a timeout, query that operation and never replay it with a new ID until its absence is established. Refresh state after `stale_state`, `selection_changed`, `document_closed`, or `session_changed`.

Prefer semantic commands and inputs:

- Use `start` followed by typed `input` calls for interactive geometry, points, entity handles, structure picks, selection completion, text, Enter, and cancellation.
- Use `run` for a complete command line, and respect `waiting_input`; inspect its prompt and language-independent option keywords before continuing.
- Use `property` with IDs returned by `properties`, and `action` with IDs returned by `commands`.
- Let OCS and its geometry kernel calculate geometry. Verify important results with entity/document queries and a current `ocs_capture` image.
- Save to an explicit path, wait for `completed`, reopen when persistence or round-trip correctness matters, and inspect state afterward.

Keep user edits and open tabs intact unless the task requires changing them. A successful transport response can still report `waiting_input`, `failed`, `cancelled`, or a running operation; only `completed` means that operation finished.
