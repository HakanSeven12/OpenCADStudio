---
summary: Attaches a DGN file to the drawing as an underlay.
related: DGNADJUST, DGNCLIP, DGNIMPORT, PDFATTACH, DWFATTACH
ribbon: Insert tab › Reference panel
---

## Usage
1. Choose a DGN file in the file dialog.
2. In the attach dialog, pick the model, the conversion units and the path type. For the insertion point, scale and rotation, type values or choose to specify them on screen.
3. Click **Attach**.
4. If you chose to specify values on screen, pick them in the drawing.

## Command-line form
`-DGNATTACH` asks everything on the command line, which suits scripts:
1. `Path to DGN file to attach:` Type the file path.
2. `Enter name of model or [?]:` Type a model name, press Enter for the first model, or type `?` to list the models.
3. `Specify conversion units [Master/Sub] <Master>:` **Master** (`M`) uses the file's master units; **Sub** (`S`) scales the underlay so that sub units become drawing units.
4. `Specify insertion point:` Pick a point.
5. `Specify scale factor or [Unit]:` Type a scale, or type `U` to choose a unit (MM, Centimeter, Meter, Kilometer, Inch, Foot, Yard, MILe, Unitless).
6. `Specify rotation:` Type an angle or press Enter.

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, `DGNATTACH` uses the command-line form too.
- If you type a file name after `DGNATTACH`, the file dialog is skipped and the command line asks for the rest.
- The underlay is a reference to the file; its elements are not copied into the drawing. Use [`DGNIMPORT`](DGNIMPORT-Command) to bring them in as objects.
