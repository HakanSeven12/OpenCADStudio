# Parametric Constraints

Constraints are rules that geometry keeps when you edit it. A **geometric** constraint fixes a relationship (these two lines stay parallel); a **dimensional** constraint fixes a value (this line is 50 long). Change one value and the shape follows, keeping all its rules.

The tools are on the **Parametric** tab.

## Geometric constraints

| | Constraint | Command | Keeps |
|---|---|---|---|
| <img src="images/icons/constrain/coincident.svg" width="20" alt=""> | Coincident | `GCCOINCIDENT` | Two points together, or a point on a curve. |
| <img src="images/icons/constrain/colinear.svg" width="20" alt=""> | Collinear | `GCCOLLINEAR` | Two lines on one infinite line. |
| <img src="images/icons/constrain/concentric.svg" width="20" alt=""> | Concentric | `GCCONCENTRIC` | Circles, arcs and ellipses on one centre. |
| <img src="images/icons/constrain/fixed.svg" width="20" alt=""> | Fix | `GCFIX` | A point or object in place. |
| <img src="images/icons/constrain/parallel.svg" width="20" alt=""> | Parallel | `GCPARALLEL` | Two lines parallel. |
| <img src="images/icons/constrain/perpendicular.svg" width="20" alt=""> | Perpendicular | `GCPERPENDICULAR` | Two lines at 90°. |
| <img src="images/icons/constrain/horizontal.svg" width="20" alt=""> | Horizontal | `GCHORIZONTAL` | A line, or two points, horizontal. |
| <img src="images/icons/constrain/vertical.svg" width="20" alt=""> | Vertical | `GCVERTICAL` | A line, or two points, vertical. |
| <img src="images/icons/constrain/tangent.svg" width="20" alt=""> | Tangent | `GCTANGENT` | A curve tangent to another. |
| <img src="images/icons/constrain/smooth.svg" width="20" alt=""> | Smooth | `GCSMOOTH` | A spline joined smoothly to another curve. |
| <img src="images/icons/constrain/symmetric.svg" width="20" alt=""> | Symmetric | `GCSYMMETRIC` | Two objects or points mirrored about a line. |
| <img src="images/icons/constrain/equal.svg" width="20" alt=""> | Equal | `GCEQUAL` | Two lines of one length, or two arcs of one radius. |
| <img src="images/icons/constrain/midpoint.svg" width="20" alt=""> | Midpoint | `MPCONSTRAINT` | A point at the middle of a line or arc. |
| <img src="images/icons/constrain/center_point.svg" width="20" alt=""> | Center Point | `CPCONSTRAINT` | A point at the centre of a circle or arc. |
| <img src="images/icons/constrain/point_on_curve.svg" width="20" alt=""> | Point on Curve | `OCCONSTRAINT` | A point on a curve. |
| <img src="images/icons/constrain/normal.svg" width="20" alt=""> | Normal | `NRCONSTRAINT` | A line normal to a curve. |
| <img src="images/icons/constrain/equal_distance.svg" width="20" alt=""> | Equal Distance | `EDCONSTRAINT` | Equal distances between pairs of points. |

<img src="images/icons/constrain/auto.svg" width="20" alt=""> `AUTOCONSTRAIN` adds the constraints that the selected geometry already meets: lines that are parallel get Parallel, ends that touch get Coincident, and so on. With `CONSTRAINTINFER` on, constraints are added as you draw.

Constraint bars next to the geometry show its constraints. <img src="images/icons/constrain/show_all.svg" width="20" alt=""> Show all and <img src="images/icons/constrain/hide_all.svg" width="20" alt=""> Hide all toggle every bar.

## Dimensional constraints

| | Constraint | Command | Fixes |
|---|---|---|---|
| <img src="images/icons/constrain/distance.svg" width="20" alt=""> | Aligned | `DCALIGNED` | The distance between two points. |
| <img src="images/icons/constrain/distance_x.svg" width="20" alt=""> | Horizontal | `DCHORIZONTAL` | The horizontal distance. |
| <img src="images/icons/constrain/distance_y.svg" width="20" alt=""> | Vertical | `DCVERTICAL` | The vertical distance. |
| <img src="images/icons/constrain/angle.svg" width="20" alt=""> | Angular | `DCANGULAR` | The angle between two lines. |
| | Radius | `DCRADIUS` | The radius of a circle or arc. |
| | Diameter | `DCDIAMETER` | The diameter of a circle or arc. |
| <img src="images/icons/constrain/convert.svg" width="20" alt=""> | Convert | `DCCONVERT` | Turns an existing dimension into a constraint. |

Double-click a dimensional constraint to change its value. A value can be a number, a name (`width = 120`) or an expression that uses other names (`height = width / 2`).

**Dynamic or annotational.** Dynamic constraints are for editing: they keep their size on screen and do not plot. Annotational constraints look and plot like dimensions. Switch with `DCFORM` or in Properties.

## The Parameters Manager

<img src="images/icons/constrain/parameters.svg" width="20" alt=""> `PARAMETERS` lists every dimensional constraint and user parameter of the drawing with its expression and value. Edit the expressions here, add your own parameters, and rename them.

## Removing constraints

<img src="images/icons/constrain/delete.svg" width="20" alt=""> `DELCONSTRAINT` removes all constraints from the selected objects. Moving or stretching a constrained object respects its constraints. In the Block Editor, geometry is coloured as unconstrained, partially constrained, fully constrained or over-constrained, so you can see when a block has too many or too few rules.

## Settings

`CONSTRAINTSETTINGS` opens the settings that choose which constraint types `AUTOCONSTRAIN` applies. How dimensional constraint names and values are shown is set with `CONSTRAINTNAMEFORMAT`, and whether constraints are inferred while you draw with `CONSTRAINTINFER`.
