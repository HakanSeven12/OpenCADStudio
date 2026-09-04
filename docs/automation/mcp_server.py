"""Local stdio MCP adapter for a running OpenCADStudio GUI."""
from pathlib import Path
import tempfile
from typing import Any
from mcp.server.fastmcp import FastMCP, Image
from live import LiveOcs, READ_OPS, launch, sessions

mcp = FastMCP("OpenCADStudio", instructions="Use ocs_sessions, then ocs_read(state/commands) before editing. All changes target the actual GUI document. Preserve session_id, document_id and revisions. waiting_input means the command or dialog still needs input. Poll running operations; never replay a mutation with a new request_id after a timeout. Geometry is handled by OCS and its kernel.")
clients: dict[str, LiveOcs] = {}


def client(session_id: str) -> LiveOcs:
    if session_id not in clients:
        clients[session_id] = LiveOcs(session_id)
    return clients[session_id]


@mcp.tool()
def ocs_sessions(launch_if_none: bool = True) -> list[dict[str, Any]]:
    """List real OCS GUI sessions and documents. Launch installed OCS if none is running."""
    available = launch() if launch_if_none else sessions()
    return [s["state"] for s in available]


@mcp.tool()
def ocs_read(session_id: str, op: str = "state", parameters: dict[str, Any] | None = None) -> dict[str, Any]:
    """Read state, commands/actions, query (type/layer/offset/limit), entities, layers, header, properties, measure(handles), history, events(after), operation(request_id)."""
    if op not in READ_OPS:
        raise ValueError("Use ocs_execute for mutations")
    return client(session_id).request({**(parameters or {}), "op": op})


@mcp.tool()
def ocs_execute(session_id: str, request: dict[str, Any], wait_seconds: float = 30) -> dict[str, Any]:
    """Execute one semantic action. op: new/open(path)/activate(document_id)/start(cmd)/run(cmd)/input(kind,text or point)/select(handles)/property(field,value)/action(name)/save(path)/undo/redo/cancel. Input kinds: text, token, point(space wcs/ucs/relative), entity or structure(handle,point), selection, enter. Supply document_id and revision from state. request_id is the idempotency key. Accepted/running is not completion. Query operation after a timeout."""
    return client(session_id).request(request, wait_seconds)


@mcp.tool()
def ocs_capture(session_id: str) -> Image:
    """Capture the actual current OCS window as PNG for visual verification."""
    with tempfile.TemporaryDirectory(prefix="ocs-capture-") as directory:
        path = Path(directory) / "window.png"
        result = client(session_id).request({"op": "capture", "path": str(path)})
        if not result.get("ok") or result.get("status") != "completed":
            raise RuntimeError(str(result))
        return Image(data=path.read_bytes(), format="png")


if __name__ == "__main__":
    mcp.run(transport="stdio")
