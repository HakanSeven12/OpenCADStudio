---
summary: Removes the selected objects from the drawing.
related: OOPS, UNDO, SELECT
ribbon: Draw tab › Modify panel
---

## Usage
1. Select the objects to remove, then run `ERASE`. The objects are removed at once.
2. If nothing is selected, the command asks you to select objects. Pick them, then press Enter.

## Notes
- Objects on locked layers are skipped.
- [`OOPS`](OOPS-Command) brings back the objects removed by the last `ERASE`. [`UNDO`](UNDO-Command) also reverses it.
- Erasing an associative hatch removes its link to the boundary; the command line says so.
- In the Block Editor, a selected parameter or action is deleted when no objects are selected.
- Pressing the Delete key with objects selected also removes them.
