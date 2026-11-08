---
summary: Exports all 3D objects of the drawing to a faceted STEP file.
related: EXPORTSTEP, STLOUT, FBXEXPORT, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `STEPOUT`.
2. In the save dialog, enter a file name (`.step` or `.stp`).
3. The file is written in the background. The command line shows `STEPOUT: exported to "<path>"`.

## Notes
- There is no selection step. Every 3D object shown in the drawing (solids, surfaces, meshes) is exported, at full resolution.
- Each object is written as a faceted body made of flat triangles, in millimetres. A watertight object becomes a closed solid; any other object becomes an open shell.
- If the drawing has no 3D objects, you see `STEPOUT: no 3D mesh data in this drawing.`
- [`EXPORTSTEP`](EXPORTSTEP-Command) and [`STPOUT`](STPOUT-Command) do the same thing.
