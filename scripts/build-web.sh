#!/bin/sh
# Canonical web bundle build: Trunk plus a fonts sanity sync and a service
# worker cache-version stamp.
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
# The wrapper also stamps the service worker's cache version: web/sw.js ships
# a "v1" placeholder (fine for `trunk serve`), but a deployed bundle must get
# a fresh cache name on every build or clients would keep serving the previous
# deploy's ~55 MB of cached assets. The hash covers every file in the finished
# bundle (fonts re-sync included, sw.js itself excluded), so any content
# change — or any Trunk re-hash of the wasm — produces a new version.
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
# Hard-fail with a clear message when there are no fonts to verify.
# (find | head -1 survives multiple matches and spaces in paths.)
first_font=$(find web/fonts -name '*.ttf' -print 2>/dev/null | head -n 1)
if [ -z "$first_font" ]; then
  echo "build-web.sh: no .ttf fonts found under web/fonts — nothing to ship or verify" >&2
  exit 1
fi
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
# Stamp the service worker's cache version from the finished bundle. Every
# file in dist/app except sw.js itself feeds the hash: the deployed sw.js
# then carries a cache name unique to exactly this bundle's contents, and its
# activate handler deletes any older cache.
[ -f dist/app/sw.js ] || {
    echo "build-web.sh: dist/app/sw.js missing — is web/sw.js still listed as a copy-file link in web-app.html?" >&2
    exit 1
}
bundle_hash=$(find dist/app -type f ! -name sw.js -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c 1-12)
sed -i "s/^const CACHE_VERSION = \"[^\"]*\";/const CACHE_VERSION = \"$bundle_hash\";/" dist/app/sw.js
grep -q "^const CACHE_VERSION = \"$bundle_hash\";" dist/app/sw.js || {
    echo "error: failed to stamp the cache version into dist/app/sw.js" >&2
    exit 1
}

echo "web bundle built; verified $count fonts in dist/app/fonts; sw.js cache version $bundle_hash"
