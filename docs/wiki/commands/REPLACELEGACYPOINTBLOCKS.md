---
summary: Replaces block references that carry a POINT attribute with COGO points.
related: CONVERTCADPOINTS, CREATEBLOCKSFROMPOINTS
---

## Usage
1. Select the point blocks and press Enter.
2. If a point number is already used, choose how to resolve it.

## Notes
- The number comes from the POINT attribute, the elevation from ELEV (else the block's z), the description from DESC; the block is erased.
- A duplicate number can take an offset, be merged, overwrite the existing point, be numbered from a value or take the next free number; the choice can apply to all duplicates.
- The command is one undo step.
