---
summary: Places points or blocks along an object at a fixed distance from each other.
related: DIVIDE, PTYPE
ribbon: Draw tab › Draw panel (expanded)
---

## Usage
1. Select the object to measure. The end nearer to where you click is where measuring starts.
2. Type the segment length, or pick two points to show the length, or type `B` to place a block.
3. Points (or blocks) are placed at every full segment length along the object.

## Options
| Option | What it does |
|---|---|
| **Block** (`B`) | Places a block instead of points. Enter the name of a block in the drawing, then answer `Align block with object? [Yes/No] <Yes>`. Yes turns each block to follow the object's direction. |

## Notes
- On a closed object, measuring starts at the object's own start point.
- The length must be greater than zero. At most 32767 markers are placed.
- Works on curves such as lines, arcs, circles and polylines, flat or in 3D.
