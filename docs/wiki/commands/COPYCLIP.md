---
summary: Copies the selected objects to the clipboard, with the lower-left corner of the selection as the base point.
related: COPYBASE, CUTCLIP, PASTECLIP, PASTEBLOCK, PASTEORIG
ribbon: Draw tab › Clipboard panel
---

## Usage
1. Select objects and press Enter. If you selected objects first, this step is skipped.
2. The objects are copied. The command line reports how many.

## Notes
- You can paste the objects into the same drawing or another open drawing with [`PASTECLIP`](PASTECLIP-Command).
- A picture of the objects in SVG form is also put on the system clipboard as text, so other programs can paste it.
- While you edit multiline text, the command copies the selected text instead of drawing objects.
- To choose the base point yourself, use [`COPYBASE`](COPYBASE-Command).
