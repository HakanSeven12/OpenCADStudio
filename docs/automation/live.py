"""Persistent local GUI client; no CAD implementation lives in this adapter."""
from __future__ import annotations
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import uuid
from typing import Any

READ_OPS = {"state", "hello", "query", "entities", "layers", "header", "properties", "measure", "history", "commands", "events", "operation"}


def discovery_dir() -> Path:
    if sys.platform == "win32":
        base = Path(os.environ["APPDATA"])
    elif sys.platform == "darwin":
        base = Path.home() / "Library/Application Support"
    else:
        base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return base / "OpenCADStudio/automation"


def exchange(descriptor: dict, request: dict, timeout: float = 15) -> dict:
    payload = {**request, "token": descriptor["token"], "session_id": descriptor["session_id"], "protocol": 1}
    wire = (json.dumps(payload, ensure_ascii=False) + "\n").encode()
    if len(wire) > 1_048_576:
        raise ValueError("Request exceeds 1 MiB")
    with socket.create_connection(("127.0.0.1", descriptor["port"]), timeout=timeout) as sock:
        sock.sendall(wire)
        with sock.makefile("rb") as stream:
            line = stream.readline(16 * 1024 * 1024 + 1)
        if not line or len(line) > 16 * 1024 * 1024:
            raise ConnectionError("No valid OCS response; query request_id before retrying a mutation")
        return json.loads(line)


def sessions() -> list[dict]:
    found = []
    for path in sorted(discovery_dir().glob("*.json")):
        descriptor = None
        try:
            if os.name == "posix" and (path.stat().st_uid != os.getuid() or path.stat().st_mode & 0o077):
                continue
            descriptor = json.loads(path.read_text())
            state = exchange(descriptor, {"op": "hello"}, timeout=1)
            if state.get("ok") and state.get("session_id") == descriptor["session_id"]:
                found.append({"descriptor": descriptor, "state": state})
        except (OSError, ValueError, KeyError):
            # A crashed process cannot remove its descriptor. Only reap a file
            # whose recorded PID is certainly gone; a busy live session keeps
            # its discovery record for the next call.
            try:
                os.kill(int(descriptor["pid"]), 0)
            except (ProcessLookupError, TypeError, KeyError):
                try:
                    path.unlink()
                except OSError:
                    pass
            continue
    return found


class LiveOcs:
    def __init__(self, session_id: str | None = None):
        available = sessions()
        if session_id:
            available = [s for s in available if s["state"]["session_id"] == session_id]
        if len(available) != 1:
            raise ValueError("Choose session_id from sessions; found " + str(len(available)))
        self.descriptor = available[0]["descriptor"]
        self.state = available[0]["state"]
        self.client_id = uuid.uuid4().hex

    def request(self, request: dict, wait_seconds: float = 30) -> dict:
        req = dict(request)
        if req.get("op") not in READ_OPS:
            req.setdefault("request_id", uuid.uuid4().hex)
            req.setdefault("client_id", self.client_id)
            req.setdefault("document_id", self.state["document_id"])
            req.setdefault("revision", self.state["revision"])
            if req.get("op") in {"input", "property", "run", "action", "save", "undo", "redo"}:
                req.setdefault("selection", self.state.get("selection", []))
        response = exchange(self.descriptor, req)
        deadline = time.monotonic() + min(max(wait_seconds, 0), 60)
        while response.get("status") in ("accepted", "running") and time.monotonic() < deadline:
            time.sleep(0.05)
            response = exchange(self.descriptor, {"op": "operation", "request_id": req["request_id"]})
        if "state" in response:
            self.state = response["state"]
        elif req.get("op") in ("hello", "state") and response.get("ok"):
            self.state = response
        return response


def launch(binary: str | None = None) -> list[dict]:
    existing = sessions()
    if existing:
        return existing
    executable = binary or os.environ.get("OCS_BINARY", str(Path.home() / ".local/share/ocs-control/OpenCADStudio"))
    log_dir = Path.home() / ".local/share/ocs-control"
    log_dir.mkdir(parents=True, exist_ok=True)
    with (log_dir / "gui.log").open("ab") as log:
        proc = subprocess.Popen([executable, "--new-instance"], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"OCS exited ({proc.returncode}); inspect {log_dir / 'gui.log'}")
        available = sessions()
        if available:
            return available
        time.sleep(0.2)
    raise TimeoutError("OCS is still starting; call sessions again")


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("request", nargs="?", help="JSON request; omit to list sessions")
    parser.add_argument("--session")
    parser.add_argument("--launch", action="store_true")
    args = parser.parse_args()
    if args.launch:
        launch()
    if args.request:
        print(json.dumps(LiveOcs(args.session).request(json.loads(args.request)), ensure_ascii=False))
    else:
        print(json.dumps([s["state"] for s in sessions()], ensure_ascii=False))
