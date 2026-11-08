---
summary: Sets the fill color of the window selection area (dragged left to right).
type: Integer
saved: Program settings
default: 0 (Theme color)
related: WINDOWSAREACOLOR, CROSSINGAREACOLOR, SELECTIONAREA, SELECTIONAREAOPACITY
---

## Values
| Value | Meaning |
|---|---|
| 0 | The accent color of the current theme |
| 1-255 | That color index |

## Notes
- [`WINDOWSAREACOLOR`](WINDOWSAREACOLOR-System-Variable) is the same setting under another name.
- The "Window selection color" setting in the [`OPTIONS`](OPTIONS-Command) window sets it too.
- The area is only filled while [`SELECTIONAREA`](SELECTIONAREA-System-Variable) is 1.
