---
summary: Applies an equal constraint: lines and straight polyline segments get the same length, circles and arcs the same radius.
related: GEOMCONSTRAINT, AUTOCONSTRAIN, CONSTRAINTBAR, DELCONSTRAINT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select the first object, or type **Multiple**. This object sets the size.
2. Select the second object. It is resized to match the first, and the constraint is added.

If two or more suitable objects are already selected when you start, the constraint is applied to them at once. The first one selected sets the size.

## Options
| Option | What it does |
|---|---|
| **Multiple** (`M`) | After the first object, keeps asking for objects to make equal to it. Press Enter to finish; the command line then reports that the lengths or radii were made equal. |

## Notes
- You can pick lines, straight segments of polylines, circles and arcs. Pick the segment you want on a polyline.
- A line or segment can only be made equal to a line or segment, and a circle or arc only to a circle or arc. An object that does not fit is refused and you are asked again.
- If the same equal constraint already exists, the command says so and adds nothing.
- `GCEQUAL` runs the same command.
