---
summary: Opens the Geometry Cleanup palette, which finds line, arc, elliptical arc and polyline ends that almost meet other geometry and repairs them one by one.
related: GEOMETRYCLEANUPCLOSE, OVERKILL, GAPCHECK
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Select objects, or run the command and click **Select Objects** in the palette.
2. The palette lists the errors it found, grouped by type, with the location of each.
3. Click a row to zoom to the error and select its objects.
4. Click a repair button at the end of the row. The next error is picked and zoomed to.

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
- Each repair is one undo step.
