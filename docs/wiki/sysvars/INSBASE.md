---
summary: Sets the base point of the current space, used as the insertion point when this drawing is inserted into another drawing.
type: Point
saved: Drawing
default: 0,0,0
related: BASE, INSERT, WBLOCK
---

## Values
Any point. Pick a point or type its coordinates. Press Enter to keep the current one.

## Notes
- Model space and paper space each have their own base point. The value shown and set is the one of the current space.
- [`BASE`](BASE-Command) sets the same point.
- When you insert a drawing file as a block, its model space base point becomes the block's base point.
