---
summary: Controls whether the sheet set of a drawing is found and opened when the drawing is opened.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: SSMAUTOOPEN, SSFOUND, SHEETSET
---

## Values
| Value | Meaning |
|---|---|
| 0 | The sheet set is not looked for. |
| 1 | When you open a drawing that is a sheet of a sheet set, the sheet set is found and opened too. |

## Notes
- The sheet set file is looked for at its stored path, then next to the drawing. The result is in [`SSFOUND`](SSFOUND-System-Variable).
- Whether the Sheet Set Manager is shown at the same time is set by [`SSMAUTOOPEN`](SSMAUTOOPEN-System-Variable).
