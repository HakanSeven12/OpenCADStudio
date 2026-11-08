---
summary: Adds geometric constraints to the selected objects automatically, based on how the objects already meet and line up.
related: CONSTRAINTSETTINGS, GEOMCONSTRAINT, DELCONSTRAINT, CONSTRAINTBAR
ribbon: Parametric tab › Geometric panel; Block Editor tab › Geometric panel
---

## Usage
1. Select objects, or type **Settings** (`S`) to change the rules first.
2. Press Enter.

The command line reports how many constraints were applied to how many objects. You can also select the objects before you start the command.

## Options
| Option | What it does |
|---|---|
| **Settings** (`S`) | Opens the AutoConstrain settings: which constraint types are applied, in which order, and the distance and angle tolerances. |

## Notes
- The constraint types that can be inferred are coincident, collinear, parallel, perpendicular, tangent, concentric, horizontal, vertical and equal. Equal is off by default.
- By default, tangent objects must share a point and perpendicular objects must intersect to be constrained. The default distance tolerance is 0.05 and the angle tolerance is 1 degree.
- If no relation is found, nothing is added and the command line reports 0 constraints.
- All constraints added in one run are one undo step.
