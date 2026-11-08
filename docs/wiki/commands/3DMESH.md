---
summary: Creates a polygon mesh from an M by N grid of vertices that you place one by one.
related: 3DFACE, MESH, 3DPOLY
---

## Usage
1. Enter the size of the mesh in the M direction (2 to 256).
2. Enter the size of the mesh in the N direction (2 to 256).
3. Specify the location for each vertex. The prompt shows the vertex index, for example (0, 0), (0, 1) and so on, row by row.
4. The mesh is created when the last vertex is placed.

## Notes
- A size outside 2 to 256 is ignored and the prompt is repeated.
- Each vertex keeps its own Z value.
- A preview of the grid follows the cursor while you place vertices.
- Pressing Enter does not end the command early; place all M × N vertices or press Esc to cancel.
