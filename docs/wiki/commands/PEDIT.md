---
summary: Edits 2D polylines and polygon meshes, and turns lines and arcs into polylines.
related: PLINE, JOIN, SPLINEDIT, EXPLODE, 3DMESH
ribbon: Draw tab › Modify panel
---

## Usage
1. Select a polyline, or type **Multiple** (`M`) to edit several at once.
2. If you pick a line or an arc, answer **Yes** to turn it into a polyline first.
3. Enter an option. Repeat as needed.
4. Type **eXit** (`X`) or press Enter to finish.

With **Multiple**, select the objects, press Enter, and answer whether to convert the selected lines and arcs to polylines.

## Options
| Option | What it does |
|---|---|
| **Close** (`C`) | Adds a segment from the last vertex to the first. |
| **Open** (`O`) | Removes the closing segment. |
| **Join** (`J`) | Select lines, arcs and polylines that meet the ends of the polyline and press Enter to join them into it. With **Multiple**, asks for a fuzz distance and a join type instead. |
| **Width** (`W`) | Sets one width for the whole polyline. You can also type `W` and the value together, for example `W 0.5`. |
| **Fit** (`F`) | Replaces the segments with arcs that pass through every vertex. Tangents set with **Edit vertex** are used. |
| **Spline** (`S`) | Fits a smooth spline curve through the vertex frame. |
| **Decurve** (`D`) | Removes fit or spline curves and returns to straight segments. |
| **Edit vertex** (`E`) | Steps through the vertices to edit one at a time (see below). Not offered with **Multiple**. |
| **Ltype gen** (`L`) | Turns linetype generation **ON** or **OFF**: whether the linetype pattern runs continuously over the whole polyline. |
| **Reverse** (`R`) | Reverses the order of the vertices. |
| **Undo** (`U`) | Undoes the last PEDIT operation. |
| **eXit** (`X`) | Ends the command. |

**Join type** (`J`, with **Multiple**) chooses how ends further apart than zero but within the fuzz distance are joined: **Extend** (`E`) extends or trims the segments to meet, **Add** (`A`) adds a segment between them, **Both** (`B`) extends where it can and adds a segment otherwise.

### Edit vertex
The current vertex is marked. Enter one of:

| Option | What it does |
|---|---|
| **Next** (`N`) / **Previous** (`P`) | Moves the mark to the next or previous vertex. Enter moves to the next one. |
| **Break** (`B`) | Marks this vertex, then move to a second vertex with **Next** or **Previous** and type **Go** (`G`). The part between the two vertices is removed; with both marks on one vertex, the polyline is split there. |
| **Insert** (`I`) | Adds a new vertex after the marked one at a point you specify. |
| **Move** (`M`) | Moves the marked vertex to a point you specify. |
| **Straighten** (`S`) | Marks this vertex, then move to a second vertex and type **Go** (`G`) to replace the part between them with a straight segment. |
| **Tangent** (`T`) | Sets a tangent direction at the vertex, used by **Fit**. |
| **Width** (`W`) | Sets the starting and ending widths of the segment after the vertex. |
| **eXit** (`X`) | Returns to the main options. |

### Polygon meshes
When you select a polygon mesh, the options are:

| Option | What it does |
|---|---|
| **Edit vertex** (`E`) | Steps through the mesh vertices with **Next**, **Previous**, **Left**, **Right**, **Up** and **Down**, and moves one with **Move** (`M`). **Regen** (`G`) redraws, **eXit** (`X`) returns. |
| **Smooth surface** (`S`) | Smooths the mesh, using [`SURFTYPE`](SURFTYPE-System-Variable), [`SURFU`](SURFU-System-Variable) and [`SURFV`](SURFV-System-Variable). |
| **Desmooth** (`D`) | Returns the mesh to its control points. |
| **M close** / **M open** (`MC` / `MO`) | Closes or opens the mesh in the M direction. |
| **N close** / **N open** (`NC` / `NO`) | Closes or opens the mesh in the N direction. |
| **Undo** (`U`) | Undoes the last operation. |
| **eXit** (`X`) | Ends the command. |

## Notes
- If you select a polyline before starting the command, the select step is skipped.
- The join type and fuzz distance are remembered for the session.
