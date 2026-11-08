---
summary: Reverses the direction of lines, polylines, splines and helices.
related: PEDIT, SPLINEDIT, JOIN
ribbon: Draw tab › Modify panel (expanded)
---

## Usage
1. Select the objects, or start the command and select them at the prompt.
2. Press Enter. Each supported object is reversed in place.

## Notes
- Lines swap their start and end points.
- Polylines keep their arcs and widths; the start and end widths of each segment swap with the direction.
- Splines reverse their control points and fit points. Helices reverse their turn.
- Other object types are ignored. Objects on locked layers are skipped.
- Reversing changes how linetype patterns run and where commands that use the start point begin.
