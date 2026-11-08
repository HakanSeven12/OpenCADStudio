---
summary: Draws an arc from three points, or from a start point, center, end point, angle, chord length, direction or radius.
related: CIRCLE, PLINE, ELLIPSE, FILLET
ribbon: Draw tab › Draw panel (Arc drop-down)
---

## Usage
The default method draws an arc through three points:
1. Specify start point of arc, or type **Center** (`C`).
2. Specify second point of arc, or type **Center** (`C`) or **End** (`E`).
3. Specify end point of arc.

Start, center method (after **Center** at the second prompt, or **Center** first and then the start point):
1. Specify the start point and the center point.
2. Specify end point of arc, or type **Angle** (`A`) or **chord Length** (`L`). A plain number typed here is the length of the arc along the circle.

Start, end method (after **End** at the second prompt):
1. Specify end point of arc.
2. Specify center point of arc, or type **Angle** (`A`), **Direction** (`D`) or **Radius** (`R`).

Press Enter at the first prompt to continue an arc tangent from the end of the last line or arc you drew.

## Options
| Option | What it does |
|---|---|
| **Center** (`C`) | Asks for the center point of the arc next. |
| **End** (`E`) | Asks for the end point after the start point, then the center or another value. |
| **Angle** (`A`) | Asks for the included angle. You can type it or pick a point. |
| **chord Length** (`L`) | Asks for the length of the chord between start and end. |
| **Direction** (`D`) | Asks for the tangent direction at the start point. |
| **Radius** (`R`) | Asks for the radius. You can type it or pick a point. |

## Notes
- Arcs run counterclockwise. Hold Ctrl to flip the direction while you pick.
- In the three-point method you can pick a point with the Tangent object snap to make the arc tangent to that object.
- The Arc drop-down on the ribbon starts the other arc methods directly: 3-Point, Start Center End, Start Center Angle, Start Center Length, Start End Angle, Start End Direction, Start End Radius, Center Start End, Center Start Angle, Center Start Length, and Continue.
- The arc is drawn in the current working plane (UCS).
