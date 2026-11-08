---
summary: Locks a point or a whole object in place so other constraints cannot move it.
related: GEOMCONSTRAINT, GCCOINCIDENT, DELCONSTRAINT, GCSHOW
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select point or [Object] <Object>: pick a constraint point on an object, such as an endpoint or a center.
2. Or press Enter (or type `O`), then Select object: pick the object to fix as a whole.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Fixes a whole object instead of a single point. This is the default when you press Enter. |

## Notes
- Objects that can be fixed: lines, polyline segments, circles, arcs, ellipses and splines.
- If one supported object is already selected when you start the command, it is fixed at once.
- If no valid point or object is found, the command says so and asks again.
- Fixing the same thing twice is refused with a message.
