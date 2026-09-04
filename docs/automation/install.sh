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
cp "$ROOT/docs/automation/connect.sh" "$DEST/connect.sh"
chmod 755 "$DEST/connect.sh"
mkdir -p "$DEST/integrations/codex/opencadstudio-control/agents"
cp "$ROOT/integrations/codex/opencadstudio-control/SKILL.md" "$DEST/integrations/codex/opencadstudio-control/SKILL.md"
cp "$ROOT/integrations/codex/opencadstudio-control/agents/openai.yaml" "$DEST/integrations/codex/opencadstudio-control/agents/openai.yaml"
rm -f "$DEST/mcp_server.py"
rm -rf "$DEST/venv"
"$DEST/connect.sh" all
printf 'Installed the client-neutral OpenCADStudio MCP runtime.\n'
