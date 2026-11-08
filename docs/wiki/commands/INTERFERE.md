---
summary: Creates a new solid from the overlap of two selected solids and keeps both originals.
related: INTERSECT, UNION, SUBTRACT, MASSPROP
---
## Usage
1. Select exactly two 3D solids.
2. Run `INTERFERE`. If the solids overlap, a new solid is added for the shared volume and the command line confirms it.

## Notes
- The two original solids are not changed. To replace them with their overlap, use [`INTERSECT`](INTERSECT-Command).
- If the solids do not overlap, the command line says so and nothing is added.
- Solids on locked layers are not used.
- The command can be undone.
