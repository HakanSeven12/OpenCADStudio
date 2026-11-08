---
summary: Moves, rotates and optionally scales objects by matching one, two or three pairs of source and destination points.
related: 3DALIGN, MOVE, ROTATE, ALIGNLEFT
ribbon: Draw tab › Modify panel (slide-out)
---

## Usage
1. Select the objects, or start the command, select them and press Enter.
2. Specify the 1st source point, then the 1st destination point.
3. Specify the 2nd source point, or press Enter to only move the objects by the first pair.
4. Specify the 2nd destination point.
5. Specify the 3rd source point and the 3rd destination point, or press Enter to continue with two pairs.
6. With two pairs, answer whether to scale the objects based on the alignment points. Press Enter for No.

## Options
| Option | What it does |
|---|---|
| **Yes** (`Y`) | Scales the objects so the source points match the distance between the destination points. |
| **No** (`N`) | Moves and rotates the objects without scaling. This is the default. |

## Notes
- One pair moves the objects. Two pairs also rotate them. Three pairs also set the plane, so objects can be turned in 3D; with three pairs there is no scale question.
- A second point that coincides with the first, or a third point on the line of the first two, is ignored and the prompt is repeated.
- Lines from each source point to its destination point are shown while you pick.
