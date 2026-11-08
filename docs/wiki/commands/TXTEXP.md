---
summary: Replaces selected text with polylines that trace the outlines of its characters.
related: EXPLODE, TXT2MTXT, TEXT, MTEXT
---
## Usage
1. Select the text objects and press Enter. You can also select them before you start.
2. The command line reports how many objects were exploded.

## Notes
- Works on single-line and multiline text. Other objects in the selection are left unchanged; if no text is selected, the command reports `TXTEXP: no text selected.`
- Each character outline becomes a polyline on the text's layer, with the text's properties. Flat text gives 2D polylines; tilted text gives 3D polylines.
- The original text is removed, so it can no longer be edited as text.
- Objects on locked layers are left out.
- You can undo the change.
