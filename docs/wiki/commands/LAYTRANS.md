---
summary: Translates the drawing's layers onto the layers of another drawing or standards file, moving every object across.
related: LAYMRG, LAYERS, LAYERSTATE, PURGE
---

## Usage
1. Run `LAYTRANS`. The Layer Translator opens.
2. Under **Translate to**, click **Load…** and choose a DWG, DWS, DWT or DXF file. Its layers become the targets.
3. Select one or more layers under **Translate from** and a target layer under **Translate to**, then click **Map**. Repeat for other layers.
4. Click **Map same** to pair every layer whose name the two sets share.
5. Click **Translate**. Objects on each mapped layer move to its target layer, and the emptied source layers are deleted.

## Options
| Option | What it does |
|---|---|
| **Map** | Maps the selected source layers onto the selected target layer. |
| **Map same** | Maps every source layer that has a target of the same name. |
| **Remove** | Removes the selected mapping from the list. |
| **Save mappings…** | Writes the mapping list to a text file, one `from -> to` line per mapping. |
| **Load mappings…** | Reads a mapping list from such a file. |
| **Force objects to ByLayer** | Sets the color, linetype and lineweight of each moved object to ByLayer, so the target layer's properties show. |
| **Write translation log** | Writes a log of what was moved and skipped to a `.laytrans.log` file beside the drawing. |

## Notes
- If the target layer does not exist in the drawing yet, it is created with the properties it has in the loaded file. An existing layer of that name keeps its own properties.
- Layer `0` and the current layer are never translated. Mappings that cannot run are skipped and reported.
- Objects inside block definitions and block attributes move too.
- `LAYTRANS <file>` loads the file and translates at once. It pairs the drawing's layers with target layers of the same name, ignoring case.
- The translation can be undone in one step.
