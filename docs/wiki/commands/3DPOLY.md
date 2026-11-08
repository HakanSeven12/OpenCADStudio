---
summary: Draws a 3D polyline of straight segments whose vertices can each have their own Z value.
related: PLINE, PEDIT, 3DFACE
---

## Usage
1. Specify the start point of the polyline.
2. Specify the next point. Repeat for more segments.
3. Press Enter to finish, or type `C` to close the polyline.

## Options
| Option | What it does |
|---|---|
| **Close** (`C`) | Closes the polyline back to the start point and ends the command. Available after three points. |
| **Undo** (`U`) | Removes the last point. |

## Notes
- A 3D polyline has straight segments only; there are no arc segments.
- Pressing Enter with fewer than two points cancels without creating anything.
