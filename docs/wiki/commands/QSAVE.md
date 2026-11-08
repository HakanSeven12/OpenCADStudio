---
summary: Saves the current drawing to its file, without asking for a name.
related: SAVE, SAVEAS, QNEW
web: In the web app, the drawing is written back to the file it was opened from when the browser allows it; otherwise it is downloaded.
---

## Usage
1. Run the command. The drawing is saved to its file in the version it already has.

## Notes
- A drawing that has never been saved asks for a location. It is saved in the default file format, without the version picker.
- To save under a new name or in another version, use [`SAVEAS`](SAVEAS-Command).
- A session started read-only refuses to save.
- If the drawing has data extraction tables that are out of date, you may be asked to update them first.
