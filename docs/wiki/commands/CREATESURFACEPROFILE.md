---
summary: Creates a profile of an alignment sampled from a TIN surface.
related: CREATEPROFILE, CREATESURFACE, CREATEPROFILEVIEW
---

## Usage
1. Select an alignment.
2. Select the TIN surface to sample.
3. Specify the offset from the alignment at which it samples: positive to the right, negative to the left (0 along the alignment).

## Notes
- The profile samples the surface along the whole alignment.
- It stores the surface and the offset, not elevations: its elevations are computed from the surface when the profile is rebuilt. OpenCAD Studio does not draw it yet.
- The alignment is written again with the profile in its list, so it must be one OpenCAD Studio can write again.
