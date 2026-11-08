---
summary: Holds the default block name offered by the command line form of INSERT.
type: String
saved: Not saved
default: "" (none)
related: INSERT, BLOCKNAVIGATE
---

## Values
Any block name. Type `.` to clear it.

## Notes
- Each time you insert a block, the variable takes that block's name.
- `-INSERT` offers it as the default only when the current drawing defines a block of that name.
- Each open drawing has its own value. It is lost when the drawing is closed.
