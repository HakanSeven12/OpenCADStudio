---
summary: Sets the percentage of the point limit that point clouds show.
type: Integer
saved: Drawing
default: 15
related: POINTCLOUDPOINTMAX, POINTCLOUDLOD, POINTCLOUDRTDENSITY
---

## Values
Accepted values are whole numbers from 1 to 100.

## Notes
- The number of points shown for a cloud is this percentage of [`POINTCLOUDPOINTMAX`](POINTCLOUDPOINTMAX-System-Variable), scaled by [`POINTCLOUDLOD`](POINTCLOUDLOD-System-Variable) in tenths. A larger cloud shows every n-th point.
