---
summary: Sets the running object snap modes, one bit for each mode.
type: Integer (bit-coded)
saved: Program settings
default: 63 (Endpoint, Midpoint, Center, Node, Quadrant and Intersection)
related: OSNAP, APERTURE, OSNAPZ, ORTHOMODE
---

## Values
Add up the values of the snap modes you want on.

| Value | Meaning |
|---|---|
| 0 | No running object snaps. |
| 1 | Endpoint |
| 2 | Midpoint |
| 4 | Center |
| 8 | Node |
| 16 | Quadrant |
| 32 | Intersection |
| 64 | Insertion |
| 128 | Perpendicular |
| 256 | Tangent |
| 512 | Nearest |
| 1024 | Geometric center |
| 2048 | Apparent intersection |
| 4096 | Extension |
| 8192 | Parallel |
| 16384 | Object snap is turned off; the modes are kept. |

## Notes
- [`OSNAP`](OSNAP-Command), the F3 key and the status bar set the same value.
- The snap set belongs to you, not to the drawing, so it is kept in the program settings. It is also written to the drawing when you save.
- When an older file or a DXF file stores a value other than 0, that value is used when the file is opened.
- Grid snap and the 3D object snaps are not part of this value.
