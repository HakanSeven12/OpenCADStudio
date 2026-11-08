---
summary: Pastes the copied objects into the current space at a point you pick.
related: COPYCLIP, CUTCLIP, COPYBASE, PASTEORIG, PASTEBLOCK, PASTESPEC
ribbon: Draw tab › Clipboard panel
---

## Usage
1. Copy or cut objects first with [`COPYCLIP`](COPYCLIP-Command), [`CUTCLIP`](CUTCLIP-Command) or [`COPYBASE`](COPYBASE-Command).
2. Start `PASTECLIP`. A preview of the objects follows the cursor.
3. Specify the insertion point.

The objects are placed relative to their base point: the lower-left corner of the copied objects, or the base point you picked with `COPYBASE`.

## Notes
- Press Enter or Esc at the prompt to cancel without pasting.
- When nothing was copied inside the program, the command reads the system clipboard instead:
  - SVG markup is pasted as drawing objects, and you place it with the insertion prompts.
  - Other text is fed to the command line as if you typed it, one line after another.
  - An image (for example a screenshot) is attached as a raster image.
  - If the clipboard holds nothing usable, an error is shown.
- For very large selections the preview shows only a bounding rectangle, so placement stays smooth.
- You can undo the paste.
