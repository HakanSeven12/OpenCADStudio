---
summary: Imports the solids from an ACIS text file (.sat) into the current drawing.
related: IMPORT
---

## Usage
1. Start the command.
2. Choose a `.sat` file in the file dialog.
3. The solids are added to the drawing as one undo step.

## Command-line form
When [`FILEDIA`](FILEDIA-System-Variable) is 0, the command asks `Enter SAT file name:` at the command line instead of opening a dialog. You can also type the file name right after the command, for example `ACISIN part.sat`. This suits scripts.

## Notes
- If the file is not found, the command reports it together with the current folder.
