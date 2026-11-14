---
summary: Attaches one or more pages of a PDF file to the drawing as underlays.
related: PDFCLIP, PDFADJUST, PDFIMPORT, DWFATTACH, DGNATTACH, IMAGEATTACH
ribbon: Insert tab › Reference panel
---

## Usage
1. Start `PDFATTACH` and choose the PDF file in the file dialog.
2. In the Attach dialog, choose the pages, the path type, and the placement: insertion point, scale and rotation. Each can be set in the dialog or picked on screen.
3. Click **Attach**.
4. If you chose to pick any manual value on screen, answer those prompts. When **Place automatically (GeoPDF)** is checked, **Attach** instead completes immediately without insertion, scale, or rotation prompts.

## Command-line form
`-PDFATTACH` asks everything on the command line, so it suits scripts:

1. **Path to PDF file to attach:** type the file path.
2. **Enter page number or [?] <1>:** type a page number. Type `?` to list the pages of the file.
3. **Specify insertion point:**
4. **Specify scale factor or [Unit] <1>:** type a scale, or **Unit** (`U`) to scale from a unit: MM, Centimeter, Meter, Kilometer, Inch, Foot, Yard, MILe or Unitless.
5. **Specify rotation:** the rotation of the last attached underlay is offered as the default.

You can also type the file path right after the command name, for example `-PDFATTACH plan.pdf`.

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, or a file path follows the command name, `PDFATTACH` uses the command-line prompts too.
- Without usable GeoPDF metadata, several pages attached together are placed at the same point with the same scale and rotation.
- In root model space (not paper space or the block editor), when the drawing has a `GEODATA` CRS (explicit `INSUNITS` for a projected CRS; a geographic CRS places without units), an interactive attach uses a page's unambiguous ISO `/VP` or legacy `/LGIDict` georeferencing. Each page is evaluated independently. The PDF and drawing must have proven equivalent CRSs; no cross-datum reprojection is performed.
- `-PDFATTACH` itself always asks the manual placement prompts; the **Place automatically (GeoPDF)** shortcut only exists in the Attach dialog.
- **Place automatically (GeoPDF)** is checked by default when every selected page can use automatic placement. Clear it to use the manual placement controls instead. While checked, those controls are disabled.
- If any selected page cannot use automatic placement, the checkbox is cleared and disabled, and the dialog uses manual placement for the selection.
- Automatic placement supports affine rotation and non-uniform X/Y scale, including EPSG:5514 east/north coordinates. Registration coordinates the PDF stores at single precision still fit; a registration point that genuinely moved keeps the manually entered values. Missing metadata, unknown units, multiple frames, conflicting ISO and legacy dictionaries, PCSM, shear, reflection, paper space, and non-Z-normal underlays also keep the manually entered values. The node-graph **PDF Underlay** node is always manual.
- The underlay and its definition are created together, so cancelling leaves nothing behind and one undo removes both.
