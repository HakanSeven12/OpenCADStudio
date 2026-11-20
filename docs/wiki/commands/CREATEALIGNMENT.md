---
summary: Creates an alignment from a line or polyline.
related: CREATEPROFILE, CREATEOFFSETALIGNMENT
---

## Usage
1. Select a line or 2D polyline.
2. Press Enter, or first set options:
   - **Radius**: a curve of this radius at each corner (0 for none).
   - **Spiral length**: spirals of this length on each side of each curve (0 for none).
   - **Type**: Centerline (the default), Utility or Rail (type part of a name, or `?` to list them).
   - **Gauge**: the track gauge of a rail alignment (1.435 when not given).

## Notes
- The alignment is a native alignment of fixed lines with free curves; the polyline stays in the drawing.
- Polylines with arc segments are not supported yet.
