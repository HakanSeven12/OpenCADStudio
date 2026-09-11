#!/usr/bin/env python3
"""Validate local links, anchors, assets, context targets, and basic HTML structure."""

from __future__ import annotations

import json
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote


HELP = Path(__file__).resolve().parents[1]


class Document(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: set[str] = set()
        self.links: list[str] = []
        self.assets: list[str] = []
        self.has_main = False
        self.has_title = False
        self.images_without_alt = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if values.get("id"):
            self.ids.add(values["id"] or "")
        if tag == "a" and values.get("href"):
            self.links.append(values["href"] or "")
        if tag in {"img", "script", "link"}:
            target = values.get("src") or values.get("href")
            if target:
                self.assets.append(target)
        if tag == "img" and "alt" not in values:
            self.images_without_alt += 1
        self.has_main |= tag == "main"
        self.has_title |= tag == "title"


def load_documents() -> dict[str, Document]:
    documents: dict[str, Document] = {}
    for path in sorted(HELP.glob("*.html")):
        document = Document()
        document.feed(path.read_text(encoding="utf-8"))
        documents[path.name] = document
    return documents


def validate_target(source: str, target: str, documents: dict[str, Document], errors: list[str]) -> None:
    if target.startswith(("http:", "https:", "mailto:", "javascript:")):
        return
    filename, _, fragment = target.partition("#")
    filename = unquote(filename) or source
    destination = HELP / filename
    if not destination.is_file():
        errors.append(f"{source}: missing target {target}")
        return
    if fragment and destination.suffix == ".html":
        document = documents.get(destination.name)
        if document is None or fragment not in document.ids:
            errors.append(f"{source}: missing anchor {target}")


def main() -> int:
    documents = load_documents()
    errors: list[str] = []
    if not documents:
        errors.append("No generated HTML pages found.")

    for name, document in documents.items():
        if not document.has_main:
            errors.append(f"{name}: missing <main>")
        if not document.has_title:
            errors.append(f"{name}: missing <title>")
        if document.images_without_alt:
            errors.append(f"{name}: {document.images_without_alt} image(s) missing alt")
        for target in document.links + document.assets:
            validate_target(name, target, documents, errors)

    context = json.loads((HELP / "context-map.json").read_text(encoding="utf-8"))
    for group in ("commands", "surfaces"):
        for key, target in context.get(group, {}).items():
            validate_target(f"context-map.json:{group}.{key}", target, documents, errors)

    if errors:
        print("Help validation failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    links = sum(len(document.links) for document in documents.values())
    print(f"Validated {len(documents)} pages, {links} links, and {sum(len(context.get(group, {})) for group in ('commands', 'surfaces'))} context targets.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

