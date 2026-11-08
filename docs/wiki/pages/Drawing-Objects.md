# Drawing Objects

The drawing tools are on the **Draw** tab, in the **Draw** panel. Every tool is also a command; the alias is in brackets. Prompts show the options of each step in square brackets: type the capital letters of an option, or click it in the command line.

Use [object snaps and typed coordinates](Precision-Drawing) to place points exactly.

## Lines and polylines

| | Tool | Command | What you get |
|---|---|---|---|
| <img src="images/icons/line.svg" width="24" alt=""> | Line | `LINE` (`L`) | Separate straight segments, one per pair of points. `Undo` takes back the last one; Enter ends. |
| <img src="images/icons/polyline.svg" width="24" alt=""> | Polyline | `PLINE` (`PL`) | One object of connected segments, straight or curved, with widths if you want them. |
| <img src="images/icons/line.svg" width="24" alt=""> | 3D Polyline | `3DPOLY` | Connected straight segments whose points can be at any height. |
| <img src="images/icons/xline.svg" width="24" alt=""> | Construction Line | `XLINE` (`XL`) | A line without ends, for layout. Options: **Hor**, **Ver**, **Ang**, **Bisect**, **Offset**. |
| <img src="images/icons/ray.svg" width="24" alt=""> | Ray | `RAY` | A line with a start point that runs on without end. |
| | Multiline | `MLINE` (`ML`) | Several parallel lines drawn at once, for walls and roads. Styles come from `MLSTYLE`. |

**Polyline options.** In line mode: **Arc** (switch to arcs), **Close**, **Halfwidth**, **Length**, **Undo**, **Width**. In arc mode you can set the arc by **Angle**, **CEnter**, **Direction**, **Radius** or **Second pt**, and **Line** goes back to straight segments. Width can taper: give a different start and end width.

## Circles and arcs

<img src="images/icons/circle/circle.svg" width="24" alt=""> `CIRCLE` (`C`). The **Circle** list on the ribbon has every way to define one:

| | Method |
|---|---|
| <img src="images/icons/circle/circle_cr.svg" width="20" alt=""> | Centre and radius |
| <img src="images/icons/circle/circle_cd.svg" width="20" alt=""> | Centre and diameter |
| <img src="images/icons/circle/circle_2p.svg" width="20" alt=""> | Two points: the ends of a diameter |
| <img src="images/icons/circle/circle_3p.svg" width="20" alt=""> | Three points on the circle |
| <img src="images/icons/circle/circle_ttr.svg" width="20" alt=""> | Tangent to two objects, with a radius |
| <img src="images/icons/circle/circle_ttt.svg" width="20" alt=""> | Tangent to three objects |

<img src="images/icons/arc/arc.svg" width="24" alt=""> `ARC` (`A`). The **Arc** list has the eleven ways to build an arc from start point (S), centre (C), end point (E), angle (A), length of chord (L), radius (R) and direction (D):

| | Method | | Method |
|---|---|---|---|
| <img src="images/icons/arc/arc_3p.svg" width="20" alt=""> | Three points | <img src="images/icons/arc/arc_csa.svg" width="20" alt=""> | Centre, start, angle |
| <img src="images/icons/arc/arc_sce.svg" width="20" alt=""> | Start, centre, end | <img src="images/icons/arc/arc_csl.svg" width="20" alt=""> | Centre, start, length |
| <img src="images/icons/arc/arc_sca.svg" width="20" alt=""> | Start, centre, angle | <img src="images/icons/arc/arc_sea.svg" width="20" alt=""> | Start, end, angle |
| <img src="images/icons/arc/arc_scl.svg" width="20" alt=""> | Start, centre, length | <img src="images/icons/arc/arc_sed.svg" width="20" alt=""> | Start, end, direction |
| <img src="images/icons/arc/arc_cse.svg" width="20" alt=""> | Centre, start, end | <img src="images/icons/arc/arc_ser.svg" width="20" alt=""> | Start, end, radius |
| <img src="images/icons/arc/arc_cont.svg" width="20" alt=""> | Continue tangent from the last line or arc | | |

Arcs run counter-clockwise from the start point. Hold **Ctrl** while dragging to flip the direction where the method allows it.

<img src="images/icons/donut.svg" width="24" alt=""> `DONUT` (`DO`) draws filled rings; an inside diameter of 0 gives a filled dot.

## Ellipses

<img src="images/icons/ellipse.svg" width="24" alt=""> `ELLIPSE` (`EL`):

| | Method |
|---|---|
| <img src="images/icons/ellipse/ellipse_ctr.svg" width="20" alt=""> | Centre, then the end of each axis |
| <img src="images/icons/ellipse/ellipse_axis.svg" width="20" alt=""> | The two ends of one axis, then the other half-axis |
| <img src="images/icons/ellipse/ellipse_arc.svg" width="20" alt=""> | Elliptical arc: an ellipse, then start and end angles |

## Rectangles and polygons

<img src="images/icons/shapes/rect.svg" width="24" alt=""> `RECTANG` (`REC`) draws a closed polyline rectangle.

| | Method |
|---|---|
| <img src="images/icons/shapes/rect.svg" width="20" alt=""> | Two opposite corners |
| <img src="images/icons/shapes/rect_rot.svg" width="20" alt=""> | Rotated: a first edge at any angle, then the width |
| <img src="images/icons/shapes/rect_cen.svg" width="20" alt=""> | From the centre |

Before the first corner you can set **Chamfer** or **Fillet** corners, an **Elevation**, a **Thickness** and a line **Width**. After the first corner you can give the rectangle by **Area**, by **Dimensions** or by **Rotation** instead of a second corner.

<img src="images/icons/shapes/poly_i.svg" width="24" alt=""> `POLYGON` (`POL`) draws a regular polygon with 3 to 1024 sides:

| | Method |
|---|---|
| <img src="images/icons/shapes/poly_i.svg" width="20" alt=""> | Inscribed: corners on a circle you give |
| <img src="images/icons/shapes/poly_c.svg" width="20" alt=""> | Circumscribed: edges touching a circle you give |
| <img src="images/icons/shapes/poly_e.svg" width="20" alt=""> | By the two ends of one edge |

## Splines and curves

<img src="images/icons/spline.svg" width="24" alt=""> `SPLINE` (`SPL`) draws a smooth curve. Choose the method first: **Fit** (the curve passes through the points you click) or **Control vertices** (the points pull the curve towards them, as with <img src="images/icons/spline_cv.svg" width="20" alt=""> `SPLINECV`). Edit a spline afterwards with `SPLINEDIT`, or with its grips.

<img src="images/icons/helix.svg" width="24" alt=""> `HELIX` draws a 2D spiral or a 3D helix from a base radius, a top radius and a height, for springs, threads and sweep paths.

`SKETCH` records freehand lines as you move the mouse.

## Points

<img src="images/icons/point.svg" width="24" alt=""> `POINT` (`PO`) places point objects; <img src="images/icons/multipoint.svg" width="24" alt=""> **Multiple Points** keeps placing them until Esc. How points look is set with `PTYPE` (or `PDMODE` and `PDSIZE`).

<img src="images/icons/divide.svg" width="24" alt=""> `DIVIDE` (`DIV`) places points or blocks to split an object into equal parts; `MEASURE` (`ME`) places them at a fixed distance.

## Regions, boundaries and masks

| | Tool | Command | What you get |
|---|---|---|---|
| <img src="images/icons/boundary.svg" width="24" alt=""> | Boundary | `BOUNDARY` (`BO`) | A closed polyline or region around an area you click inside. |
| <img src="images/icons/region.svg" width="24" alt=""> | Region | `REGION` | A flat area object made from closed shapes; it can be combined with Union, Subtract and Intersect. |
| <img src="images/icons/wipeout.svg" width="24" alt=""> | Wipeout | `WIPEOUT` (`WO`) | A polygon that hides what is behind it, for clean space under text. |
| <img src="images/icons/revcloud.svg" width="24" alt=""> | Revision Cloud | `REVCLOUD` | A cloud outline to mark changes. Rectangular, polygonal or freehand; the arc length can be set. |
| | 2D Solid | `SOLID` | A filled triangle or quadrilateral. |

## Hatches

Hatching, solid fills and gradients have a page of their own: [Hatches and Fills](Hatches-and-Fills).

## Object properties while you draw

New objects get the current layer, colour, linetype, lineweight and transparency shown in the **Properties** panel of the Draw tab. Set them before you draw, or change the objects afterwards; see [Layers and Object Properties](Layers-and-Properties).
