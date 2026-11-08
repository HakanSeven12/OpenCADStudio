---
summary: Makes two circles, arcs, ellipses or polyline arc segments share the same center.
related: GEOMCONSTRAINT, GCTANGENT, GCEQUAL, GCSHOW, DELCONSTRAINT
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select first object: pick a circle, arc, ellipse or polyline arc segment.
2. Select second object: pick another curved object.

The constraint is applied and the two centers are made to coincide.

## Notes
- You can select the two objects first and then start the command; the constraint is applied at once.
- If only one of the preselected objects is valid, the command keeps it and asks for the second object.
- Both objects must lie on the same plane.
- Other constrained objects connected to these may move too; the command line lists any objects it moved that you did not pick.
