---
summary: Breaks compound objects such as polylines, blocks, dimensions and hatches into simple objects.
related: BURST, TXTEXP, PEDIT, JOIN
ribbon: Draw tab › Modify panel
---

## Usage
1. Select the objects to explode, or start the command first and select them at the `Select objects:` prompt.
2. Press Enter. The objects are replaced by their pieces and the command reports how many were exploded.

## What each object becomes
| Object | Result |
|---|---|
| Polyline (2D, 3D, lightweight) | Lines, and arcs for curved segments |
| Block reference | The objects inside the block, placed where the reference shows them |
| Multiple-block reference | One copy of the block contents for each row and column |
| Multiline | Lines, one run for each style element |
| Dimension | Lines, arrowheads and text |
| Leader and multileader | Their lines, arrowheads and text |
| Multiline text | Single-line text objects |
| Region | Lines, arcs, circles, ellipses and splines |
| Hatch (pattern) | The lines of its pattern, clipped to the boundary |
| Center mark | Separate lines |

## Notes
- Objects on locked layers are skipped.
- Object types that cannot be exploded are left as they are. If nothing in the selection can be exploded, you see `EXPLODE: no explodable objects selected.`
- Solid and gradient hatches are not exploded.
- When the `EXPLMODE` system variable is 0, block references that are not scaled uniformly and positively are kept whole, with the message `The object is not uniformly and positively scaled.`
- Exploding a block reference drops its attribute values. Use [`BURST`](BURST-Command) to keep them as text.
- Very large multiple-block arrays are left intact instead of being expanded.
- Multileaders are exploded at the sizes stored for the current annotation scale.
- The whole operation is one undo step.
