---
summary: Moves the selected objects up or down so their centers line up on one horizontal line through the center of the selection.
related: ALIGNLEFT, ALIGNHCENTER, ALIGNRIGHT, ALIGNTOP, ALIGNBOTTOM, ALIGN
ribbon: Draw tab › Modify panel (slide-out)
---

## Usage
1. Select two or more objects.
2. Start the command. The objects move at once, and the command line reports how many were aligned.

## Notes
- Each object moves only along Y, so its center sits on the horizontal center line of the combined extents.
- The command uses the objects' extents in the XY plane. It has no prompts and does not ask for a selection; with fewer than two objects selected it only asks you to select objects.
- A complete group is moved as one unit.
- Objects on locked layers are left out.
- If any selected object is held by a fix constraint, the command stops with a message.
- The move can be undone in one step.
