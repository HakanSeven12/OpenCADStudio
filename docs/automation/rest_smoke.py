#!/usr/bin/env python3
"""Black-box smoke test for the REST transport (`--http <port>`).

Drives the whole entity lifecycle over plain HTTP — the same API surface any
language can call — against a real drawing session: create → verify by query
→ transform → mark with xdata → define a block → save → reopen → verify
everything persisted → erase → undo. Then exercises the P1 surface the same
way, always through real files: where filters, sysvars, the structured layout
lifecycle + page setup reads/writes + percent-encoded names + per-page plotting
to real PDFs, a .dwt template → new-from-template round trip, a cross-document
copy between two open documents, groups and selection
sets, and a close-with-discard. Uses only the Python standard library.

    python3 docs/automation/rest_smoke.py target/debug/OpenCADStudio
"""

import json
import os
import secrets
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


TOKEN = secrets.token_hex(32)


def call(port: int, method: str, path: str, body=None):
    """One HTTP call; returns (status, parsed-json-or-None)."""
    data = None
    headers = {"Content-Type": "application/json", "Authorization": f"Bearer {TOKEN}"}
    if body is not None:
        data = json.dumps(body).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/v1{path}", data=data, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as error:
        raw = error.read()
        return error.code, (json.loads(raw) if raw else None)


def wait_ready(process, port: int, timeout: float = 60.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process.poll() is not None:
            raise SystemExit(f"server exited early: {process.returncode}")
        try:
            status, body = call(port, "GET", "/ready")
            if status == 200 and body.get("ok"):
                return
        except (urllib.error.URLError, ConnectionError, OSError):
            time.sleep(0.25)
    raise SystemExit("server never became ready")


def expect(condition, message):
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    exe = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    port = 8091
    out_dir = Path(tempfile.mkdtemp(prefix="ocs_rest_smoke_"))
    process = subprocess.Popen(
        [str(exe), "--http", str(port)],
        env={**os.environ, "OCS_API_TOKEN": TOKEN},
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        wait_ready(process, port)

        # 0. Readiness + capability discovery.
        status, ready = call(port, "GET", "/ready")
        expect(status == 200 and ready["session_id"], f"ready: {ready}")
        status, capabilities = call(port, "GET", "/capabilities")
        expect(capabilities["operations"]["entities_create"] is True, "capabilities")
        for operation in (
            "layouts", "page_setup_get", "layout_create", "layout_rename",
            "layout_activate", "layout_delete", "page_setup_set", "xrefs",
            "xref_reload", "xref_unload", "xref_detach", "xref_bind",
            "xref_set_path", "xref_set_type",
        ):
            expect(capabilities["operations"].get(operation) is True,
                   f"structured capability {operation}: {capabilities}")
        status, references = call(port, "GET", "/xrefs")
        expect(status == 200 and references["count"] == 0
               and references["xrefs"] == [], f"empty xref read: {references}")
        status, missing = call(port, "POST", "/xref_reload", {"id": "FFFFFFFF"})
        expect(status == 404 and missing["code"] == "xref_missing",
               f"structured xref missing refusal: {missing}")

        # 1. Create a batch of typed entities; the missing layer is created.
        status, created = call(port, "POST", "/entities", {
            "entities": [
                {"type": "Line", "start": [0, 0], "end": [100, 0], "layer": "FRAME"},
                {"type": "LwPolyline", "vertices": [[0, 0], [100, 0], [100, 60], [0, 60]], "closed": True},
                {"type": "Text", "value": "PAGE-01", "position": [10, 50], "height": 3.0},
                {"type": "Circle", "center": [80, 30], "radius": 5},
            ]
        })
        expect(status == 201 and created["result"]["created"] == 4, f"create: {created}")
        handles = created["result"]["handles"]
        expect(created["result"]["layers_created"] == ["FRAME"], "layer auto-created")

        # 2. Verify by query — the geometry really landed.
        status, query = call(port, "GET", "/entities?type=Line&detail=full")
        expect(query["entities"][0]["end"] == [100.0, 0.0, 0.0], f"line geometry: {query}")
        status, query = call(port, "GET", f"/entities/{handles[2]}")
        expect(query["entities"][0]["value"] == "PAGE-01", "text content by handle")

        # 3. Transform: move the circle and verify, then array the frame.
        status, moved = call(port, "POST", "/entities/transform", {
            "handles": [handles[3]], "action": "move", "vector": [10, 0],
        })
        expect(moved["result"]["affected"] == 1, f"move: {moved}")
        status, query = call(port, "GET", f"/entities/{handles[3]}")
        expect(query["entities"][0]["center"][0] == 90.0, "circle moved")

        status, copied = call(port, "POST", "/entities/transform", {
            "handles": [handles[0]], "action": "copy", "vector": [0, -10],
        })
        expect(len(copied["result"]["created"]) == 1, f"copy: {copied}")

        # 4. Mark the text with extended data (RegApp registered implicitly).
        status, marked = call(
            port, "PUT", f"/entities/{handles[2]}/xdata/SPM",
            [{"code": 1000, "value": "PAGE-01"}, {"code": 1070, "value": 3}],
        )
        expect(marked["result"]["updated"] == 1, f"xdata put: {marked}")
        status, xdata = call(port, "GET", f"/entities/{handles[2]}/xdata?app=SPM")
        expect(xdata["items"][0]["xdata"]["SPM"][0] == "PAGE-01", "xdata read")

        # 5. Define a block from the two frame lines, BLOCK-command style.
        status, block = call(port, "POST", "/blocks", {
            "name": "FRAME-MARK", "base": [0, 0, 0], "handles": [handles[0], copied["result"]["created"][0]],
        })
        expect(status == 201 and block["result"]["block"] == "FRAME-MARK", f"block: {block}")

        # 6. Save → reopen: everything survives the file round trip.
        saved = out_dir / "rest_smoke.dwg"
        status, saved_response = call(port, "POST", "/save", {"path": str(saved)})
        expect(saved_response["ok"], f"save: {saved_response}")
        status, opened = call(port, "POST", "/documents", {"path": str(saved)})
        expect(opened["ok"], f"reopen: {opened}")

        status, query = call(port, "GET", "/entities?type=Insert&detail=full")
        expect(query["count"] == 1 and query["entities"][0]["block"] == "FRAME-MARK", "insert persisted")
        status, query = call(port, "GET", "/entities?type=Text")
        text_handle = query["entities"][0]["handle"]
        status, xdata = call(port, "GET", f"/entities/{text_handle}/xdata?app=SPM")
        expect(xdata["items"][0]["xdata"]["SPM"][1] == 3, "xdata persisted")

        # 7. Erase + undo — the standard lifecycle close.
        status, erased = call(port, "DELETE", f"/entities?handles={text_handle}")
        expect(erased["result"]["erased"] == 1, f"erase: {erased}")
        status, undone = call(port, "POST", "/undo")
        expect(undone["ok"], f"undo: {undone}")
        status, query = call(port, "GET", "/entities?type=Text")
        expect(query["count"] == 1, "undo restored the text")

        # 8. Where filters over entity properties (RFC 6901 pointers).
        status, circles = call(port, "POST", "/entities", {
            "entities": [
                {"type": "Circle", "center": [0, -40], "radius": 2},
                {"type": "Circle", "center": [30, -40], "radius": 8, "color": 1},
            ]
        })
        expect(status == 201, f"filter circles: {circles}")
        where = urllib.parse.quote(json.dumps([{"path": "/radius", "op": "gt", "value": 6}]))
        status, query = call(port, "GET", f"/entities?type=Circle&detail=geometry&where={where}")
        expect(query["count"] == 1 and query["entities"][0]["radius"] == 8.0, f"where filter: {query}")
        where = urllib.parse.quote(json.dumps([{"path": "/layer", "op": "eq", "value": "FRAME"}]))
        status, query = call(port, "GET", f"/entities?type=Line&where={where}")
        expect(query["count"] == 2
               and all(entity["layer"] == "FRAME" for entity in query["entities"]),
               f"summary layer where: {query}")
        where = urllib.parse.quote(json.dumps([
            {"path": "/properties/common/color/Index", "op": "eq", "value": 1},
        ]))
        status, query = call(port, "GET", f"/entities?type=Circle&detail=full&where={where}")
        expect(query["count"] == 1 and query["entities"][0]["radius"] == 8.0,
               f"full common-color where: {query}")
        circle_handles = circles["result"]["handles"]
        status, reset = call(port, "POST", "/entities_set", {
            "handles": [circle_handles[1]], "color": "ByLayer",
            "label": "REST smoke color reset",
        })
        expect(status == 200 and reset["result"] == {
            "requested": 1, "written": 1, "unchanged": 0,
        }, f"entities_set reset: {reset}")
        status, repeated = call(port, "POST", "/entities_set", {
            "handles": [circle_handles[1]], "color": "ByLayer",
        })
        expect(repeated["result"]["written"] == 0
               and repeated["result"]["unchanged"] == 1,
               f"entities_set no-op: {repeated}")
        status, refused = call(port, "POST", "/entities_set", {
            "handles": [circle_handles[1], "FFFFFFFF"], "color": 1,
        })
        expect(status == 404 and refused["code"] == "entity_absent",
               f"entities_set atomic refusal: {refused}")
        status, query = call(port, "GET", f"/entities/{circle_handles[1]}")
        expect(query["entities"][0]["properties"]["common"]["color"] == "ByLayer",
               f"bad handle caused no partial color write: {query}")
        status, undone = call(port, "POST", "/undo")
        expect(undone["ok"], f"entities_set undo: {undone}")
        status, query = call(port, "GET", f"/entities/{circle_handles[1]}")
        expect(query["entities"][0]["properties"]["common"]["color"] == {"Index": 1},
               f"entities_set one-step undo: {query}")
        status, query = call(port, "GET", f"/entities?where={urllib.parse.quote('not-json')}")
        expect(status == 400 and query["code"] == "invalid_where", f"bad where: {query}")

        # 9. Sysvars: set over POST, read back over GET, unknown refused.
        status, set_response = call(port, "POST", "/sysvars", {"set": {"ltscale": 3.5, "mirrtext": 1}})
        expect(status == 200 and set_response["ok"], f"sysvar set: {set_response}")
        status, read_back = call(port, "GET", "/sysvars?names=ltscale,mirrtext")
        expect(read_back["result"]["values"]["ltscale"] == 3.5, f"sysvar read: {read_back}")
        expect(read_back["result"]["values"]["mirrtext"] == 1, "mirrtext read back")
        status, refused = call(port, "POST", "/sysvars", {"set": {"not_a_sysvar": 1}})
        expect(status == 400 and refused["code"] == "unknown_sysvar", f"unknown sysvar: {refused}")

        # 10. Sheets: structured list/create/get/rename/activate/delete,
        # page setup field preservation, encoded names, then per-layout PDFs.
        status, listing = call(port, "GET", "/layouts")
        expect(status == 200 and listing["current"] == "Model", f"layout list: {listing}")
        model = next(item for item in listing["layouts"] if item["name"] == "Model")
        expect(set(model) == {"name", "handle", "current", "model", "tab_order"},
               f"structured Model entry: {model}")
        expect(model["current"] is True and model["model"] is True, "Model is current")

        status, layout = call(port, "POST", "/layouts", {"name": "PLAN"})
        expect(status == 201 and "PLAN" in layout["result"]["layouts"], f"layout: {layout}")
        status, listing = call(port, "GET", "/layouts")
        plan = next(item for item in listing["layouts"] if item["name"] == "PLAN")
        expect(listing["current"] == "Model" and plan["current"] is False,
               "layout_create does not activate")

        status, setup = call(port, "PUT", "/layouts/PLAN/page-setup", {
            "paper": "ISO_A4_(210.00_x_297.00_MM)", "orientation": "portrait",
            "scale": "1:50", "center": False, "offset_x": 12.5, "offset_y": -4.0,
            "area": "window", "window": [0, 0, 100, 60],
        })
        expect(status == 200 and "ISO_A4" in setup["result"]["paper"], f"page setup: {setup}")
        status, setup = call(port, "GET", "/layouts/plan/page-setup")
        expect(status == 200 and setup["layout"] == "PLAN", f"canonical page setup: {setup}")
        expect(setup["page_setup"]["offset_x"] == 12.5
               and setup["page_setup"]["offset_y"] == -4.0
               and setup["page_setup"]["area"] == "window"
               and setup["page_setup"]["window"] == [0.0, 0.0, 100.0, 60.0],
               f"offset/area/window applied: {setup}")
        status, updated = call(port, "PUT", "/layouts/PLAN/page-setup", {
            "orientation": "landscape",
        })
        expect(status == 200, f"orientation-only setup: {updated}")
        status, setup = call(port, "GET", "/layouts/PLAN/page-setup")
        page_setup = setup["page_setup"]
        expect(page_setup["orientation"] == "landscape" and page_setup["rotation"] == 90,
               f"orientation applied: {setup}")
        expect(page_setup["scale"] == {"numerator": 1.0, "denominator": 50.0, "fit": False}
               and page_setup["center"] is False and page_setup["offset_x"] == 12.5
               and page_setup["offset_y"] == -4.0 and page_setup["area"] == "window"
               and "ISO_A4" in page_setup["paper"],
               f"orientation-only update preserves setup: {setup}")

        status, renamed = call(port, "PATCH", "/layouts/plan", {"new_name": "Floor Plan"})
        expect(status == 200 and renamed["result"]["name"] == "PLAN"
               and renamed["result"]["new_name"] == "Floor Plan", f"layout rename: {renamed}")
        encoded_name = urllib.parse.quote("Floor Plan", safe="")
        status, activated = call(port, "POST", "/layouts/floor%20plan/activate", {})
        expect(status == 200 and activated["result"]["layout"] == "Floor Plan"
               and activated["result"]["current"] == "Floor Plan", f"layout activate: {activated}")
        status, setup = call(port, "GET", f"/layouts/{encoded_name}/page-setup")
        expect(status == 200 and setup["layout"] == "Floor Plan", f"encoded page setup: {setup}")
        status, listing = call(port, "GET", "/layouts")
        current = next(item for item in listing["layouts"] if item["current"])
        expect(listing["current"] == "Floor Plan" and current["name"] == "Floor Plan",
               f"canonical current layout: {listing}")

        plot_base = out_dir / "smoke_plot.pdf"
        status, plotted = call(port, "POST", "/plot", {
            "path": str(plot_base), "layout": "all", "per_page": True,
        })
        expect(plotted["ok"] and len(plotted["result"]["files"]) >= 2, f"per-page plot: {plotted}")
        for entry in plotted["result"]["files"]:
            pdf = Path(entry["path"])
            expect(pdf.read_bytes().startswith(b"%PDF"), f"not a PDF: {pdf}")
            pdf.unlink()

        status, deleted = call(port, "DELETE", f"/layouts/{encoded_name}")
        expect(status == 200 and deleted["result"]["deleted"] == "Floor Plan"
               and deleted["result"]["current"] == "Model", f"layout delete: {deleted}")
        status, missing = call(port, "GET", f"/layouts/{encoded_name}/page-setup")
        expect(status == 404 and missing["code"] == "layout_missing", f"deleted layout: {missing}")

        # 11. Template round trip: save as .dwt, then start the drawing from
        # the template — the entities and their xdata survive.
        status, query = call(port, "GET", "/entities")
        count_before = query["count"]
        template = out_dir / "rest_smoke.dwt"
        status, saved_dwt = call(port, "POST", "/save", {"path": str(template)})
        expect(saved_dwt["ok"], f"dwt save: {saved_dwt}")
        expect(template.stat().st_size > 64, ".dwt written (DWG bytes)")
        status, templated = call(port, "POST", "/documents", {"template": str(template)})
        expect(templated["ok"] and templated["result"]["total"] == count_before,
               f"new from template: {templated}")
        status, query = call(port, "GET", "/entities?type=Text")
        expect(query["count"] == 1, "text survived the template")
        text_handle = query["entities"][0]["handle"]
        status, xdata = call(port, "GET", f"/entities/{text_handle}/xdata?app=SPM")
        expect(xdata["items"][0]["xdata"]["SPM"][0] == "PAGE-01", "xdata survived the template")

        # 12. Cross-document copy: an empty POST /documents is
        # A fresh second document in its own tab.
        status, state = call(port, "GET", "/state")
        source_doc = state["document_id"]
        status, fresh = call(port, "POST", "/documents", {})
        expect(status == 201 and fresh["ok"], f"second document: {fresh}")
        status, state = call(port, "GET", "/state")
        target_doc = state["document_id"]
        expect(target_doc != source_doc, "two open documents")
        status, switched = call(port, "POST", "/activate", {"document_id": source_doc})
        expect(switched["ok"], f"activate source: {switched}")
        status, query = call(port, "GET", "/entities?type=Line")
        line_handles = [entity["handle"] for entity in query["entities"]]
        expect(len(line_handles) == 2, f"source lines: {query}")
        status, copy_op = call(port, "POST", "/entities/copy-to", {
            "handles": line_handles, "document_id": target_doc,
        })
        expect(status == 201 and copy_op["result"]["count"] == 2, f"copy-to: {copy_op}")
        status, switched = call(port, "POST", "/activate", {"document_id": target_doc})
        expect(switched["ok"], f"activate target: {switched}")
        status, query = call(port, "GET", "/entities?type=Line")
        expect(query["count"] == 2, f"lines copied into the second document: {query}")

        # 13. Groups, selection sets, block replace/delete and a stable file
        # identity over the second document.
        all_lines = [entity["handle"] for entity in query["entities"]]
        status, group = call(port, "POST", "/groups", {
            "name": "SMOKE-GROUP", "handles": all_lines,
        })
        expect(status == 201 and group["ok"], f"group: {group}")
        status, selection = call(port, "POST", "/selection-sets", {
            "name": "smoke-set", "handles": all_lines,
        })
        expect(status == 201 and selection["ok"], f"selection set save: {selection}")
        status, recalled = call(port, "GET", "/selection-sets/smoke-set")
        expect(len(recalled["result"]["handles"]) == 2, f"selection set load: {recalled}")
        status, selected = call(port, "GET", "/selection-sets/smoke-set?select=true")
        expect(selected["result"]["selected"] == 2, f"selection set select: {selected}")
        status, identity = call(port, "POST", "/file-identity", {})
        expect(status == 200 and len(identity["result"]["identity"]) == 36,
               f"file identity: {identity}")
        status, again = call(port, "POST", "/file-identity", {})
        expect(again["result"]["identity"] == identity["result"]["identity"],
               "file identity stable")
        status, block = call(port, "POST", "/blocks", {
            "name": "MARK", "base": [0, 0, 0], "handles": all_lines,
        })
        expect(status == 201, f"block define: {block}")
        # The define consumed the source lines; replace wraps fresh content
        # (SPM re-import draws new content and blocks it under the barcode).
        status, fresh = call(port, "POST", "/entities", {
            "entities": [{"type": "Point", "location": [9, 9]}],
        })
        expect(status == 201, f"fresh point: {fresh}")
        status, replaced = call(port, "POST", "/blocks", {
            "name": "MARK", "base": [0, 0, 0],
            "handles": fresh["result"]["handles"], "replace": True,
        })
        expect(status == 201, f"block replace: {replaced}")
        status, deleted = call(port, "DELETE", "/blocks/MARK?force=true")
        expect(status == 200 and deleted["result"]["erased"] >= 1, f"block delete: {deleted}")

        # 14. Close the dirty second document with discard.
        status, closed = call(port, "DELETE", f"/documents/{target_doc}?discard=true")
        expect(status == 200 and closed["result"]["closed"] is True, f"close: {closed}")
        status, state = call(port, "GET", "/state")
        expect(state["document_id"] == source_doc, "back on the first document")

        print("rest smoke: OK")
    finally:
        process.terminate()
        process.wait(timeout=30)


if __name__ == "__main__":
    main()
