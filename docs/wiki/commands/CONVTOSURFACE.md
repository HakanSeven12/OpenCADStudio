---
summary: Converts meshes, polygon meshes, polyface meshes, 3D solids and regions to surface objects.
related: CONVTOSOLID, THICKEN, SMOOTHMESHCONVERT
---

## Usage
1. Select the objects to convert, then press Enter. Objects selected before the command are used directly.

## Notes
- A mesh becomes a surface of its faces, smooth or faceted as [`SMOOTHMESHCONVERT`](SMOOTHMESHCONVERT-System-Variable) sets; open meshes convert too.
- A solid or region becomes a surface of all its faces.
- The surfaces keep each object's layer and colour. When [`DELOBJ`](DELOBJ-System-Variable) is not 0, the original objects are deleted.
- You can undo the conversion in one step.
