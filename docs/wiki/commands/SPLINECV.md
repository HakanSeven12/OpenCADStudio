---
summary: Draws a spline by placing its control vertices.
related: SPLINE, SPLINEDIT
ribbon: Draw tab › Draw panel (slide-out)
---

`SPLINECV` starts [`SPLINE`](SPLINE-Command) with the **Control vertices** method already chosen.

## Usage
1. Specify the first control point, or type an option.
2. Specify the next control points.
3. Press Enter to finish, or type `C` to close the spline.

## Options
| Option | What it does |
|---|---|
| **Method** (`M`) | Switches to the **Fit** (`F`) method or back to **Control vertices** (`CV`). |
| **Degree** (`D`) | Enter a degree from 1 to 10. Default is 3. |
| **Object** (`O`) | Select spline-fit polylines and press Enter to turn them into splines. |
| **Close** (`C`) | With three or more points, closes the spline and finishes. |
| **Undo** (`U`) | Removes the last point. |

## Notes
- The degree is lowered automatically when there are too few points for it.
