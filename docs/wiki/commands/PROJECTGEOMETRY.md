---
summary: Projects curves onto a surface or a solid face.
related: SURFTRIM, SURFEXTRACTCURVE, SURFACEAUTOTRIM
---

## Usage
1. Select the curves, then press Enter.
2. Pick the surface or face to project onto.
3. With the 2 Points direction, specify the start and end points of the direction.

## Options
- `PROJECTGEOMETRY UCS` (Project to UCS): along the UCS Z axis.
- `PROJECTGEOMETRY VIEW` (Project to View): along the current view direction.
- `PROJECTGEOMETRY POINTS` (Project to 2 Points): along the direction between two points.

## Notes
- The projected curves are drawn as new curves; the originals are kept.
- With [`SURFACEAUTOTRIM`](SURFACEAUTOTRIM-System-Variable) on (the Auto Trim button), a surface loses the area the projected curves enclose instead.
