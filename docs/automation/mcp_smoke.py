"""Protocol smoke test for the native client-neutral MCP endpoint."""
import json
from pathlib import Path
import subprocess
import sys


TOOLS = {
    "ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture",
    "ocs_inspect", "ocs_query", "ocs_find_text", "ocs_audit", "ocs_commands",
    "ocs_measure", "ocs_render", "ocs_run", "ocs_new", "ocs_open", "ocs_save",
    "ocs_save_verified", "ocs_select", "ocs_undo", "ocs_redo", "ocs_cancel",
}
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
    assert "summary" in definitions["ocs_execute"]["inputSchema"]["properties"]["response_detail"]["enum"]
    assert "batch_result" in definitions["ocs_read"]["inputSchema"]["properties"]["op"]["enum"]
    execute_request = definitions["ocs_execute"]["inputSchema"]["properties"]["request"]
    variants = execute_request.get("anyOf") or execute_request.get("oneOf")
    # Resolve local references from the complete tool input schema.
    root = definitions["ocs_execute"]["inputSchema"]
    def resolve(variant):
        if "$ref" not in variant:
            return variant
        ref = variant["$ref"]
        assert ref.startswith("#/")
        resolved = root
        for part in ref[2:].split("/"):
            resolved = resolved[part.replace("~1", "/").replace("~0", "~")]
        return resolved
    variants = [resolve(variant) for variant in variants]
    op_enum = execute_request["properties"]["op"]["enum"]
    assert len(variants) == len(op_enum)
    for shipped in ("entities_create", "entities_set", "entities_delete", "entities_transform",
                    "block_define", "block_delete", "file_identity", "xdata_set",
                    "view_focus", "wblock", "plot",
                    "entities_copy_to", "group_create", "selection_set_save",
                    "selection_set_load", "close", "sysvar", "layout_create",
                    "layout_rename", "layout_activate", "layout_delete",
                    "page_setup_set", "text_replace", "dgn_import",
                    "xref_reload", "xref_unload", "xref_detach", "xref_bind",
                    "xref_set_path", "xref_set_type"):
        assert shipped in op_enum, shipped
    read_ops = definitions["ocs_read"]["inputSchema"]["properties"]["op"]["enum"]
    for shipped in ("layouts", "xrefs", "page_setup_get", "xdata_get", "text_search", "text_audit"):
        assert shipped in read_ops, shipped
    assert execute_request["properties"]["steps"]["maxItems"] == 64
    variant_map = {
        v["properties"]["op"].get("const") or v["properties"]["op"]["enum"][0]: v
        for v in variants
    }
    assert variant_map["run"]["properties"]["cmd"]["type"] == "string"
    assert len(variant_map["xref_set_path"]["oneOf"]) == 2
    assert "set_properties" in execute_request["properties"]["op"]["enum"]
    assert "save_verified" in execute_request["properties"]["op"]["enum"]
    assert "record_schema" in definitions["ocs_read"]["inputSchema"]["properties"]["op"]["enum"]
    assert "audit" in definitions["ocs_read"]["inputSchema"]["properties"]["op"]["enum"]
    read_parameters = definitions["ocs_read"]["inputSchema"]["properties"]["parameters"]["properties"]
    assert {"collection", "where", "paths", "target_format", "target_version", "layout"} <= set(read_parameters)
    assert read_parameters["layout"]["type"] == "string"
    assert variant_map["save_verified"]["properties"]["target_version"]["enum"][0] == "R14"
    assert variant_map["input"]["properties"]["kind"]["enum"] == [
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
    missing_batch = request(process, {
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "ocs_read", "arguments": {
            "ocs_session_id": "missing", "op": "batch_result",
            "parameters": {"request_id": "expired-batch"},
        }},
    })
    assert missing_batch["result"]["isError"] is True
    assert "absent or expired" in missing_batch["result"]["content"][0]["text"]
    guides = request(process, {
        "jsonrpc": "2.0", "id": "guides", "method": "resources/list",
    })
    guide_uris = {r["uri"] for r in guides["result"]["resources"]
                  if r["uri"].startswith("cad://guides/")}
    assert guide_uris == {f"cad://guides/{name}" for name in
                          ("workflow", "architecture", "capture", "batch-bindings")}
    for uri in sorted(guide_uris):
        resource = request(process, {
            "jsonrpc": "2.0", "id": uri, "method": "resources/read", "params": {"uri": uri},
        })
        assert resource["result"]["contents"][0]["text"].startswith("# ")
    guide_tool = request(process, {
        "jsonrpc": "2.0", "id": "guide-tool", "method": "tools/call",
        "params": {"name": "ocs_read", "arguments": {
            "ocs_session_id": "offline", "op": "guide", "parameters": {"name": "capture"},
        }},
    })
    assert not guide_tool["result"].get("isError"), guide_tool
    assert guide_tool["result"]["structuredContent"]["ok"] is True
    assert "resource_link" in guide_tool["result"]["structuredContent"]["contents"][0]["text"]
    entity_schema = request(process, {
        "jsonrpc": "2.0", "id": "entity-schema", "method": "tools/call",
        "params": {"name": "ocs_read", "arguments": {
            "op": "entity_schema", "parameters": {"type": "Circle"},
        }},
    })
    assert not entity_schema["result"].get("isError"), entity_schema
    contract = entity_schema["result"]["structuredContent"]["schema"]
    assert contract["required"] == ["type", "center", "radius"]
    validated = request(process, {
        "jsonrpc": "2.0", "id": "preflight", "method": "tools/call",
        "params": {"name": "ocs_read", "arguments": {
            "op": "validate", "parameters": {"request": {
                "op": "entities_create", "entities": contract["examples"],
            }},
        }},
    })
    assert validated["result"]["structuredContent"]["valid"] is True
    assert validated["result"]["structuredContent"]["executed"] is False
    rejected = request(process, {
        "jsonrpc": "2.0", "id": "bad-preflight", "method": "tools/call",
        "params": {"name": "ocs_read", "arguments": {
            "op": "validate", "parameters": {"request": {
                "op": "entities_create", "entities": [{"type": "Circle", "center": ["bad", 0], "radius": 5}],
            }},
        }},
    })
    assert rejected["result"]["isError"] is True
    assert "/entities/0/center/0" in rejected["result"]["content"][0]["text"]
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
    assert "io.modelcontextprotocol/tasks" in discovered["result"]["capabilities"]["extensions"]

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
    definitions = {tool["name"]: tool for tool in tools["result"]["tools"]}
    assert definitions["ocs_execute"]["inputSchema"]["properties"]["response_detail"]["default"] == "compact"
    assert definitions["ocs_capture"]["inputSchema"]["properties"]["scope"]["default"] == "viewport"

    missing_cmd = request(process, {
        "jsonrpc": "2.0",
        "id": "missing-cmd",
        "method": "tools/call",
        "params": {
            "name": "ocs_execute",
            "arguments": {
                "ocs_session_id": "missing",
                "request": {"op": "run", "request_id": "run-1"},
            },
            "_meta": meta,
        },
    })
    # Tool errors are text content; the output schema covers successes only.
    assert missing_cmd["result"]["isError"] is True
    assert "structuredContent" not in missing_cmd["result"]
    error = missing_cmd["result"]["content"][0]["text"]
    assert "Missing cmd" in error
    assert "LINE 0,0 10,10" in error

    invalid_mutation = request(process, {
        "jsonrpc": "2.0",
        "id": "invalid-mutation",
        "method": "tools/call",
        "params": {
            "name": "ocs_execute",
            "arguments": {"ocs_session_id": "missing", "request": {"op": "undo"}},
            "_meta": meta,
        },
    })
    assert invalid_mutation["result"]["isError"] is True
    assert "structuredContent" not in invalid_mutation["result"]
    assert "request_id" in invalid_mutation["result"]["content"][0]["text"]

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
