# OpenCAD Studio Help

This directory is a self-contained, static user-help system. It is intentionally
not wired into the application yet.

## Authoring model

- `manual.md` is the single source of truth for user-facing prose.
- `tools/build_help.py` splits the level-two chapters into static HTML pages.
- `assets/css/` and `assets/js/` provide the shared responsive presentation,
  navigation, dark mode, print styling, and local search.
- `context-map.json` reserves stable help targets for future command/dialog
  integration without changing application source now.
- `assets/images/` is for screenshots and workflow illustrations referenced by
  `manual.md`.

Generated HTML and `search-index.js` are committed so the manual works directly
from disk without a web server. Do not edit generated pages by hand.

## Build and validate

Requirements: Python 3 and Pandoc.

```sh
python3 help/tools/build_help.py
python3 help/tools/build_help.py --check
python3 help/tools/check_help.py
```

Open `help/index.html` in a browser to review the generated manual. When future
application work is safe, package the complete `help/` directory and connect the
Help menu or contextual controls to the URLs in `context-map.json`.

## Content rules

1. Keep level-two headings unique; each one is a chapter boundary.
2. Keep level-three headings stable because their generated IDs are public help
   targets.
3. Describe observable behavior, not implementation details.
4. Use task-first procedures and state prerequisites before numbered steps.
5. Mark safety, compatibility, data-loss, and known limitations explicitly.
6. Regenerate and run both checks before committing documentation changes.

