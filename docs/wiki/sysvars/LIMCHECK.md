---
summary: Controls whether points outside the model space drawing limits are refused.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: LIMITS, PLIMCHECK
---

## Values
| Value | Meaning |
|---|---|
| 0 | Points may be placed anywhere. |
| 1 | A point outside the limits is refused with "Outside limits." |

## Notes
- [`LIMITS`](LIMITS-Command) sets the limits; its ON and OFF options set this variable while you work in model space.
- [`LIMITS`](LIMITS-Command) itself can always pick points outside the old limits.
- Paper space uses [`PLIMCHECK`](PLIMCHECK-System-Variable) instead.
