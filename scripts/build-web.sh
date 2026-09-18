#!/bin/sh
# Canonical web bundle build: Trunk plus a fonts sanity sync.
#
# Trunk's copy-dir pipeline always copies web/fonts in full, but its final
# "apply staging to dist" step wipes the old dist and then moves dist/app/.stage
# into place. If that wipe fails on Windows (typically a transient lock from a
# local file server or antivirus with a file open under dist), the build exits
# non-zero and leaves dist stale with .stage fully populated — easy to miss,
# and a stale bundle would 404 the fonts the app fetches from fonts/*.ttf.
# This wrapper therefore re-syncs web/fonts into dist/app/fonts after a
# successful Trunk build and verifies every font byte-for-byte, so a bundle
# produced through it can never ship without the font set.
#
# Usage: sh scripts/build-web.sh [extra trunk build args]
#   local:  sh scripts/build-web.sh
#   CI:     sh scripts/build-web.sh --locked --html-output index.html web-app.html
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Git Bash rewrites leading-slash arguments such as --public-url /app/ into
# Windows paths; keep them verbatim. Harmless on other platforms.
MSYS_NO_PATHCONV=1
export MSYS_NO_PATHCONV

trunk build --release --dist dist/app --public-url /app/ "$@"

# Mirror the font sources over the bundle and verify the copy-dir output.
mkdir -p dist/app/fonts
cp -f web/fonts/* dist/app/fonts/

count=0
for font in web/fonts/*.ttf; do
    name=$(basename "$font")
    cmp -s "$font" "dist/app/fonts/$name" || {
        echo "error: dist/app/fonts/$name differs from $font" >&2
        exit 1
    }
    count=$((count + 1))
done
echo "web bundle built; verified $count fonts in dist/app/fonts"
