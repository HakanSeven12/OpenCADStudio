---
summary: Joins the end of an open spline to the end of another curve with curvature continuity.
related: GCTANGENT, GCCOINCIDENT, GEOMCONSTRAINT, SPLINE, GCSHOW
ribbon: Parametric tab › Geometric panel
---

## Usage
1. Select first spline curve endpoint: click near the end of an open spline.
2. Select second curve endpoint: click near the end of a line, arc, polyline segment or another open spline.

The spline is adjusted so its end meets the other curve's end and blends into it smoothly.

## Notes
- The first object must be an open spline. Closed splines are refused.
- Both curves must be different objects and lie on the same plane.
- You can select the spline and then the target curve first, and start the command; the constraint is applied at once.
- After a rejected pick, the command shows the reason and asks again.
