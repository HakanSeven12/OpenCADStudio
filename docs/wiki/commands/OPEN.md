---
summary: Opens an existing DWG, DXF or template drawing in a new tab.
related: NEW, QNEW, SAVEAS, RECOVER, CLOSE
web: In the web app OPEN always uses the browser's file picker; typed file paths are not used.
---

## Usage
1. Run the command. A file dialog opens.
2. Pick a drawing. The file types offered are DWG, DXF, DWT templates, backup and autosave files, and other supported import formats.
3. The drawing opens in a new tab. If it is already open, its tab is shown instead.

## Command-line form
With [`FILEDIA`](FILEDIA-System-Variable) set to 0, or while a script runs, OPEN asks on the command line: Enter name of drawing to open. You can also type `OPEN <path>` in one line. This suits scripts.

## Notes
- If the typed file does not exist, an error is shown.
