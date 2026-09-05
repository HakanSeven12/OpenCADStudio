"""Repeatable black-box speed and capability check for the native MCP server."""

import json
from pathlib import Path
import subprocess
import sys
import time


MODERN = "2026-07-28"
META = {
    "io.modelcontextprotocol/protocolVersion": MODERN,
    "io.modelcontextprotocol/clientInfo": {"name": "ocs-eval", "version": "1"},
    "io.modelcontextprotocol/clientCapabilities": {
        "extensions": {"io.modelcontextprotocol/tasks": {}}
    },
}


class Client:
    def __init__(self, server: Path) -> None:
        self.process = subprocess.Popen(
            [str(server), "--mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.serial = 0
        self.rpc_calls = 0
        self.tool_calls = 0
        self.tasks = 0
        self.request_bytes = 0
        self.response_bytes = 0

    def rpc(self, method: str, params: dict) -> dict:
        assert self.process.stdin and self.process.stdout
        self.serial += 1
        payload = {"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params}
        wire = json.dumps(payload, separators=(",", ":"))
        self.request_bytes += len(wire.encode())
        self.rpc_calls += 1
        self.process.stdin.write(wire + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        self.response_bytes += len(line.encode())
        response = json.loads(line)
        if "error" in response:
            raise RuntimeError(response["error"])
        return response["result"]

    def tool(self, name: str, arguments: dict) -> dict:
        self.tool_calls += 1
        result = self.rpc("tools/call", {"name": name, "arguments": arguments, "_meta": META})
        if result.get("resultType") == "task":
            self.tasks += 1
            task_id = result["taskId"]
            poll_interval = result.get("pollIntervalMs", 250)
            while True:
                time.sleep(poll_interval / 1000)
                task = self.rpc("tasks/get", {"taskId": task_id, "_meta": META})
                if task["status"] in {"failed", "cancelled"}:
                    raise RuntimeError(task)
                if task["status"] == "completed":
                    result = task["result"]
                    break
        structured = result.get("structuredContent")
        if structured is None:
            raise RuntimeError(result)
        if structured.get("ok") is False:
            raise RuntimeError(structured)
        return structured

    def close(self) -> None:
        assert self.process.stdin
        self.process.stdin.close()
        if self.process.wait(timeout=5) != 0:
            raise RuntimeError(self.process.stderr.read() if self.process.stderr else "MCP exited")


def main() -> None:
    server = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    client = Client(server)
    started = time.perf_counter()
    handles: list[str] = []
    session: str | None = None
    base = 0
    succeeded = False
    try:
        discovery = client.rpc("server/discover", {"_meta": META})
        assert "io.modelcontextprotocol/tasks" in discovery["capabilities"]["extensions"]
        sessions = client.tool("ocs_sessions", {"launch_if_none": True})["result"]
        session = sessions[0]["session_id"]
        active = next(
            document for document in sessions[0]["documents"]
            if document["id"] == sessions[0]["document_id"]
        )
        if active.get("start"):
            client.tool(
                "ocs_execute",
                {"session_id": session, "request": {"op": "new", "request_id": "eval-new"}},
            )

        base = 1_000_000 + int(time.time()) % 100_000
        draw = client.tool(
            "ocs_execute",
            {
                "session_id": session,
                "response_detail": "changed_entities",
                "wait_seconds": 0,
                "request": {
                    "op": "batch",
                    "request_id": f"eval-draw-{base}",
                    "steps": [
                        {"op": "run", "cmd": f"LINE {base-5},0 {base+5},0"},
                        {"op": "run", "cmd": f"LINE {base},-5 {base},5"},
                        {"op": "run", "cmd": f"CIRCLE {base+20},0 2"},
                    ],
                },
            },
        )
        assert draw["completed_steps"] == 3, draw
        assert client.tasks > 0, "wait_seconds=0 did not exercise MCP Tasks"
        changed = draw["changed_entities"]
        line_handles = [entity["handle"] for entity in changed if entity["type"] == "Line"]
        circle_handles = [entity["handle"] for entity in changed if entity["type"] == "Circle"]
        assert len(line_handles) == 2 and len(circle_handles) == 1, changed
        handles = line_handles + circle_handles

        crossings = client.tool(
            "ocs_read",
            {"session_id": session, "op": "query", "parameters": {"intersections": line_handles}},
        )
        assert crossings["count"] == 1 and crossings["intersections"][0]["point"] == [base, 0.0]

        nearest = client.tool(
            "ocs_read",
            {"session_id": session, "op": "query", "parameters": {"near": [base + 20, 0], "handles": handles, "limit": 1}},
        )
        assert nearest["entities"][0]["handle"] == circle_handles[0]

        measured = client.tool(
            "ocs_read",
            {"session_id": session, "op": "measure", "parameters": {"handles": circle_handles}},
        )
        assert abs(measured["measurements"][0]["curve"]["area"] - 12.566370614359172) < 1e-9
        succeeded = True
    finally:
        if handles and session:
            try:
                client.tool(
                    "ocs_execute",
                    {
                        "session_id": session,
                        "request": {
                            "op": "batch",
                            "request_id": f"eval-clean-{base}",
                            "steps": [
                                {"op": "select", "handles": handles},
                                {"op": "run", "cmd": "ERASE"},
                            ],
                        },
                    },
                )
            except Exception:
                pass
        elapsed = (time.perf_counter() - started) * 1000
        metrics = {
            "ok": succeeded,
            "elapsed_ms": round(elapsed, 1),
            "tool_calls": client.tool_calls,
            "tasks": client.tasks,
            "rpc_calls": client.rpc_calls,
            "request_bytes": client.request_bytes,
            "response_bytes": client.response_bytes,
        }
        client.close()
        print(json.dumps(metrics, indent=2))


if __name__ == "__main__":
    main()
