---
summary: Creates a 3D solid cone, or a truncated cone when you give a top radius.
related: CYLINDER, BOX, PYRAMID, SPHERE, EXTRUDE
ribbon: Model tab › Create panel
---

## Usage
1. Specify the center point of the base, or type an option.
2. Specify the base radius, or type `D` and give the diameter.
3. Specify the height, or type an option.

Each value prompt shows the last value used in angle brackets; press Enter to accept it.

## Options
| Option | What it does |
|---|---|
| **3P** (`3P`) | Defines the base circle by three points on it. |
| **2P** (`2P`) | Defines the base circle by the two endpoints of a diameter. |
| **Ttr** (`TTR`) | Pick two objects the base circle is tangent to, then give the base radius. |
| **Elliptical** (`E`) | Makes an elliptical base. Pick both endpoints of the first axis, then the distance to the other axis. |
| **Center** (`C`) | In Elliptical: pick the base center, the distance to the first axis, then the endpoint of the second axis. |
| **Diameter** (`D`) | Asks for the base diameter instead of the radius. |
| **2Point** (`2P`) | Gives the height as the distance between two points you pick. |
| **Axis endpoint** (`A`) | Pick the tip of the axis. This sets both the height and the direction of the cone. |
| **Top radius** (`T`) | Asks for a top radius, which makes a truncated cone. Then asks for the height. |

## Notes
- The default top radius is 0, which makes a pointed cone.
- The result is a 3D solid.
