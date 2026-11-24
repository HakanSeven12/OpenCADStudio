---
summary: Opens the Visibility States palette, where the visibility states of a BricsCAD parametric block are shown, switched, ordered and edited in model space of the block's own drawing.
related: VISIBILITYSTATES, VISIBILITYSTATESPANELCLOSE, BVSTATE
ribbon: Parametric tab › Manage panel › Visibility States
web: Not available in the web app.
---

## Usage
1. Open the drawing of a BricsCAD parametric block (the block is the drawing's model space), or any drawing to make its model space one.
2. Run `VISIBILITYSTATESPANELOPEN`, or click **Visibility States** on the Parametric tab. `VISIBILITYSTATESPANELCLOSE` closes the palette.

## The parameters
Each visibility parameter is listed with the number of objects it rules and its states. The state shown is marked.

| Control | What it does |
|---|---|
| A state | Shows that state. |
| ▲ / ▼ | Moves the state up or down. Properties lists the states of a block reference in this order. |
| ▸ (or double-click) | Opens the state's page. |
| **Add state** | Adds a state with every object of the parameter hidden, shows it and opens its page. |
| **+** at the top | Adds a parameter: type its name and its first state, then select the objects it starts with. |

## A state's page
Every object the parameter rules is listed with its type, the layer it is on when shown, and its handle.

| Control | What it does |
|---|---|
| Eye | Shows or hides the object in this state. |
| ✕ | Removes the object from the parameter: its states no longer show or hide it. |
| A row | Selects the object in the drawing. |
| Select objects button | Adds objects: they show in this state only. |
| **Show** | Shows this state. |
| ◂ | Back to the parameters. |

A warning sign marks an object that shows the same in every state of the parameter.

## Notes
- Each change is one undo step, and is stored the way BricsCAD stores it (see [`VISIBILITYSTATES`](VISIBILITYSTATES-Command), which does the same from the command line).
- The palette works in model space only. In the Block Editor or a layout it says so.
- States are not renamed or deleted here yet.
