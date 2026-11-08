---
summary: Removes the part of an object between two points, or splits it at one point.
related: BREAKATPOINT, TRIM, JOIN, LENGTHEN
ribbon: Draw tab › Modify panel (expanded)
---

## Usage
1. Select the object. The point where you click it is the first break point.
2. Specify the second break point. The part between the two points is removed.

## Options
| Option | What it does |
|---|---|
| **First point** (`F`) | Lets you specify the first break point again instead of using the point where you selected the object. |
| **@** | Typed as the second point, breaks the object at the first point without removing anything. |

## Notes
- You can break lines, arcs, circles, ellipses, lightweight polylines and splines. Other objects are reported as not supported.
- Breaking a circle leaves an arc.
- The command reports how many pieces the object was split into. If nothing changed, it says so.
- Objects on locked layers cannot be broken.
