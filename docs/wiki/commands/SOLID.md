---
summary: Draws filled triangles and quadrilaterals from three or four corner points.
related: 3DFACE, HATCH, PLINE
---

## Usage
1. Specify the first point.
2. Specify the second point.
3. Specify the third point.
4. Specify the fourth point, or press Enter to draw a triangle and end the command.
5. After a fourth point, the quadrilateral is drawn and its last edge starts the next shape. Keep specifying third and fourth points, or press Enter to finish.

## Notes
- The corners are joined in the order 1-2-4-3. Picking the points in a circle gives a bow-tie shape; pick them in a Z shape for a plain quadrilateral. The preview shows the result.
- The shape is drawn on the current UCS plane.
