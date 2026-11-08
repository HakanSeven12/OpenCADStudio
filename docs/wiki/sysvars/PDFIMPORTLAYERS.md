---
summary: Sets which layers PDFIMPORT puts the imported objects on.
type: Integer
saved: Not saved
default: 0 (PDF layers)
related: PDFIMPORT, PDFIMPORTMODE, PDFIMPORTFILTER
---

## Values
| Value | Meaning |
|---|---|
| 0 | The PDF's own layers, named `PDF_` and the layer name. Content outside them goes to layers for each kind of object. |
| 1 | One layer for each kind of object: geometry, solid fills, text and images. |
| 2 | The current layer. |

## Notes
- The PDF Import Settings dialog of [`PDFIMPORT`](PDFIMPORT-Command) sets the same option.
- The value lasts for the current session only.
