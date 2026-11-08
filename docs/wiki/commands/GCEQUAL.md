---
summary: Makes lines or polyline segments the same length, or circles and arcs the same radius.
related: GEOMCONSTRAINT, GCPARALLEL, GCCONCENTRIC, GCSHOW, DELCONSTRAINT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select first object or type `M` for Multiple: pick a line, polyline segment, circle or arc.
2. Select second object: pick an object of the same kind.

The second object takes the length or radius of the first.

## Options
| Option | What it does |
|---|---|
| **Multiple** (`M`) | Pick the first object, then keep picking objects to make equal to it. Each pick is applied at once. Press Enter to finish. |

## Notes
- Lines and straight polyline segments are made equal in length; circles and arcs are made equal in radius.
- You can select two or more objects first and then start the command: all of them are made equal to the first one you selected.
- Picking the same object twice is refused.
