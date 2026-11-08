---
summary: Copies the drawing file and the files it references (external references, raster images and underlays) into a folder named after the drawing.
related: ETRANSMIT, XREF, SAVE
web: In the web app the files are collected into a ZIP file that is downloaded instead of a folder.
---

## Usage
1. Save the drawing.
2. Run `ARCHIVE`.

The command creates a folder named `<drawing name>_archive` next to the drawing file. It copies the drawing file into it, then every external reference, raster image and underlay file that the drawing uses.

## Notes
- `ETRANSMIT` runs the same command.
- The drawing must have been saved at least once. Otherwise an error asks you to save it first.
- The drawing file is copied as it is on disk. Save first if you want recent changes in the package.
- Referenced files are looked for at their stored path, then relative to the drawing, then by file name next to the drawing.
- The command line reports how many files were copied, and lists any referenced files that could not be found.
- You cannot run the command while a test block window is open.
