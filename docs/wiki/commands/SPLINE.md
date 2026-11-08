---
summary: Draws a smooth spline through fit points or from control vertices.
related: SPLINECV, SPLINEDIT, SPLINEFIT, PEDIT
ribbon: Draw tab › Draw panel (slide-out)
---

## Usage
1. Specify the first point, or type an option.
2. Specify the next points. The preview shows the curve.
3. Press Enter to finish. A spline needs at least two points.

## Options
| Option | What it does |
|---|---|
| **Method** (`M`) | Chooses how points are used: **Fit** (`F`) the curve passes through the points, or **Control vertices** (`CV`) the points pull the curve. |
| **Knots** (`K`) | Fit method only. Chooses the knot parameterization: **Chord** (`CH`), **Square root** (`S`) or **Uniform** (`U`). |
| **Degree** (`D`) | Control vertices method only. Enter a degree from 1 to 10. Default is 3. |
| **Object** (`O`) | Select spline-fit polylines and press Enter to turn them into splines. |
| **Tangency** (`T`) | Fit method only. After the first point, point the start tangent direction. After more points, point the end tangent direction; the spline is then finished. |
| **Close** (`C`) | With three or more points, closes the spline and finishes. |
| **Undo** (`U`) | Removes the last point. |

## Notes
- Choose the method before the first point. After the first point, the method, knots, degree and object options are no longer offered.
