---
summary: Creates a 3D solid cylinder with a circular or elliptical base.
related: CONE, BOX, SPHERE, EXTRUDE
ribbon: Model tab › Create panel
---

## Usage
1. Specify the center point of the base, or type an option.
2. Specify the base radius, or type `D` to give the diameter. Press Enter to use the value shown in brackets.
3. Specify the height, or type an option. Press Enter to use the value shown in brackets.

## Options
| Option | What it does |
|---|---|
| **3P** (`3P`) | Defines the base circle by three points on it. |
| **2P** (`2P`) | Defines the base circle by the two endpoints of its diameter. |
| **Ttr** (`TTR`) | Defines the base circle by two objects it is tangent to, then a radius. |
| **Elliptical** (`E`) | Makes an elliptical base. Pick both ends of the first axis, then the end of the second axis. Type `C` to start from the center instead. |
| **Diameter** (`D`) | At the radius prompt: enter the base diameter instead. |
| **2Point** (`2P`) | At the height prompt: the height is the distance between two points you pick. |
| **Axis endpoint** (`A`) | At the height prompt: pick the end of the cylinder's axis. This sets both the height and the direction of the cylinder. |

## Notes
- The base is placed on the current working plane (UCS).
- The command remembers the last radius and height and offers them as defaults next time.
