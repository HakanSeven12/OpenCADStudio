---
summary: Measures the distance and angles between two points, and their X, Y and Z offsets.
related: MEASUREGEOM, AREA, ID, LIST
ribbon: Draw tab › Measure panel
---

## Usage
1. Specify the first point.
2. Specify the second point. While you move the cursor, a rubber band line and a live distance readout follow it.

The command line shows the result and the command ends:

- **Distance**: the straight distance between the points.
- **Angle in XY Plane**: the direction from the first point to the second in the XY plane.
- **Angle from XY Plane**: how steeply the line rises out of the XY plane.
- **Delta X**, **Delta Y**, **Delta Z**: the offsets from the first point to the second.

## Notes
- Values are measured in the current UCS.
- Lengths use the drawing's length units and precision ([`LUNITS`](LUNITS-System-Variable), [`LUPREC`](LUPREC-System-Variable)). Angles use [`AUPREC`](AUPREC-System-Variable) decimals and are counted from [`ANGBASE`](ANGBASE-System-Variable) in the direction set by [`ANGDIR`](ANGDIR-System-Variable).
- Use object snaps to measure between exact points on objects.
