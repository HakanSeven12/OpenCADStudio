#!/usr/bin/env bash
# Build the in-tree API v7 Python plugin and stage a complete plugin package.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <stage directory> [--debug] [--target <triple>]" >&2
  exit 2
fi

stage_dir=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$1")
shift
profile=release
target=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) profile=debug; shift ;;
    --target)
      [[ $# -ge 2 ]] || { echo "--target requires a triple" >&2; exit 2; }
      target=$2
      shift 2
      ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

plugin_dir=$(cd "$(dirname "$0")/.." && pwd)
ocs_repo=$(cd "$plugin_dir/../.." && pwd)
cd "$plugin_dir"

build_args=(build --locked --features experimental-host-model)
[[ $profile == release ]] && build_args+=(--release)
[[ -n $target ]] && build_args+=(--target "$target")
cargo "${build_args[@]}"

case "$(uname -s)" in
  Darwin) library="libopencad_python.dylib" ;;
  Linux) library="libopencad_python.so" ;;
  MINGW*|MSYS*|CYGWIN*) library="opencad_python.dll" ;;
  *) echo "unsupported platform" >&2; exit 2 ;;
esac

target_dir="$plugin_dir/target"
[[ -n $target ]] && target_dir="$target_dir/$target"
source_library="$target_dir/$profile/$library"
[[ -f $source_library ]] || { echo "build did not produce $source_library" >&2; exit 1; }

mkdir -p "$stage_dir"
cp "$source_library" "$stage_dir/$library"
python3 - "$stage_dir/plugin.toml" "$(rustc --version)" "$ocs_repo/Cargo.lock" <<'PY'
from pathlib import Path
import re
import sys

def acadrust_source(path):
    packages = re.findall(r"(?ms)^\[\[package\]\]\n(.*?)(?=^\[\[package\]\]|\Z)", Path(path).read_text())
    sources = []
    for package in packages:
        if re.search(r'^name = "opencadcodec"$', package, re.M):
            match = re.search(r'^source = "([^"]+)"$', package, re.M)
            if match:
                sources.append(match.group(1))
    if len(sources) != 1:
        raise SystemExit(f"expected one opencadcodec source in {path}, found {len(sources)}")
    return sources[0]

manifest = Path("plugin.toml").read_text()
if "api_version = 7" not in manifest:
    raise SystemExit("bundled plugin.toml must target API v7")
manifest = manifest.replace("__RUSTC_VERSION__", sys.argv[2])
manifest = manifest.replace("__ACADRUST_SOURCE__", acadrust_source(sys.argv[3]))
if "__" in manifest:
    raise SystemExit("unresolved manifest placeholder")
Path(sys.argv[1]).write_text(manifest)
PY
echo "Staged bundled API v7 plugin in $stage_dir"
