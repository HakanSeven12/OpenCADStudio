---
summary: Sets the pattern a new hatch starts with.
type: String
saved: Not saved
default: _SOLID (solid fill)
related: HATCH, HPGAPTOL, HPASSOC
---

## Values
The name of a pattern in the pattern file, for example `ANSI31` or `SOLID`. Type `.` to use the first pattern of the pattern file. A name that is not in the pattern file is refused.

## Notes
- Typed alone, it asks for the new name. `SETVAR HPNAME name` sets it directly.
- When you choose a pattern or a solid fill in [`HATCH`](HATCH-Command), the variable takes that name. Making a gradient sets it to `SOLID`.
- It lasts for the current session only and starts as `_SOLID` each time the program starts.
