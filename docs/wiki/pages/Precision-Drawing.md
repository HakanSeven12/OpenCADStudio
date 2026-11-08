# Precision Drawing

A CAD drawing is only as good as the points it is built from. This page covers the ways to put a point exactly where it belongs: typed coordinates, object snaps, tracking, ortho, polar tracking, the grid and the user coordinate system.

## Typing coordinates

Whenever a command asks for a point, you can type it instead of clicking.

| Form | Example | Meaning |
|---|---|---|
| Absolute | `100,50` | X = 100, Y = 50 from the origin. |
| Relative | `@20,0` | 20 to the right of the last point. |
| Polar | `@50<30` | 50 units from the last point, at 30°. |
| 3D | `10,20,5` · `@0,0,10` | With a Z value. |
| Cylindrical | `@50<30,10` | Polar in the XY plane, then 10 up. |
| Spherical | `@50<30<45` | 50 units at 30° in the plane and 45° up. |
| Forced absolute | `#100,50` | Absolute even while dynamic input expects relative values. |

Lengths and angles can be typed the way the drawing shows them, for example `5'-6 1/2"` in architectural units or a surveyor's bearing such as `N45d20'6"E`.

**Direct distance entry.** After the first point, point the cursor in a direction and type only a distance: `LINE`, click a point, move the cursor to the right, type `40` Enter, and you get a 40-unit line to the right. This works well together with Ortho or polar tracking.

## Dynamic input

<img src="images/icons/status/dyn.svg" width="20" alt=""> With **Dynamic Input** on (**F12**), the prompt and input fields appear next to the cursor. Type a value and press **Tab** to move to the next field (length, then angle); a typed value locks that field while you move the cursor for the rest.

## Object snaps

<img src="images/icons/status/osnap.svg" width="20" alt=""> Object snaps pull the cursor to exact points on existing objects. Turn running snaps on or off with **F3**; choose which are active with `OSNAP` (`OS`) or by right-clicking the status bar button.

| | Snap | Override | Snaps to |
|---|---|---|---|
| <img src="images/icons/osnap/endpoint.svg" width="20" alt=""> | Endpoint | `END` | The end of a line, arc or polyline segment. |
| <img src="images/icons/osnap/midpoint.svg" width="20" alt=""> | Midpoint | `MID` | The middle of a line, arc or segment. |
| <img src="images/icons/osnap/center.svg" width="20" alt=""> | Center | `CEN` | The centre of a circle, arc or ellipse. |
| <img src="images/icons/osnap/geometric_center.svg" width="20" alt=""> | Geometric Center | `GCEN` | The centroid of a closed polyline. |
| <img src="images/icons/osnap/node.svg" width="20" alt=""> | Node | `NOD` | A point object, or a dimension definition point. |
| <img src="images/icons/osnap/quadrant.svg" width="20" alt=""> | Quadrant | `QUA` | The 0°, 90°, 180° and 270° points of a circle or arc. |
| <img src="images/icons/osnap/intersection.svg" width="20" alt=""> | Intersection | `INT` | Where two objects cross. |
| <img src="images/icons/osnap/extension.svg" width="20" alt=""> | Extension | `EXT` | Along the imaginary extension of a line or arc. |
| <img src="images/icons/osnap/insertion.svg" width="20" alt=""> | Insertion | `INS` | The insertion point of a block, text or attribute. |
| <img src="images/icons/osnap/perpendicular.svg" width="20" alt=""> | Perpendicular | `PER` | The foot of a perpendicular to an object. |
| <img src="images/icons/osnap/tangent.svg" width="20" alt=""> | Tangent | `TAN` | The tangent point on a circle or arc. |
| <img src="images/icons/osnap/nearest.svg" width="20" alt=""> | Nearest | `NEA` | The closest point on an object. |
| <img src="images/icons/osnap/apparent.svg" width="20" alt=""> | Apparent Intersection | `APP` | Where two objects would cross in the current view. |
| <img src="images/icons/osnap/parallel.svg" width="20" alt=""> | Parallel | `PAR` | A point on a line parallel to another line. |
| <img src="images/icons/osnap/grid.svg" width="20" alt=""> | Grid | | The nearest grid point. |

**Single-pick overrides.** At any point prompt, type the three letters of a snap (`MID`, `CEN` and so on) to use that snap for the next pick only, whatever the running snaps are. `NON` turns snapping off for one pick. The right-click menu has the same choices under **Snap Overrides**, together with **From** and **Mid Between 2 Points**.

**Point filters and helpers.** `FROM` takes a base point and then an offset from it. `M2P` (or `MTP`) snaps to the midpoint between two points you pick.

### 3D object snaps

**F4** turns on the separate 3D snaps for solids and surfaces: vertex, midpoint on edge, centre of face, knot, perpendicular to face and nearest to face. Choose them on the **3D Object Snap** tab of `DSETTINGS`.

## Object snap tracking

<img src="images/icons/status/otrack.svg" width="20" alt=""> With tracking on (**F11**), rest the cursor on a snap point for a moment to acquire it; a small cross marks it. Move away and dotted alignment paths appear horizontally, vertically or at polar angles from it. Click on a path, or where two paths cross, or type a distance along the path.

## Ortho and polar tracking

<img src="images/icons/status/ortho.svg" width="20" alt=""> **Ortho** (**F8**) keeps the cursor horizontal or vertical relative to the last point.

<img src="images/icons/status/polar.svg" width="20" alt=""> **Polar tracking** (**F10**) shows alignment paths at set angle steps (every 15°, 30°, 45° and so on) and snaps the cursor to them. Set the step with `POLARANG` or on the **Polar Tracking** tab of `DSETTINGS`. Ortho and polar tracking turn each other off.

## Grid and snap

**F7** (`GRID`) shows a grid; **F9** (`SNAP`) turns on snap mode, which makes the cursor jump in fixed steps. Set the spacing on the **Snap and Grid** tab of `DSETTINGS`, or with `GRIDUNIT`. For isometric drawings, set an isometric snap style and press **F5** to switch between the left, top and right isoplanes.

## The user coordinate system (UCS)

<img src="images/icons/ucs_icon.svg" width="20" alt=""> The world coordinate system (WCS) is fixed. A user coordinate system (UCS) is one you place yourself: move its origin, turn it, or lay it on a face, and typed coordinates, ortho, the grid and plan views follow it.

`UCS` options: a new **Origin**; **3Point** (origin, X direction, Y direction); align to an **OBject** or a solid **Face**; align to the **View**; rotate about **X**, **Y** or **Z**; back to **World**; **Save** and restore by name; **Delete** a saved one.

You can also drag the UCS icon's grips: the origin grip moves it, the axis grips turn it.

<img src="images/icons/status/ducs.svg" width="20" alt=""> **Dynamic UCS** (**F6**) lines up a temporary UCS with the face of a solid under the cursor while you draw, so you can sketch straight onto a face.

`UCSFOLLOW` makes the view turn to the plan of each new UCS.
