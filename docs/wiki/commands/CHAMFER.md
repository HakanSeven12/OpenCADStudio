---
summary: Bevels the corner between two lines, or between two adjacent segments of a polyline, by two distances.
related: FILLET, CHAMFEREDGE, TRIM, EXTEND
ribbon: Draw tab › Modify panel
---

## Usage
1. Run `CHAMFER`. The prompt shows the current distances, for example "Select first line [D1=… D2=…]:".
2. Pick the first line, or type an option.
3. Pick the second line. For a polyline, pick the adjacent segment of the same polyline.
4. The two lines are cut back by the first and second distance and joined by a new chamfer line.

## Options
| Option | What it does |
|---|---|
| **Distance** (`D`) | Asks for the first and then the second chamfer distance. The second distance offers the first as its default. You can also type `D 5` or `D 5 3` to set both at once. |
| **Multiple** (`M`) | Keeps the command running so you can chamfer several corners. |
| **Undo** (`U`) | In Multiple mode, undoes the last chamfer. |
| **No trim** (`N`) | Keeps the original lines as they are and only adds the chamfer line. |
| **Trim** (`T`) | Cuts the original lines back to the chamfer (the default). |

## Notes
- Lines, rays, construction lines and polyline segments can be chamfered. Arcs cannot.
- With both distances 0 and Trim on, the two lines are trimmed or extended to meet in a sharp corner.
- No trim works only with two separate lines, and needs at least one distance above 0.
- If the distances are longer than the lines allow, you see "Distance is too large."
- The Trim setting is shared with [`FILLET`](FILLET-Command).
- If an edge of a 3D solid is selected when you start the command, `CHAMFER` chamfers the solid edge instead, as [`CHAMFEREDGE`](CHAMFEREDGE-Command) does.
- On the ribbon, `CHAMFER` is in the Fillet drop-down.
