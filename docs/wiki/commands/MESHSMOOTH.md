---
summary: Converts solids, surfaces, regions, 3D faces, polygon meshes and polyface meshes to mesh objects.
related: MESHOPTIONS, MESHSMOOTHMORE, CONVTOSOLID
---

## Usage
1. Select the objects to convert, then press Enter.

## Notes
- Each object is replaced by a mesh with its layer and colour.
- Solids, surfaces and regions are tessellated as [`MESHOPTIONS`](MESHOPTIONS-Command) sets; the mesh gets the smoothness level [`FACETERSMOOTHLEV`](FACETERSMOOTHLEV-System-Variable).
