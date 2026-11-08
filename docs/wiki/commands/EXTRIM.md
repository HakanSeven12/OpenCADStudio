---
summary: Trims every object on one side of a cutting boundary in a single step.
related: TRIM, EXTEND, CLIP
---

## Usage
1. Type `EXTRIM`.
2. At `Select cutting boundary:`, pick a line, arc, circle, polyline, ellipse or spline.
3. At `Click the side to trim away:`, click on the side you want to remove. A preview shows what is kept and what is removed.
4. Everything that crosses the boundary is cut, and the parts on the clicked side are removed.

## Notes
- Objects that lie completely on the clicked side are erased.
- A picked line is treated as an endless line, so it cuts across the whole drawing. Other boundaries are used as drawn.
- Lines, arcs and circles keep their type. Cut polylines, ellipses and splines become lightweight polylines.
- Press Enter or Esc to cancel before you click a side.
- If nothing is affected, the command ends without changes.
