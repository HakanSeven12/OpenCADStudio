---
summary: Creates lot lines from lines and polylines, and the parcels they enclose.
related: CREATEFEATURELINE, CREATEALIGNMENT
---

## Usage
1. Select lines or polylines, then press Enter.

## Notes
- A closed polyline makes a closed lot line, an open one an open lot line; arc segments stay arcs.
- Lot lines may cross and touch each other: every area they enclose becomes a parcel, and a parcel that a new line cuts keeps the part holding its label.
- Lot lines go on the drawing's last site; a drawing without one, or a site the line cannot join (it would meet an alignment or a feature line), gets a new site.
- A lot line must not cross itself or run along another line.
