---
summary: Applies an equal-distance constraint, so the distance between two points stays the same as the distance between two other points.
related: ECONSTRAINT, GEOMCONSTRAINT, DCLINEAR
---

## Usage
1. Specify the first point of the first pair.
2. Specify the second point of the first pair.
3. Specify the first point of the second pair.
4. Specify the second point of the second pair.

The constraint is added and the command ends.

## Notes
- Each pick must land on a constraint point of an object, such as an endpoint, a center or a vertex. Turn on the Endpoint or Center object snap to hit them. If a pick misses, the command reports it and ends without adding anything.
