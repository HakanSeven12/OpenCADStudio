---
summary: Creates a 3D solid pyramid with 3 to 32 sides, pointed or with a flat top.
related: CONE, BOX, POLYGON
ribbon: Model tab › Create panel (primitives drop-down)
---

## Usage
1. `Specify center point of base or [Edge/Sides]:` pick the base center, or type an option.
2. `Specify base radius or [Inscribed] <1.0000>:` pick a point or type the radius. Press Enter for the default.
3. `Specify height or [2Point/Axis endpoint/Top radius] <1.0000>:` pick a point, type a height, or type an option. Press Enter for the default.

## Options
| Option | What it does |
|---|---|
| **Edge** (`E`) | Sets the base from the two endpoints of one base edge. |
| **Sides** (`S`) | Sets the number of sides, from 3 to 32. The default is 4. |
| **Inscribed** (`I`) / **Circumscribed** (`C`) | Measures the base radius to the corners (inscribed) or to the middle of the edges (circumscribed). The default is circumscribed. |
| **2Point** (`2P`) | Sets the height as the distance between two points you pick. |
| **Axis endpoint** (`A`) | Sets the top point; this also sets the pyramid's direction. |
| **Top radius** (`T`) | Gives the pyramid a flat top of this radius. 0 makes a pointed pyramid. |

## Notes
- The values you use are remembered as the next defaults.
