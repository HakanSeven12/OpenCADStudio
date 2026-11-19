---
summary: Creates feature lines from lines and polylines.
related: CREATEALIGNMENT, CREATESURFACE
---

## Usage
1. Select lines, 2D polylines or 3D polylines, then press Enter; polyline arcs stay arcs.

## Notes
- Each object makes one feature line through its vertices, with their elevations.
- Feature lines join the drawing's last site; a drawing without one, or a site they cannot join, gets a new site.
- Feature lines on one site must not cross or touch each other.
