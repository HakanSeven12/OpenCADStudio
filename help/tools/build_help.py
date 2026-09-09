#!/usr/bin/env python3
"""Build the static OpenCAD Studio HTML help system from help/manual.md."""

from __future__ import annotations

import argparse
import html
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path


HELP = Path(__file__).resolve().parents[1]
SOURCE = HELP / "manual.md"


@dataclass(frozen=True)
class Page:
    filename: str
    title: str
    summary: str


PAGES = (
    Page("getting-started.html", "Getting Started", "Begin a drawing safely and learn how to use this manual."),
    Page("files-drawings.html", "Files and Drawings", "Open, save, recover, and move drawings with their dependencies."),
    Page("workspace-navigation.html", "Workspace and Navigation", "Work with spaces, views, selection, and object snaps."),
    Page("create-edit-geometry.html", "Create and Edit Geometry", "Draw, modify, grip-edit, erase, and restore geometry."),
    Page("layers-properties.html", "Layers and Properties", "Organize objects and control their inherited appearance."),
    Page("blocks-references.html", "Blocks and References", "Create reusable content, arrays, attributes, and file references."),
    Page("annotation-dimensions.html", "Annotation and Dimensions", "Add readable text, dimensions, leaders, and scaled annotation."),
    Page("layouts-plotting.html", "Layouts and Plotting", "Prepare paper-space sheets and verify plotted output."),
    Page("3d-modeling.html", "3D Modeling and Visualization", "Create and inspect solids, surfaces, materials, and visual styles."),
    Page("parametric-constraints.html", "Parametric Constraints", "Capture geometric intent and diagnose solve or history issues."),
    Page("commands-shortcuts.html", "Commands and Shortcuts", "Use prompts, autocomplete, aliases, and keyboard conventions."),
    Page("automation-plugins.html", "Automation and Plugins", "Control drawings programmatically and extend the native app."),
    Page("troubleshooting-recovery.html", "Troubleshooting and Recovery", "Diagnose file, geometry, rendering, and history problems."),
)


def slug(text: str) -> str:
    value = re.sub(r"[^\w\s-]", "", text.lower(), flags=re.UNICODE)
    value = re.sub(r"[\s_]+", "-", value)
    return value.strip("-")


def split_chapters(markdown: str) -> dict[str, str]:
    chapters: dict[str, list[str]] = {}
    current: list[str] | None = None
    for line in markdown.splitlines():
        match = re.match(r"^## (.+)$", line)
        if match:
            title = match.group(1).strip()
            if title in chapters:
                raise RuntimeError(f"Duplicate chapter: {title}")
            current = [line]
            chapters[title] = current
        elif current is not None:
            current.append(line)
    return {title: "\n".join(lines) for title, lines in chapters.items()}


def pandoc(markdown: str) -> str:
    process = subprocess.run(
        ["pandoc", "--from=gfm", "--to=html5", "--wrap=none"],
        input=markdown,
        text=True,
        capture_output=True,
        check=True,
    )
    return process.stdout.strip()


def headings(markdown: str) -> list[tuple[str, str]]:
    output: list[tuple[str, str]] = []
    for line in markdown.splitlines():
        match = re.match(r"^### (.+)$", line)
        if match:
            title = match.group(1).strip()
            output.append((slug(title), title))
    return output


def sidebar(current: Page | None, chapters: dict[str, str]) -> str:
    parts = ['<nav id="help-sidebar" class="sidebar" aria-label="Help chapters">']
    index_class = ' class="current-topic"' if current is None else ""
    parts.append(f'<div{index_class}><a class="chapter-link" href="index.html">Help overview</a></div>')
    for page in PAGES:
        classes = ' class="current-topic"' if page == current else ""
        opened = " open" if page == current else ""
        links = "".join(
            f'<li><a href="{page.filename}#{anchor}">{html.escape(title)}</a></li>'
            for anchor, title in headings(chapters[page.title])
        )
        parts.append(
            f'<details{classes}{opened}><summary><a href="{page.filename}">{html.escape(page.title)}</a></summary>'
            f'<ul>{links}</ul></details>'
        )
    parts.append("</nav>")
    return "\n".join(parts)


def overview() -> str:
    cards = "".join(
        f'<a class="chapter-card" href="{page.filename}"><strong>{html.escape(page.title)}</strong>'
        f'<span>{html.escape(page.summary)}</span></a>'
        for page in PAGES
    )
    return f"""
<section class="hero">
  <p class="eyebrow">OpenCAD Studio User Manual</p>
  <h2>Find the workflow, command, or concept you need</h2>
  <p>Learn how to create and edit drawings, exchange DWG and DXF files, prepare layouts, model in 3D, use constraints, automate work, and recover from common problems.</p>
</section>
<div class="quick-paths" aria-label="Common help paths">
  <a href="getting-started.html#a-first-drawing">Create a first drawing</a>
  <a href="files-drawings.html#recovery-and-backups">Recover a drawing</a>
  <a href="troubleshooting-recovery.html#report-a-reproducible-problem">Report a problem</a>
</div>
<h2 id="manual-chapters">Manual chapters</h2>
<div class="chapter-grid">{cards}</div>
""".strip()


def shell(page: Page | None, title: str, body: str, chapters: dict[str, str]) -> str:
    previous_link = ""
    next_link = ""
    if page is not None:
        index = PAGES.index(page)
        previous = PAGES[index - 1] if index else None
        following = PAGES[index + 1] if index + 1 < len(PAGES) else None
        previous_link = (
            f'<a rel="prev" href="{previous.filename}">← {html.escape(previous.title)}</a>'
            if previous else '<a rel="prev" href="index.html">← Help overview</a>'
        )
        next_link = (
            f'<a rel="next" href="{following.filename}">{html.escape(following.title)} →</a>'
            if following else ""
        )
    elif PAGES:
        next_link = f'<a rel="next" href="{PAGES[0].filename}">{html.escape(PAGES[0].title)} →</a>'

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="OpenCAD Studio user manual — {html.escape(title)}">
  <title>{html.escape(title)} — OpenCAD Studio Help</title>
  <link rel="stylesheet" href="assets/css/core.css">
  <link rel="stylesheet" href="assets/css/opencad.css">
  <script src="search-index.js" defer></script>
  <script src="assets/js/help.js" defer></script>
</head>
<body>
  <header class="topbar">
    <button class="nav-toggle" type="button" aria-controls="help-sidebar" aria-expanded="true" title="Toggle chapter navigation"><span aria-hidden="true">☰</span><span class="sr-only">Toggle chapter navigation</span></button>
    <a class="brand" href="index.html">OpenCAD Studio Help</a>
    <div class="search-shell">
      <label class="sr-only" for="help-search">Search help</label>
      <input id="help-search" class="help-search" type="search" role="combobox" placeholder="Search commands and topics" autocomplete="off" aria-autocomplete="list" aria-controls="search-results" aria-expanded="false">
      <span class="search-key" aria-hidden="true">⌘K</span>
      <ul id="search-results" class="search-results" role="listbox" hidden></ul>
    </div>
  </header>
  {sidebar(page, chapters)}
  <main id="main-content">
    <div class="content-wrap">
      <p class="page-kicker">OpenCAD Studio User Manual</p>
      <h1 class="content-title">{html.escape(title)}</h1>
      {body}
      <nav class="page-navigation" aria-label="Previous and next chapter">{previous_link}{next_link}</nav>
      <p class="date-footer">OpenCAD Studio Help · Generated from <code>help/manual.md</code></p>
    </div>
  </main>
</body>
</html>
"""


class TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        if data.strip():
            self.parts.append(data.strip())


def search_entries(chapters: dict[str, str]) -> list[dict[str, str]]:
    entries: list[dict[str, str]] = []
    for page in PAGES:
        chapter = chapters[page.title]
        sections = re.split(r"(?=^### )", chapter, flags=re.MULTILINE)
        for section in sections:
            match = re.match(r"^### (.+)$", section.splitlines()[0] if section.splitlines() else "")
            if not match:
                continue
            title = match.group(1).strip()
            extractor = TextExtractor()
            extractor.feed(pandoc(section))
            entries.append({
                "title": title,
                "chapter": page.title,
                "url": f"{page.filename}#{slug(title)}",
                "text": " ".join(extractor.parts),
            })
    return entries


def generated_outputs() -> dict[str, str]:
    markdown = SOURCE.read_text(encoding="utf-8")
    chapters = split_chapters(markdown)
    expected = {page.title for page in PAGES}
    if set(chapters) != expected:
        raise RuntimeError(
            f"Manual chapter mapping mismatch. Missing: {sorted(expected - set(chapters))}; "
            f"extra: {sorted(set(chapters) - expected)}"
        )

    output = {"index.html": shell(None, "Help overview", overview(), chapters)}
    for page in PAGES:
        chapter_body = "\n".join(chapters[page.title].splitlines()[1:]).lstrip()
        output[page.filename] = shell(page, page.title, pandoc(chapter_body), chapters)
    index_json = json.dumps(search_entries(chapters), ensure_ascii=False, separators=(",", ":"))
    output["search-index.js"] = f"window.OCS_HELP_INDEX = {index_json};\n"
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Fail if generated output is stale.")
    args = parser.parse_args()
    output = generated_outputs()

    if args.check:
        stale = [name for name, content in output.items() if not (HELP / name).is_file() or (HELP / name).read_text(encoding="utf-8") != content]
        if stale:
            print("Stale generated help: " + ", ".join(stale), file=sys.stderr)
            return 1
        print(f"Generated help is current ({len(output) - 1} pages plus search index).")
        return 0

    for name, content in output.items():
        (HELP / name).write_text(content, encoding="utf-8")
    print(f"Generated {len(output) - 1} pages and search-index.js in {HELP}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
