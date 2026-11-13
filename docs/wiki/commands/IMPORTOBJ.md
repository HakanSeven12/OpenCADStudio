---
summary: Imports a Wavefront OBJ file into the drawing as MESH objects, one for each object of the file.
related: IMPORT, ACISIN, STLOUT
---
## Usage
1. Pick the `.obj` file in the file window.
2. Each object of the file becomes a MESH on the current layer, with its polygons as the file has them. The command line confirms the import.

## Notes
- The meshes snap, show in layout viewports and save with the drawing like any MESH.
- OBJ is Y-up; the import turns it so that the file's Y axis points along Z.
- `IMPORT` with **All Files** reads `.obj` files the same way.
- A file without usable geometry is reported on the command line.
- The import can be undone.
