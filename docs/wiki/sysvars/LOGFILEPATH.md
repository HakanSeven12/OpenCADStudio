---
summary: Sets the folder that command line log files are written to.
type: String
saved: Program settings
default: "" (the logs folder inside the settings folder)
related: LOGFILEMODE, LOGFILEON, LOGFILEOFF
---

## Values
Any folder path. Type `.` to go back to the default folder. The folder is created when the first log file is written.

## Notes
- After a change, the next line starts a new log file in the new folder.
- Asked with no value, it shows the folder in use.
