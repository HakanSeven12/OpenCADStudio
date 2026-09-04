#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
DEST=${XDG_DATA_HOME:-"$HOME/.local/share"}/ocs-control
mkdir -p "$DEST"
cargo build --manifest-path "$ROOT/Cargo.toml" --release
cp "$ROOT/target/release/OpenCADStudio" "$DEST/OpenCADStudio.new"
chmod 755 "$DEST/OpenCADStudio.new"
mv -f "$DEST/OpenCADStudio.new" "$DEST/OpenCADStudio"
cp "$ROOT/docs/automation/live.py" "$DEST/live.py"
cp "$ROOT/docs/automation/mcp_server.py" "$DEST/mcp_server.py"
uv venv "$DEST/venv"
uv pip install --python "$DEST/venv/bin/python" -r "$ROOT/docs/automation/requirements-mcp.txt"
SKILLS=${CODEX_HOME:-"$HOME/.codex"}/skills
mkdir -p "$SKILLS/opencadstudio-control/agents"
cp "$ROOT/integrations/codex/opencadstudio-control/SKILL.md" "$SKILLS/opencadstudio-control/SKILL.md"
cp "$ROOT/integrations/codex/opencadstudio-control/agents/openai.yaml" "$SKILLS/opencadstudio-control/agents/openai.yaml"
codex mcp remove opencadstudio >/dev/null 2>&1 || true
codex mcp add opencadstudio -- "$DEST/venv/bin/python" "$DEST/mcp_server.py"
printf 'Installed OpenCADStudio and its persistent Codex MCP connection.\n'
