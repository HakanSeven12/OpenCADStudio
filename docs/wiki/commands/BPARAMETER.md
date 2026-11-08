---
summary: In the Block Editor, adds a parameter to the block to make it dynamic.
related: BACTION, BASSOCIATE, BVSTATE, BLOOKUPTABLE, BGRIPSET, BAUTHORPALETTE, BPARAMETERS
---

## Usage
1. Run the command in the Block Editor.
2. Enter the parameter type.
3. Follow the prompts for that type: its points, its label location, and its other settings.

## Options
| Option | What it does |
|---|---|
| **Alignment** (`A`) | Places an alignment base point and its tangent direction. |
| **Base** (`B`) | Places or moves the insertion anchor of the dynamic block. |
| **Point** (`O`) | Places a point parameter and its position label. |
| **Linear** (`L`) | Places a base point, an end point and a distance label. |
| **Polar** (`P`) | Places a base point, an end point, and distance and angle labels. |
| **XY** (`X`) | Places a base point and a corner, with horizontal and vertical distance labels. |
| **Rotation** (`R`) | Places a center, a radius and a default rotation angle. |
| **Flip** (`F`) | Places a reflection line and its flip-state label. |
| **Visibility** (`V`) | Places a visibility parameter. Manage its states with [`BVSTATE`](BVSTATE-Command). |
| **Lookup** (`LOOKUP`) | Places a Lookup parameter. Edit its table with [`BLOOKUPTABLE`](BLOOKUPTABLE-Command). |

## Notes
- The command works only in the Block Editor.
- A parameter alone does not change geometry. Add an action with [`BACTION`](BACTION-Command), or use the Parameter Sets tab of the Authoring Palettes to place a parameter with its action in one go.
- Each new parameter is offered a default name that is not yet used in the block.
- Linear, Polar, XY, Rotation and similar types also ask for a name, label, description, whether the property shows in the Properties palette, the number of grips, and an optional value set (increment, minimum and maximum, or a list of values).
- The same parameters are on the Parameters tab of the Authoring Palettes ([`BAUTHORPALETTE`](BAUTHORPALETTE-Command)).
