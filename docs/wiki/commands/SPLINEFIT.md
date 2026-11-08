---
summary: Replaces a selected polyline with a smooth spline that passes through its vertices.
related: SPLINE, SPLINEDIT, PEDIT
---

## Usage
1. Select a polyline.
2. Run `SPLINEFIT`. The polyline is replaced by a spline through its vertices, and the command line reports the number of points.

## Notes
- Select the polyline before you start the command; there is no selection prompt. If several objects are selected, only the first polyline is used.
- The polyline needs at least three vertices.
- Only the vertex positions are used. Arc segments of the polyline are not kept.
- Polylines on locked layers are skipped.
- The change can be undone.
