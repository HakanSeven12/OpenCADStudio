---
summary: Controls when constraint bars are shown for geometric constraints.
type: Integer (bit-coded)
saved: Program settings
default: 3 (both)
related: CONSTRAINTBARMODE, CONSTRAINTBAR, GEOMCONSTRAINT, AUTOCONSTRAIN
---

## Values
| Value | Meaning |
|---|---|
| 0 | Neither of the options below. |
| 1 | Show the constraint bar of a constraint right after it is applied. |
| 2 | Show the constraint bars of the selected objects. |

Values add up: 3 means both.

## Notes
- [`CONSTRAINTBAR`](CONSTRAINTBAR-Command) still shows or hides bars by hand.
