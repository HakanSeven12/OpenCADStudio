---
summary: Controls what happens when the chosen printer does not support the paper size of the page setup.
type: On/Off (0/1)
saved: Program settings
default: 0 (keep the sheet)
related: PLOT, PLOTOFFSET, PLOTROTMODE
---

## Values
| Value | Meaning |
|---|---|
| 0 | The paper size is kept and a message says the printer does not support it. |
| 1 | The paper size is changed to the printer's default sheet, and a message says so. |

## Notes
- Used by the Plot dialog when a printer is chosen.
- It is kept with the Plot dialog settings. A value other than 0 or 1 is refused.
