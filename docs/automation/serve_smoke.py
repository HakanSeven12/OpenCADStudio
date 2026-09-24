#!/usr/bin/env python3
"""Black-box smoke test for the headless automation server (`--serve`).

Exercises the ops an external client needs: state discovery, the wblock
export, the plot-to-PDF export, and the capability advertisement. Uses only
the Python standard library; treats the executable as a black box.

    python3 docs/automation/serve_smoke.py target/debug/OpenCADStudio
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path


class Serve:
    def __init__(self, exe: Path):
        self.process = subprocess.Popen(
            [str(exe), "--serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
        )
        self.ready = json.loads(self.process.stdout.readline())
        assert self.ready["ok"] is True, self.ready
        assert self.ready["session_id"], self.ready

    def send(self, request: dict) -> dict:
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        return json.loads(self.process.stdout.readline())

    def close(self) -> None:
        self.process.stdin.close()
        self.process.wait(timeout=30)


def main() -> None:
    exe = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    out_dir = Path(tempfile.mkdtemp(prefix="ocs_serve_smoke_"))
    server = Serve(exe)
    try:
        state = server.send({"protocol": 1, "op": "state"})
        document_id = state["document_id"]

        # Draw something to export and plot.
        server.send({"op": "new"})
        for request_id, cmd in [("run-1", "LINE 0,0 100,80"), ("run-2", "CIRCLE 50,40 20")]:
            response = server.send({"op": "run", "cmd": cmd})
            assert response["ok"] is True, response

        # Read-only ops stay on the legacy (non-envelope) shape.
        summary = server.send({"op": "entities"})
        assert summary["total"] == 2, summary

        # wblock: export the two entities to a standalone DXF.
        export = out_dir / "wblock.dxf"
        response = server.send({
            "protocol": 1,
            "op": "wblock",
            "request_id": "smoke-wblock-1",
            "document_id": document_id,
            "path": str(export),
            "handles": [
                entity["handle"]
                for entity in server.send({"op": "query", "detail": "summary"})["entities"]
            ],
        })
        assert response["ok"] is True, response
        assert response["status"] == "completed", response
        assert response["result"]["entities"] == 2, response
        assert export.exists()

        # plot: render model space to a one-page PDF.
        pdf = out_dir / "plot.pdf"
        response = server.send({
            "protocol": 1,
            "op": "plot",
            "request_id": "smoke-plot-1",
            "document_id": document_id,
            "path": str(pdf),
        })
        assert response["ok"] is True, response
        assert response["result"]["pages"] == 1, response
        assert pdf.read_bytes()[:4] == b"%PDF"

        # The file operations are discoverable through capabilities.
        capabilities = server.send({"op": "capabilities"})
        operations = capabilities["operations"]
        assert operations["wblock"] is True, operations
        assert operations["plot"] is True, operations
        assert operations["embed_image"] is True, operations
        print("serve smoke: OK")
    finally:
        server.close()


if __name__ == "__main__":
    main()
