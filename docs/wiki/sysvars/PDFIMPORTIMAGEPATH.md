---
summary: Sets the folder that PDFIMPORT saves extracted raster images to.
type: String
saved: Not saved
default: PDF Images (a folder next to the PDF file)
related: PDFIMPORT, PDFIMPORTFILTER
---

## Values
A folder name or path. A relative name is taken from the folder of the PDF file; an absolute path is used as given. Type `.` to save the images in the folder of the PDF file itself.

## Notes
- The images are saved as PNG files and attached as image references.
- Images are imported only when [`PDFIMPORTFILTER`](PDFIMPORTFILTER-System-Variable) does not leave them out.
- The value lasts for the current session only.
