---
summary: Adds to the selection every object of the same type that also matches the chosen properties, such as layer and name.
related: SELSIM, SELECTALL, QSELECT, FILTER
---

## Usage
1. Select one or more sample objects, then type `SELECTSIMILAR`. Or start the command with nothing selected, select the samples at the prompt `Select objects or [SEttings]:` and press Enter.
2. Every object in the current space with the same type as a sample, and the same chosen properties, is added to the selection.
3. The command line reports how many objects were added.

## Options
| Option | What it does |
|---|---|
| **Settings** (`SE`) | Opens the Select Similar Settings window. Under **Similar Based On**, tick the properties that must match: Color, Layer, Linetype, Linetype scale, Lineweight, Plot style, Object style, Name. |

## Notes
- The object type always has to match. By default the layer and the name (for example the block name) must match too.
- "Object style" compares the text, dimension or other style of the object.
- The settings are saved and used the next time. You can also type `SELECTSIMILAR SETTINGS` to open the window.
- The sample objects stay selected.
