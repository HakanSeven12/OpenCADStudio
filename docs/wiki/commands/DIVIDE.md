---
summary: Places point objects or blocks at equal spacing along an object, splitting it into a given number of segments.
related: MEASURE, POINT, DDPTYPE, INSERT
ribbon: Draw tab › Draw panel
---

## Usage
1. Select the object to divide.
2. Enter the number of segments, or type **Block**.

Markers are placed between the segments. An open object gets one marker fewer than the number of segments; a closed object gets one marker per segment. The object itself is not changed.

## Options
| Option | What it does |
|---|---|
| **Block** (`B`) | Places a block instead of points. Enter the name of a block defined in the drawing. Then answer **Align block with object? [Yes/No] <Yes>**: **Yes** turns each block to follow the direction of the object at its spot, **No** keeps it unrotated. Then enter the number of segments. |

## Notes
- The number of segments must be from 2 to 32767.
- You can divide lines, arcs, circles, ellipses, polylines, splines, including 3D polylines and 3D splines.
- Points can be hard to see with the default point style. Use [`DDPTYPE`](DDPTYPE-Command) to pick a visible style.
- The tool is in the slide-out of the Draw panel.
