---
summary: Creates COGO points at the locations you specify.
related: CREATEPOINTGROUP, CREATESURFACE
---

## Usage
1. Specify a location for each point; type x,y,z to give its elevation.
2. Press Enter to end.

## Notes
- Each point is a native COGO point with the next free point number, kept in the drawing's point database and in `_All Points`.
- A drawing that lacks the point style and settings gets them.
- Erasing a COGO point removes it from the point database and its point groups too.
