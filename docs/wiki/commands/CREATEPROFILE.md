---
summary: Creates a layout profile on an alignment from its PVIs.
related: CREATEALIGNMENT, CREATESURFACEPROFILE
---

## Usage
1. Select an alignment.
2. Specify each PVI as station,elevation; press Enter after the last.
3. Specify the vertical curve value at the inner PVIs (0 for none).
4. Press Enter, or first set **Curve type**: what the value gives, Length (the default), K value (length per percent of grade change) or Radius.

## Notes
- The profile is a native layout profile of tangents with symmetric parabolic curves.
- A drawing that lacks the profile style gets it.
- The alignment must be one OpenCAD Studio can write again: an alignment changed by another program is refused.
