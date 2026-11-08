---
summary: Freezes or thaws layers in the current layout viewport only, or in all viewports of the layout.
related: LAYER, VPCLIP, SYNCPVIEWPORTS, MSPACE
---

## Usage
1. Switch to a paper layout and enter a viewport (double-click inside it, or use [`MSPACE`](MSPACE-Command)).
2. Run `VPLAYER`. The command line lists the layers already frozen in that viewport.
3. At `VPLAYER  F <layer> = Freeze  |  T <layer> = Thaw  |  Enter = Exit`, type `F` or `T` followed by one or more layer names, for example `F Walls Doors`.
4. Repeat for more layers. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Freeze** (`F`) | Freezes the named layers in the current viewport. |
| **Thaw** (`T`) | Thaws the named layers in the current viewport. |
| **ALL** | Typed after `F` or `T` (`F ALL Walls`), applies the change to every viewport in the layout. |

## Notes
- Layer names are not case-sensitive. Separate several names with spaces.
- The command stays active after each change, so you can freeze and thaw several layers in a row.
- The command line reports how many layers were frozen or thawed, and the Layer panel's viewport columns update at once.
- In model space, or when no viewport is active, the command only prints a message.
- Each change can be undone.
