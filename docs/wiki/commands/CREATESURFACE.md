---
summary: Creates a TIN surface from COGO points or points.
related: CREATEPOINTS, CREATEPOINTGROUP
---

## Usage
1. Select COGO points or point objects, then press Enter.
2. Select lines or polylines to use as breaklines, or press Enter for none.
3. Select a closed polyline as the outer boundary, or press Enter for none.

## Notes
- Needs three or more points apart in plan; of points at the same plan position the first is used.
- The surface is a native TIN surface triangulated from the points. Its definition lists the points, the breaklines and the boundary, each kept as a reference to the objects they came from.
- Breaklines become triangle edges, their vertices joining the surface; a breakline through another vertex is left as the triangulation has it.
- Triangles outside the outer boundary are hidden.
- Lines, polylines and 3D polylines can be breaklines and boundaries.
