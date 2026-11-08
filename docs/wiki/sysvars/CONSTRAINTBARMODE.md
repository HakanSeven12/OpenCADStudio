---
summary: Sets which kinds of geometric constraints show in constraint bars.
type: Integer (bit-coded)
saved: Program settings
default: 4095 (all kinds)
related: CONSTRAINTBARDISPLAY, CONSTRAINTBAR, CONSTRAINTSETTINGS, GEOMCONSTRAINT
---

## Values
| Value | Meaning |
|---|---|
| 1 | Horizontal |
| 2 | Vertical |
| 4 | Perpendicular |
| 8 | Parallel |
| 16 | Tangent |
| 32 | Smooth |
| 64 | Coincident |
| 128 | Concentric |
| 256 | Collinear |
| 512 | Symmetric |
| 1024 | Equal |
| 2048 | Fix |

Values add up. The accepted range is 0 to 4095. A cleared bit hides that kind of constraint from the bars; the constraint itself stays.
