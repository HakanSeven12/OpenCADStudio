---
summary: Draws a revision cloud along a rectangle, a polygon or a freehand path, turns an existing object into a cloud, or reshapes part of a cloud.
related: WIPEOUT, REVERSE, PEDIT
ribbon: Annotate tab › Markup panel
---

## Usage
1. The command line shows the current minimum and maximum arc length, the style and the type.
2. Specify the first point, or type an option. Press Enter to use **Object**.
3. Finish the shape for the current type:
   - **Rectangular**: specify the opposite corner.
   - **Polygonal**: specify the next points. Press Enter to close the cloud (at least three points).
   - **Freehand**: move the crosshairs along the cloud path. Click near the start point to close the cloud, or press Enter to leave it open.
4. For an open freehand cloud, an object or a modified cloud, answer `Reverse direction [Yes/No] <No>:`.

## Options
| Option | What it does |
|---|---|
| **Arc length** (`A`) | Sets the approximate arc length. Type a length or pick two points. The value is kept in the drawing. |
| **Object** (`O`) | Turns a picked line, arc, circle, ellipse, spline or polyline into a cloud. The cloud replaces the object and keeps its properties. |
| **Rectangular** (`R`) | Draws the cloud as a rectangle from two corners. |
| **Polygonal** (`P`) | Draws the cloud through points you pick. |
| **Freehand** (`F`) | Draws the cloud along the path of the crosshairs. |
| **Style** (`S`) | Chooses **Normal** (`N`) or **Calligraphy** (`C`) arcs. The style is kept for the session. |
| **Modify** (`M`) | Redraws part of an existing closed cloud. See below. |
| **Undo** (`U`) | Removes the last point (Polygonal type and Modify). |

## Modify
1. Select the closed polyline cloud to change.
2. Specify the next points of the new part. Type `F` (**First point**) to pick a different start point on the cloud.
3. End the new part with a point close to the cloud.
4. Pick the side of the old cloud to erase.
5. Answer the Reverse direction question.

## Notes
- The type you pick last (Rectangular, Polygonal or Freehand) is used the next time.
- The settings `REVCLOUDCREATEMODE`, `REVCLOUDARCVARIANCE`, `REVCLOUDGRIPS` and `REVCLOUDAPPROXARCLEN` can also be typed at the command line.
- Clouds are drawn on the current working plane.
