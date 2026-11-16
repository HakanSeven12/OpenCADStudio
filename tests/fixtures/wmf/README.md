# Windows metafile import fixtures

Read by the tests in `src/io/wmf_import.rs`.

Written by two independent writers from small SVG drawings (sizes in
millimetres, y down):

| File | Writer | What it holds |
| --- | --- | --- |
| `shapes.wmf` | Inkscape 1.4.4 | 200 × 120 mm: an open polyline (10, 10) (60, 10) (60, 50) (90, 30), black, 1 mm; a triangle (110, 10) (190, 10) (150, 60) filled `#c00` and outlined `#00c`, 2 mm; a 40 × 30 mm rectangle at (10, 70) filled `#0a0`, outlined black; a circle of radius 20 round (90, 90), outlined `#a0a`, 1.5 mm, no fill; and the text "WMF text" (Arial, 10 mm), which Inkscape writes as filled letter outlines |
| `rings.wmf` | Inkscape 1.4.4 | 100 × 100 mm: a square (10, 10)–(90, 90) with a square hole (30, 30)–(70, 70), filled `#048` (even-odd), outlined black; then three triangles with tips at (20, 80), (55, 80) and (90, 80), filled `#840`, no outline. Both are POLYPOLYGON records |
| `curves.wmf` | Inkscape 1.4.4 | 150 × 80 mm: a cubic Bézier path from (10, 70) to (145, 40), black, 0.8 mm; an ellipse round (40, 40), radii 30 and 15, filled `#ff0`, outlined black. Inkscape flattens both into points |
| `text.wmf` | LibreOffice 26.8.3 | Three texts as TEXTOUT records on the baseline, in 1/100 mm: "Plain" (Arial, 8 mm, black) at (10, 20); "Bold italic" (Times New Roman, bold, italic, 6 mm, `#00c`) at (10, 40); "Tilted" (Arial, 6 mm, `#c00`) at (50, 70), turned 30° counter-clockwise. LibreOffice crops the picture to what it draws and writes the angle as 29.9° |

Inkscape writes units of 1/1200 inch and LibreOffice 1/100 mm; both write
a placeable header, a window origin and extent, and the fill as a brush
with the outline as a pen. Neither writes rectangles, ellipses, arcs, pies,
lines or EXTTEXTOUT records (both draw shapes as polygons), so the tests
build those records themselves; LibreOffice reads them as the tests expect.
