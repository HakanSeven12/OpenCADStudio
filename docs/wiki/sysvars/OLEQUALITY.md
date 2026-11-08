---
summary: Sets the plot quality that new OLE objects get.
type: Integer
saved: Program settings
default: 3 (automatic)
related: INSERTOBJ, OLEFRAME, OLEHIDE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Monochrome |
| 1 | Low quality graphics |
| 2 | High quality graphics |
| 3 | Automatic (new objects get high quality graphics) |

## Notes
- The value is written to each OLE object placed with [`INSERTOBJ`](INSERTOBJ-Command). Objects already in the drawing are not changed.
- A value outside 0 to 3 is refused.
