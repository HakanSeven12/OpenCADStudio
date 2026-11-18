---
summary: Sets whether new surfaces are procedural or NURBS surfaces.
type: Integer
saved: Not saved
default: 0
related: SURFNETWORK, SURFPATCH, SURFBLEND, CONVTONURBS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Procedural surfaces |
| 1 | NURBS surfaces |

## Notes
- Surfaces made by [`SURFNETWORK`](SURFNETWORK-Command), [`SURFPATCH`](SURFPATCH-Command), [`SURFBLEND`](SURFBLEND-Command), [`SURFOFFSET`](SURFOFFSET-Command) and [`SURFFILLET`](SURFFILLET-Command) follow it.
- The value is kept for the current session only.
