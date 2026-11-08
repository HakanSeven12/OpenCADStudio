---
summary: Joins two or more 3D solids, regions or surfaces into one object.
related: SUBTRACT, INTERSECT, REGION, SOLIDEDIT
ribbon: Model tab › Boolean panel
---
## Usage
1. Select the objects to join and press Enter. If you selected two or more suitable objects before you start, the command runs at once.
2. The command line reports how many result objects were created.

## Notes
- Objects are joined by type: solids with solids, regions with regions, surfaces with surfaces. Selecting two solids and two regions gives one joined solid and one joined region.
- You need at least two objects of the same type. Otherwise the command reports `select at least two solids, Regions, or planar Surfaces of the same type.`
- The result takes the layer and other properties of the first object of its type. The original objects are removed, and the result is selected.
- Objects on locked layers are left out.
- If the geometry cannot be joined safely, the command reports the reason and the drawing is not changed.
- You can undo the change.
