---
summary: Sets the user coordinate system (UCS) you draw in: move its origin, rotate it, align it to a face, object or the view, and save or restore it by name.
related: UCSICON, PLAN, RENAME, CREATESKETCH
---
## Usage
1. At `UCS specify new origin or [Face/OBject/World/View/3Point/Z/X/Y/Origin/Save/Delete] or name:`, do one of these:
   - Pick three points: the new origin, a point on the positive X axis, and a point in the positive XY plane. Press Enter after the first point to move only the origin, or after the second to set the origin and X axis.
   - Type an option.
   - Type the name of a saved UCS to make it current.
2. Answer the prompt of the option you chose.

Press Enter at the first prompt to list the current UCS and the saved ones.

## Options
| Option | What it does |
|---|---|
| **Face** (`F`) | Click a planar face of a 3D solid. The UCS lies on that face, with the origin at the point you clicked. |
| **OBject** (`OB`) | Click a planar object (for example a closed polyline or a circle). The UCS takes its plane. |
| **World** (`W`) | Returns to the world coordinate system. |
| **View** (`V`) | Turns the XY plane to face the screen, keeping the origin. |
| **3Point** (`3P`) | Sets the UCS from three points, as described above. |
| **Z** / **X** / **Y** | Rotates the current UCS about that axis by the angle you type, in degrees. |
| **Origin** (`O`) | Moves the origin to the point you pick or type, keeping the axes. |
| **Save** (`S`) | Saves the current UCS under the name you type. |
| **Delete** (`D`) | Deletes the saved UCS with the name you type. |
| **?** (`?`) | Lists the current UCS and the saved ones. |

## Notes
- You can type an option and its value on one line, for example `UCS Z 90`, `UCS ORIGIN 10,20,0` or `UCS SAVE Front`. This suits scripts.
- Saved UCSs are stored in the drawing. Use [`RENAME`](RENAME-Command) to rename one.
- The UCS icon, grid, ortho and the drawing plane of new objects follow the current UCS.
- The current UCS is saved with the drawing, for model space and for each layout viewport.
- You can undo a UCS change.
