---
summary: Controls whether EXPLODE and BURST break up block references that are not scaled evenly.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: EXPLODE, BURST
---

## Values
| Value | Meaning |
|---|---|
| 0 | Block references with different X, Y and Z scales, or a negative scale, are not exploded. A message tells you which objects were skipped. |
| 1 | All block references can be exploded, whatever their scale. |

## Notes
- Used by [`EXPLODE`](EXPLODE-Command) and [`BURST`](BURST-Command).
- Changing the value can be undone.
