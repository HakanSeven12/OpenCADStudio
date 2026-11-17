---
summary: Exports all 3D objects of the drawing to a STEP file, solids as exact B-reps.
related: EXPORTSTEP, STLOUT, FBXEXPORT, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `STEPOUT`, or `STEPOUT AP203` / `STEPOUT AP242` for another schema.
2. In the save dialog, enter a file name (`.step` or `.stp`).
3. The file is written in the background. The command line shows `STEPOUT: exported to "<path>"`.

## Notes
- There is no selection step. Every 3D object shown in the drawing (solids, regions, surfaces, meshes) is exported.
- Solids, regions and surfaces are written as exact B-reps: their planes, cylinders, cones, spheres, tori and spline surfaces, with each object's layer and colour. A solid becomes a closed solid (with its voids); a region or surface becomes a surface model. Anything else (meshes, or an object whose geometry cannot be written exactly) is written as flat faces at full resolution.
- Coordinates are written in millimetres, converted from the drawing's insertion units.
- The schema is AP214 unless you name another: `STEPOUT AP203` or `STEPOUT AP242`.
- If the drawing has no 3D objects, you see `STEPOUT: no 3D mesh data in this drawing.`
- [`EXPORTSTEP`](EXPORTSTEP-Command) and [`STPOUT`](STPOUT-Command) do the same thing.
