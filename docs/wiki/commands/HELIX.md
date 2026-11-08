---
summary: Creates a 3D helix, or a flat spiral when the height is zero.
related: SPLINE, SWEEP
ribbon: Draw tab › Draw panel
---

## Usage
1. The command line shows the current number of turns and twist direction.
2. Specify center point of base.
3. Specify base radius or [Diameter]: pick a point or type a value. Press Enter to keep the value shown.
4. Specify top radius or [Diameter]: pick a point or type a value. Press Enter to use the base radius.
5. Specify helix height or [Axis endpoint/Turns/turn Height/tWist]: pick a point or type a height. Press Enter to use the value shown.

## Options
| Option | What it does |
|---|---|
| **Diameter** (`D`) | Enter a diameter instead of a radius, for the base or the top. |
| **Axis endpoint** (`A`) | Pick the end of the helix axis. This sets both the height and the direction, so the helix can lean away from the Z axis. |
| **Turns** (`T`) | Sets the number of turns (up to 500). |
| **turn Height** (`H`) | Sets the distance between turns. The number of turns then follows from the height. |
| **tWist** (`W`) | Sets the winding direction: **Clockwise** (`CW`) or **Counterclockwise** (`CCW`). |

## Notes
- A height of zero makes a flat spiral.
- The base and top radius cannot both be zero.
- Without **Axis endpoint**, the helix rises along the Z axis of the current UCS.
- The base radius, height, turns and twist you use become the defaults for the next helix.
