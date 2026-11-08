---
summary: Bevels selected edges of a 3D solid by two distances.
related: FILLETEDGE, CHAMFER, SOLIDCHAMFER
ribbon: Model tab › Edges panel
---

## Usage
1. Run `CHAMFEREDGE`. The current distances are shown as "Distance1 = …, Distance2 = …".
2. At "Select an edge or [Loop/Distance]:", pick one or more edges of a solid.
3. Press Enter. A preview of the chamfer is shown.
4. At "Press Enter to accept the chamfer or [Distance]:", press Enter to apply it.

## Options
| Option | What it does |
|---|---|
| **Loop** (`L`) | Pick an edge to select the whole loop of edges around a face. Then **Accept** (`A`) the loop or show the **Next** (`N`) loop that shares the edge. **Edge** (`E`) goes back to picking single edges. |
| **Distance** (`D`) | Asks "Specify Distance1 or [Expression]" and then "Specify Distance2 or [Expression]". Press Enter to keep the value shown. |
| **Expression** (`E`) | At a distance prompt, type the distance as an expression. |

## Notes
- Edges selected before you start the command are used.
- Press Escape to cancel without changing the solid.
- [`SOLIDCHAMFER`](SOLIDCHAMFER-Command) runs the same command.
