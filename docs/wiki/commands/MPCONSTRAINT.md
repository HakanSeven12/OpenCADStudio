---
summary: Adds a geometric constraint that keeps a point at the midpoint of a line or arc.
related: CPCONSTRAINT, OCCONSTRAINT, GEOMCONSTRAINT, CONSTRAINTBAR
---

## Usage
1. Select exactly one object: the line or arc the point should stay at the middle of, then press Enter. If one object is already selected, this step is skipped.
2. Specify the point: pick an endpoint, center or vertex of another object.
3. The constraint is added, and the drawing is solved so the point sits at the midpoint.

## Notes
- If more than one object is selected when you start, the selection is cleared and an error asks you to select exactly one.
- If the pick does not land on an endpoint, center or vertex, an error appears and the command ends. Turn on Endpoint or Center object snap to help.
