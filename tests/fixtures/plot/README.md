Page-setup fixtures saved by a commercial CAD application (2016 release, DXF
AC1027 and the matching DWG). Provided by Panos for the plotting parity track;
the drawing holds one 10 m line (`$INSUNITS` = 6, metres) and three paper
layouts, each with a viewport of that line and its dimension:

| Layout | Device | Media (`4`) | `44`×`45` | `73` | `72` | `142`/`143` | `147` | `75` |
|---|---|---|---|---|---|---|---|---|
| `A4 1-100` | `DWG To PDF.pc3` | `ISO_A4_(297.00_x_210.00_MM)` | 297 × 210 | 0 | mm | 1 / 1 | 25.4 | 16 |
| `A3 - plotter` | system printer `HPB602DE (HP OfficeJet Pro 7740 series)` | `A3` | 297.01 × 419.99 | 1 | mm | 1 / 1 | 25.4 | 16 |
| `ARCH D - PLOTTER` | the application's second PDF plotter configuration (`… PDF (High Quality Print).pc3`) | `ARCH_D_(36.00_x_24.00_Inches)` | 914.4 × 609.6 | 0 | in | 1 / 25.4 | 0.03937 | 16 |

What the file establishes (see `src/app/update/file.rs`, `plot_settings_from_dialog`):

- A landscape sheet is written either as the driver's own landscape medium
  with rotation 0 (the PDF driver lists both orientations) or as a portrait
  medium with rotation 90 (system printers) — rotation is relative to the
  medium's orientation, and the medium's exact driver dimensions are kept.
- Margins (`40`–`43`, mm) belong to the medium's own edges: the PDF driver's
  A4 has 5.79375 left/right and 17.79375 bottom/top. Layout limits (`10`/`20`)
  put the sheet at minus the *displayed* margins: at rotation 90 the medium's
  top margin (20.02) becomes the layout's left one, its left margin (5.00) the
  layout's bottom one.
- Plot flags `70` = 672 on every layout: draw viewports first, print
  lineweights, plot with plot styles — the "use standard scale" bit stays clear
  even with a standard code in `75`; the model tab (fit to paper) sets it.
- `147` is `142/143` × 25.4 on millimetre page setups and `142/143` itself on
  inch ones.
- An inch page setup over a millimetre paper space stores "1:1" as
  `1 in = 25.4 units` (`75` = 16 kept).
