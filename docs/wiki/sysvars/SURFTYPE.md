---
summary: Sets the kind of smoothing PEDIT Smooth surface applies to polygon meshes.
type: Integer
saved: Drawing
default: 6 (cubic B-spline)
related: PEDIT, SURFU, SURFV
---

## Values
| Value | Meaning |
|---|---|
| 5 | Quadratic B-spline surface |
| 6 | Cubic B-spline surface |
| 8 | Bezier surface |

Only 5, 6 or 8 is accepted.

## Notes
- Used by the Smooth surface option of [`PEDIT`](PEDIT-Command) on 3D polygon meshes.
- Also set in Options › 3D Modeling.
