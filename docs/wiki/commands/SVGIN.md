---
summary: Imports an SVG file as a new block and lets you place it in the drawing.
related: IMPORT, WMFIN, INSERT
---

## Usage
1. Type `SVGIN`.
2. Pick an `.svg` or `.svgz` file in the file dialog. If [`FILEDIA`](FILEDIA-System-Variable) is 0, type the file name at the command line instead (the `.svg` extension is added if you leave it out).
3. Place the block with the usual insertion prompts (insertion point, scale, rotation).

## Notes
- The drawing becomes a new block named `SVG0`, `SVG1`, and so on.
- It is scaled to fit half of the current view.
- If the file is not valid SVG, or has no shapes, an error is shown and nothing is imported.
- The import can be undone in one step.
