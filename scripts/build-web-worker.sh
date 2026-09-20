#!/bin/sh
set -eu

cargo build --locked --release --target wasm32-unknown-unknown --package ocs_web_worker
worker_out="${TRUNK_STAGING_DIR:?}/worker_pkg"
mkdir -p "$worker_out"

# The wasm-bindgen-cli schema must exactly match the wasm-bindgen crate the
# worker was compiled against (the version recorded in Cargo.lock). The Nix dev
# shell ships an unrelated wasm-bindgen-cli, so resolve the matching version
# from the lock file and install it if the one on PATH does not match.
want_version="$(awk '
  /^\[\[package\]\]$/ { in_pkg = 0 }
  /^name = "wasm-bindgen"$/ { in_pkg = 1; next }
  in_pkg && /^version = / {
    gsub(/"/, "", $3); print $3; exit
  }
' Cargo.lock)"

have_version="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"

if [ "$have_version" != "$want_version" ]; then
  bindgen_dir="${CARGO_HOME:-$HOME/.cargo}/wasm-bindgen-$want_version"
  if [ ! -x "$bindgen_dir/bin/wasm-bindgen" ]; then
    cargo install --locked --version "$want_version" \
      --root "$bindgen_dir" wasm-bindgen-cli
  fi
  PATH="$bindgen_dir/bin:$PATH"
fi

wasm-bindgen \
  --target web \
  --out-dir "$worker_out" \
  --out-name ocs_web_worker \
  target/wasm32-unknown-unknown/release/ocs_web_worker.wasm
