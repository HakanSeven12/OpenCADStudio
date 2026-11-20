---
summary: Exports the current layout or model space straight to a DWF file, using the current plot settings.
related: EXPORT, EXPORTPDF, EXPORTSVG, DWFATTACH, PLOT
web: Not available in the web app.
---

## Usage
1. Type `EXPORTDWF`.
2. In the save dialog, pick a file name and location. The dialog proposes a `.dwf` name.
3. The DWF is written. The command line shows `Exported: <path>`.

## Command-line form
Type a file name after the command to skip the dialog: `EXPORTDWF <file.dwf>`. Add `ALL` to write every paper layout as a sheet of the same file: `EXPORTDWF <file.dwf> ALL`. The command line shows the number of sheets, the file size and the size of the first sheet.

## Notes
- No plot dialog opens. The export uses the settings last chosen in the plot dialog (sheet, scale, plot style, and so on).
- The file is a DWF 6 package with one sheet for each layout exported, named after its layout.
- Lines, lineweights, colors, linetypes, solid fills, hatches, wipeouts and text are written. Raster images and the plot stamp are not.
- A DWF written here can be attached to a drawing as an underlay with [`DWFATTACH`](DWFATTACH-Command).
- If the plot settings name an output folder, the file goes there directly and no save dialog opens.
- In the save dialog of [`EXPORT`](EXPORT-Command), [`EXPORTPDF`](EXPORTPDF-Command) and [`EXPORTSVG`](EXPORTSVG-Command), a file name ending in `.dwf` writes a DWF too.
