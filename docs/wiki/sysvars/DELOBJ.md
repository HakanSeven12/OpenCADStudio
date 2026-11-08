---
summary: Controls whether the source objects are deleted when you create solids or surfaces from them.
type: Integer
saved: Program settings
default: 1
related: EXTRUDE, REVOLVE, SWEEP, LOFT, SPLINEDIT, SURFACEASSOCIATIVITY
---

## Values
| Value | Meaning |
|---|---|
| 0 | Keep all source objects. |
| 1 | Delete the profiles and cross sections. Keep sweep paths and loft guides. |
| 2 | Delete the profiles and cross sections, and also the sweep paths and loft guides. |
| 3 | Delete the profiles and cross sections. Delete paths and guides only when the result is a solid, not a surface. |

## Notes
- Used by [`EXTRUDE`](EXTRUDE-Command), [`REVOLVE`](REVOLVE-Command), [`SWEEP`](SWEEP-Command), [`LOFT`](LOFT-Command) and conversion to surfaces. When converting a spline with [`SPLINEDIT`](SPLINEDIT-Command), any value other than 0 deletes the original.
- An associative surface (see [`SURFACEASSOCIATIVITY`](SURFACEASSOCIATIVITY-System-Variable)) keeps the path it depends on.
