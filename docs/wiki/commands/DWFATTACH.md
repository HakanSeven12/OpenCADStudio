---
summary: Attaches a sheet of a DWF or DWFx file to the drawing as an underlay.
related: DWFADJUST, DWFCLIP, PDFATTACH, DGNATTACH, ATTACH
ribbon: Insert tab › Reference panel
---

## Usage
1. Run `DWFATTACH`. A file picker opens. Choose a DWF or DWFx file.
2. The attach dialog opens. It shows the sheets in the file as thumbnails, with the file name, a **Browse...** button and the number of sheets.
3. Pick the sheet to attach. The dialog shows its size.
4. Under **Placement**, set the **Insertion point**, **Scale** and **Rotation**. Turn on **On screen** for a value to give it on the command line instead.
5. Choose the **Path type**: **Full path**, **Relative path** or **No path**. **File details** shows where the file was found and the path that will be saved.
6. Click **Attach**. Answer any prompts left for **On screen** values.

## Command-line form
`-DWFATTACH` works without the dialog, so it suits scripts. You can also type a file name after `DWFATTACH`. The prompts are:

1. **Path to DWF file to attach**: type the file path. `.dwf` is added when the name has no extension.
2. **Enter name of sheet or [?]**: type a sheet name, or press Enter for the first one. Type `?` to list the sheets (wildcards allowed).
3. **Specify insertion point**.
4. **Specify scale factor or [Unit]**: type a scale, or `U` to choose the unit the sheet is measured in (MM, Centimeter, Meter, Kilometer, Inch, Foot, Yard, MILe, Unitless). The base size is shown after you choose a unit.
5. **Specify rotation**: the last rotation you used is the default.

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, `DWFATTACH` uses the command-line form.
- The **Attach Underlay** drop-down on the ribbon also offers PDF, image, DGN and point cloud files.
- After attaching, use [`DWFADJUST`](DWFADJUST-Command) for fade and contrast and [`DWFCLIP`](DWFCLIP-Command) to clip.
