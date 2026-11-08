---
summary: Shows the path of the sheet set found for the drawing that was opened last.
type: String, read-only
saved: Not saved
default: "" (empty)
related: SSLOCATE, SSMAUTOOPEN, SHEETSET
---

## Notes
- When a drawing that belongs to a sheet set is opened and [`SSLOCATE`](SSLOCATE-System-Variable) is 1, the sheet set file is looked for, first at its stored path and then next to the drawing. The path that was found is kept here; it is empty when nothing was found.
- It cannot be set.
