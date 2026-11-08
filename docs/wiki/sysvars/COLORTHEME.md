---
summary: Switches the program window between the dark and the light theme.
type: Integer
saved: Program settings
default: 0 (Dark)
related: COLORSCHEME, OPTIONS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Dark theme. |
| 1 | Light theme. |

## Notes
- When read, the value is 1 if the background of the current theme is light, otherwise 0. The theme name is shown too.
- For other themes use [`COLORSCHEME`](COLORSCHEME-System-Variable).
