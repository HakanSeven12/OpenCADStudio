---
summary: Copies the selected objects onto the layer of an object you pick, then lets you move the copies.
related: LAYMATCH, COPY, MATCHPROP, LAYER
ribbon: Draw tab › Layers panel
---

## Usage
1. Select the objects to copy and press Enter. If you selected objects first, this step is skipped.
2. Select an object on the destination layer.
3. The copies are made on that layer, in the same place as the originals.
4. Specify a base point to move the copies, or press Enter to leave them where they are.
5. Specify the second point. Press Enter instead to use the first point as the displacement.

## Options
| Option | What it does |
|---|---|
| **Displacement** (`D`) | Type a displacement for the copies. Press Enter for no move. |
| **eXit** (`X`) | Leaves the copies in place and ends. This is the default at the base point prompt. |

## Notes
- Objects on locked layers are not copied.
- Copying and moving count as one step for undo.
