---
summary: Opens the Geometry Cleanup palette, which finds line, arc, elliptical arc and polyline ends that almost meet other geometry and repairs them one by one.
related: GEOMETRYCLEANUPCLOSE, OVERKILL, GAPCHECK
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Select objects, or run the command and click **Select Objects** in the palette.
2. The palette lists the errors it found, grouped by type, with the location of each.
3. Click a row to zoom to the error and select its objects. This starts review mode: the error's objects are drawn red and the rest of the drawing is faded.
4. Click a repair button at the end of the row. The next error is picked and zoomed to. Put the pointer on a repair button first to see its result in the drawing.
5. Or click **Keep** to keep the error as designed: the drawing does not change, and later scans do not list the error.

## Error types
| Type | What it is | Repairs |
|---|---|---|
| **Gap** | An end almost meets the end of another object. | **Stretch** moves the end onto the other end (not for an elliptical arc). **Move** moves the whole object. **Extend** takes two straight ends to the corner where their lines cross. |
| **Overshoot** | An end runs a little past an object it crosses. | **Trim** cuts the end back to the crossing. |
| **Undershoot** | An end stops a little short of an object. | **Extend** runs the end on along its line, arc or ellipse. |

## Notes
- Lines, arcs, elliptical arcs, lightweight polylines and 2D polylines on the world XY plane in model space are scanned. Whole ellipses, curve- or spline-fit 2D polylines and splines are not. Objects on locked or frozen layers are left out.
- The tolerance is proposed from the drawing: 1% of the mean length of its lines and polyline segments.
- While the palette is open, it scans again after each change of the drawing, so the list stays current. **Rescan** does it by hand.
- A scan of 10,000 or more objects that it can read runs while you work. The palette shows **Scanning N objects…** in place of the list, and a **Cancel** button. **Cancel**, or closing the palette, stops the scan and clears the list. The drawing does not change.
- Each repair is one undo step.
- Each error in view has a red X in the drawing. Put the pointer on it for a menu of its repairs and **Keep as Designed**; put the pointer on a repair to see its result.
- The kept errors are kept in the user settings for each drawing. **Kept as Designed** at the end of the list shows them: **Restore** lists one again, **Restore All** lists all of them again. A drawing that has no FINGERPRINTGUID gets one at the first Keep, and is saved with it.
- In review mode a toolbar is over the drawing. Its **Auto Zoom** button sets [`GEOMETRYCLEANUPZOOMPAN`](GEOMETRYCLEANUPZOOMPAN-System-Variable), and **Close** ends review mode, as [`GEOMETRYERRORREVIEWCLOSE`](GEOMETRYERRORREVIEWCLOSE-Command) does.
- [`GEOMETRYCLEANUPSTATE`](GEOMETRYCLEANUPSTATE-System-Variable) tells whether the palette is open.
