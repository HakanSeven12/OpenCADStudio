---
summary: Opens the Layer Walk palette, where you pick layers in a list and see only the objects on them.
related: LAYERS, LAYISO, LAYERSTATE, LAYOFF
ribbon: Draw tab › Layers panel
---

## Usage
1. Run `LAYWALK`. The Layer Walk palette opens with every visible layer selected.
2. Click a layer in the list to show only that layer. Ctrl-click adds or removes a layer, and Shift-click selects a range. Double-click a layer to hold it, so it stays shown.
3. Type a pattern such as `*WALL*` in the filter box to list only matching layers.
4. Click **Close** when you are done. Run `LAYWALK` again to close it as well.

## Options
| Option | What it does |
|---|---|
| **Select Objects** | Pick objects in the drawing to select their layers in the list. |
| **Restore on exit** | When checked (the default), closing the palette returns every layer to how it was. |
| **Layer 0 always on** | Keeps layer `0` shown while you walk. |
| **Keep Result** | Closes the palette and keeps the layers shown and hidden as they are now. |
| **Purge** | Deletes the selected layers that no object uses, after you confirm. |

Right-click the list for more: **Hold Selection** and **Release Selection** keep layers shown while you click others, **Select Unreferenced** selects unused layers, **Save Layer State...** saves the current view of layers, **Copy as Filter** and **Save Current Filter** keep the filter pattern, **Zoom to Layer Extents** zooms to the objects on the selected layers, and **Select Objects on Layer** selects them.

## Notes
- While you walk, the layer settings in the drawing do not change. The drawing is not marked as changed and nothing goes to the undo list, unless you keep the result.
- **Purge** can be undone.
