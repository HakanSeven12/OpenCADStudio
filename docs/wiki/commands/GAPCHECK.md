---
summary: Finds small openings between the loose ends of selected curves and lets you step through them, without changing the drawing.
related: OVERKILL, JOIN, BOUNDARY
---

## Usage
1. Select the lines, arcs, open polylines and other open curves to check, or run the command and select them at the Select objects prompt.
2. Type the largest gap to look for, in drawing units, or press Enter for the one shown.
3. The gaps found are marked in the drawing and the view zooms to the first one. The prompt shows its number, its size and the handles of the two objects.
4. Press Enter or type `N` to zoom to the next gap, or type an option.
5. Type `X` or press Esc to end. The markers go and nothing in the drawing has changed.

## Options
| Option | What it does |
|---|---|
| **Next** (`N`) | Zooms to the next gap. Enter does the same. |
| **Previous** (`P`) | Zooms to the previous gap. |
| **Distance** (`D`) | Asks for a new largest gap and checks the selection again. |
| **eXit** (`X`) | Ends the command. |

## Notes
- A loose end is an end of an open curve that no other selected end meets. Ends closer than 0.000001 meet and are not a gap.
- Each loose end is paired with the nearest other loose end within the distance. The two ends of one open polyline count as a gap too.
- Closed curves (circles, closed polylines) have no ends and are skipped, as are rays and construction lines.
- A gap found is a place to look at, not an error: some openings are meant to be there. Close the ones you want with `JOIN`, `FILLET` (radius 0) or `EXTEND`.
- The command does not change the drawing, the undo history or `HPGAPTOL`.
