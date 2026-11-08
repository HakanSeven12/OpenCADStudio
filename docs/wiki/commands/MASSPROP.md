---
summary: Lists volume, centroid and inertia values of selected 3D solids, and area, perimeter and centroid of selected 2D curves.
related: AREA, DIST, LIST, MEASUREGEOM
---

## Usage
1. Select the objects first.
2. Type `MASSPROP`. The results for each object appear in the command line.

## Notes
- You must select objects before you start the command. With nothing selected it shows an error and ends.
- For 3D solids and bodies it shows volume, centroid, moments of inertia, products of inertia, radii of gyration and principal moments about the centroid. A solid with no closed body shows an error instead.
- For circles, arcs, lines, lightweight polylines and ellipses it shows area, perimeter and centroid.
- Other object types are skipped without a message.
