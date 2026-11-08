---
summary: In the Block Editor, adds an action to a parameter of a dynamic block and chooses the objects the action changes.
related: BPARAMETER, BACTIONSET, BASSOCIATE, BACTIONBAR, BLOOKUPTABLE, BEDIT
---

## Usage
1. Enter action type: **Move**, **Scale**, **Stretch**, **Polar Stretch**, **Rotate**, **Flip**, **Array** or **Lookup**.
2. Select a parameter endpoint or enter its name. Only parameters that suit the action can be chosen.
3. For **Stretch** and **Polar Stretch**, specify the first corner and the opposite corner of the action frame, or type **CPolygon** to draw a polygon frame.
4. Select objects for the action, then press Enter.
5. For **Polar Stretch**, select the objects that should only rotate, then press Enter.
6. For **Array**, specify the distance between columns, and for an XY parameter also the distance between rows. For a polar parameter, specify the distance between items.
7. Specify action marker location.

You can also give the type on the same line, for example `BACTION MOVE`.

## Options
| Option | What it does |
|---|---|
| **CPolygon** | Draws the stretch frame as a polygon. Pick the vertices, type **Undo** to remove the last one, and **Close** or Enter to finish. |
| **Base** / **End** | For Polar Stretch, chooses which point of the parameter the frame belongs to. |
| **Multiplier** | For Move, Stretch and Polar Stretch, sets a factor applied to the parameter change. |
| **Offset** | For Move, Stretch and Polar Stretch, sets an angle offset for the action. |

## Notes
- The command works only while a block is open in the Block Editor, and the block needs a parameter that suits the action. Otherwise an error tells you what is missing.
- **Lookup** adds a lookup action and goes on to the lookup table. See [`BLOOKUPTABLE`](BLOOKUPTABLE-Command).
- `BACTIONTOOL` runs the same command. [`BARRAY`](BARRAY-Command) starts it with the Array type already chosen.
