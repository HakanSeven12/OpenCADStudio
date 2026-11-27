---
summary: Copies the selected objects to the clipboard with a base point you pick, and removes them from the drawing.
related: CUTCLIP, COPYBASE, PASTECLIP, PASTEBLOCK
---

## Usage
1. Specify the base point.
2. Select objects and press Enter. If you selected objects first, this step is skipped.
3. The objects are copied and erased. The command line reports how many objects were cut and the base point.

## Notes
- When you paste, the objects are placed relative to this base point.
- [`CUTCLIP`](CUTCLIP-Command) does the same but uses the lower-left corner of the selection as the base point.
- Objects on locked layers are not cut.
- [`UNDO`](UNDO-Command) restores the cut objects in one step.
- A picture of the objects in SVG form is also put on the system clipboard as text, so other programs can paste it.
