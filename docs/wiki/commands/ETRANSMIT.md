---
summary: Packages the drawing and its external dependencies into one ZIP file.
related: ARCHIVE, XREF, SAVE
web: The ZIP file is downloaded by the browser.
---

## Usage
1. Save the drawing. The web app serializes the current drawing automatically.
2. Run `ETRANSMIT` (or its alias `ZIP`).

On desktop the command creates `<drawing name>_transmit.zip` next to the saved drawing. The web app downloads the same kind of ZIP. [`ARCHIVE`](ARCHIVE-Command) collects the same dependencies but retains its folder output on desktop.

The package contains:

- The drawing and its recursively resolved external references.
- Raster images, PDF/DWF/DGN underlays, and point-cloud source files.
- Referenced TrueType/OpenType fonts, SHX fonts, big fonts, and SHX shape files, including inline MTEXT font overrides.
- Custom CTB/STB plot style tables named by layouts or active for plotting.
- `opencadstudio.lin`, generated from the drawing's current linetype table when it has writable definitions.
- `ETRANSMIT.txt`, listing packaged files, missing or conflicting dependencies, and linetype warnings.

## Notes
- The drawing must have been saved at least once. Desktop packages the drawing as it is on disk; save first to include recent changes. Web packages the current document.
- Built-in stroke fonts, bundled linetype shapes, and built-in plot tables are not duplicated.
- References are collected flat and are not bound or rewritten. Repeated references to identical bytes are deduplicated. Different files with the same package name are reported as conflicts instead of overwriting one another.
- Original `.lin` import paths are not retained in a drawing, so the package contains generated definitions rather than copies of the original source files. `.fmp` font-mapping files are not currently collected.
- On the web, only original bytes loaded or registered in the current browser session can be packaged. Custom SHX/big-font and point-cloud bytes are not currently retained by the web app and are reported as missing. Fetched substitute fonts are never packaged under the requested font's name.
- Install packaged fonts, shape files, and plot tables in the receiving program's support folders if it does not find them beside the drawing automatically. Check `ETRANSMIT.txt` before treating the package as complete.
- You cannot run the command while a test block window is open.
