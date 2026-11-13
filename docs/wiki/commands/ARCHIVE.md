---
summary: Copies the drawing and its external dependencies into a folder named after the drawing.
related: ETRANSMIT, XREF, SAVE
web: The files are collected into a ZIP file that is downloaded instead of a folder.
---

## Usage
1. Save the drawing.
2. Run `ARCHIVE`.

On desktop the command creates `<drawing name>_archive` next to the drawing. The web app downloads `<drawing name>_transmit.zip` instead. [`ETRANSMIT`](ETRANSMIT-Command) uses ZIP output on both platforms.

Both commands collect recursive external references, raster images, PDF/DWF/DGN underlays, point clouds, referenced TrueType/OpenType and SHX fonts, big fonts, SHX shape files, custom plot style tables, and generated linetype definitions. `ETRANSMIT.txt` reports the package contents and any missing or conflicting dependencies.

## Notes
- The drawing must have been saved at least once. Otherwise an error asks you to save it first.
- Desktop copies the drawing as it is on disk. Save first if you want recent changes in the package. Web serializes the current document.
- Referenced files are looked for at their stored path, then relative to their owning drawing, then by file name next to that drawing. Fonts and plot tables can also resolve from their configured support folders.
- Built-in fonts, bundled shapes, and built-in plot tables are omitted. Original `.lin` imports are represented by generated `opencadstudio.lin` definitions when possible; `.fmp` font mappings are not collected.
- Files are flattened without rewriting references. Same-named files with different contents are reported as conflicts, never silently overwritten.
- Browser packages can contain only original bytes available in the current session; unavailable resources are reported. See `ETRANSMIT` for the web-specific limitations.
- You cannot run the command while a test block window is open.
