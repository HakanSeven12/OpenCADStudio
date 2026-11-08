---
summary: Sets the shape used to draw point objects.
type: Integer
saved: Drawing
default: 0 (Dot)
related: PDSIZE, DDPTYPE, PTYPE, POINT
---

## Values
| Value | Meaning |
|---|---|
| 0 | A dot. |
| 1 | Nothing is drawn. The point can still be picked. |
| 2 | A plus sign (+). |
| 3 | A cross (x). |
| 4 | A short vertical tick. |
| +32 | Adds a circle around the shape. |
| +64 | Adds a square around the shape. |

Add the values to combine them. For example, 35 is a cross in a circle, and 98 is a plus sign in a circle and a square.

## Notes
- All points in the drawing change at once when you change the value.
- Points on the DEFPOINTS layer always show as dots.
- The size of the shapes comes from [`PDSIZE`](PDSIZE-System-Variable).
- [`DDPTYPE`](DDPTYPE-Command) and [`PTYPE`](PTYPE-Command) set this value from a dialog.
