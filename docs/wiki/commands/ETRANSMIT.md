---
summary: Packages the drawing and its external dependencies into one ZIP file.
related: ARCHIVE, XREF, SAVE
web: The ZIP file is downloaded by the browser.
---

## Usage
1. Save the drawing. The web app serializes the current drawing automatically.
2. Choose the package layout in **Options → Open and Save**:
   - **Preserve source folders** retains source-relative paths inside the root drawing's folder where possible.
   - **Structured by file type** (the default) groups dependencies into `xref/`, `Fonts/`, `Images/`, `Underlays/`, `PointClouds/`, `PlotStyles/`, and `Linetypes/`.
   - **Flat (all files at root)** places every file at the package root.
3. Run `ETRANSMIT` (or its alias `ZIP`). The option is shared with `ARCHIVE`.

On desktop the command creates `<drawing name>_transmit.zip` next to the saved drawing. The web app downloads the same kind of ZIP. [`ARCHIVE`](ARCHIVE-Command) collects the same dependencies but retains its folder output on desktop.

The package contains:

- The drawing and its recursively resolved external references.
- Raster images, PDF/DWF/DGN underlays, and point-cloud source files.
- Referenced TrueType/OpenType fonts, SHX fonts, big fonts, and SHX shape files, including inline MTEXT font overrides. Structured packages place all of these flat in the shared root `Fonts/` folder, as AutoCAD does.
- Custom CTB/STB plot style tables named by layouts or active for plotting.
- A custom external `.lin`/`.dnl` file retained by `CENTERLTYPEFILE`, when present. Embedded linetype definitions are already stored in the drawing and are not exported as invented `.lin` files.
- `ETRANSMIT.txt`, listing packaged files, missing dependencies, and warnings.

Missing or conflicting dependencies produce warnings; they do not prevent the available drawing and resolved dependencies from being exported.

## References and filenames
- Packaged drawings are rewritten copies; the original drawings are never modified. Xrefs remain separate files—they are not bound.
- Xref and other resource paths are relative to their owning drawing. In Structured packages a root drawing uses paths such as `xref/name.dwg` and `Images/name.png`, while one xref references a sibling xref as `name.dwg` and another resource type as `../Images/name.png`.
- Preserve keeps existing relative paths when their targets remain inside the root drawing's source hierarchy. Resources outside that hierarchy fall back to safe root filenames, with references rewritten accordingly.
- Flat and the default Structured font layout use bare font filenames, matching AutoCAD's actual DWG output. Preserve retains relative font paths where possible. OpenCADStudio finds fonts beside a drawing and in its own or ancestor `Fonts/` folders.
- Default Structured plot-table references likewise remain bare names; desktop opening discovers tables beside the owning drawing and in ancestor `PlotStyles/` folders, and resolves preserved relative table references against that drawing.
- Repeated references to one source reuse its package entry. Identical non-drawing bytes can also be deduplicated.
- Different files with the same package name receive a unique suffix such as `_1`; every affected reference is rewritten to the allocated name. Files are never silently overwritten.
- Desktop opening accepts preserved, structured, and flat font/xref layouts, including nested preserved hierarchies and flat references to files in `xref/`.

## Notes
- The drawing must have been saved at least once. Desktop packages the drawing as it is on disk; save first to include recent changes. Web packages the current document.
- Built-in stroke fonts, bundled linetype shapes, built-in plot tables, and the default `acad.lin`/`acadiso.lin` catalogs are not duplicated. `.fmp` font-mapping files are not currently collected.
- On the web, only original bytes loaded or registered in the current browser session can be packaged. Custom SHX/big-font, external linetype, and point-cloud bytes are not currently retained by the web app and are reported as missing. Fetched substitute fonts are never packaged under the requested font's name.
- Other CAD programs may still require packaged fonts, shape files, and plot tables to be installed in their support folders. Check `ETRANSMIT.txt` before treating the package as complete.
- You cannot run the command while a test block window is open.
