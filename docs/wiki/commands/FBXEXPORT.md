---
summary: Exports the 3D objects of the drawing to a binary FBX file for visualization tools.
related: EXPORTSTL, EXPORTSTEP, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `FBXEXPORT`.
2. In the save dialog, pick a file name (`.fbx`).
3. The file is written in the background. The command line shows `FBXEXPORT: exported to "<path>"`.

## Notes
- Every 3D object shown in the drawing (solids, surfaces, meshes) is exported, at full resolution. There is no selection step.
- Each object becomes its own model with a material in its display color.
- The Z axis stays up. The file's unit scale comes from the drawing units ([`INSUNITS`](INSUNITS-System-Variable)); a unitless metric drawing is taken as millimeters, an imperial one as inches.
- If the drawing has no 3D objects, you see `FBXEXPORT: no 3D mesh data in this drawing.`
- `EXPORTFBX` does the same thing.
