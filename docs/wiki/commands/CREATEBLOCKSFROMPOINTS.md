---
summary: Turns COGO points into block references carrying their number, elevation and description as attributes.
related: CONVERTCADPOINTS, REPLACELEGACYPOINTBLOCKS
---

## Usage
1. Run the command; a window opens.
2. Choose a spatial filter (none, the current display, or a window picked in the drawing), the point groups, the block (an existing block with a POINT attribute, or a new one) and the block layer.
3. Press **OK**.

## Notes
- A new block has the attributes ELEV, POINT and DESC on layers ELEV, PNTS and DESC.
- A point whose number a block already carries asks to Skip, Overwrite or Cancel; blocks already made stay.
