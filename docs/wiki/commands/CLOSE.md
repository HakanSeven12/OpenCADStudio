---
summary: Closes the current drawing tab.
related: SAVE, QUIT, OPEN, NEW
---

## Usage
1. Run `CLOSE`.
2. If the drawing has unsaved changes, you are asked whether to save them first.
3. The tab closes.

## Notes
- When file dialogs are turned off with [`FILEDIA`](FILEDIA-System-Variable) set to 0, or a script is running, the save question is asked on the command line: "Save changes to <name>? [Yes/No] <Yes>:". **Yes** saves, then closes; **No** closes without saving.
