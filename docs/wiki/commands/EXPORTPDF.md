---
summary: Exports the current layout or model space straight to a PDF file, using the current plot settings.
related: EXPORT, EXPORTSVG, PLOT, PUBLISH
web: In the web app the PDF is downloaded by the browser instead of saved to a folder you pick.
---

## Usage
1. Type `EXPORTPDF`, or choose **Export PDF** in the application menu.
2. In the save dialog, pick a file name and location.
3. The PDF is written. The command line shows `Exported: <path>`.

## Notes
- No plot dialog opens. The export uses the settings last chosen in the plot dialog (sheet, scale, plot style, and so on).
- If the plot settings name an output folder, the file goes there directly and no save dialog opens.
- If "open when done" is set in the plot settings, the PDF opens in your viewer afterwards.
- Use [`PLOT`](PLOT-Command) when you want to change the settings first, and [`PUBLISH`](PUBLISH-Command) for several layouts in one PDF.
