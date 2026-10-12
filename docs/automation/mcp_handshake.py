"""Bounded stdio regression probe; point this at the packaged launcher on macOS."""
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading


def probe(command: list[str], timeout: float = 10) -> None:
    # A separate process group lets a failed probe clean up a broken launcher
    # and any GUI child it incorrectly spawned. Never touch existing editors.
    with tempfile.TemporaryFile() as stderr:
        process = subprocess.Popen(
            [*command, "--mcp"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8", start_new_session=os.name == "posix",
            env={**os.environ, "OCS_SKIP_SCHEMA_SYNC": "1"},
        )
        lines = queue.Queue()

        def read_stdout() -> None:
            try:
                for line in process.stdout:
                    lines.put(line)
            finally:
                lines.put(None)

        threading.Thread(target=read_stdout, daemon=True).start()

        def request(request_id: int, method: str, params: dict) -> dict:
            process.stdin.write(json.dumps({
                "jsonrpc": "2.0", "id": request_id,
                "method": method, "params": params,
            }) + "\n")
            process.stdin.flush()
            # stdin stays open while waiting: a server buffering until EOF
            # must fail this test just like the launcher that ignores --mcp.
            try:
                line = lines.get(timeout=timeout)
            except queue.Empty as error:
                raise AssertionError(f"Timed out waiting for {method}") from error
            assert line is not None, f"stdout closed before {method} responded"
            response = json.loads(line)
            assert response["jsonrpc"] == "2.0", response
            assert response["id"] == request_id, response
            assert "error" not in response, response
            return response["result"]

        try:
            initialized = request(1, "initialize", {
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "packaged-handshake", "version": "1"},
            })
            assert initialized["protocolVersion"] == "2025-11-25", initialized
            assert initialized["serverInfo"]["name"] == "OpenCADStudio", initialized
            process.stdin.write(
                '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'
            )
            process.stdin.flush()
            tools = request(2, "tools/list", {})
            assert {tool["name"] for tool in tools["tools"]} == {
                "ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture",
            }, tools
            request(3, "ping", {})
            process.stdin.close()
            assert process.wait(timeout=timeout) == 0, process.returncode
        finally:
            if os.name == "posix":
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            elif process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            process.stdin.close()
            process.stdout.close()
            stderr.seek(0)
            diagnostics = stderr.read().decode("utf-8", errors="replace")
            if diagnostics:
                print(diagnostics, file=sys.stderr, end="")


if __name__ == "__main__":
    probe([str(Path(sys.argv[1]).resolve())])
    print("MCP initialize, tools/list, ping and EOF shutdown passed")
