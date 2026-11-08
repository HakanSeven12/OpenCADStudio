---
summary: Exports the current layout or model space straight to an SVG file, using the current plot settings.
related: EXPORT, EXPORTPDF, PLOT
web: In the web app the SVG is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `EXPORTSVG`.
2. In the save dialog, pick a file name and location. The dialog proposes an `.svg` name.
3. The SVG is written. The command line shows `Exported: <path>`.

## Notes
- No plot dialog opens. The export uses the settings last chosen in the plot dialog (sheet, scale, plot style, and so on).
- `EXPORTSVG` also makes SVG the output choice for later [`EXPORT`](EXPORT-Command) runs.
- If the plot settings name an output folder, the file goes there directly and no save dialog opens.
