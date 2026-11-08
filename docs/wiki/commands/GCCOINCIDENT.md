---
summary: Applies a coincident constraint that keeps two points together, or keeps a point on a curve.
related: CCONSTRAINT, GEOMCONSTRAINT, AUTOCONSTRAIN, DELCONSTRAINT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Type `GCCOINCIDENT`.
2. At `Select first point or [Object/Autoconstrain] <Object>:`, click a constraint point such as an endpoint, or click a curve.
3. If you clicked a point, at `Select second point or object:` click another point, or a curve for the first point to lie on.
4. The constraint is applied.

If you start with a curve (or type `O` and pick one), the command asks `Select point or [Multiple]:` for the point to place on that curve.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Pick a curve first, then the points that must lie on it. |
| **Multiple** (`M`) | After picking a curve: place several points on it. Press Enter when done. |
| **Autoconstrain** (`A`) | Select objects and press Enter. Coincident constraints are added wherever their points already meet. |

## Notes
- Curves you can use: lines, polylines, circles, arcs, ellipses and splines.
- [`CCONSTRAINT`](CCONSTRAINT-Command) does the same thing.
