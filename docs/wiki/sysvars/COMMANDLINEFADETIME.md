---
summary: Sets how long history lines stay visible above the command line before they fade out.
type: Integer
saved: Program settings
default: 3000 (milliseconds)
related: CLIPROMPTLINES, COMMANDLINEOPACITY, COMMANDLINE
---

## Values
An integer from 0 to 60000, in milliseconds. 0 hides the temporary history lines.

## Notes
- Type `COMMANDLINEFADETIME` to be asked for a value, or use [`SETVAR`](SETVAR-Command).
