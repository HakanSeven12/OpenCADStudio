---
summary: Opens the Layer Manager palette, where you create, rename, delete and change layers.
related: LAYER, LAYERSTATE, LAYWALK, LAYMCUR, LAYISO
ribbon: Draw tab › Layers panel
---

## Usage
1. Run `LAYERS`, or click **Layers** on the ribbon.
2. The Layer Manager palette opens.

## Notes
- The palette lists every layer with its name, status, on, freeze, lock, plot, color, linetype, lineweight, transparency and description. Click a cell to change that property.
- Use **New** to add a layer, **Set Current** to make the selected layer current, and **Delete** to remove it. **Rename** changes a layer's name.
- The tree on the left holds filters and groups: all layers, used and unused layers, layers of selected objects, xref layers, and your own property filters.
- The more menu also offers **Layer States**, **Turn all on**, **Thaw all**, **Unlock all** and similar actions.
- To change layers from the command line instead, use [`LAYER`](LAYER-Command) or `-LAYER`.
