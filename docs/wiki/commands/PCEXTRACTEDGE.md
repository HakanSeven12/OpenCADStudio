---
summary: Draws a line where two planar surfaces of a point cloud meet.
related: PCEXTRACTCORNER, PCEXTRACTCENTERLINE, PCEXTRACTSECTION, POINTCLOUDATTACH
---

## Usage
1. Specify the first plane: click on a flat area of the point cloud.
2. Specify the second plane: click on another flat area.

A line is drawn along the intersection of the two planes. It runs over the length covered by the cloud points near the edge.

## Notes
- A plane is found from the cloud points under the cursor: a plane is fitted to the points around the front-most point in the pick box, then grown over the connected points on that plane.
- Only structured (segmented) scans give planes. On an unstructured scan, or where the points under the cursor are not flat, the prompt asks again.
- If the two planes are nearly parallel, no line is drawn and a message says so.
