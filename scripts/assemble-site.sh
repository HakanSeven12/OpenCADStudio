#!/bin/sh
set -eu

output="${1:-dist}"
test -f "$output/app/index.html"
mkdir -p "$output/assets"

install -m 0644 index.html "$output/index.html"
install -m 0644 site/site.css "$output/site.css"
install -m 0644 site/robots.txt "$output/robots.txt"
install -m 0644 site/sitemap.xml "$output/sitemap.xml"
install -m 0644 site/site.webmanifest "$output/site.webmanifest"
install -m 0644 site/CNAME "$output/CNAME"
install -m 0644 site/og.png "$output/og.png"
install -m 0644 site/workspace.png "$output/assets/workspace.png"
install -m 0644 site/modeling.png "$output/assets/modeling.png"
install -m 0644 assets/logo.svg "$output/assets/logo.svg"
