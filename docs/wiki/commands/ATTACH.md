---
summary: Picks a file and attaches it to the drawing as a reference: a drawing, raster image, PDF, DWF, DGN or point cloud.
related: XATTACH, IMAGEATTACH, PDFATTACH, DWFATTACH, DGNATTACH, POINTCLOUDATTACH, XREF
---

## Usage
1. Run `ATTACH`. The Select Reference File dialog opens.
2. Choose a file type and pick the file.
3. Go on with the attach step for that file type:
   - A drawing (DWG or DXF) opens the Attach External Reference dialog.
   - A raster image is placed as an image.
   - A PDF, DWF or DGN file opens the underlay attach dialog, where you choose the page, sheet or model.
   - A point cloud (RCP or RCS) opens the Attach Point Cloud dialog.

You can also type the file name after the command, for example `ATTACH "C:\Plans\site.dwg"`. The file picker is then skipped.

## Command-line form
`-ATTACH` asks for everything on the command line, so it suits scripts:
1. Path to file to attach: type the full path of the file.
2. For a drawing file, specify reference type **Attachment** (`A`) or **Overlay** (`O`). Press Enter for Attachment.

PDF, DWF, DGN and point cloud files go on to the command-line form of their own attach command. Images are placed directly.

## Notes
- The file type you last chose in the picker is remembered.
- A file picked under the "All DGN Files" type is treated as a DGN file.
- With [`FILEDIA`](FILEDIA-System-Variable) set to 0, `ATTACH` works like `-ATTACH`.
- If the file cannot be found, an error is shown.
