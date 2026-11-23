---
summary: Edits the points and elevations of a feature line.
related: CREATEFEATURELINE, CREATEPARCEL
---

## Usage
1. Select a feature line.
2. Specify points as x,y,elevation; press Enter to end.
3. Press Enter, or first set **Mode**:
   - **Insert** (the default): a point on one of the line's points sets its elevation; a point beside a straight segment becomes an elevation point there, at the given elevation.
   - **Delete**: removes the line's point nearest to each point given.

## Notes
- The ends of the line stay PIs, and a line keeps at least two points.
- Points are not inserted on arc segments.
- The site's other members keep their points; the edited line must not cross or touch them.
