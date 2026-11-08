---
summary: Controls whether file commands show a file dialog or ask for the file name on the command line.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: FILEASSOC, OPEN, SAVE
---

## Values
| Value | Meaning |
|---|---|
| 0 | File commands ask for the file name on the command line. |
| 1 | File commands show a file dialog. |

## Notes
- It affects commands that have a command-line form, such as opening, saving, attaching references and underlays, and importing files.
- While a script runs, file commands always ask on the command line.
