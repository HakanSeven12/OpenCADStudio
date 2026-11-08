---
summary: Exports all 3D objects of the drawing to a binary STL file.
related: EXPORTSTL, STEPOUT, FBXEXPORT, EXPORT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `STLOUT`.
2. In the save dialog, enter a file name (`.stl`).
3. The file is written in the background. The command line shows `STLOUT: exported to "<path>"`.

## Notes
- There is no selection step. Every 3D object shown in the drawing (solids, surfaces, meshes) goes into one file, at full resolution.
- The file is always binary STL.
- If the drawing has no 3D objects, you see `STLOUT: no 3D mesh data in this drawing.`
- [`EXPORTSTL`](EXPORTSTL-Command) does the same thing.
