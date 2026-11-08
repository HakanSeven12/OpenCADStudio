---
summary: Shows what differs between the current drawing and another DWG or DXF file, and lets you step through each difference.
related: AUDIT, OPEN
web: Not available in the web app.
---

## Usage
1. Type `COMPARE`. A file dialog opens.
2. Pick the DWG or DXF drawing to compare with.
3. The differences are drawn over the current drawing in colour. The prompt shows how many objects were added, removed and modified.
4. Press Enter or type `N` to zoom to the next difference, or type an option.
5. Type `X` or press Esc to end.

## Options
| Option | What it does |
|---|---|
| **Next** (`N`) | Zooms to the next difference. Enter does the same. |
| **Previous** (`P`) | Zooms to the previous difference. |
| **Export** (`E`) | Asks for a report file and writes the list of differences as a CSV file. Press Enter to take the proposed file name. |
| **eXit** (`X`) | Ends the command. |

## Command-line form
`COMPARE <path>` compares with that file at once, without the file dialog.

## Notes
- Objects are matched by what they draw: type, layer and geometry. A copy with the same geometry matches even with a different handle.
- An object that moved or changed shows as removed in one place and added in the other. If both files give it the same handle (two revisions of one file), it shows as modified.
- Default colours: green for added, red for removed, orange for modified. Change them with `COMPARECOLORADDED`, `COMPARECOLORREMOVED` and `COMPARECOLORMODIFIED` (a colour index, a name, or R,G,B).
- `COMPARETOLERANCE` sets how close two points must be, in drawing units, to count as the same.
- The proposed report is named `<drawing>-compare.csv` and sits next to the current drawing.
- While you step through, the prompt shows the number of the current difference, the kind of change and the object.
