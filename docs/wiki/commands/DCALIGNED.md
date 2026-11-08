---
summary: Adds a dimensional constraint for the true distance between two points, or between a point and a line, or between two lines.
related: DCLINEAR, DCHORIZONTAL, DCVERTICAL, DIMCONSTRAINT
ribbon: Parametric tab › Dimensional panel
---

## Usage
1. Specify the first constraint point, or type an option. Press Enter for **Object**.
2. Specify the second constraint point.
3. Specify the dimension line location.
4. Enter the value, or a name and value such as `width=50`. Press Enter to keep the measured value shown in brackets.

The geometry changes to match the value you enter.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Select one line or arc and constrain the distance between its endpoints. |
| **Point & line** (`P`) | Specify a point and select a line (type `L` to select the line first). The distance is measured perpendicular to the line. |
| **2Lines** (`2L`) | Select two lines. The second line is made parallel to the first, and the distance between them is constrained. |

## Notes
- The value can be a number, an expression, or the name of an existing parameter. Manage parameters with [`PARAMETERS`](PARAMETERS-Command).
- The new constraint uses the current constraint form, dynamic or annotational. Change it with [`DCFORM`](DCFORM-Command) or [`CCONSTRAINTFORM`](CCONSTRAINTFORM-System-Variable).
