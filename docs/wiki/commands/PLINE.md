---
summary: Draws a 2D polyline: one object made of connected straight and arc segments, with constant or tapering width.
related: PEDIT, LINE, 3DPOLY, RECTANG, POLYGON, EXPLODE, JOIN
ribbon: Draw tab › Draw panel
---

## Usage
1. Specify the start point.
2. Specify the next point, or type an option. Each point adds a segment.
3. Type **Arc** (`A`) to draw arc segments, and **Line** (`L`) to go back to straight segments.
4. Press Enter to finish, or type **Close** to close the polyline.

Arc segments start tangent to the previous segment unless you choose another way to define them.

## Options
In line mode:

| Option | What it does |
|---|---|
| **Arc** (`A`) | Switches to arc segments. |
| **Close** (`C`) | Draws a straight segment back to the start point and ends the command. Offered once there are at least two segments. |
| **Halfwidth** (`H`) | Sets the starting and ending half-width of the next segment. |
| **Length** (`L`) | Draws a segment of the length you type, in the direction of the previous segment. |
| **Undo** (`U`) | Removes the last vertex. You can undo back to the start point. |
| **Width** (`W`) | Sets the starting and ending width of the next segment. |

In arc mode:

| Option | What it does |
|---|---|
| **Angle** (`A`) | Specify the included angle, then the endpoint, or **CEnter** (`CE`) or **Radius** (`R`). |
| **CEnter** (`CE`) | Specify the center point, then the endpoint, or **Angle** (`A`) or **Length** (`L`) of the chord. |
| **CLose** (`CL`) | Closes the polyline with an arc tangent to the last segment. `C` also works. |
| **Direction** (`D`) | Specify the tangent direction at the start of the arc, then the endpoint. |
| **Halfwidth** (`H`) | Same as in line mode. |
| **Line** (`L`) | Switches back to straight segments. |
| **Radius** (`R`) | Specify the radius, then the endpoint, or **Angle** (`A`) and the chord direction. |
| **Second pt** (`S`) | Specify a second point on the arc, then the endpoint. |
| **Undo** (`U`) | Leaves the current arc prompt, or removes the last vertex. |
| **Width** (`W`) | Same as in line mode. |

## Notes
- The ending width of a segment becomes the starting width of the next one. The width you set is remembered for the next polyline you draw.
- Clicking on the start point again (for example with an endpoint snap) closes the polyline.
- The polyline already exists while you draw, so you can snap to its earlier segments. Pressing Esc keeps the segments drawn so far.
- The polyline is drawn in the current UCS.
- Edit polylines later with [`PEDIT`](PEDIT-Command).
