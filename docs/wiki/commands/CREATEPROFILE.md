---
summary: Creates a Civil 3D layout profile on an alignment from its PVIs.
related: CREATEALIGNMENT
---

## Usage
1. Select an alignment.
2. Specify each PVI as station,elevation; press Enter after the last.
3. Specify the vertical curve length at the inner PVIs (0 for none).

## Notes
- The profile is a native Civil 3D layout profile of tangents with symmetric parabolic curves.
- A drawing without Civil 3D data gets the profile style it needs.
- The alignment must be one OpenCAD Studio can write again: an alignment Civil 3D has changed is refused.
