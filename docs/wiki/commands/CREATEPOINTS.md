---
summary: Creates Civil 3D COGO points at the locations you specify.
related: CREATEPOINTGROUP, CREATESURFACE
---

## Usage
1. Specify a location for each point; type x,y,z to give its elevation.
2. Press Enter to end.

## Notes
- Each point is a native Civil 3D COGO point with the next free point number, kept in the drawing's point database and in `_All Points`.
- A drawing without Civil 3D data gets the point style and settings it needs.
- Erasing a COGO point removes it from the point database and its point groups too.
