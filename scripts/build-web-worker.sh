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
  target/wasm32-unknown-unknown/release/ocs_web_worker.wasm
