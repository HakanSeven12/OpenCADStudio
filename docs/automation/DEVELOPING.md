# Developing the MCP bridge

Operator-facing setup lives in [README.md](README.md). This note is for
contributors iterating on the bridge itself: the stdio JSON-RPC layer in
`src/mcp.rs` and the operation validators in `src/mcp_ops.rs`.

## Fast iteration: no version bumps needed

- Run the bridge straight from a local build: `target/debug/OpenCADStudio --mcp`
  for speed, `target/release/OpenCADStudio --mcp` when close to shipping.
  Both report the same handshake; only the `build_profile` field differs.
- Tool-surface changes need no version-number change. `serverInfo` carries a
  content `tool_schema` digest plus the `build_rev` revision hash, so staleness
  is observable without a release.
- If you change any tool schema, the `tool_schema_digest_is_pinned` test
  fails: review the diff, then update `TOOL_SCHEMA_DIGEST` deliberately
  (never blindly).

## Sessions are per client connection

Clients bind the tool surface once when they connect, so after every
rebuild, restart the client session (or reconnect its MCP servers) to pick
up the new schema. The bridge helps: it watches its own executable and, once
it detects a rebuild, finishes the in-flight request and exits, so the next
call spawns a fresh bridge. No in-flight call ever fails because of this;
at worst the following call pays one respawn.

## Session discovery

`ocs_sessions` reads GUI descriptor files and verifies each one is alive
before probing it: process-liveness checks come first (cached snapshots
where the OS has no process table to read), dead descriptors are deleted,
and every remaining TCP probe uses a bounded timeout. Any step that cannot
run fails open toward treating the session as alive, so a broken check can
slow discovery but never wedge it.

Concurrent launches are serialized with `automation/starting.lock`
(`{"pid":..,"started":unix_secs}`, 60s TTL): the first `launch_if_none: true`
caller claims it and spawns the GUI, later callers with a fresh live claim
wait for the same descriptor instead of opening another window. Stale claims
and claims from dead pids are reclaimed; an unusable directory fails open so
a broken lock can never wedge launching. Covered by
`startup_lock_serializes_concurrent_launches` (claim, wait, stale-reclaim,
dead-pid-reclaim, release).

## Cancellation

`run()` reads stdin on a feeder thread into a channel (`CancelPump`); the
main loop stays the only stdout writer. Wait loops harvest the channel in
short quanta instead of sleeping blind. A `notifications/cancelled` naming
the in-flight request dismisses the GUI prompt with a second `{"op":"cancel"}`
exchange, drops the late result, and sends nothing back. Cancels arriving
between polls resolve through a pending-operation map; re-polls for dismissed
operations answer `cancelled` without touching the GUI. Interactive picks
(`user_select`, `getpoint`) have a ten-minute single-call ceiling ending in
an explicit timeout error; everything else keeps the one-minute clamp.

## Error routing

Tool-call failures are classified at the source (`CallError`): tool-domain
failures (bad arguments, failed operations) carry model-actionable guidance
and stay `isError` results; bridge and GUI infrastructure failures (no live
session, dead transport) become `-32603`; cancelled requests get no response.
Resource-not-found follows the negotiated era (`-32602` modern, `-32002`
legacy). Error results omit `structuredContent`: strict clients validate it
against `outputSchema`, which only describes success.

## Safety model

Never assume the client prompts before destructive calls. `ocs_execute` is
marked `destructiveHint: true`, but confirmation behavior is client-defined
and unobservable from here — server-side validation is the only safety net.
Validate every argument before touching the drawing, keep destructive and
read-only paths in separate ops, and prefer reversible effects.

## Rebuilding while the bridge runs

If the linker refuses to replace the binary while a bridge process runs
from it, move the running binary aside under a new name and rebuild. The
live bridge keeps serving from the moved file; restart the client
afterwards so it respawns from the fresh binary, and delete the moved file
once nothing runs from it anymore.

## Conformance gates

These tests hold the spec line; keep them green and extend them with every
protocol change: `tool_schema_digest_is_pinned`,
`initialize_is_spec_pure_with_experimental_build`,
`malformed_requests_get_jsonrpc_errors`,
`legacy_list_results_carry_sep2549_ttl`,
`resource_not_found_code_follows_era`,
`non_object_input_is_invalid_request`,
`unknown_tool_is_a_protocol_error_with_names`,
`cancel_matching_and_wait_deadlines`.
For end-to-end proof, run the official MCP Inspector CLI against a local
build (headless-safe methods only: `initialize`, `tools/list`,
`resources/list`, `resources/read` — anything touching the drawing needs a
live desktop session and stays in unit tests).
