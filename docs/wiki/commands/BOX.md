---
summary: Creates a 3D solid box or cube.
related: WEDGE, CYLINDER, EXTRUDE
ribbon: Model tab › Create panel
---

## Usage
1. Specify the first corner of the base, or type **Center** to start from the center point.
2. Specify the other corner of the base, or type an option.
3. Specify the height, or type **2Point**. The box is created.

## Options
| Option | What it does |
|---|---|
| **Center** (`C`) | The first point you give is the center of the base. The other corner then sets half the length and width in each direction. |
| **Cube** (`C`) | Asks for one length and creates a cube with equal length, width and height. |
| **Length** (`L`) | Asks for the length, then the width, then the height. The direction you point in for the length sets the rotation of the box. |
| **2Point** (`2P`) | Asks for two points; the distance between them is the height. |

## Notes
- The box is built in the current UCS, with its base on the XY plane of that UCS.
- A negative height builds the box below the base.
- Zero length, width or height is not accepted; the prompt waits for another value.
