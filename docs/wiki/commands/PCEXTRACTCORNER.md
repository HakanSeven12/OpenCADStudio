---
summary: Places a point where three planar surfaces of a point cloud meet.
related: PCEXTRACTEDGE, PCEXTRACTCENTERLINE, PCEXTRACTSECTION, POINTCLOUDATTACH
---

## Usage
1. Specify the first plane: click on a flat area of the point cloud.
2. Specify the second plane.
3. Specify the third plane.

A point object is placed where the three planes meet.

## Notes
- A plane is found from the cloud points under the cursor: a plane is fitted to the points around the front-most point in the pick box, then grown over the connected points on that plane.
- Only structured (segmented) scans give planes. On an unstructured scan, or where the points under the cursor are not flat, the prompt asks again.
- If the planes are nearly parallel, no point is placed and a message says so.
