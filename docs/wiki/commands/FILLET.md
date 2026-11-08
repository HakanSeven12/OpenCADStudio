---
summary: Rounds the corner between two objects with an arc of a set radius, or rounds every corner of a polyline.
related: CHAMFER, FILLETEDGE, BLEND, EXTEND, TRIM
ribbon: Draw tab › Modify panel
---

## Usage
1. Type `FILLET`. The prompt shows the current radius: `Select first object or [Polyline/Radius]  [R=...]:`.
2. Click the first object near the corner you want to round.
3. Click the second object. The arc is added and both objects are trimmed or extended to meet it.

With a radius of 0 the two objects are trimmed or extended to a sharp corner.

## Options
| Option | What it does |
|---|---|
| **Polyline** (`P`) | Pick a lightweight polyline. Every corner of it is rounded with the current radius. |
| **Radius** (`R`) | Type a new radius, or pick two points to measure it. Press Enter to keep the current one. You can also type `R 5` to set the radius in one go. |
| **Trim** (`T`) | Choose **Trim** (`T`) or **No trim** (`N`). With No trim, the original objects are left as they are and only the arc is added. |
| **Multiple** (`M`) | Keep filleting pairs of objects until you press Enter or Esc. |
| **Undo** (`U`) | In Multiple mode, takes back the last fillet. |

## Notes
- You can fillet lines, arcs, polyline segments, rays, construction lines, circles, ellipses and splines.
- If a 3D solid edge is selected when you start `FILLET`, it runs [`FILLETEDGE`](FILLETEDGE-Command) instead.
- The radius and trim mode are remembered for the next time.
- Objects on locked layers cannot be changed.
