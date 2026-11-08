---
summary: Imports a Windows metafile (WMF) as a block and places it in the drawing.
related: IMPORT, SVGIN, INSERT
---

## Usage
1. Run `WMFIN`. A file dialog opens; choose a `.wmf` file.
2. Place the picture with the insertion prompts: insertion point, scale and rotation.

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, the command asks `Enter name of WMF file to import (no extension):` on the command line instead; `.wmf` is added when you give no extension.
- The metafile becomes a new block named `WMF0`, `WMF1` and so on, sized to fit half of the current view.
- Text in the metafile gets text styles named after its fonts (`WMF-<font>…`).
- A file that is not a valid metafile is reported and nothing is added.
- The import is one undo step.
