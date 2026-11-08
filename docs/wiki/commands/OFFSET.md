---
summary: Creates a parallel copy of a line, arc, circle, ellipse, polyline, spline, ray or construction line at a set distance or through a point.
related: COPY, JOIN, FILLET
ribbon: Draw tab › Modify panel
---

## Usage
1. The current settings are shown (Erase source, Layer and `OFFSETGAPTYPE`).
2. Specify offset distance or [Through/Erase/Layer]. Type a distance, press Enter to keep the last one, or pick two points to measure it.
3. Select object to offset or [Exit/Undo] <Exit>.
4. Specify point on side to offset. The copy is made on that side.
5. Select the next object, or press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Through** (`T`) | Instead of a fixed distance, you specify a through point for each copy. |
| **Erase** (`E`) | Erase source object after offsetting? [Yes/No]. |
| **Layer** (`L`) | Enter layer option for offset objects [Current/Source]. **Current** puts copies on the current layer, **Source** on the layer of the original. |
| **Multiple** (`M`) | Keeps offsetting the new copy by the same distance, one click after another. Press Enter to go to the next object. |
| **Undo** (`U`) | Removes the last copy. |
| **Exit** (`E`) | Ends the command. |

## Notes
- If objects are already selected when you start, the command goes straight to the side step with a preview that follows the cursor. Click a through point, or type a distance; a negative distance places the copy on the other side.
- The Erase, Layer and Through choices are kept for the next OFFSET in the session.
- `OFFSETGAPTYPE` sets how outward polyline offsets close corner gaps: 0 extends the segments, 1 fills with an arc, 2 with a chamfer line.
