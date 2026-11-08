---
summary: Copies the selected objects to the clipboard and removes them from the drawing.
related: COPYCLIP, COPYBASE, PASTECLIP
ribbon: Draw tab › Clipboard panel
---

## Usage
1. Select the objects first, or start the command and select them at the prompt.
2. Press Enter to finish the selection. The objects are copied and erased.

## Notes
- The base point of the copy is the lower-left corner of the selection's bounding box.
- Objects on locked layers are not cut.
- A picture of the objects (SVG) is also placed on the system clipboard, so you can paste it into other programs.
- Paste the objects back with [`PASTECLIP`](PASTECLIP-Command). [`UNDO`](UNDO-Command) restores the cut objects.
