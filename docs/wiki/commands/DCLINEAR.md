---
summary: Adds a dimensional constraint for the horizontal or vertical distance between two points.
related: DCHORIZONTAL, DCVERTICAL, DCALIGNED, DIMCONSTRAINT
ribbon: Parametric tab › Dimensional panel
---

## Usage
1. Specify the first constraint point, or press Enter (or type `O`) to select an object.
2. Specify the second constraint point. With **Object**, select a line or arc instead; its two endpoints are used.
3. Specify the dimension line location. Where you place it decides whether the horizontal or the vertical distance is constrained.
4. Enter the value, or a name and value such as `width=50`. Press Enter to keep the measured value shown in brackets.

The geometry changes to match the value you enter.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Select one object and constrain the distance between its endpoints. |

## Notes
- The value can be a number, an expression, or the name of an existing parameter. Manage parameters with [`PARAMETERS`](PARAMETERS-Command).
- The new constraint uses the current constraint form, dynamic or annotational. Change it with [`DCFORM`](DCFORM-Command) or [`CCONSTRAINTFORM`](CCONSTRAINTFORM-System-Variable).
