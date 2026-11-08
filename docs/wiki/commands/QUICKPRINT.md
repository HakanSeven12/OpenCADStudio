---
summary: Prints the selected objects to a PDF file with no dialog, framing their extents.
related: PLOT, PAGESETUP, EXPORTPDF
---

## Usage
1. Select the objects to print. The prompt shows how many are selected.
2. Press Enter. A PDF file is written.

## Notes
- The plot area is the box around the selected objects, with a small margin.
- The PDF is saved next to the drawing, named after the drawing with a time stamp added. An unsaved drawing writes to the system's temporary folder, named `drawing`.
- The command works only in model space.
- Pressing Enter with nothing selected ends the command.
- If the selection has nothing printable, an error is shown and no file is written.
