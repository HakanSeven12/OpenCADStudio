---
summary: Exports the 3D objects of the drawing to a binary STL file.
related: STLOUT, EXPORTSTEP, FBXEXPORT, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `EXPORTSTL`.
2. In the save dialog, pick a file name (`.stl`).
3. The file is written in the background. The command line shows `STLOUT: exported to "<path>"`.

## Notes
- Every 3D object shown in the drawing (solids, surfaces, meshes) goes into one STL file, at full resolution. There is no selection step.
- If the drawing has no 3D objects, you see `STLOUT: no 3D mesh data in this drawing.`
- [`STLOUT`](STLOUT-Command) does the same thing.
