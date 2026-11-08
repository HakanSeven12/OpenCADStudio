---
summary: Draws a rectangle as a closed polyline from two opposite corners, with optional chamfers, fillets, rotation, width, elevation and thickness.
related: RECTANG, POLYGON, PLINE
ribbon: Draw tab › Draw panel
---

## Usage
1. Specify the first corner, or type an option to change the rectangle settings first.
2. Specify the opposite corner, or type **Area**, **Dimensions** or **Rotation**.
3. The rectangle is created and the command ends.

## Options
| Option | What it does |
|---|---|
| **Chamfer** (`C`) | Asks for the first and second chamfer distances. The corners are chamfered and the fillet radius is set to 0. |
| **Elevation** (`E`) | Sets the height of the rectangle above the working plane. |
| **Fillet** (`F`) | Asks for a fillet radius. The corners are rounded and the chamfer distances are set to 0. |
| **Thickness** (`T`) | Sets the thickness the rectangle is drawn with. |
| **Width** (`W`) | Sets the polyline width. |
| **Area** (`A`) | Asks for the area, then whether to calculate from **Length** (`L`) or **Width** (`W`), then that value. The other side is worked out, counting the area that chamfers or fillets cut off. |
| **Dimensions** (`D`) | Asks for the length and width, then for a point that sets which way the rectangle goes from the first corner. |
| **Rotation** (`R`) | Asks for the rotation angle, or **Pick points** (`P`) to pick two points that set it. |

## Notes
- The settings are remembered for the next rectangle in the session. When a chamfer, fillet or elevation is set, the command line shows the current rectangle modes.
- Press Enter at a value prompt to keep the value shown in angle brackets.
- Ortho is turned off while the command runs.
- The Draw panel's rectangle list also has **Rectangle - Rotated** and **Rectangle - Center**.
