---
summary: Imports a Wavefront OBJ or glTF (.gltf, .glb) file into the drawing as MESH objects, one for each object of the file.
related: IMPORT, ACISIN, STLOUT
---
## Usage
1. Pick the `.obj`, `.gltf` or `.glb` file in the file window.
2. Each object of the file becomes a MESH on the current layer, with its polygons as the file has them. The command line confirms the import.

## Notes
- The meshes snap, show in layout viewports and save with the drawing like any MESH.
- OBJ and glTF are Y-up; the import turns them so that the file's Y axis points along Z.
- glTF: each mesh a scene node places becomes one MESH, moved, turned and scaled by its node. Its triangles are kept and their shared corners joined. glTF is in metres, so the mesh is scaled to the drawing's units (INSUNITS); a unitless drawing takes it as is. A `.gltf` reads its buffers from the file itself or from `.bin` files in its folder. Draco-compressed meshes and triangle strips or fans are skipped.
- `IMPORT` with **All Files** reads `.obj`, `.gltf` and `.glb` files the same way.
- A file without usable geometry is reported on the command line.
- The import can be undone.
