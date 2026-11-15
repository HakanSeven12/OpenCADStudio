#!/bin/sh
# Palette family audit: per palette file, how many generic iced colours
# (`theme.palette()`, `background.*` / `primary.*` pairs, `Color::from_rgb*`)
# and literal sizes (`.size/.padding/.spacing/.width/.height` or
# `Length::Fixed` with a number) it still has. Colours belong in
# `chrome::`, text sizes in `text::` and measurements in `kit_metrics`
# (docs/ui/PALETTE_FAMILY_PLAN.md).
#
#   sh docs/ui/audit_counts.sh           print the counts
#   sh docs/ui/audit_counts.sh --check   fail when a count is above its
#                                        line in docs/ui/audit_counts.txt
#   sh docs/ui/audit_counts.sh --write   rewrite that baseline
#
# The baseline only goes down: lower it (--write) when a file gets cleaner.
set -eu
cd "$(dirname "$0")/../.."
BASELINE=docs/ui/audit_counts.txt
FILES="
src/ui/properties.rs
src/ui/command_line.rs
src/ui/node_graph.rs
src/ui/window/graphic_attributes.rs
src/ui/window/browser.rs
src/ui/window/pc_manager.rs
src/ui/window/authoring_palettes.rs
src/ui/window/tool_palettes.rs
src/ui/window/parameters_manager.rs
src/ui/window/count_palette.rs
src/ui/window/layer_walk.rs
src/ui/window/quick_calc/view.rs
src/ui/window/quick_calc/variables_view.rs
src/ui/window/quick_calc/keypad_view.rs
src/ui/window/block_palette.rs
src/ui/window/xref_manager.rs
src/ui/window/layers.rs
src/ui/window/sheet_set.rs
src/ui/window/plugin_panel.rs
src/ui/window/assistant.rs
"
count() {
    # Comments do not count.
    grep -v '^[[:space:]]*//' "$2" | grep -cE "$1" || true
}
COLOURS='theme\.palette\(\)|\b(palette|p|pal)\.(background|primary)\.|Color::from_rgb'
# A zero is not a size: `.padding(0)`, `[0.0, km::CELL_PAD_X]`, `width(0)`.
NUM='-?([1-9]|0\.[0-9]*[1-9])'
LITERALS="\.(size|padding|spacing|width|height)\([[:space:]]*\[?$NUM|Length::Fixed\([[:space:]]*$NUM"
report() {
    for f in $FILES; do
        [ -f "$f" ] || continue
        printf '%s %s %s\n' "$f" "$(count "$COLOURS" "$f")" "$(count "$LITERALS" "$f")"
    done
}
case "${1:-}" in
--write)
    report > "$BASELINE"
    echo "wrote $BASELINE"
    ;;
--check)
    fail=0
    report | while read -r f colours literals; do
        base=$(grep "^$f " "$BASELINE" || true)
        if [ -z "$base" ]; then
            echo "$f: not in $BASELINE (run --write)"
            exit 1
        fi
        bc=$(echo "$base" | cut -d' ' -f2)
        bl=$(echo "$base" | cut -d' ' -f3)
        if [ "$colours" -gt "$bc" ]; then
            echo "$f: $colours generic colours (baseline $bc): use chrome:: tokens"
            exit 1
        fi
        if [ "$literals" -gt "$bl" ]; then
            echo "$f: $literals literal sizes (baseline $bl): use text:: / kit_metrics"
            exit 1
        fi
    done || fail=1
    if [ "$fail" -ne 0 ]; then
        exit 1
    fi
    echo "kit audit: no file above its baseline"
    ;;
*)
    printf '%-42s %8s %8s\n' file colours literals
    report | while read -r f c l; do printf '%-42s %8s %8s\n' "$f" "$c" "$l"; done
    ;;
esac
