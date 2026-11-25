---
summary: Converts closed meshes, polygon meshes and polyface meshes to 3D solids.
related: CONVTOSURFACE, MESHCAP, SMOOTHMESHCONVERT
---

## Usage
1. Select the meshes, then press Enter.

## Notes
- [`SMOOTHMESHCONVERT`](SMOOTHMESHCONVERT-System-Variable) chooses smooth or faceted, optimized or not (also from the Convert Mesh panel).
- A mesh with holes is left as it is; close it with [`MESHCAP`](MESHCAP-Command) first.
- [`DELOBJ`](DELOBJ-System-Variable) decides whether the meshes are kept.
