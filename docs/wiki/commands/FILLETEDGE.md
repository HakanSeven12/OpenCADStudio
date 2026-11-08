---
summary: Rounds the edges of a 3D solid with a fillet of a set radius.
related: CHAMFEREDGE, FILLET, SHELL, UNION
ribbon: Model tab › Edges panel
---

## Usage
1. Type `FILLETEDGE`.
2. At `Select an edge or [Chain/Loop/Radius]:`, click the solid edges to round. A preview shows the result.
3. Press Enter when you have picked the edges.
4. At `Press Enter to accept the fillet or [Radius]:`, press Enter to apply it.

## Options
| Option | What it does |
|---|---|
| **Chain** (`C`) | Picking one edge also picks the edges connected to it in a chain. **Edge** (`E`) goes back to picking single edges. |
| **Loop** (`L`) | Pick an edge to select a whole loop of edges around a face. At `Enter an option [Accept/Next] <Accept>:`, type **Next** (`N`) to try the other loop, or **Accept** (`A`) to keep it. |
| **Radius** (`R`) | Type the fillet radius. **Expression** (`E`) lets you type a math expression instead. |

## Notes
- If you select a solid edge first, the command starts with that edge.
- The whole fillet is one undo step.
