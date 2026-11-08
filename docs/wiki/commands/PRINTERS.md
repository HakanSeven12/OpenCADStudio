---
summary: Lists on the command line what the system reports about printing: the default printer, every printer it finds, and how a plot reaches them.
related: PLOT, PRINTALL, PAGESETUP
web: Not available in the web app; it only reports that printing is not available.
---

## Usage
- Type `PRINTERS` to list the default printer and all printers the system reports, or the reason they could not be listed.
- Type `PRINTERS` followed by a printer name to list the sheets that printer reports, with their margins, and its default sheet.

## Notes
- On Windows, the report also says whether plots go through a PDF application or straight to the printer.
- Asking a single printer about its sheets can take a moment for an offline network printer.
- The report is plain text, so you can copy it into a bug report.
