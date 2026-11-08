---
summary: Moves selected objects by a distance and direction you give with two points or a displacement.
related: COPY, 3DMOVE, ROTATE, STRETCH
ribbon: Draw tab › Modify panel
---

## Usage
1. Select objects, then press Enter. If objects are already selected, this step is skipped.
2. Specify the base point, or type `D` to give a displacement.
3. Specify the destination. A preview shows the objects following the cursor.

## Options
| Option | What it does |
|---|---|
| **Displacement** (`D`) | At the base point prompt: the next point or typed coordinate (for example `10,5`) is used as the move distance in X, Y and Z. |
| **Use first point as displacement** (Enter) | At the destination prompt: press Enter to move the objects by the base point's coordinates, measured from the origin. |

## Notes
- Objects on locked layers are left out of the selection and do not move.
- MOVE works in 3D: the move is the full difference between the two points, including Z.
