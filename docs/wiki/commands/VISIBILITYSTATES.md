---
summary: Creates and edits the visibility states of a BricsCAD parametric block in model space of the block's own drawing, stored the way BricsCAD stores them.
related: VISIBILITYSTATESPANELOPEN, BVSTATE, BVSHOW, BVHIDE
web: Not available in the web app.
---

## Usage
1. Open the drawing of a BricsCAD parametric block (the block is the drawing's model space), or any drawing to make its model space one, and work in model space.
2. Type `VISIBILITYSTATES`.
3. At "Enter a new parameter or [...]", type the name of a visibility parameter. A name not in the list makes a new parameter.
4. At "Enter a new state for ... or [...]", type the name of a state. A name not in the list makes a new state, in which every object the parameter rules starts hidden.
5. Pick an action (see below), or press Enter for **Add objects**.
6. For every action but **Select state**, select the objects and press Enter. Objects you selected before starting the command are used without asking.

## Actions
| Action | What it does |
|---|---|
| **Add objects** (`A`) | The parameter rules the objects: they show in this state only. Objects it already rules do not change. |
| **Remove objects** (`R`) | The parameter no longer rules the objects, so its states no longer show or hide them. |
| **make objects Visible** (`V`) | The objects show in this state. Objects the parameter did not rule yet show in this state only. |
| **make objects Invisible** (`I`) | The objects are hidden in this state. Objects the parameter did not rule yet show in every other state. |
| **Select state** (`S`) | Makes the state current, showing and hiding the objects the parameters rule. |

## Notes
- This is BricsCAD's own command for its parametric blocks. A drawing saved as DWG keeps the states where BricsCAD finds them: BricsCAD lists and switches them, and can go on editing them with its own `VISIBILITYSTATES`. Where BricsCAD inserted the block into another drawing, a reference to it lists the states in Properties, where you pick one.
- An object ruled by several parameters shows only when every one of them shows it.
- New suppression and object names follow the names the drawing already uses; in a drawing without any they follow the interface language (English or German names).
- The command works in model space only. In the Block Editor it does nothing, because the copies of the block that its references show would keep the old states; edit the block's own drawing instead.
- The drawing must be DWG 2010 or later. Dynamic blocks made in OCS or other CAD programs use [`BVSTATE`](BVSTATE-Command) instead.
- Each run of the command is one undo step.
- The [Visibility States palette](VISIBILITYSTATESPANELOPEN-Command) does the same with the mouse, and also orders the states.
