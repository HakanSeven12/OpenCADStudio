---
summary: Sets the surface density in the M direction when PEDIT smooths a polygon mesh.
type: Integer
saved: Drawing
default: 6
related: SURFV, SURFTYPE, PEDIT
---

## Values
A whole number from 0 to 200. When the mesh is smoothed, values below 2 are used as 2.

## Notes
- Used by the Smooth surface option of [`PEDIT`](PEDIT-Command) on 3D polygon meshes. Higher values give a smoother, denser mesh.
- Also set in Options › 3D Modeling.
