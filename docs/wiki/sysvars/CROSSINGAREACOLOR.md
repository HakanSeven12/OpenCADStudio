---
summary: Sets the color of the crossing selection area (dragged from right to left).
type: Integer
saved: Program settings
default: 0 (Theme color)
related: WINDOWAREACOLOR, SELECTIONAREA, SELECTIONAREAOPACITY, OPTIONS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Use the color of the current theme. |
| 1-255 | A color index. |

## Notes
- Also set in the [`OPTIONS`](OPTIONS-Command) dialog.
- The area is only filled when [`SELECTIONAREA`](SELECTIONAREA-System-Variable) is on.
