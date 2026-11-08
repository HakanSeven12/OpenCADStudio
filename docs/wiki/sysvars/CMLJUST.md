---
summary: Sets the justification that new multilines are drawn with.
type: Integer
saved: Drawing
default: 0 (Top)
related: CMLSCALE, CMLSTYLE, MLINE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Top: the picked points run along the top line. |
| 1 | Zero: the picked points run along the center (zero offset). |
| 2 | Bottom: the picked points run along the bottom line. |

## Notes
- [`MLINE`](MLINE-Command) reads this value and stores the justification you choose there.
