---
summary: Adds a mirrored copy of one selected 3D solid across the X, Y or Z plane through its center.
related: MIRROR, 3DROTATE, 3DALIGN, 3DMOVE
---

## Usage
1. Select exactly one 3D solid, or start the command, select it and press Enter.
2. Choose the mirror plane: type `X`, `Y` or `Z`.
3. A mirrored copy is added. The original stays, and the copy is selected.

## Options
| Option | What it does |
|---|---|
| **X** (`X`) | Mirrors across the plane square to the X axis through the solid's center. |
| **Y** (`Y`) | Mirrors across the plane square to the Y axis through the solid's center. |
| **Z** (`Z`) | Mirrors across the plane square to the Z axis through the solid's center. |

## Notes
- You can also type the plane directly, for example `3DMIRROR X`.
- The command works on 3D solids only. Objects on locked layers are ignored.
- To mirror other objects across a line you pick, use [`MIRROR`](MIRROR-Command).
- The change can be undone in one step.
