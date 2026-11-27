#!/bin/sh
set -eu

cargo build --locked --release --target wasm32-unknown-unknown --package ocs_web_worker
# On Windows, Trunk passes an extended-length path (\\?\C:\...) that MSYS sh
# tools cannot handle; strip the prefix so local web builds work there too.
# CI (Linux) paths never carry the prefix and pass through unchanged.
staging=$(printf '%s\n' "${TRUNK_STAGING_DIR:?}" | sed 's|^\\\\?\\||')
worker_out="${staging:?}/worker_pkg"
mkdir -p "$worker_out"
wasm-bindgen \
  --target web \
  --out-dir "$worker_out" \
  --out-name ocs_web_worker \
  "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/ocs_web_worker.wasm"

if [ -f "${staging:?}/index.html" ] && command -v python3 >/dev/null 2>&1; then
  python3 - "${staging:?}/index.html" <<'PY'
import importlib.util
import sys
from pathlib import Path

html_path = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("build_site", Path("scripts/build-site.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
html = html_path.read_text(encoding="utf-8")
html_path.write_text(mod.stamp_csp_inline_script_hashes(html), encoding="utf-8")
PY
fi
