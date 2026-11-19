---
summary: Creates a Civil 3D corridor along an alignment with an assembly.
related: CREATEALIGNMENT, CREATEPROFILE, CREATESAMPLELINES
---

## Usage
1. Select the baseline alignment; it must have a profile.
2. Select an assembly of the drawing.

## Notes
- The corridor has one baseline along the alignment and its newest profile, and one region over the whole alignment applying the assembly every 25 units.
- It is written out of date, as a corridor before its first rebuild: Civil 3D builds it when the corridor is rebuilt.
- The drawing needs a corridor style and an assembly, as drawings made in Civil 3D have.
