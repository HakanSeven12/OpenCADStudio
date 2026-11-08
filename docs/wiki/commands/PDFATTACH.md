---
summary: Attaches one or more pages of a PDF file to the drawing as underlays.
related: PDFCLIP, PDFADJUST, PDFIMPORT, DWFATTACH, DGNATTACH, IMAGEATTACH
ribbon: Insert tab › Reference panel
---

## Usage
1. Start `PDFATTACH` and choose the PDF file in the file dialog.
2. In the Attach dialog, choose the pages, the path type, and the placement: insertion point, scale and rotation. Each can be set in the dialog or picked on screen.
3. Click **Attach**.
4. If you chose to pick any value on screen, answer those prompts.

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
- Several pages attached together are placed at the same point with the same scale and rotation.
- The underlay and its definition are created together, so cancelling leaves nothing behind and one undo removes both.
