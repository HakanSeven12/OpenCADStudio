---
summary: Clips a layout viewport to a closed object or to a polygon you draw, or removes its clip.
related: CLIP, MVIEW, VPLAYER, XCLIP
---

## Usage
1. At `Select viewport to clip:`, pick a viewport in a paper layout.
2. At `Select clipping object or [Polygonal] <Polygonal>:`, pick a circle, a full ellipse or a closed polyline in the same layout, or press Enter (or type `P`) to draw a polygon.
3. For a polygon, specify the start point, then the next points. Type `C` or press Enter once you have three or more points to close it.

## Options
| Option | What it does |
|---|---|
| **Polygonal** (`P`) | Draws the clip boundary point by point. |
| **Delete** (`D`) | At the clipping object prompt, removes the clip of a viewport that is already clipped. |
| **Arc** (`A`) | While drawing the polygon, switches to arc segments. Arc mode offers **Angle** (`A`), **CEnter** (`CE`), **CLose** (`CL`), **Direction** (`D`), **Line** (`L`), **Radius** (`R`), **Second pt** (`S`) and **Undo** (`U`); a point gives the arc end point. |
| **Close** (`C`) | Closes the polygon (three points or more) and applies the clip. |
| **Length** (`L`) | Asks `Specify length of line:` and adds a segment of that length in the direction of the last segment. |
| **Undo** (`U`) | Removes the last polygon point. |

## Notes
- The viewport shrinks to the extents of the new boundary, and its view moves with it, so the model stays where it was on the sheet.
- An earlier clip boundary is erased when you clip again.
- A picked object that is not closed is refused with `Object is not a closed curve.`
- The layout's own paper-space viewport cannot be clipped.
- The whole change is one undo step.
