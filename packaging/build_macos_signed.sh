#!/usr/bin/env bash
# Build, sign, notarize and package OpenCADStudio.app + .dmg for macOS arm64.
#
# Mirrors the `build-macos` job in .github/workflows/release.yml, then goes
# further: instead of the CI's ad-hoc signature it produces a Developer ID
# signed, hardened-runtime, notarized and stapled bundle. Ad-hoc signing is
# what breaks the QuickLook thumbnail extension and "Open with" registration
# in the field (#365): appexes must be sandboxed *and* validly signed before
# macOS will run them.
#
# Inputs (environment):
#   DEVELOPER_ID    "Developer ID Application: ..." identity. Auto-derived
#                   from the keychain when unset. Set to "-" for an ad-hoc
#                   build (CI parity, no notarization).
#   NOTARY_PROFILE  notarytool keychain profile. Unset → skip notarization.
#   VERSION         Bundle version. Default: version from Cargo.toml.
#
# Output: dist/OpenCADStudio.app and dist/OpenCADStudio-v<VERSION>-macos-arm64.dmg
set -euo pipefail

cd "$(dirname "$0")/.."
TARGET=aarch64-apple-darwin
DIST=dist
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"

if [ -z "${DEVELOPER_ID:-}" ]; then
    DEVELOPER_ID="$(security find-identity -v -p codesigning \
        | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)"
fi
[ -n "$DEVELOPER_ID" ] || { echo "No Developer ID identity found; set DEVELOPER_ID (or '-' for ad-hoc)." >&2; exit 1; }
echo "==> Version $VERSION, signing as: $DEVELOPER_ID"

echo "==> cargo build (app + thumbnailer staticlib)"
cargo build --release --target "$TARGET"
# The staticlib crate-type is only emitted when the crate is built as a
# target (as a plain dependency cargo produces just the rlib).
cargo build --release --target "$TARGET" -p dwg-thumbnailer

echo "==> icons"
rm -rf "$DIST" && mkdir -p "$DIST"
ICONSET="$DIST/OpenCADStudio.iconset"
mkdir -p "$ICONSET"
for SIZE in 16 32 64 128 256 512 1024; do
    rsvg-convert -w $SIZE -h $SIZE assets/logo.svg -o "$ICONSET/icon_${SIZE}x${SIZE}.png"
done
for BASE in 16 32 128 256 512; do
    cp "$ICONSET/icon_$((BASE * 2))x$((BASE * 2)).png" "$ICONSET/icon_${BASE}x${BASE}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$DIST/AppIcon.icns"
for T in dwg dxf; do
    SET="$DIST/$T.iconset"
    mkdir -p "$SET"
    for SIZE in 16 32 64 128 256 512 1024; do
        rsvg-convert -w $SIZE -h $SIZE "assets/mimetypes/image-vnd.$T.svg" \
            -o "$SET/icon_${SIZE}x${SIZE}.png"
    done
    for BASE in 16 32 128 256 512; do
        cp "$SET/icon_$((BASE*2))x$((BASE*2)).png" "$SET/icon_${BASE}x${BASE}@2x.png"
    done
    iconutil -c icns "$SET" -o "$DIST/$(echo "$T" | tr a-z A-Z).icns"
done

echo "==> QuickLook thumbnail extension (.appex)"
EXT="$DIST/DWGThumbnail.appex"
rm -rf "$EXT" && mkdir -p "$EXT/Contents/MacOS"
swiftc \
    -sdk "$(xcrun --sdk macosx --show-sdk-path)" \
    -target arm64-apple-macos11 \
    -O -parse-as-library -application-extension \
    -module-name DWGThumbnail \
    -import-objc-header crates/dwg-thumbnailer/macos/dwg_thumbnailer.h \
    crates/dwg-thumbnailer/macos/ThumbnailProvider.swift \
    -L "target/$TARGET/release" -ldwg_thumbnailer \
    -framework QuickLookThumbnailing -framework CoreGraphics \
    -framework ImageIO -framework Foundation -framework Security \
    -framework SystemConfiguration -liconv \
    -Xlinker -e -Xlinker _NSExtensionMain \
    -o "$EXT/Contents/MacOS/DWGThumbnail"
sed "s/__VERSION__/$VERSION/g" crates/dwg-thumbnailer/macos/Info.plist > "$EXT/Contents/Info.plist"

echo "==> assemble .app"
APP="$DIST/OpenCADStudio.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/PlugIns"
cp "target/$TARGET/release/OpenCADStudio" "$APP/Contents/MacOS/OpenCADStudio"
chmod +x "$APP/Contents/MacOS/OpenCADStudio"
cp "$DIST/AppIcon.icns" "$DIST/DWG.icns" "$DIST/DXF.icns" "$APP/Contents/Resources/"
cp -R "$EXT" "$APP/Contents/PlugIns/"
sed "s/__VERSION__/$VERSION/g" packaging/Info.plist > "$APP/Contents/Info.plist"

echo "==> codesign"
if [ "$DEVELOPER_ID" = "-" ]; then
    # CI-parity ad-hoc signature; cannot be notarized.
    codesign --force --deep --sign - --timestamp=none "$APP"
else
    # Inside-out: the appex first (sandbox entitlement is REQUIRED for a
    # QuickLook extension to run), then the outer bundle. Hardened runtime
    # and a secure timestamp on every layer for notarization.
    codesign --force --timestamp --options runtime \
        --entitlements crates/dwg-thumbnailer/macos/entitlements.plist \
        -s "$DEVELOPER_ID" "$APP/Contents/PlugIns/DWGThumbnail.appex"
    codesign --force --timestamp --options runtime \
        -s "$DEVELOPER_ID" "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

echo "==> dmg"
DMG="$DIST/OpenCADStudio-v$VERSION-macos-arm64.dmg"
rm -f "$DMG"
for i in 1 2 3 4 5; do
    if hdiutil create -volname "Open CAD Studio" -srcfolder "$APP" -ov -format UDZO "$DMG"; then
        break
    fi
    echo "hdiutil failed (attempt $i), retrying..." >&2
    sleep 3
done
[ -f "$DMG" ] || { echo "hdiutil failed permanently" >&2; exit 1; }

if [ -n "${NOTARY_PROFILE:-}" ] && [ "$DEVELOPER_ID" != "-" ]; then
    echo "==> notarize (this can take a while; a first submission from a new"
    echo "    Developer ID can be held for hours — that is normal)"
    SUBMIT_OUT="$(xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait 2>&1 | tee /dev/stderr)"
    SUBMISSION_ID="$(echo "$SUBMIT_OUT" | sed -n 's/^[[:space:]]*id: \([0-9a-f-]*\)$/\1/p' | head -1)"
    # Always pull the log — it names the exact per-file cause on Invalid.
    [ -n "$SUBMISSION_ID" ] && xcrun notarytool log "$SUBMISSION_ID" \
        --keychain-profile "$NOTARY_PROFILE" || true
    echo "$SUBMIT_OUT" | grep -q "status: Accepted" || { echo "Notarization NOT accepted" >&2; exit 1; }
    xcrun stapler staple "$DMG"
    xcrun stapler staple "$APP"
    spctl -a -vvv "$APP"
else
    echo "==> NOTARY_PROFILE unset (or ad-hoc build): skipping notarization"
fi

shasum -a 256 "$DMG" > "$DMG.sha256"
echo "==> done: $DMG"
