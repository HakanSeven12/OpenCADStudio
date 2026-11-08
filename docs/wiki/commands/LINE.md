---
summary: Draws a series of connected straight line segments; each segment is a separate line object.
related: PLINE, XLINE, RAY, LENGTHEN
ribbon: Draw tab › Draw panel
---

## Usage
1. At `Specify first point:`, pick the start point. Press Enter to start from the last point you drew.
2. At `Specify next point  [Undo]:`, pick the end of the first segment.
3. Keep picking points. Each pick adds one segment.
4. Press Enter or Esc to finish, or type `C` to close the shape.

## Options
| Option | What it does |
|---|---|
| **Close** (`C`) | Draws a last segment back to the first point and ends the command. Available once a segment is drawn. |
| **Undo** (`U`) | Removes the last segment. If only the start point is placed, clears it. |
| **eXit** (`X`) | Ends the command and keeps every segment drawn. |

## Notes
- With the tangent object snap, you can start or end a segment tangent to a circle or ellipse. If both ends snap tangent, the line becomes the common tangent of the two curves.
- You can type a distance at the next-point prompt to draw a segment of that length in the direction of the cursor.
