---
summary: Controls whether everything the command line shows is also written to a log file.
type: On/Off (0/1)
saved: Program settings
default: 0 (Off)
related: LOGFILEON, LOGFILEOFF, LOGFILEPATH
---

## Values
| Value | Meaning |
|---|---|
| 0 | No log file is written. |
| 1 | Every command line line is written to a text file, with the seconds since the file was started. |

## Notes
- [`LOGFILEON`](LOGFILEON-Command) and [`LOGFILEOFF`](LOGFILEOFF-Command) set it to 1 and 0.
- The file is created by the first line written after the log is turned on. Its path is shown on the command line.
- The file goes to the folder set by [`LOGFILEPATH`](LOGFILEPATH-System-Variable).
- Not available in the web version.
