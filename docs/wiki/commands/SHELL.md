---
summary: Hollows a 3D solid into a thin-walled shell of a given thickness, with optional open faces.
related: SOLIDEDIT, FILLETEDGE, CHAMFEREDGE, SUBTRACT
ribbon: Model tab › Edges panel
---

## Usage
1. Select a 3D solid. If you selected one solid before you start the command, this step is skipped.
2. Click the faces you want to remove (to leave open), or press Enter to keep all faces closed.
3. Enter the shell offset distance, or press Enter to use the default shown.

## Options
| Option | What it does |
|---|---|
| **Undo** (`U`) | Undoes the last face pick. |
| **Add** (`A`) | Switches to picking faces to add back (keep closed). |
| **Remove** (`R`) | Switches back to picking faces to remove. |
| **ALL** (`ALL`) | Removes all faces, or adds all faces back, depending on the current step. |

## Notes
- The distance must be a nonzero number.
- Shelling works on boxes, cylinders and spheres. Other solids report an error and are not changed.
- If the distance removes all material, the command says so; use a smaller distance or add a face back.
- Solids on locked layers are not offered as a preselection.
