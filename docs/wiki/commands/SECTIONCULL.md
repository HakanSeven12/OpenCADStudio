---
summary: Keeps only what lies inside a section object and removes the rest of the drawing space for good.
related: SECTIONPLANE, SECTION, SLICE
---

## Usage
1. Select a section object, or turn on a live section in the current space.
2. Type `SECTIONCULL`.
3. The command line reports how many objects were erased and how many were cut.

## Notes
- Solids, surfaces and closed meshes that cross the section are cut to it. Open meshes are clipped face by face.
- Objects wholly outside the section are erased. Objects across its boundary that cannot be cut stay whole.
- Objects on locked layers are not changed.
- If nothing lies outside the section, nothing changes. If a cut object cannot be prepared, the whole command is cancelled.
- The change is one undo step.
