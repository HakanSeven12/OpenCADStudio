---
summary: Sets whether the arcs of a revision cloud have different lengths.
type: On/Off (0/1)
saved: Program settings
default: ON (1)
related: REVCLOUD, REVCLOUDAPPROXARCLEN
---

## Values
| Value | Meaning |
|---|---|
| 0 (OFF) | All arcs have the same length. |
| 1 (ON) | Arc lengths vary at random between two thirds and four thirds of the arc length. |

## Notes
- You can type 0, 1, ON or OFF.
- The arc length comes from [`REVCLOUDAPPROXARCLEN`](REVCLOUDAPPROXARCLEN-System-Variable).
