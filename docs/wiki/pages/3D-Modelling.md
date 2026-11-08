# 3D Modelling

Open CAD Studio builds exact solids (boundary representation): faces, edges and vertices on true planes, cylinders, cones, spheres, tori and spline surfaces. Solids are saved in DWG and DXF as standard 3D solid objects, so they open in other programs that read these formats, and solids from other programs open here.

The tools are on the **Model** tab. For orbiting, views and visual styles, see [Navigating the Drawing](Navigating-the-Drawing#orbit-and-3d-views).

## Primitives

| | Solid | Command |
|---|---|---|
| <img src="images/icons/model/box.svg" width="24" alt=""> | Box (or cube) | `BOX` |
| <img src="images/icons/model/cylinder.svg" width="24" alt=""> | Cylinder | `CYLINDER` (`CYL`) |
| <img src="images/icons/model/cone.svg" width="24" alt=""> | Cone | `CONE` |
| <img src="images/icons/model/sphere.svg" width="24" alt=""> | Sphere | `SPHERE` |
| <img src="images/icons/model/pyramid.svg" width="24" alt=""> | Pyramid | `PYRAMID` |
| <img src="images/icons/model/wedge.svg" width="24" alt=""> | Wedge | `WEDGE` (`WE`) |
| <img src="images/icons/model/torus.svg" width="24" alt=""> | Torus | `TORUS` (`TOR`) |
| <img src="images/icons/model/polysolid.svg" width="24" alt=""> | Polysolid (a wall along a path) | `POLYSOLID` (`PSOLID`) |

Draw the base in the current UCS, then give the height. With [Dynamic UCS](Precision-Drawing#the-user-coordinate-system-ucs) on, the base lies on the face under the cursor.

## Solids from 2D shapes

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/model/extrude.svg" width="24" alt=""> | Extrude | `EXTRUDE` (`EXT`) | Pushes closed shapes into solids, or open curves into surfaces. Give a height, a **Direction**, a **Path**, or a **Taper angle**. |
| <img src="images/icons/model/revolve.svg" width="24" alt=""> | Revolve | `REVOLVE` (`REV`) | Turns a shape around an axis. |
| <img src="images/icons/model/sweep.svg" width="24" alt=""> | Sweep | `SWEEP` | Moves a profile along a path: pipes, handrails, threads along a helix. |
| <img src="images/icons/model/loft.svg" width="24" alt=""> | Loft | `LOFT` | Builds a solid through a series of cross-sections. |
| <img src="images/icons/model/presspull.svg" width="24" alt=""> | Press/Pull | `PRESSPULL` | Click inside a closed area or on a face and pull: it is extruded, or cut into the solid. |
| <img src="images/icons/model/thicken.svg" width="24" alt=""> | Thicken | `THICKEN` | Gives a surface a thickness. |

`MODE` in Extrude chooses between solid and surface results.

## Boolean operations

| | Tool | Command | Result |
|---|---|---|---|
| <img src="images/icons/model/union.svg" width="24" alt=""> | Union | `UNION` (`UNI`) | One solid from several. |
| <img src="images/icons/model/subtract.svg" width="24" alt=""> | Subtract | `SUBTRACT` (`SU`) | The first solids minus the second ones: holes, pockets, cut-outs. |
| <img src="images/icons/model/intersect.svg" width="24" alt=""> | Intersect | `INTERSECT` (`IN`) | Only the volume the solids share. |
| <img src="images/icons/model/interfere.svg" width="24" alt=""> | Interference | `INTERFERE` (`INF`) | Select two solids first: it creates their overlap as a new solid and keeps both originals. |

The same commands combine 2D regions.

## Edges

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/model/fillet.svg" width="24" alt=""> | Fillet Edge | `FILLETEDGE` | Rounds edges of a solid, singly or as connected chains. |
| <img src="images/icons/model/chamfer.svg" width="24" alt=""> | Chamfer Edge | `CHAMFEREDGE` | Bevels edges of a solid. |
| <img src="images/icons/model/shell.svg" width="24" alt=""> | Shell | `SHELL` | Hollows a box, cylinder or sphere to a wall thickness, optionally leaving faces open. |

## Cutting and sections

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/model/slice.svg" width="24" alt=""> | Slice | `SLICE` (`SL`) | Cuts solids with a plane: by three points, an object, a surface, the Z axis, the view, or the XY, YZ or ZX plane. Keep one side or both. |
| <img src="images/icons/model/sectionplane.svg" width="24" alt=""> | Section Plane | `SECTIONPLANE` | Places a section object that cuts the view of the model live. |
| | Section | `SECTION` (`SEC`) | Draws the outline where a plane square to the X, Y or Z axis cuts a solid, as lines. |
| <img src="images/icons/model/flatshot.svg" width="24" alt=""> | Flatshot | `FLATSHOT` (`FSHOT`) | Projects the edges of solids straight down onto the Z=0 plane as a flat top view. |

## Editing solids

- Ctrl+click a face, edge or vertex of a solid to select just that part, then move it with its grip.
- `3DROTATE` turns objects about an axis through a base point. `3DMIRROR` adds a mirrored copy of a solid across its X, Y or Z plane. `3DALIGN` moves and turns one solid from three source points to three target points. <img src="images/icons/model/array3d.svg" width="24" alt=""> `3DARRAY` copies objects in rows, columns and levels. `3DMOVE` is the same as `MOVE`, which works in 3D with typed or snapped points.
- `CONVTOSURFACE` turns objects into surfaces; `EXPLODE` turns a solid into its faces.

## Measuring

`MASSPROP` reports the volume, mass, centroid, moments of inertia and radii of gyration of solids. Object snaps for solids (vertex, edge midpoint, face centre and others) turn on with **F4**; see [Precision Drawing](Precision-Drawing#3d-object-snaps).

## Exporting

| Command | Writes |
|---|---|
| `EXPORTSTEP` (`STEPOUT`) | STEP files for other CAD programs. |
| `EXPORTSTL` (`STLOUT`) | STL meshes for 3D printing. |
| `FBXEXPORT` | FBX for visualisation tools. |

OBJ meshes and SAT solids can be imported; see [Files and Formats](Files-and-Formats#importing-and-exporting).
