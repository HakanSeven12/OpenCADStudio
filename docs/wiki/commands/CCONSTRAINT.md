---
summary: Adds a coincident constraint that keeps two points together, or keeps a point on a curve.
related: GEOMCONSTRAINT, AUTOCONSTRAIN, DELCONSTRAINT, GCCOINCIDENT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Run `CCONSTRAINT`.
2. At "Select first point or [Object/Autoconstrain] <Object>:", pick a constraint point on an object (such as an endpoint), or pick a curve.
3. If you picked a point, select the second point or object. The first point is constrained to it.
4. If you picked a curve, select the point that must stay on that curve.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Pick the curve first, then the point (or points) that must lie on it. Pressing Enter at the first prompt does the same. |
| **Autoconstrain** (`A`) | Select several objects and press Enter. Coincident constraints are added wherever their points already meet. |
| **Multiple** (`M`) | After you picked a curve: keep picking points to place on it. Press Enter when done. |
| **Point** (`P`) | After you picked a curve: pick a single point (the default). |

## Notes
- Curves that can take a point are lines, polylines, circles, arcs, ellipses and splines.
- [`GCCOINCIDENT`](GCCOINCIDENT-Command) runs the same command.
