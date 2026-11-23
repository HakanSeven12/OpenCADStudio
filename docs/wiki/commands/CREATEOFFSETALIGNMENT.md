---
summary: Creates an offset alignment that follows another alignment at a distance.
related: CREATEALIGNMENT, CREATECORRIDOR
---

## Usage
1. Select the parent alignment.
2. Specify the offset from the parent: positive to the right, negative to the left.

## Notes
- The offset alignment runs along the whole parent: each line of the parent gives a parallel line, each arc a concentric arc.
- The parent is written again with an anchor at each end of the offset alignment, so it must be an alignment OpenCAD Studio can write again, not on a site, made of lines and arcs only.
- The offset alignment is named Alignment and a number.
