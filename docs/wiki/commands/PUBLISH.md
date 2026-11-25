---
summary: Same as `PRINTALL`: outputs several paper layouts at once to one multi-page PDF or to the printer. With a sheet list (.dsd) it publishes the sheets of many drawings into one PDF with a bookmark per sheet.
related: PRINTALL, PLOT, PAGESETUP, SCRIPT
web: Print opens a PDF of the chosen layouts in a browser tab, for the browser's print dialog; there are no system printers. Sheet lists are not available.
---

`PUBLISH` opens the same dialog as [`PRINTALL`](PRINTALL-Command). See that page for the steps.

## Typed forms
- `PUBLISH <file.pdf> [sheet names…]` publishes layouts of the current drawing into one PDF without the dialog: the named ones, or all paper layouts.
- `PUBLISH <list.dsd>` (also `-PUBLISH`) publishes a sheet list: the sheets of as many drawings as it names, in its order. Put the path in quotes if it contains spaces.

## Sheet lists
A sheet list is the `.dsd` drawing set description file that the PUBLISH command of other CAD programs saves. Each `[DWF6Sheet:<name>]` section is one sheet:
- `DWG=` the drawing (a relative path starts from the sheet list's folder),
- `Layout=` the layout, or `Model` for model space,
- `Setup=` optionally a named page setup of the drawing to plot with instead of the layout's own.

`[Target]` says where the sheets go:
- `Type=6` – one PDF, the file named by `DWF=`,
- `Type=5` – a PDF per sheet, named after the sheet, in the folder named by `OUT=`.

`[PdfOptions] CreateBookmarks=FALSE` leaves out the bookmarks. Other keys are ignored.

## Notes
- Every sheet plots with its own page setup, as it does from its own drawing.
- The PDF has a bookmark per sheet, titled with the sheet's name, that opens the whole page. Layers of the same name are one layer of the PDF across all sheets.
- The drawings are opened one at a time and closed again without saving; they leave no lock file. A drawing that is already open is published as it is and stays open.
- The PDF is written while the drawings are opened, so a set of thousands of sheets does not have to fit in memory. It replaces the file only when it is complete.
- Each sheet is reported on the command line with its paper size, or with the reason it could not be published; the other sheets are published all the same.
- Unattended: `OpenCADStudio --batch publish.scr` with the row `PUBLISH "set.dsd"` needs no open drawing. Plot transparency follows [`PLOTTRANSPARENCYOVERRIDE`](PLOTTRANSPARENCYOVERRIDE-System-Variable).
