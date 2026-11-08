---
summary: Turns closed planar shapes, or connected planar edges that form closed loops, into region objects.
related: BOUNDARY, UNION, SUBTRACT, EXTRUDE, PRESSPULL
ribbon: Draw tab › Draw panel
---

## Usage
1. Select objects and press Enter. If objects are already selected, the command runs at once.
2. A region is created for each closed shape and for each closed loop formed by the selected open objects.

## Notes
- Closed shapes are closed planar objects, for example circles and closed polylines.
- Open objects such as lines and arcs must lie in one plane and meet end to end to form a closed loop.
- Objects that are already regions are skipped.
- New regions go on the current layer.
- The command line reports how many regions were created.
- [`DELOBJ`](DELOBJ-System-Variable) decides whether the source objects are deleted. When it is 0, they are kept.
- The command can be undone in one step.
