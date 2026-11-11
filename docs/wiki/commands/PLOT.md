---
summary: Opens the Plot dialog to print the current layout or model view, or to save it as a PDF or SVG file.
related: PAGESETUP, PLOTWINDOW, PRINTALL, QUICKPRINT, PLOTSTYLE, PLOTSTYLEEDITOR, PRINTERS, EXPORTPDF
web: The web app has no system printers; it always saves the plot as a PDF file, and -PLOT opens the Plot dialog instead of asking on the command line.
---

## Usage
1. Start `PLOT`. The Plot dialog opens.
2. Choose the printer or output: the system default printer, another printer, **PDF**, or **SVG**.
3. Set the paper size, orientation, plot area, plot scale, plot offset, plot style table and other plot options.
4. Click **Preview** to check the result, if you want.
5. Click **Print**, **Export PDF** or **Export SVG**.

## Options
| Setting | What it does |
|---|---|
| **Page setups** | Pick, create, copy, rename, delete or import named page setups, and make one current for the layout. |
| **Printer / plotter** | Where the plot goes. **Properties…** edits the driver options of a system printer. |
| **Paper size** | The sheet. **Custom…** adds your own sheet size with its printable margins. |
| **Orientation** | Portrait or landscape. |
| **Plot area** | **Layout** (in a layout), **Extents**, **Limits**, **Display**, **Window**, or a saved view. **Pick…** lets you pick a window in the drawing. |
| **Plot scale** | **Fit to paper**, a standard scale, or a custom scale. **Scale lineweights** scales lineweights with the plot. |
| **Plot offset** | X and Y offset on the paper, or **Center the plot**. |
| **Plot style table (pen assignments)** | The CTB table used for colours, lineweights and screening. **Edit…** opens the plot style editor. |
| **Shaded viewport options** | Shade plot setting and quality (dots per inch) for model space. |
| **Plot options** | **Plot in background**, **Object lineweights**, **Plot transparency**, **Plot with plot styles**, **Paper space last**, **Hide paperspace objects**, **Plot stamp**, **Plot upside-down**, **Merge overlapping lines**, and **Save changes to layout**. |
| **Copies** | How many copies to print. |

For PDF and SVG output, you also choose how the file is named, which folder it goes to, and whether to open it when done. SVG output additionally offers **Non-scaling strokes** under **Plot options**: off by default, or on to keep stroke widths constant under downstream SVG transforms, which is useful for web-map layers and does not change the CAD plot scale. SVG also offers **Text as outlines**: off by default to retain searchable text, or on to draw font-independent glyph paths. PDF never sees either flag.

## Command-line form
`-PLOT` asks for the plot settings on the command line instead of opening the dialog, so it suits scripts:

1. **Detailed plot configuration? [Yes/No] <No>**
2. **Enter a layout name**, the current layout by default.
3. **Enter an output device name <PDF>:** a device name containing `PDF` or `SVG` writes a file of that type.
4. With a detailed configuration: paper size, paper units, orientation, plot upside down, plot area (**Display**, **Extents**, **Limits** or **Window** in model space; **Layout** instead of **Limits** in a layout), window corners for **Window**, plot scale or **Fit**, plot offset or **Center**, plot styles and the table name (`.` for none), and lineweights.
5. **Enter file name:** a name is proposed from the drawing and layout.
6. **Proceed with plot [Yes/No] <Y>:**

The page setup name, paper units, shade plot, paper-space order and save-to-page-setup answers are accepted so scripts run, but they do not change the plot. `-PLOT` only writes to a file; to print on a printer, use `PLOT`.

## Notes
- If the drawing has data extraction tables that are out of date, you are asked about them before the dialog opens.
- `PLOT` cannot be used inside the block editor.
- [`PLOTWINDOW`](PLOTWINDOW-Command) sets the plot window by picking two corners.
