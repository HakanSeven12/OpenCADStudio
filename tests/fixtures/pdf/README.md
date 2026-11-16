# PDF reader fixtures

Read by the tests in `src/scene/model/pdf_vector.rs`, which also build
small PDFs of their own in memory (one content stream each).

| File | Writer | What it holds |
| --- | --- | --- |
| `inkscape.pdf` | Inkscape 1.4.4 (cairo 1.18.2) | One 8 × 6 inch page, from an SVG with y down in points: a polyline (72, 72) (216, 72) (216, 144) in `#c00`, 2 pt; a circle of radius 36 round (360, 108) in `#00c`, 1 pt, which cairo writes as four Béziers; a 144 × 72 rectangle at (72, 216) filled `#0a0` and stroked black, 3 pt, by one `B` operator; "Hello PDF" (Arial, 24 pt) with its baseline at (288, 252); "Tilted" (Times New Roman, 18 pt, `#00c`) at (288, 324), turned 30° counter-clockwise; and a 2 × 2 picture (red, green / blue, white at half opacity), 72 pt across at (432, 288), as an image with a soft mask. Both fonts are embedded as subsets (`PNEMAL+ArialMT`, `ADQHQH+TimesNewRomanPSMT`) |

In the reader's terms (inches, y up from the page's lower-left corner) the
polyline runs (1, 5) (3, 5) (3, 4), the circle is round (5, 4.5) with radius
0.5, the rectangle spans (1, 2)–(3, 3), the texts start at (4, 2.5) and
(4, 1.5), and the picture's lower-left corner is at (6, 1), half an inch
per pixel.
