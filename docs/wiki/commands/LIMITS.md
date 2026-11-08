---
summary: Sets the drawing limits rectangle of the current space, and turns limits checking on or off.
related: GRID, ZOOM, MVIEW
---

## Usage
1. At `Specify first corner or [On / Off] <x,y>:`, pick or type the lower-left corner, or press Enter to keep the current one.
2. At `Specify opposite corner <x,y>:`, pick or type the upper-right corner, or press Enter to keep the current one.

## Options
| Option | What it does |
|---|---|
| **On** (`ON`) | Turns limits checking on. Points outside the limits are refused with `Outside limits.` |
| **Off** (`OFF`) | Turns limits checking off. |

## Notes
- The limits belong to the space you work in: model space, or the current paper space layout.
- The grid is drawn only inside the limits, unless the grid is set to show beyond them.
- The corners must enclose an area. Two corners on one line are refused.
- `LIMITS` itself can always take points outside the old limits.
- You can also type `LIMITS SET x1 y1 x2 y2`, `LIMITS ON` or `LIMITS OFF`.
- The change can be undone.
