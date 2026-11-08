---
summary: Sets how block attributes are displayed in the drawing.
type: Integer
saved: Drawing
default: 1 (Normal)
related: ATTDISP, ATTMODE0, ATTMODE1, ATTMODE2
---

## Values
| Value | Meaning |
|---|---|
| 0 | Off: no attributes are shown |
| 1 | Normal: visible attributes are shown; invisible ones stay hidden |
| 2 | On: all attributes are shown, including invisible ones |

## Notes
- [`ATTDISP`](ATTDISP-System-Variable) sets this value and redraws the drawing.
- The attribute display list on the Insert ribbon tab shows and sets this value.
