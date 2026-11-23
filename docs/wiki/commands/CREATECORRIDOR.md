---
summary: Creates a corridor along an alignment with an assembly.
related: CREATEALIGNMENT, CREATEPROFILE, CREATESAMPLELINES
---

## Usage
1. Select the baseline alignment; it must have a profile.
2. Select an assembly of the drawing.
3. Press Enter, or first set options (type part of a name, or `?` to list them):
   - **Surfaces**: Top, Datum, Top and Datum, or None.
   - **Lock**: Stations (the default) keeps the region at its stations; Geometry ties its ends to the alignment's geometry, so they move with it when the alignment is edited.

## Notes
- The corridor has one baseline along the alignment and its newest profile, and one region over the whole alignment applying the assembly every 25 units.
- Each surface follows the assembly's links of its code (Top or Datum), added as breaklines, with the overhang corrected to the top or the bottom links, and is bounded by the corridor's extents. Without a choice the corridor gets a Top surface.
- It is written out of date, as a corridor before its first rebuild: it is built, with its surfaces, when the corridor is rebuilt.
- The drawing needs a corridor style and an assembly.
- With the Geometry lock the alignment is written again with an anchor at each region end, so it must be one OpenCAD Studio can write again.
