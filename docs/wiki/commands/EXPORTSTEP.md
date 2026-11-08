---
summary: Exports the 3D objects of the drawing to a faceted STEP file.
related: STEPOUT, EXPORTSTL, FBXEXPORT, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `EXPORTSTEP`.
2. In the save dialog, pick a file name (`.step` or `.stp`).
3. The file is written in the background. The command line reports where it was saved.

## Notes
- Every 3D object shown in the drawing (solids, surfaces, meshes) is exported, at full resolution. There is no selection step.
- Each object is written as a faceted body made of flat triangles. A watertight object becomes a closed solid; others become open shells.
- If the drawing has no 3D objects, you see `STEPOUT: no 3D mesh data in this drawing.`
- [`STEPOUT`](STEPOUT-Command) does the same thing.
