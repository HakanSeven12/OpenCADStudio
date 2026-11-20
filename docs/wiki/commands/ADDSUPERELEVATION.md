---
summary: Adds superelevation to the curves of an alignment.
related: CREATEALIGNMENT, ADDDESIGNSPEEDS, CREATECORRIDOR
---

## Usage
1. Select an alignment with curves.
2. Press Enter, or first set options:
   - **Normal slope**: the lanes' normal crown slope (0.02 by default).
   - **Superelevation rate**: the full superelevation rate (0.06 by default).
   - **Runoff length**: the length from level crown to full superelevation (30 by default).

## Notes
- Each curve (an arc or a spiral-curve-spiral) gets critical stations: level crown, reverse crown and full superelevation a third of the runoff into the curve, then the same back out at its end.
- The outside of the curve rises to the rate and the inside falls to it; shoulders follow their lanes.
- The new superelevation replaces the alignment's own. The alignment is written again, so it must be one OpenCAD Studio can write again; offset alignments take none.
