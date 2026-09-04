"""Protocol smoke test for the native client-neutral MCP endpoint."""
import json
from pathlib import Path
import subprocess
import sys

def main() -> None:
    server = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    process = subprocess.Popen(
        [str(server), "--mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert process.stdin and process.stdout

    def request(payload: dict) -> dict:
        process.stdin.write(json.dumps(payload) + "\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())

    initialized = request({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "1"}},
    })
    assert initialized["result"]["serverInfo"]["name"] == "OpenCADStudio"
    assert initialized["result"]["instructions"]
    process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    process.stdin.flush()
    tools = request({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    names = {tool["name"] for tool in tools["result"]["tools"]}
    assert names == {"ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"}, names
    sessions = request({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "ocs_sessions", "arguments": {"launch_if_none": False}},
    })
    assert not sessions["result"].get("isError"), sessions
    process.stdin.close()
    assert process.wait(timeout=5) == 0

if __name__ == "__main__":
    main()
