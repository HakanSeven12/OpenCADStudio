---
summary: Stops writing the command line to the log file and shows where the file is.
related: LOGFILEON
web: Not available in the web app.
---

## Usage
1. Type `LOGFILEOFF`.
2. The command line shows the path of the log file that was being written, and logging stops.

## Notes
- It is the same as setting [`LOGFILEMODE`](LOGFILEMODE-System-Variable) to 0. The setting is saved with your profile.
- If logging is already off, nothing happens.
- When you turn logging on again, a new file is started.
