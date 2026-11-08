# Plotting and Publishing

<img src="images/icons/plot.svg" width="24" alt=""> `PLOT` (**Ctrl+P**) prints the current layout or model view on a printer, or saves it as a PDF.

## The Plot dialog

![The Plot dialog](images/screens/plot-dialog.png)

| Setting | Choices |
|---|---|
| **Printer / plotter** | The printers of your system, or PDF output. The driver's own options are available where the printer reports them. |
| **Paper size** | The sizes the printer offers. |
| **What to plot** | On a layout: the **Layout**. In model space: **Extents**, **Limits**, **Display** or a **Window** you pick. |
| **Plot offset** | Moves the plot on the paper, or **Center the plot**. |
| **Plot scale** | **Fit to paper**, or a scale such as 1:100; **Scale lineweights** with the plot. |
| **Plot style table** | The pen table (CTB or STB) that maps colours or styles to printed colour, width and screening. **Edit…** opens it. |
| **Shaded viewport options** | How 3D views print (as displayed, wireframe, hidden, shaded) and the quality. |
| **Plot options** | Plot in background, object lineweights, plot transparency, plot with plot styles, paper space last, hide paper space objects, merge overlapping lines, plot stamp. |
| **Orientation** | Portrait or landscape, and **Plot upside-down**. |

**Preview** shows the result on the sheet before you print. **Save changes to layout** keeps the settings with the layout.

Other ways to print:

| Command | What it does |
|---|---|
| `QUICKPRINT` (`QP`) | Prints with the current settings, no dialog. |
| `PLOTWINDOW` (`PW`) | Prints a window you pick, at once. |
| `EXPORTPDF` (`EPDF`) | Saves a PDF with the last plot settings, without opening the Plot dialog. |
| `-PLOT` | The command-line version, for scripts. |
| `PRINTERS` | Lists the printers the system reports, for troubleshooting. |

## Page setups

<img src="images/icons/pagesetup.svg" width="24" alt=""> `PAGESETUP` saves plot settings under a name and assigns them to layouts, so every sheet of a set prints the same way. `PSETUPIN` imports page setups from another drawing.

## Plot styles

Plot styles change how objects print without changing the drawing:

- **Colour-dependent** tables (`.ctb`) map each of the 255 index colours to a printed colour, lineweight, linetype and screening. The common case: every colour prints black, at a width chosen by colour.
- **Named** tables (`.stb`) define styles by name, which you assign to layers or objects.

`STYLESMANAGER` lists the plot style tables; `PLOTSTYLEEDITOR` edits them. Turn on **Display plot styles** in the page setup to see them on screen.

## Publishing several sheets

<img src="images/icons/plot.svg" width="24" alt=""> `PUBLISH` (or `PRINTALL`) plots the layouts of the drawing, each with its own page setup, to the printer or into one multi-page PDF. `BACKGROUNDPLOT` controls whether plotting runs in the background while you keep working.

## Sheet sets

<img src="images/icons/sheetset.svg" width="24" alt=""> A sheet set organises the layouts of many drawings as one set of sheets, with subsets, sheet numbers and titles, and set-wide properties that title blocks can show through fields.

| Command | What it does |
|---|---|
| `SHEETSET` (`SSM`) | Opens the Sheet Set Manager. |
| `NEWSHEETSET` | Creates a sheet set. |
| `OPENSHEETSET` | Opens a sheet set file. |

From the Sheet Set Manager you can open sheets, add layouts from drawings, renumber and rename them, and publish the whole set.

## Exporting other formats

- `EXPORTSVG` saves the drawing as SVG.
- `EXPORT` lists every export format.
- 3D exports (STL, STEP) are on [3D Modelling](3D-Modelling#exporting).
