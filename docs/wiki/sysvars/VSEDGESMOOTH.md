---
summary: Sets the crease angle below which mesh edges are not drawn in the shaded-with-edges visual styles.
type: Real (degrees)
saved: Program settings
default: 1
related: VSCURRENT, VSEDGECOLOR
---

The accepted range is 0 to 180 degrees. Edges where the faces meet at a flatter angle than this are hidden.

## Notes
- Used by Flat Shaded + Edges and Gouraud Shaded + Edges. See [`VSCURRENT`](VSCURRENT-System-Variable).
