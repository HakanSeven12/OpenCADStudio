---
summary: Sets how the corners are closed when a polyline is offset outward.
type: Integer
saved: Program settings
default: 0 (extend)
related: OFFSET
---

## Values
| Value | Meaning |
|---|---|
| 0 | The segments are extended until they meet. |
| 1 | The corners are filleted (rounded with an arc). |
| 2 | The corners are chamfered (cut with a straight line). |

## Notes
- Used by [`OFFSET`](OFFSET-Command). A value outside 0 to 2 is refused.
