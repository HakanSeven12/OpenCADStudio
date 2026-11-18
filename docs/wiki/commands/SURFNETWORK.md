---
summary: Creates a surface through a network of curves running in two directions.
related: SURFPATCH, LOFT, SURFACEMODELINGMODE
---

## Usage
1. Select the curves in the first direction, then press Enter.
2. Select the curves in the second direction, then press Enter.

## Notes
- Every curve of one direction must cross every curve of the other direction once; the first and last curves of each direction bound the surface.
- Lines, polylines, splines, arcs and other curves are taken; arcs and ellipses are first fitted with splines.
- The surface is a NURBS surface when SURFACEMODELINGMODE is 1.
