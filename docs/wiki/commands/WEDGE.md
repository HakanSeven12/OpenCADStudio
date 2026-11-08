---
summary: Creates a solid wedge from a rectangular base and a height.
related: BOX, CYLINDER, CONE, PYRAMID, SPHERE, TORUS
ribbon: Model tab › Create panel (Box drop-down › Wedge)
---

## Usage
1. At `WEDGE  Specify first corner or [Center]:`, pick the first corner of the base.
2. At `WEDGE  Specify other corner or [Cube/Length]:`, pick the opposite corner.
3. At `WEDGE  Specify height or [2Point] <...>:`, specify the height, or press Enter to use the value shown.

## Options
| Option | What it does |
|---|---|
| **Center** (`C`) | Starts from the center of the base: `Specify center point:`, then the other corner. |
| **Cube** (`C`) | Asks `Specify cube length:` and uses it for length, width and height. |
| **Length** (`L`) | Asks `Specify length:` and `Specify width:` instead of a second corner. |
| **2Point** (`2P`) | Sets the height as the distance between two points you pick. |

## Notes
- The sloped face falls along the base's length, from full height on the first corner's side to zero at the far end.
- The default height is the height of the last wedge you made in the session.
- A preview of the wedge follows the cursor.
