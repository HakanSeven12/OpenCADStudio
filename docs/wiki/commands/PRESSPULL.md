---
summary: Pulls or pushes a bounded area, a planar curve or a planar solid face to create or change 3D solids and surfaces.
related: EXTRUDE, UNION, SUBTRACT, REGION
ribbon: Model tab › Create panel
---

## Usage
1. `Select object or bounded area:` click inside an area closed by drawing objects, click a planar curve, or click a flat face of a solid. The boundary under the cursor is highlighted before you click.
2. `Specify signed extrusion height or [Multiple/Undo]:` move the cursor along the shown direction and click, or type a height. A negative height goes the other way.
3. The result is created and the command asks for the next area. Press Enter or Esc to stop.

## Options
| Option | What it does |
|---|---|
| **Multiple** (`M`) | Lets you pick more areas or faces, then press Enter to give one height to all of them. |
| **New body** (`N`) | With a solid's face, or an area on it, picked: creates a separate solid from that area and leaves the original solid unchanged. **Modify solid** (`MODIFY`) switches back. |
| **Undo** (`U`) | Removes the last picked area or face. |

## Notes
- A closed area or closed curve becomes a solid. An open curve becomes a surface.
- An area drawn on a face of a solid is added to or cut from that solid, depending on the direction of the height.
- Clicking a solid face pushes or pulls that face. If other drawing objects divide the face, only the area you clicked is used. Hold Ctrl while you click a face to offset the whole face instead.
- Hold Shift while you click to add more areas or faces to the selection.
- Objects on locked or hidden layers cannot be picked.
- Objects selected before you start the command are used as the first picks.
