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
- Lines, lineweights, colors, linetypes, solid fills, hatches, wipeouts, text and raster images are written. The plot stamp is not.
- A raster image that stands upright and whole on the sheet keeps its own pixels. One that is turned, clipped, seen at an angle through a 3D viewport or finer than the plot quality is drawn again, at up to 150, 300 or 600 pixels per inch for a Low, Normal or High plot quality.
- Text in a stroke font (an SHX font) is written as its strokes, so it looks the same in any reader. Text in a TrueType font is written as text under its font's name while that is enough to place it; text with a width factor or an oblique angle, mirrored text, and text seen at an angle through a 3D viewport is written as filled outlines. With **Text as outlines** on (the plot dialog lists it with its SVG options), all TrueType text is written as outlines that need no font. Underlines, overlines and strikethroughs are drawn either way.
- A DWF written here can be attached to a drawing as an underlay with [`DWFATTACH`](DWFATTACH-Command).
- If the plot settings name an output folder, the file goes there directly and no save dialog opens.
- In the save dialog of [`EXPORT`](EXPORT-Command), [`EXPORTPDF`](EXPORTPDF-Command) and [`EXPORTSVG`](EXPORTSVG-Command), a file name ending in `.dwf` writes a DWF too.
