#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SERVER=${OCS_MCP_COMMAND:-"$ROOT/OpenCADStudio"}
NAME=opencadstudio

require_server() {
    [ -x "$SERVER" ] || {
        printf 'OpenCADStudio MCP executable not found: %s\n' "$SERVER" >&2
        exit 1
    }
}

connect_codex() {
    SKILLS=${CODEX_HOME:-"$HOME/.codex"}/skills
    SOURCE="$ROOT/integrations/codex/opencadstudio-control"
    if [ -f "$SOURCE/SKILL.md" ]; then
        mkdir -p "$SKILLS/opencadstudio-control/agents"
        cp "$SOURCE/SKILL.md" "$SKILLS/opencadstudio-control/SKILL.md"
        cp "$SOURCE/agents/openai.yaml" "$SKILLS/opencadstudio-control/agents/openai.yaml"
    fi
    codex mcp remove "$NAME" >/dev/null 2>&1 || true
    codex mcp add "$NAME" -- "$SERVER" --mcp
    printf 'Connected Codex to OpenCADStudio.\n'
}

connect_claude_code() {
    claude mcp remove "$NAME" >/dev/null 2>&1 || true
    claude mcp add --transport stdio --scope user "$NAME" -- "$SERVER" --mcp
    printf 'Connected Claude Code to OpenCADStudio.\n'
}

print_config() {
    ESCAPED=$(printf '%s' "$SERVER" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '{"mcpServers":{"%s":{"command":"%s","args":["--mcp"]}}}\n' "$NAME" "$ESCAPED"
}

require_server
case ${1:-all} in
    all)
        FOUND=false
        if command -v codex >/dev/null 2>&1; then connect_codex; FOUND=true; fi
        if command -v claude >/dev/null 2>&1; then connect_claude_code; FOUND=true; fi
        if [ "$FOUND" = false ]; then
            printf 'No supported AI CLI was found. Use this configuration in any MCP client:\n'
            print_config
        fi
        ;;
    codex)
        command -v codex >/dev/null 2>&1 || { printf 'Codex CLI is not installed.\n' >&2; exit 1; }
        connect_codex
        ;;
    claude-code|claude)
        command -v claude >/dev/null 2>&1 || { printf 'Claude Code is not installed.\n' >&2; exit 1; }
        connect_claude_code
        ;;
    config)
        print_config
        ;;
    *)
        printf 'Usage: %s [all|codex|claude-code|config]\n' "$0" >&2
        exit 2
        ;;
esac
