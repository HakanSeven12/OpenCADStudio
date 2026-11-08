---
summary: Measures the area and perimeter of a region you pick point by point, or of selected objects, with running totals for added and subtracted areas.
related: DIST, MEASUREGEOM, MASSPROP, LIST
ribbon: Draw tab › Measure panel (Measure drop-down)
---

## Usage
1. Specify first corner point, or type an option. Press Enter to select objects instead.
2. Specify the next points. The area so far is shown next to the cursor.
3. Press Enter to calculate. You need at least three points.

The command line shows the area and perimeter.

To measure objects:
1. Press Enter or type **Objects** (`O`) at the first prompt.
2. Select objects and press Enter.

## Options
| Option | What it does |
|---|---|
| **Objects** (`O`) | Measures the selected objects instead of picked points. The areas of all measurable objects are added together. |
| **Add area** (`A`) | Turns on add mode. Each area you measure next is added to a running total. |
| **Subtract area** (`S`) | Turns on subtract mode. Each area you measure next is subtracted from the running total. |
| **Back** (`B`) | While selecting objects, measures what you have selected and returns to the point prompt. |

## Notes
- In add or subtract mode the command keeps running, so you can mix point regions and objects. Each result shows the current area and the new total. Measured regions stay highlighted until the command ends.
- Closed curves, polylines, splines, regions and solid surfaces can be measured. A perimeter is shown when the object has one.
- If none of the selected objects has a measurable area, a message says so.
- Points are measured in 3D, so a region that does not lie flat in the XY plane gives its true area, not its projection.
