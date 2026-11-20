---
summary: Sets where the plot offset is measured from.
type: On/Off (0/1)
saved: Program settings
default: 0 (Printable area)
related: PLOT, PLOTROTMODE
---

## Values
| Value | Meaning |
|---|---|
| 0 | The offset is measured from the corner of the printable area. |
| 1 | The offset is measured from the edge of the paper. |

## Notes
- It applies to editable X and Y offsets in the [`PLOT`](PLOT-Command) dialog. Saved offsets for the Layout plot area use the printable corner.
- Only 0 and 1 are accepted.
