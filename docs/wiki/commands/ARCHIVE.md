---
summary: Copies the drawing and its external dependencies into a folder named after the drawing.
related: ETRANSMIT, XREF, SAVE
web: The files are collected into a ZIP file that is downloaded instead of a folder.
---

## Usage
1. Save the drawing.
2. Choose **Preserve source folders**, **Structured by file type** (the default), or **Flat (all files at root)** in **Options → Open and Save**. Structured uses `xref/`, `Fonts/`, `Images/`, `Underlays/`, `PointClouds/`, `PlotStyles/`, and `Linetypes/`.
3. Run `ARCHIVE`.

On desktop the command creates `<drawing name>_archive` next to the drawing. The web app downloads `<drawing name>_transmit.zip` instead. [`ETRANSMIT`](ETRANSMIT-Command) uses ZIP output on both platforms and the same layout option.

Both commands collect recursive external references, raster images, PDF/DWF/DGN underlays, point clouds, referenced TrueType/OpenType and SHX fonts, big fonts, SHX shape files, custom plot style tables, and a custom external `CENTERLTYPEFILE` source. `ETRANSMIT.txt` reports the package contents and any missing dependencies.

## Notes
- The drawing must have been saved at least once. Otherwise an error asks you to save it first.
- Desktop copies and rewrites the last-saved drawing. Save first if you want recent changes in the package. Web serializes the current document.
- Referenced files are looked for at their stored path, then relative to their owning drawing, then by file name next to that drawing. Fonts and plot tables can also resolve from their configured support folders.
- Preserve retains source-relative paths inside the root drawing's folder where possible; resources outside it fall back to safe root filenames. Structured groups files by type, and Flat places every file at the root. Packaged references are rewritten to match; default Structured font and plot-table references remain bare filenames. The originals are not modified.
- Different files with colliding names receive unique suffixes and their references are retargeted. Nothing is silently overwritten.
- Embedded linetypes stay only in the drawing; no generated `.lin` is emitted. Only an external `.lin`/`.dnl` retained by `CENTERLTYPEFILE` is packaged. Default catalogs, built-in fonts/shapes, and built-in plot tables are omitted. `.fmp` font mappings are not collected.
- Desktop opening supports preserved, structured, and flat layouts. Browser packages can contain only original bytes available in the current session; unavailable resources are reported. See `ETRANSMIT` for web-specific limitations.
- You cannot run the command while a test block window is open.
