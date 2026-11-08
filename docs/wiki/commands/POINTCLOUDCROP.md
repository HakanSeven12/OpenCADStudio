---
summary: Crops a point cloud to a rectangle, polygon or circle drawn on the current view, keeping the points inside or outside it.
related: POINTCLOUDUNCROP, POINTCLOUDMANAGER, POINTCLOUDATTACH
---

## Usage
1. `Select point cloud:` pick a point cloud.
2. `Specify first corner point or [Polygon/Circular]:` pick the first corner of a rectangle, or type an option.
3. `Specify other corner point:` pick the opposite corner.
4. `Keep points inside or outside? [Inside/Outside] <Inside>:` type `I` or `O`, or press Enter for the default.

For a polygon, pick the first point, then the next points, and press Enter to close it (at least three points). For a circle, pick the center, then pick a point or type the radius.

## Options
| Option | What it does |
|---|---|
| **Polygon** (`P`) | Draws a polygonal crop boundary. |
| **Circular** (`C`) | Draws a circular crop boundary. |
| **Undo** (`U`) | While drawing a polygon, removes the last point. |
| **Invert** (`I`) | Flips all existing crops, so what was hidden is shown. Offered only when the cloud has crops. |
| **Remove last** (`R`) | Removes the newest crop. Offered only when the cloud has crops. |
| **ON** / **OFF** | Shows or hides the effect of the crops without removing them. Offered only when the cloud has crops. |
| **Inside** (`I`) / **Outside** (`O`) | Keeps the points inside or outside the new boundary. The last choice is the next default. |

## Notes
- The boundary is drawn on the view plane and runs along the view direction, so a crop made in a side view cuts through the cloud from that side.
- Crops add up. Each new crop is applied on top of the earlier ones, and crops are shown again when you add one.
- With only point clouds selected, a toolbar on the right edge of the drawing area offers Rectangular, Polygonal and Circular Crop, Show Cropping, Invert Cropping and Remove All Cropping.
- To remove all crops at once, use [`POINTCLOUDUNCROP`](POINTCLOUDUNCROP-Command).
