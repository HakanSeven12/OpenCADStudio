# IGES import fixtures

Read by the tests in `src/io/iges_import.rs`.

Five files were written by FreeCAD 1.1.4 (Open CASCADE 7.8) with
`Part.setStaticValue("write.iges.brep.mode", 1)`, which writes manifold solids
(entity 186) with their shells, faces, loops and edges:

| File | Made with | What it holds |
| --- | --- | --- |
| `box.igs` | `Part.makeBox(10, 20, 30)` | A solid on six planes (190), in millimetres |
| `cylinder.igs` | `Part.makeCylinder(5, 10)` | A solid on a cylinder (192) and two planes, in millimetres |
| `inch_cube.igs` | `Part.makeBox(25.4, 25.4, 25.4)` with `write.iges.unit` = `INCH` | A 1 inch cube written in inches (unit flag 1) |
| `spline.igs` | `Part.BSplineCurve().interpolate(...)` through (0,0,0), (10,5,0), (20,0,3) and (30,8,0) | One free cubic spline curve (126) |
| `two_solids.igs` | `Part.makeCompound([Part.makeBox(10, 20, 30), Part.makeBox(5, 5, 5, Vector(100, 0, 0))])` | Two solids, the box at entry 3 and the cube at entry 131 |

Each was written with `shape.exportIges(path)` and read back by FreeCAD as the
same shape.

`attributes.igs` was written by hand (fixed columns checked by a script) to
cover the directory-entry attributes the FreeCAD files leave at their defaults.
It holds four straight spline curves (126, degree 1) along x from 0 to 10:

| Entry | y | Colour | Level | Transform | Expected |
| --- | --- | --- | --- | --- | --- |
| 7 | 0 | 2 (red) | 0 | none | colour index 1, layer `0` |
| 9 | 5 | pointer to entry 1, a colour definition (314) of 60 %, 80 %, 100 % | 5, named `PIPES` by entry 3 (406 form 3) | none | RGB 153, 204, 255, layer `PIPES` |
| 11 | 10 | 1 (black) | 7, unnamed | entry 5 (124): +100 in x | colour index 250, layer `7`, x from 100 to 110 |
| 13 | 15 | none | 0 | none | not read: its status marks it as used by another entity |

`rational_arc.igs` was written by hand too: a quarter circle of radius 10
round the origin in the XY plane, from (10, 0, 0) to (0, 10, 0), as a rational
quadratic spline (126) with control points (10,0,0), (10,10,0), (0,10,0) and
weights 1, √2/2, 1. FreeCAD reads it as an arc of radius 10 and length 5π.

Faces exported in Open CASCADE's faces mode (trimmed surfaces on Plane
entities, 108) are not read yet; see #3048.

`plane108.igs` (issue #3048) and `revolution_large_angle.igs` (issue #3047)
were written by hand: a 10 × 10 square trimmed face on a Plane entity (108),
and a surface of revolution (120) whose end angle is 1,000,000 radians.
