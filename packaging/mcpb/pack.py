#!/usr/bin/env python3
"""Build the client-neutral OpenCADStudio MCP Bundle and registry metadata."""

import argparse
import hashlib
import json
from pathlib import Path
import stat
import zipfile


def manifest(version: str) -> dict:
    return {
        "manifest_version": "0.3",
        "name": "opencadstudio",
        "display_name": "OpenCADStudio",
        "version": version,
        "description": "Control and inspect live OpenCADStudio drawings through MCP",
        "long_description": "Opens OpenCADStudio when needed and provides semantic drawing commands, document inspection, measurements, and visual verification.",
        "author": {
            "name": "Hakan Seven",
            "url": "https://github.com/HakanSeven12/OpenCADStudio",
        },
        "repository": {
            "type": "git",
            "url": "https://github.com/HakanSeven12/OpenCADStudio",
        },
        "server": {
            "type": "binary",
            "entry_point": "server/linux-x86_64/OpenCADStudio",
            "mcp_config": {
                "command": "${__dirname}/server/linux-x86_64/OpenCADStudio",
                "args": ["--mcp"],
                "env": {},
                "platform_overrides": {
                    "win32": {
                        "command": "${__dirname}/server/win32-x86_64/OpenCADStudio.exe"
                    },
                    "darwin": {
                        "command": "${__dirname}/server/darwin-aarch64/OpenCADStudio"
                    },
                },
            },
        },
        "tools": [
            {"name": "ocs_sessions", "description": "List or open live editor sessions"},
            {"name": "ocs_read", "description": "Read drawing and editor state"},
            {"name": "ocs_execute", "description": "Execute a semantic editor action"},
            {"name": "ocs_capture", "description": "Capture the current editor window"},
        ],
        "keywords": ["cad", "drawing", "mcp"],
        "license": "GPL-3.0-or-later",
        "compatibility": {"platforms": ["linux", "win32", "darwin"]},
    }


def add(archive: zipfile.ZipFile, source: Path, destination: str) -> None:
    if not source.is_file():
        raise SystemExit(f"Missing MCP binary: {source}")
    info = zipfile.ZipInfo(destination, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = (stat.S_IFREG | 0o755) << 16
    archive.writestr(info, source.read_bytes())


def registry_metadata(version: str, bundle: Path) -> dict:
    digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
    return {
        "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
        "name": "io.github.HakanSeven12/opencadstudio",
        "title": "OpenCADStudio",
        "description": "Control and inspect live OpenCADStudio drawings through MCP",
        "version": version,
        "repository": {
            "url": "https://github.com/HakanSeven12/OpenCADStudio",
            "source": "github",
        },
        "websiteUrl": "https://github.com/HakanSeven12/OpenCADStudio",
        "packages": [
            {
                "registryType": "mcpb",
                "identifier": f"https://github.com/HakanSeven12/OpenCADStudio/releases/download/v{version}/{bundle.name}",
                "version": version,
                "fileSha256": digest,
                "transport": {"type": "stdio"},
            }
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--linux", type=Path, required=True)
    parser.add_argument("--windows", type=Path, required=True)
    parser.add_argument("--macos", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--server-json", type=Path, required=True)
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.output, "w") as archive:
        archive.writestr("manifest.json", json.dumps(manifest(args.version), indent=2) + "\n")
        add(archive, args.linux, "server/linux-x86_64/OpenCADStudio")
        add(archive, args.windows, "server/win32-x86_64/OpenCADStudio.exe")
        add(archive, args.macos, "server/darwin-aarch64/OpenCADStudio")

    args.server_json.parent.mkdir(parents=True, exist_ok=True)
    args.server_json.write_text(
        json.dumps(registry_metadata(args.version, args.output), indent=2) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
