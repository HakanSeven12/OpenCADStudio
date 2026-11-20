---
summary: Creates a layout profile on an alignment from its PVIs.
related: CREATEALIGNMENT, CREATESURFACEPROFILE
---

## Usage
1. Select an alignment.
2. Specify each PVI as station,elevation; press Enter after the last.
3. Specify the vertical curve value at the inner PVIs (0 for none).
4. Press Enter, or first set options:
   - **Curve type**: what the value gives, Length (the default), K value (length per percent of grade change) or Radius.
   - **Layout**: PVIs (the default) writes the profile by its PVIs; Fixed entities writes it entity by entity, fixed tangents between fixed parabolas through each curve's start, middle and end, so the curves keep their place when the profile is edited.

## Notes
- The profile is a native layout profile of tangents with symmetric parabolic curves.
- With fixed entities the curves must not overlap or pass the PVIs beside them.
- A drawing that lacks the profile style gets it.
- The alignment must be one OpenCAD Studio can write again: an alignment changed by another program is refused.
