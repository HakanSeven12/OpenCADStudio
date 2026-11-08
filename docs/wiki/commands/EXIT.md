---
summary: Closes the program, asking first about drawings with unsaved changes.
related: QUIT, CLOSE, SAVE
---

## Usage
1. Type `EXIT`.
2. If any drawing has unsaved changes, answer the save question for it.
3. The program closes.

## Notes
- Normally `EXIT` goes through the same path as closing the main window, so the usual unsaved-changes dialog appears.
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, or while a script runs, the question is asked on the command line instead: `Save changes to <name>? [Yes/No] <Yes>:`. It is asked once for each drawing with unsaved changes, in turn. This lets a script end the program.
- `EXIT` also works from the Start tab.
- [`QUIT`](QUIT-Command) does the same thing.
