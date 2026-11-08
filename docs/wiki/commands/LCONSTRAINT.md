---
summary: Adds a collinear constraint, which makes a second object lie on the same line as the first.
related: GCCOLLINEAR, GEOMCONSTRAINT, PCONSTRAINT, DELCONSTRAINT, AUTOCONSTRAIN
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select two objects, the reference first and then the one that moves. If exactly two objects are already selected, the constraint is added at once.
2. Otherwise, at the `Select objects:` prompt, pick the two objects and press Enter.
3. The second object moves to lie on the same line as the first, and the constraint is added.

## Notes
- The selection must hold exactly two objects. Otherwise the command reports an error and clears the selection.
- The same tool is on the Geometric panel of the Block Editor tab.
- `GCCOLLINEAR` runs the same command.
