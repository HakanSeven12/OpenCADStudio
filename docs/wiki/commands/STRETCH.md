---
summary: Moves the points of objects that fall inside one or more crossing windows, so the objects stretch while points outside stay put.
related: MOVE, SCALE, LENGTHEN
ribbon: Draw tab › Modify panel
---

## Usage
1. Specify the first corner of the crossing window.
2. Specify the opposite corner. Everything the window crosses is picked; only its points inside the window will move.
3. Specify the first corner of another crossing window to add more, or press Enter to continue.
4. Specify the base point.
5. Specify the new point. The objects stretch by the distance from the base point to the new point.

## Notes
- While you pick windows, hold Shift and:
  - click a highlighted grip to keep just that point from moving;
  - click an object to take it out of the stretch;
  - draw a window to keep the points inside it from moving.
- If you select objects first and press Enter at the first prompt without drawing a window, a window around the whole selection is used.
- How each object type stretches:
  - Lines, polylines (2D and 3D), splines: only the points inside the window move. Arc segments keep their height as their chord changes.
  - Arcs: an end inside the window moves; the arc keeps its height.
  - Circles, ellipses, points, block references, multiline text: the whole object moves when its center or insertion point is inside.
  - Single-line text: Aligned and Fit text stretch by their two baseline points; other text moves when its insertion point is inside.
  - Dimensions: the definition points inside the window move.
  - Layout viewports also stretch.
  - Other objects are not changed.
- Ortho and Polar do not affect the window corners.
- The preview shows the result while you move the cursor to the new point.
