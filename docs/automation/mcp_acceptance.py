"""End-to-end GUI/MCP acceptance with versioned, verified CAD outputs."""

import base64
import json
from pathlib import Path
import struct
import sys
import time
import uuid

from mcp_eval import Client, META


def request_id(prefix: str) -> str:
    return f"accept-{prefix}-{uuid.uuid4().hex}"


def main() -> None:
    server = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio.exe").resolve()
    root = Path(sys.argv[2] if len(sys.argv) > 2 else "target/acceptance").resolve()
    output = root / time.strftime("%Y%m%d-%H%M%S")
    output.mkdir(parents=True, exist_ok=False)
    client = Client(server)
    session = None
    report: dict = {"ok": False, "server": str(server), "output": str(output)}
    try:
        sessions = client.tool("ocs_sessions", {"launch_if_none": True})["result"]
        session = sessions[0]["session_id"]
        while sessions[0].get("modal"):
            client.tool(
                "ocs_execute",
                {
                    "ocs_session_id": session,
                    "request": {
                        "op": "action",
                        "request_id": request_id("close-modal"),
                        "name": "close_modal",
                    },
                },
            )
            sessions = client.tool("ocs_sessions", {"launch_if_none": False})["result"]

        client.tool(
            "ocs_execute",
            {
                "ocs_session_id": session,
                "request": {"op": "new", "request_id": request_id("new")},
            },
        )
        drawing = client.tool(
            "ocs_execute",
            {
                "ocs_session_id": session,
                "response_detail": "changed_entities",
                "request": {
                    "op": "batch",
                    "request_id": request_id("draw"),
                    "steps": [
                        {"op": "run", "cmd": "PLINE 0,0 200,0 200,120 0,120 C"},
                        {"op": "run", "cmd": "CIRCLE 50,60 15"},
                        {"op": "run", "cmd": "CIRCLE 150,60 15"},
                        {"op": "run", "cmd": "LINE 0,60 200,60"},
                        {"op": "run", "cmd": "TEXT 100,135 8 0 OPEN CAD MCP ACCEPTANCE"},
                        {"op": "action", "name": "zoom_extents"},
                    ],
                },
            },
        )
        assert drawing["completed_steps"] == 6, drawing

        audits = {}
        saves = {}
        targets = [("dwg", "2000"), ("dwg", "2013"), ("dwg", "2018"), ("dxf", "2000")]
        for target_format, target_version in targets:
            key = f"{target_format}-{target_version}"
            audit = client.tool(
                "ocs_read",
                {
                    "ocs_session_id": session,
                    "op": "audit",
                    "parameters": {
                        "target_format": target_format,
                        "target_version": target_version,
                    },
                },
            )
            assert audit["ok"] is True, audit
            audits[key] = audit
            destination = output / f"mcp-acceptance-{target_version}.{target_format}"
            saved = client.tool(
                "ocs_execute",
                {
                    "ocs_session_id": session,
                    "response_detail": "full",
                    "request": {
                        "op": "save_verified",
                        "request_id": request_id(f"save-{key}"),
                        "path": str(destination),
                        "target_format": target_format,
                        "target_version": target_version,
                    },
                },
            )
            verified = saved.get("result", saved)
            assert verified["verified"] is True, saved
            assert destination.is_file(), destination
            saves[key] = verified

        capture = client.rpc(
            "tools/call",
            {
                "name": "ocs_capture",
                "arguments": {
                    "ocs_session_id": session,
                    "scope": "viewport",
                    "max_dimension": 1600,
                },
                "_meta": META,
            },
        )
        image = capture["content"][0]
        assert image["type"] == "image" and image["mimeType"] == "image/png", capture
        png = base64.b64decode(image["data"], validate=True)
        assert png[:8] == b"\x89PNG\r\n\x1a\n", "capture is not a PNG"
        width, height = struct.unpack(">II", png[16:24])
        capture_path = output / "viewport.png"
        capture_path.write_bytes(png)

        report.update(
            {
                "ok": True,
                "session_id": session,
                "drawn_entities": drawing["changed_entities"],
                "audits": audits,
                "saves": saves,
                "capture": {"path": str(capture_path), "width": width, "height": height},
                "metrics": {
                    "tool_calls": client.tool_calls,
                    "tasks": client.tasks,
                    "rpc_calls": client.rpc_calls,
                },
            }
        )
    finally:
        client.close()
        report_path = output / "report.json"
        report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
