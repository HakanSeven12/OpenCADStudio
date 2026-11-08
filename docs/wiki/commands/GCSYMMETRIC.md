---
summary: Makes two objects or two points symmetric about a line.
related: GEOMCONSTRAINT, GCEQUAL, GCCOINCIDENT, GCSHOW, DELCONSTRAINT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select first object or [2Points] <2Points>: pick a line, circle, arc, polyline segment or ellipse.
2. Select second object: pick an object of the same kind.
3. Select symmetry line: pick a line.

To work with points instead, press Enter (or type `2P`) at the first prompt:
1. Select first point: pick an endpoint, center, midpoint or polyline vertex.
2. Select second point: pick another such point.
3. Select symmetry line: pick a line.

## Options
| Option | What it does |
|---|---|
| **2Points** (`2P`) | Makes two points symmetric instead of two objects. This is the default when you press Enter. |

## Notes
- The symmetry line must be a different object from the ones you picked.
- The two objects must be of the same kind, for example two lines or two circular curves.
- You can select three objects first (two objects, then the symmetry line) and start the command; the constraint is applied at once.
