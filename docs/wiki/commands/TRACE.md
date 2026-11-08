---
summary: Draws a solid-filled band of constant width through the points you pick, stored as a wide polyline.
related: PLINE, SOLID, PEDIT
---
## Usage
1. At `Specify trace width <1.0000>:`, type the width and press Enter, or press Enter to keep the value shown.
2. At `Specify start point:`, pick the first point.
3. At `Specify next point [Undo]:`, pick the next point. Repeat for more segments.
4. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Undo** (`U`) | Removes the last point you picked. |

## Notes
- The result is one polyline with a constant width, so it can be edited like any other polyline.
- The width must be greater than zero. If you pick a point at the width prompt, the shown width is used.
- All points lie on the working plane at the height of the first point.
- Esc cancels the command and draws nothing. Enter with fewer than two points also draws nothing.
