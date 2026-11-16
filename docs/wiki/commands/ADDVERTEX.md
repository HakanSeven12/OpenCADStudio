---
summary: Adds vertices to polylines by picking a segment and placing the new vertex.
related: DELVERTEX, PEDIT, STRETCH
---

## Usage
1. At `Select a polyline segment or [Undo]:`, pick a segment of a polyline. A new vertex starts on the segment where you picked it.
2. At `Specify new vertex location:`, click where the vertex goes, or press Enter to keep it where you picked the segment.
3. Pick more segments to add more vertices. Press Enter or Esc to finish.

## Options
| Option | What it does |
|---|---|
| **Undo** (`U`) | Undoes the last vertex added in this command. |

## Notes
- Works on 2D polylines (lightweight and old-style) and 3D polylines.
- On an arc segment both halves stay arcs through the new vertex. Segment widths are split at the new vertex.
- Each vertex is one undo step. This is the same edit as the **Add Vertex** option of a polyline's grip menu.
