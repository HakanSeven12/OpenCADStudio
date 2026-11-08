---
summary: Sets which kinds of PDF content PDFIMPORT leaves out, one bit for each kind.
type: Integer (bit-coded)
saved: Not saved
default: 8 (raster images are left out)
related: PDFIMPORT, PDFIMPORTMODE, PDFIMPORTLAYERS, PDFIMPORTIMAGEPATH
---

## Values
Add up the values of the kinds you want left out.

| Value | Meaning |
|---|---|
| 0 | Everything is imported. |
| 1 | Vector geometry is left out. |
| 2 | Text is left out. |
| 4 | Solid fills are left out. |
| 8 | Raster images are left out. |

## Notes
- The PDF Import Settings dialog of [`PDFIMPORT`](PDFIMPORT-Command) sets the same options.
- Accepts a whole number from 0 to 15. The value lasts for the current session only.
