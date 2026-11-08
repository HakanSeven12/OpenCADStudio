---
summary: Removes the volume of one set of solids, regions, surfaces or meshes from another set.
related: UNION, INTERSECT
ribbon: Model tab › Boolean panel
---

## Usage
1. Select the base solids, regions, surfaces or meshes, then press Enter.
2. Select the solids, regions, surfaces or meshes to subtract, then press Enter.
3. If any selected object is a mesh, answer `Convert selected closed Mesh objects to solids? [Yes/No] <Yes>`.

## Options
| Option | What it does |
|---|---|
| **Yes** (`Y`) | Converts closed meshes to solids before subtracting (default). |
| **No** (`N`) | Does not convert meshes. |

## Notes
- If you select objects before starting, they become the base set and the command goes straight to the second step.
- Only solids, regions, surfaces and meshes are taken; other objects in the selection are ignored.
- Objects on locked layers are not used as the base set.
