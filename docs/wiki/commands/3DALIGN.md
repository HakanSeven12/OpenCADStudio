---
summary: Moves and turns one selected 3D solid so that three source points land on three destination points, typed as 18 numbers.
related: ALIGN, 3DMOVE, 3DROTATE, 3DMIRROR
---

## Usage
1. Select exactly one 3D solid.
2. Type `3DALIGN` followed by 18 numbers: the X, Y and Z of the three source points, then the X, Y and Z of the three destination points.
   Example: `3DALIGN 0 0 0 10 0 0 0 10 0  5 5 0 5 15 0 -5 5 0`
3. The solid is replaced by the aligned solid, which stays selected.

## Notes
- Typing `3DALIGN` on its own does not prompt for points. It only prints the usage line.
- The first source point goes to the first destination point. The second point sets the direction, and the third point sets the plane. The solid is moved and turned only; it is never scaled.
- Each set of three points must be distinct and must not lie on one line, or the command reports an error.
- Objects on locked layers are ignored. If the selection is not exactly one solid, the command stops with a message.
- To align any kind of object by picking points, use [`ALIGN`](ALIGN-Command).
- The change can be undone in one step.
