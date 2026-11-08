---
summary: Draws an ellipse or an elliptical arc from its center, axis endpoints and minor axis.
related: ARC, CIRCLE, PLINE
ribbon: Draw tab › Draw panel
---

## Usage
1. Specify the center of the ellipse, or type an option.
2. Specify the endpoint of the major axis.
3. Specify a point to set the minor axis, or type its half-length.

The ellipse is created and the command ends.

## Options
| Option | What it does |
|---|---|
| **Arc** (`A`) | Draws an elliptical arc instead. You give the center, the major axis endpoint and the minor axis as above. Then specify the start angle and the end angle, by picking points or typing degrees. While you move the cursor for the end angle, the arc follows the direction you sweep in. A typed negative end angle runs clockwise. |
| **Axis** (`AXIS`) | Defines the major axis by its two endpoints instead of the center. Specify the first endpoint, the second endpoint, then the minor axis point or half-length. |

## Notes
- The minor axis is measured square to the major axis. It cannot be longer than the major axis; a larger value gives equal axes (a circle shape).
- The ellipse lies in the current working plane (UCS).
- When `PELLIPSE` is 1, the command creates a polyline made of arcs that follows the ellipse instead of a true ellipse.
- The ribbon drop-down offers **Center, Axes**, **Axis, End** and **Ellipse Arc**.
