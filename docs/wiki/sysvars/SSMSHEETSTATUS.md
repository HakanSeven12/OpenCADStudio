---
summary: Controls how the Sheet Set Manager checks the status of sheets, such as a missing or already open drawing.
type: Integer
saved: Program settings
default: 2 (check regularly)
related: SSMPOLLTIME, SHEETSET
---

## Values
| Value | Meaning |
|---|---|
| 0 | Sheet status is not checked. |
| 1 | Status is checked when a sheet set is loaded or changed. |
| 2 | As 1, and checked again every [`SSMPOLLTIME`](SSMPOLLTIME-System-Variable) seconds. |

## Notes
- A sheet is marked when its drawing file is missing, or when the drawing is open in another program or in a tab of Open CAD Studio.
