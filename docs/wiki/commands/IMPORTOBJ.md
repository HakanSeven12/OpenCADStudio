---
summary: Imports the mesh from a Wavefront OBJ file into the drawing as a 3D object.
related: IMPORT, ACISIN, STLOUT
---
## Usage
1. Pick the `.obj` file in the file window.
2. The mesh is added to the drawing and named after the file. The command line confirms the import.

## Notes
- The imported object has no solid geometry, so modelling commands such as [`UNION`](UNION-Command) cannot use it.
- A file without usable geometry is reported on the command line.
- The import can be undone.
