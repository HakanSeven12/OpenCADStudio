---
summary: Makes a line, polyline segment, text or ellipse axis, or the line through two points, parallel to the X axis of the current coordinate system.
related: GCVERTICAL, GEOMCONSTRAINT, GCPARALLEL, GCPERPENDICULAR, GCSHOW
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select an object or [2Points] <2Points>: pick a line segment, polyline segment, text, MText, or the major or minor axis of an ellipse or elliptical arc.
2. Or press Enter (or type `2P`) to use two points:
   1. Select first point: pick a constraint point on an object, such as an endpoint.
   2. Select second point: pick another constraint point.

## Options
| Option | What it does |
|---|---|
| **2Points** (`2P`) | Constrains two points so they line up horizontally. This is the default when you press Enter. |

## Notes
- The direction follows the X axis of the current UCS.
- If exactly one supported object is selected when you start the command, the constraint is applied at once.
- Picking the same point twice is refused.
