---
summary: Removes polyline vertices by picking near them.
related: ADDVERTEX, PEDIT
---

## Usage
1. At `Select a polyline vertex or [Undo]:`, pick a polyline near the vertex to remove. The vertex nearest the pick is removed.
2. Pick more vertices to remove them. Press Enter or Esc to finish.

## Options
| Option | What it does |
|---|---|
| **Undo** (`U`) | Restores the last vertex removed in this command. |

## Notes
- Works on 2D polylines (lightweight and old-style) and 3D polylines. A polyline keeps at least two vertices.
- Each vertex is one undo step. This is the same edit as the **Remove Vertex** option of a polyline's grip menu.
