"""Protocol smoke test for the native client-neutral MCP endpoint."""
import json
from pathlib import Path
import subprocess
import sys


TOOLS = {"ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"}
MODERN = "2026-07-28"


def start(server: Path) -> subprocess.Popen[str]:
    process = subprocess.Popen(
        [str(server), "--mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert process.stdin and process.stdout
    return process


def request(process: subprocess.Popen[str], payload: dict) -> dict:
    assert process.stdin and process.stdout
    process.stdin.write(json.dumps(payload) + "\n")
    process.stdin.flush()
    return json.loads(process.stdout.readline())


def close(process: subprocess.Popen[str]) -> None:
    assert process.stdin
    process.stdin.close()
    assert process.wait(timeout=5) == 0


def legacy(server: Path) -> None:
    process = start(server)

    initialized = request(process, {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "1"}},
    })
    assert initialized["result"]["serverInfo"]["name"] == "OpenCADStudio"
    assert "state.command.accepts" in initialized["result"]["instructions"]
    process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    process.stdin.flush()
    tools = request(process, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    definitions = {tool["name"]: tool for tool in tools["result"]["tools"]}
    names = set(definitions)
    assert names == TOOLS, names
    execute_request = definitions["ocs_execute"]["inputSchema"]["properties"]["request"]
    assert len(execute_request["oneOf"]) == 14
    assert execute_request["properties"]["cmd"]["examples"][0] == "LINE 0,0 10,10"
    assert execute_request["properties"]["kind"]["enum"] == [
        "text", "token", "point", "entity", "structure", "selection", "enter"
    ]
    for name in {"ocs_sessions", "ocs_read", "ocs_execute"}:
        assert definitions[name]["outputSchema"]["type"] == "object"
    assert "resultType" not in tools["result"]
    sessions = request(process, {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "ocs_sessions", "arguments": {"launch_if_none": False}},
    })
    assert not sessions["result"].get("isError"), sessions
    close(process)


def modern(server: Path) -> None:
    process = start(server)
    meta = {
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "smoke", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {},
    }
    discovered = request(process, {
        "jsonrpc": "2.0",
        "id": "discover",
        "method": "server/discover",
        "params": {"_meta": meta},
    })
    assert discovered["result"]["resultType"] == "complete"
    assert discovered["result"]["ttlMs"] >= 0
    assert discovered["result"]["cacheScope"] in {"public", "private"}

    tools = request(process, {
        "jsonrpc": "2.0",
        "id": "tools",
        "method": "tools/list",
        "params": {"_meta": meta},
    })
    assert {tool["name"] for tool in tools["result"]["tools"]} == TOOLS
    assert tools["result"]["resultType"] == "complete"
    assert tools["result"]["ttlMs"] >= 0
    assert tools["result"]["cacheScope"] in {"public", "private"}

    missing_cmd = request(process, {
        "jsonrpc": "2.0",
        "id": "missing-cmd",
        "method": "tools/call",
        "params": {
            "name": "ocs_execute",
            "arguments": {
                "session_id": "missing",
                "request": {"op": "run", "request_id": "run-1"},
            },
            "_meta": meta,
        },
    })
    error = missing_cmd["result"]["structuredContent"]
    assert error["code"] == "invalid_arguments"
    assert "LINE 0,0 10,10" in error["error"]

    invalid_mutation = request(process, {
        "jsonrpc": "2.0",
        "id": "invalid-mutation",
        "method": "tools/call",
        "params": {
            "name": "ocs_execute",
            "arguments": {"session_id": "missing", "request": {"op": "undo"}},
            "_meta": meta,
        },
    })
    assert invalid_mutation["result"]["isError"] is True
    assert "request_id" in invalid_mutation["result"]["structuredContent"]["error"]

    rejected = request(process, {
        "jsonrpc": "2.0",
        "id": "unsupported",
        "method": "tools/list",
        "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2099-01-01"}},
    })
    assert rejected["error"]["code"] == -32022
    close(process)


def main() -> None:
    server = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    legacy(server)
    modern(server)

if __name__ == "__main__":
    main()
