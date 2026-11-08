---
summary: Loads keyboard shortcuts from a text file into your shortcut list.
related: CUIEXPORT, SHORTCUTS, ALIASEDIT
ribbon: Manage tab › Customization panel
web: Not available in the web app.
---

## Usage
1. Start the command.
2. At the prompt, type the path of the shortcuts file and press Enter.

You can also give the path directly: `CUIIMPORT C:\path\shortcuts.txt`.

## Notes
- The file is plain text with one shortcut per line: the key, a space, then the command, for example `CTRL+D DIST`.
- Empty lines and lines that start with `#` are skipped.
- Each line adds or replaces the shortcut for that key. Shortcuts not in the file stay as they are.
- The new shortcuts are saved with your settings, and the command line reports how many were loaded.
- A file written by [`CUIEXPORT`](CUIEXPORT-Command) can be loaded back with this command.
