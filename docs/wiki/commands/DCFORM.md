---
summary: Switches dimensional constraints between the dynamic and annotational forms, or sets the form for new ones.
related: DIMCONSTRAINT, DCCONVERT
---

## Usage
1. Optionally select the dimensions of dimensional constraints first.
2. Enter constraint form [Annotational/Dynamic]. Press Enter to keep the current form.

With dimensional constraints selected, they are converted to the chosen form. With nothing selected, the chosen form is used for new dimensional constraints, and the [`DIMCONSTRAINT`](DIMCONSTRAINT-Command) option prompt follows.

## Options
| Option | What it does |
|---|---|
| **Annotational** (`A`) | Uses the annotational form. |
| **Dynamic** (`D`) | Uses the dynamic form. |

## Notes
- The form for new constraints is also stored in [`CCONSTRAINTFORM`](CCONSTRAINTFORM-System-Variable).
- Constraints on locked layers cannot be changed.
- If something other than a constraint dimension is selected, the command asks you to select the constraint's dimension.
