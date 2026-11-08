---
summary: Copies the selected objects to the clipboard with a base point you pick.
related: COPYCLIP, PASTECLIP, PASTEBLOCK, CUTCLIP
---

## Usage
1. Select objects and press Enter. If you selected objects first, this step is skipped.
2. Specify the base point.
3. The command line reports how many objects were copied and the base point.

## Notes
- When you paste, the objects are placed relative to this base point.
- [`COPYCLIP`](COPYCLIP-Command) does the same but uses the lower-left corner of the selection as the base point.
- A picture of the objects in SVG form is also put on the system clipboard as text, so other programs can paste it.
