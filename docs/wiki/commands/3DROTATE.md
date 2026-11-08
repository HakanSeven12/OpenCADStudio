---
summary: Rotates objects about an axis through a base point, with the axis set by X, Y, Z or a second point.
related: ROTATE, 3DMIRROR, 3DALIGN, 3DMOVE
---

## Usage
1. Select the objects, or start the command and select them, then press Enter.
2. Specify the base point.
3. Specify the rotation axis: type `X`, `Y` or `Z`, pick a second point on the axis, or press Enter for the Z axis.
4. Specify the rotation angle, or type an option. Press Enter to use the angle shown in brackets.

## Options
| Option | What it does |
|---|---|
| **X** / **Y** / **Z** (`X`, `Y`, `Z`) | At the axis prompt: uses that axis of the current UCS through the base point. |
| **Copy** (`C`) | Rotates a copy and leaves the original in place. |
| **Reference** (`R`) | Asks for a reference angle (two points or a typed value), then the new angle. The objects turn by the difference. |

## Command-line form
`3DROTATE X 90` (or `Y`, `Z`, then an angle in degrees) rotates one selected 3D solid about that axis through the solid's center, without any prompts. If the axis letter is left out, Z is used.

## Notes
- The angle is measured in the plane square to the chosen axis.
- The last angle you used becomes the default for the next rotation.
- A preview of the rotated objects and an angle arc follow the cursor.
