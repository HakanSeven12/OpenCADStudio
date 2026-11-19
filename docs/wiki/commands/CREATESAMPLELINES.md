---
summary: Creates a Civil 3D sample line group with sample lines along an alignment.
related: CREATEALIGNMENT, CREATESURFACE, CREATECORRIDOR
---

## Usage
1. Select an alignment.
2. Specify the increment between sample lines.
3. Specify the swath width on each side of the alignment.

## Notes
- Sample lines are placed at the alignment's start, every increment, and at its end.
- Every TIN surface of the drawing is sampled; its sections hold no points until Civil 3D updates them.
- The drawing needs a sample line label style, as drawings made in Civil 3D have.
