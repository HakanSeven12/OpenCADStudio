---
summary: Adds a dimensional constraint for the angle between two lines, the angle of an arc, or the angle between three points.
related: ACONSTRAINT, DIMCONSTRAINT, PARAMETERS
ribbon: Parametric tab › Dimensional panel
---

## Usage
1. Select the first line or arc, or press Enter (or type `3P`) to use three points.
2. If you picked a line, select the second line. An arc measures the angle it sweeps.
3. Specify the dimension line location.
4. Enter the value, or a name and value such as `ang1=45`. Press Enter to keep the measured value shown in brackets.

## Options
| Option | What it does |
|---|---|
| **3Point** (`3P`) | Specify the angle vertex, then the first and second angle constraint points. |

## Notes
- The geometry changes to match the angle you enter.
- The value can be a number, an expression, or the name of an existing parameter.
- The new constraint uses the current constraint form. Change it with [`DCFORM`](DCFORM-Command) or [`CCONSTRAINTFORM`](CCONSTRAINTFORM-System-Variable).
