---
summary: Creates a curb return alignment that rounds the corner between two roads.
related: CREATEINTERSECTION, CREATEALIGNMENT, CREATEOFFSETALIGNMENT
---

## Usage
1. Select the first road alignment.
2. Select the second road alignment.
3. Specify a point in the corner to round.
4. Press Enter, or first set options:
   - **Radius**: the curb return's radius (10 by default).
   - **Offset**: how far the curb lines lie from both roads (6 by default).
   - **Overlap**: the straight length kept along each road before and after the arc (5 by default).

## Notes
- The curb return is an alignment of a line, an arc and a line between the curb lines of the two roads; it is named Curb Return and a number.
- It runs along the stationing of the road whose arm behind the junction bounds the corner. A corner ahead of both roads cannot be rounded.
- The curb lines must meet on straight segments of both roads, with room for the arc and the overlaps.
