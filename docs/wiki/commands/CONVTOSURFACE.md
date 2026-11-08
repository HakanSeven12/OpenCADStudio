---
summary: Converts the selected 3D solids to surface objects.
related: THICKEN, EXTRUDE
---

## Usage
1. Select one or more 3D solids.
2. Run `CONVTOSURFACE`. Each solid becomes a surface, and the command line reports how many were converted.

## Notes
- Select the solids first; the command does not ask for objects. With no solid selected it prints an error.
- Solids on locked layers are skipped.
- The new surfaces go on the current layer. They keep the solid's edges as their wireframe.
- When [`DELOBJ`](DELOBJ-System-Variable) is not 0, the original solids are deleted. Otherwise they stay.
- You can undo the conversion in one step.
