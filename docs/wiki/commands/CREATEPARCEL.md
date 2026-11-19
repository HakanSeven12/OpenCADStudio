---
summary: Creates parcels inside closed polylines.
related: CREATEFEATURELINE, CREATEALIGNMENT
---

## Usage
1. Select closed polylines, then press Enter.

## Notes
- Each polyline becomes a lot line with the parcel inside it, on the drawing's last site; a drawing without one, or a site the parcel cannot join, gets a new site.
- Lot lines must not cross or touch each other or the site's other members.
- Polylines with arc segments are not supported yet.
