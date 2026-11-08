---
summary: Sets how PDFIMPORT builds the imported objects, one bit for each option.
type: Integer (bit-coded)
saved: Not saved
default: 6 (apply lineweights, join segments)
related: PDFIMPORT, PDFIMPORTFILTER, PDFIMPORTLAYERS
---

## Values
Add up the values of the options you want on.

| Value | Meaning |
|---|---|
| 0 | None of the options. |
| 1 | Import the objects as a block. |
| 2 | Apply the lineweights of the PDF. |
| 4 | Join segments that meet into polylines. |
| 8 | Turn solid fills into hatches. |
| 16 | Infer linetypes from dashed lines. |

## Notes
- The PDF Import Settings dialog of [`PDFIMPORT`](PDFIMPORT-Command) sets the same options.
- Accepts a whole number from 0 to 31. The value lasts for the current session only.
