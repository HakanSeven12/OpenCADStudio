---
summary: Applies a fixed constraint that locks a point, or a whole object, at its current position.
related: GCFIX, GEOMCONSTRAINT, DELCONSTRAINT, AUTOCONSTRAIN
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Type `FXCONSTRAINT`.
2. At `Select point or [Object] <Object>:`, click a constraint point such as an endpoint, midpoint or center.
3. The point is fixed in place.

To fix a whole object, type `O` or press Enter, then at `Select object:` click the object.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Fix a whole line, polyline segment, circle, arc, ellipse or spline instead of one point. |

## Notes
- If exactly one line, circle, arc, ellipse, spline or single-segment polyline is selected when you start the command, it is fixed at once.
- Clicking where there is no constraint point shows `No valid constraint point found.` and asks again.
- [`GCFIX`](GCFIX-Command) does the same thing.
