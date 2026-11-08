---
summary: Creates a 3D solid or surface by sweeping 2D profiles around an axis.
related: EXTRUDE, SWEEP, LOFT, PRESSPULL
ribbon: Model tab › Create panel
---

## Usage
1. Select the objects to revolve, then press Enter. Type `MO` first to change the creation mode.
2. Specify the axis start point, or type an option (`O`, `X`, `Y`, `Z`, `MO`).
3. If you picked a point, specify the axis end point.
4. Specify the angle of revolution, or type an option. Press Enter to use the angle shown.

## Options
| Option | What it does |
|---|---|
| **Mode** (`MO`) | Chooses **Solid** (`SO`) or **Surface** (`SU`). The choice is kept for the next time. |
| **Object** (`O`) | Uses a picked line, ray or construction line as the axis. |
| **X** / **Y** / **Z** (`X`, `Y`, `Z`) | Uses an axis of the current UCS through the origin. |
| **Start angle** (`S`) | Sets the angle at which the revolution starts. |
| **Reverse** (`R`) | Revolves in the opposite direction. |
| **Expression** (`E`) | Lets you type the angle as a math expression. |

## Notes
- The angle must not be zero and can be at most 360 degrees.
- Open profiles always make surfaces, even in Solid mode.
