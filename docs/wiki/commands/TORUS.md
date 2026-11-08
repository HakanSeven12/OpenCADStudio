---
summary: Creates a solid torus (a ring) from a center, a ring radius and a tube radius.
related: SPHERE, CYLINDER, BOX, CONE, UNION, SUBTRACT
ribbon: Model tab › Create panel (Box drop-down)
---
## Usage
1. At `Specify center point or [3P/2P/Ttr]:`, pick the center of the torus, or type an option.
2. At `Specify radius or [Diameter] <100.0000>:`, pick a point or type the radius of the ring (center to tube center).
3. At `Specify tube radius or [2Point/Diameter] <25.0000>:`, pick a point or type the radius of the tube.

Press Enter at a radius prompt to accept the value shown in `< >`. A preview of the torus follows the cursor.

## Options
| Option | What it does |
|---|---|
| **3P** (`3P`) | Sets the ring by three points on it. |
| **2P** (`2P`) | Sets the ring by the two end points of its diameter. |
| **Ttr** (`T`) | Select two tangent objects, then give the ring radius. The center is placed tangent to both. |
| **Diameter** (`D`) | At a radius prompt, asks for a diameter instead. |
| **2Point** (`2P`) | At the tube prompt, sets the tube by the two end points of its diameter. |

## Notes
- The torus lies on the current working plane (the current UCS).
- The last ring and tube radii are offered as defaults the next time you run the command in the same session.
- Both radii must be greater than zero.
