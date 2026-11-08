---
summary: Replaces two or more overlapping solids, regions or coplanar surfaces with the shape they have in common.
related: UNION, SUBTRACT, INTERFERE, REGION
ribbon: Model tab › Boolean panel
---
## Usage
1. Select the objects. With objects already selected, the command runs at once.
2. Otherwise, select the objects at the prompt and press Enter.
3. The objects are replaced by the volume or area they share.

## Notes
- Select at least two objects of the same kind: solids with solids, regions with regions, surfaces with coplanar surfaces. Each kind is intersected separately.
- The result keeps the first selected object of each group; the others are removed.
- Solids that only touch or do not overlap are kept unchanged, and so are surfaces that do not overlap; the command line reports how many objects were kept.
- Regions or surfaces that only touch, and regions that do not overlap, have no common area and are removed.
- If the shapes cannot be intersected safely, an error is shown and nothing changes.
- Objects on locked layers are not used.
- The command can be undone.
