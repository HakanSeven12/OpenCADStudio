---
summary: Controls whether the Sheet Set Manager opens when a drawing that belongs to a sheet set is opened.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: SSLOCATE, SSMSTATE, SHEETSET
---

## Values
| Value | Meaning |
|---|---|
| 0 | The sheet set is loaded, but the Sheet Set Manager is not shown. |
| 1 | The Sheet Set Manager is shown with the drawing's sheet set. |

## Notes
- Works only while [`SSLOCATE`](SSLOCATE-System-Variable) is 1 and the sheet set file is found.
