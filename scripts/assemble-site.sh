#!/bin/sh
set -eu

output="${1:-dist}"
test -f "$output/app/index.html"
mkdir -p "$output/assets"

install -m 0644 site/site.css "$output/site.css"
install -m 0644 site/robots.txt "$output/robots.txt"
install -m 0644 site/CNAME "$output/CNAME"
install -m 0644 site/og.png "$output/og.png"
install -m 0644 site/workspace.png "$output/assets/workspace.png"
install -m 0644 site/modeling.png "$output/assets/modeling.png"
python3 scripts/build-site.py "$output"
