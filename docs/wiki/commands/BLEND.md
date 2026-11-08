---
summary: Creates a spline that joins the endpoints of two open curves smoothly.
related: FILLET, SPLINE, JOIN
---

## Usage
1. Select the first open curve near the end you want to join.
2. Select the second open curve near the end you want to join. While you hover the second curve, a preview of the blend is shown.
3. The blend spline is created and the command ends.

## Options
| Option | What it does |
|---|---|
| **Continuity** (`C`) | Asks for the continuity type: **Tangent** (`T`) or **Curvature** (`C`). Press Enter to keep the current one. |

## Notes
- You can join lines, arcs, open polylines and open splines. Closed curves are not accepted.
- The end nearest to where you click is used.
- **Curvature** is the default. **Tangent** makes the blend follow the direction of both curves at their ends; **Curvature** also matches how sharply they bend.
- The original curves are not changed.
- The new spline takes its layer, color, linetype, lineweight and transparency from the first curve.
- You cannot blend a curve with itself.
