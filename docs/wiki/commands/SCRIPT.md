---
summary: Runs the commands in a script (.scr) file, one row after another.
related: SCR, FILEDIA
web: In the web app you pick the script file through the browser; typed paths are not read.
---

## Usage
1. Type `SCRIPT`. A file dialog opens for script files (`*.scr`).
2. Pick the file. The command line reports how many commands it will run, and each row runs in turn.

You can also give the path directly: `SCRIPT "C:\scripts\setup.scr"`.

## Notes
- Each row is fed to the command line as if you typed it.
- Rows that start with `#` or `;` are comments and are skipped.
- A blank row presses Enter for the running command.
- While a script runs, file commands such as [`SAVEAS`](SAVEAS-Command) and [`OPEN`](OPEN-Command) ask at the command line instead of opening a dialog.
- The program can also run a script at startup or in batch mode from its own command-line flags (desktop only).
