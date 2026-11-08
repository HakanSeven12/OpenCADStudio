---
summary: Starts writing everything the command line shows to a log file.
related: LOGFILEOFF
web: Not available in the web app.
---

## Usage
1. Type `LOGFILEON`.
2. The command line shows `LOGFILEMODE = 1` and the full path of the log file.

## Notes
- Each line in the file starts with the seconds since the file was started.
- One new file is made per session. Its name holds the start time and the process number.
- Files go to the folder set in [`LOGFILEPATH`](LOGFILEPATH-System-Variable). When that is empty, they go to a `logs` folder in the settings folder.
- The setting is saved with your profile, so logging stays on next time you start the program. It is the same as setting [`LOGFILEMODE`](LOGFILEMODE-System-Variable) to 1.
- If logging is already on, the command only shows the file path.
