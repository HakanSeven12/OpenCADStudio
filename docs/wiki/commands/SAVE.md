---
summary: Saves the current drawing to its file.
related: QSAVE, SAVEAS, SAVEALL
web: In the web app the drawing is written through the browser's file access or downloaded.
---

## Usage
1. Type `SAVE`.
2. If the drawing already has a file, it is saved there in the same format and version.
3. If the drawing has never been saved, a save dialog asks where to put it. The default file format from Options is used.

## Notes
- [`SAVEAS`](SAVEAS-Command) lets you choose a new name, format or version.
- If the drawing holds data extraction tables that are out of date, you may be asked to update them first.
- Saving is refused in a read-only session and in a Test Block window.
