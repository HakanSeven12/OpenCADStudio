---
summary: Sets the insertion base point of the current drawing.
related: INSBASE, BLOCK, WBLOCK, INSERT
ribbon: Draw tab › Block panel (expanded); Insert tab › Block Definition panel (expanded)
---

## Usage
1. Specify the base point. The prompt shows the current base point in angle brackets.
2. Press Enter instead to keep the current base point.

## Notes
- The base point is the point used as the insertion point when this drawing is inserted into another drawing as a block.
- Model space and paper space each keep their own base point. The command sets the one of the space you are working in.
- If the Z value of the new point is not zero, the command warns that the Z insertion base is not zero.
- The change can be undone.
- The same value is available as the [`INSBASE`](INSBASE-System-Variable) system variable.
