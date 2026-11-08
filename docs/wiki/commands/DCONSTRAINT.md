---
summary: Constrains the length of a selected line or the size of a selected circle or arc to a value you enter.
related: DIMCONSTRAINT, DCALIGNED, DCRADIUS, ACONSTRAINT
---

## Usage
1. Select exactly one line, circle or arc (before or after starting the command).
2. For a line: Specify distance or [Xdistance/Ydistance]. For a circle or arc: Specify distance or [Diameter].
3. Type the value, or press Enter to keep the current value shown in brackets.

## Options
| Option | What it does |
|---|---|
| **Xdistance** (`X`) | Constrains the horizontal distance between the line's endpoints. |
| **Ydistance** (`Y`) | Constrains the vertical distance between the line's endpoints. |
| **Diameter** (`D`) | For a circle or arc: enter the diameter instead of the radius. |
| **Radius** (`R`) | Switches back from diameter to radius. |

## Notes
- You can type the name of an existing parameter instead of a number; the constraint is then driven by that parameter.
- If more than one object is selected, the command asks you to select exactly one.
