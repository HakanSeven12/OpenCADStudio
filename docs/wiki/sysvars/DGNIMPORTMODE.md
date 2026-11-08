---
summary: Sets where DGN import puts the imported content.
type: Integer
saved: Program settings
default: 0 (New drawing)
related: DGNIMPORT
---

## Values
| Value | Meaning |
|---|---|
| 0 | Import into a new drawing. |
| 1 | Import into the current drawing, keeping its own definitions. |
| 2 | Import into the current drawing, with prefixed names for the imported definitions. |

## Notes
- Used by [`DGNIMPORT`](DGNIMPORT-Command).
