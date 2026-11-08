---
summary: Exports the current layout or model space straight to a PDF or SVG file, using the current plot settings.
related: EXPORTPDF, EXPORTSVG, PLOT, PUBLISH, STLOUT, STEPOUT
web: In the web app the file is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `EXPORT`.
2. In the save dialog, pick a file name and location. The file type follows the last output choice (PDF or SVG).
3. The file is written. The command line shows `Exported: <path>`.

## Notes
- No plot dialog opens. The export uses the settings last chosen in the plot dialog (sheet, scale, plot style, and so on).
- The file type is chosen from the extension: `.svg` writes SVG, anything else writes PDF.
- If the plot settings name an output folder, the file goes there directly and no save dialog opens.
- If "open when done" is set in the plot settings, the file opens in your viewer afterwards.
- If model space has nothing to print, you see `Nothing to plot: model space contains no printable geometry.`
- Use [`EXPORTPDF`](EXPORTPDF-Command) or [`EXPORTSVG`](EXPORTSVG-Command) to force one format, and [`PLOT`](PLOT-Command) for the full plot dialog.
