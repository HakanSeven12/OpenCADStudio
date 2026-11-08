---
summary: Creates construction lines that run to infinity in both directions.
related: RAY, LINE, OFFSET
ribbon: Draw tab › Draw panel (slide-out, Construction Line)
---

## Usage
1. At `XLINE  Specify a point or [Hor/Ver/Ang/Bisect/Offset]:`, pick a point the line passes through.
2. At `XLINE  Specify through point:`, pick a second point. A construction line is created.
3. Keep picking through points to add more lines from the same point. Press Enter or Esc to finish.

## Options
| Option | What it does |
|---|---|
| **Hor** (`H`) | Horizontal lines (along the current X axis) through each point you pick. |
| **Ver** (`V`) | Vertical lines (along the current Y axis) through each point you pick. |
| **Ang** (`A`) | Asks `Enter angle of xline <0> or [Reference]:`, then makes lines at that angle through each point you pick. **Reference** (`R`) asks you to select a line, ray or construction line; the angle is then measured from it. |
| **Bisect** (`B`) | Asks for the angle vertex, the angle start point and the angle end point; the line passes through the vertex and halves the angle. Further end points add more bisectors. |
| **Offset** (`O`) | Asks `Specify offset distance or [Through] <Through>:`, then `Select a line object:` (a line, ray or construction line) and the side to offset to. With **Through** (`T`) you pick a point the parallel line passes through instead. |

## Notes
- Angles and distances can also be given with Dynamic Input.
