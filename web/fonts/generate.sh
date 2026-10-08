#!/usr/bin/env bash
# Regenerate the language fonts served to the web build.
# The active language file is shared by the UI, drawing text and navigation
# labels. Every file contains the common Latin ranges, so startup needs one
# request; drawings can still fetch an additional script when necessary.
# Source: Noto fonts (SIL OFL 1.1 — see OFL.txt).
#
# Requires: pip install --break-system-packages fonttools brotli
set -euo pipefail
RAW="https://github.com/notofonts/notofonts.github.io/raw/main/fonts"
sub() { python3 -m fontTools.subset "$1" --unicodes="$2" --output-file="$3" \
        --no-hinting --desubroutinize --layout-features='*' --notdef-outline \
        --name-IDs='*' --recalc-bounds 2>/dev/null; }
dl() { curl -sL -o "$1" "$2"; }
merge() { pyftmerge /tmp/NotoSans.ttf "$1" --output-file="$2" >/dev/null; }
collection() {
    python3 - "$1" "$2" "$3" <<'PY'
import sys
from fontTools.ttLib import TTCollection, TTFont

output, latin, script = sys.argv[1:]
fonts = TTCollection()
fonts.fonts = [TTFont(latin), TTFont(script)]
fonts.save(output)
PY
}

# Latin / Cyrillic / Greek share one Noto source.
dl /tmp/NotoSans.ttf "$RAW/NotoSans/hinted/ttf/NotoSans-Regular.ttf"
sub /tmp/NotoSans.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+20A0-20BF,U+2122,U+2190-21FF,U+2200-22FF" latin.ttf
sub /tmp/NotoSans.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0400-04FF,U+0500-052F,U+2DE0-2DFF,U+A640-A69F" cyrillic.ttf
sub /tmp/NotoSans.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0370-03FF,U+1F00-1FFF" greek.ttf

# One source per remaining script.
dl /tmp/NotoArabic.ttf "$RAW/NotoSansArabic/hinted/ttf/NotoSansArabic-Regular.ttf"
merge /tmp/NotoArabic.ttf /tmp/NotoArabicMerged.ttf
sub /tmp/NotoArabicMerged.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0600-06FF,U+0750-077F,U+08A0-08FF,U+FB50-FDFF,U+FE70-FEFF" arabic.ttf

dl /tmp/NotoHebrew.ttf "$RAW/NotoSansHebrew/hinted/ttf/NotoSansHebrew-Regular.ttf"
merge /tmp/NotoHebrew.ttf /tmp/NotoHebrewMerged.ttf
sub /tmp/NotoHebrewMerged.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0590-05FF,U+FB1D-FB4F" hebrew.ttf

dl /tmp/NotoThai.ttf "$RAW/NotoSansThai/hinted/ttf/NotoSansThai-Regular.ttf"
merge /tmp/NotoThai.ttf /tmp/NotoThaiMerged.ttf
sub /tmp/NotoThaiMerged.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0E00-0E7F" thai.ttf

dl /tmp/NotoLao.ttf "$RAW/NotoSansLao/hinted/ttf/NotoSansLao-Regular.ttf"
merge /tmp/NotoLao.ttf /tmp/NotoLaoMerged.ttf
sub /tmp/NotoLaoMerged.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0E80-0EFF" lao.ttf

dl /tmp/NotoDeva.ttf "$RAW/NotoSansDevanagari/hinted/ttf/NotoSansDevanagari-Regular.ttf"
merge /tmp/NotoDeva.ttf /tmp/NotoDevaMerged.ttf
sub /tmp/NotoDevaMerged.ttf "U+0000-024F,U+1E00-1EFF,U+2000-206F,U+0900-097F,U+A8E0-A8FF" devanagari.ttf

# CJK comes from the noto-cjk repo (CFF/OTF; ttf-parser reads CFF outlines) and
# is split by language: Simplified Chinese, Traditional Chinese, Japanese and
# Korean each get their own file.
# Han ideographs share code points but differ in glyph shape, so each language
# ships its own. These are the heavy ones and remain split so startup only loads
# the active language.
CJK="https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF"
dl /tmp/NotoSC.otf "$CJK/SimplifiedChinese/NotoSansCJKsc-Regular.otf"
sub /tmp/NotoSC.otf "U+3000-303F,U+4E00-9FFF,U+FF00-FFEF" /tmp/chinese.otf
collection chinese.ttf latin.ttf /tmp/chinese.otf

dl /tmp/NotoTC.otf "$CJK/TraditionalChinese/NotoSansCJKtc-Regular.otf"
sub /tmp/NotoTC.otf "U+3000-303F,U+4E00-9FFF,U+FF00-FFEF" /tmp/traditional_chinese.otf
collection traditional_chinese.ttf latin.ttf /tmp/traditional_chinese.otf

dl /tmp/NotoJP.otf "$CJK/Japanese/NotoSansCJKjp-Regular.otf"
sub /tmp/NotoJP.otf "U+3000-303F,U+3040-30FF,U+31F0-31FF,U+4E00-9FFF,U+FF00-FFEF" /tmp/japanese.otf
collection japanese.ttf latin.ttf /tmp/japanese.otf

dl /tmp/NotoKR.otf "$CJK/Korean/NotoSansCJKkr-Regular.otf"
sub /tmp/NotoKR.otf "U+1100-11FF,U+3130-318F,U+AC00-D7A3" /tmp/korean.otf
collection korean.ttf latin.ttf /tmp/korean.otf
