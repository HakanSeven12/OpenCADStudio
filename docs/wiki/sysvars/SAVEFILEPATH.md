---
summary: Sets the folder where automatic save copies are written.
type: String
saved: Program settings
default: "" (empty: an autosave folder inside the program's settings folder)
related: SAVETIME
---

## Values
Type a folder path to use it. Type `.` to go back to the default folder. With no value, the current folder is shown and you are asked for a new one.

## Notes
- Automatic save copies have the `.sv$` extension. Each drawing gets its own copy, named after the drawing and its folder.
- How often the copies are written is set by [`SAVETIME`](SAVETIME-System-Variable).
