---
summary: Removes duplicate and overlapping objects from a selection and merges overlapping or end-to-end collinear lines and arcs.
related: PURGE, JOIN, ERASE
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Select objects, or run the command and select them at the Select objects prompt.
2. The Delete Duplicate Objects dialog opens.
3. Set the **Tolerance** and tick the properties to ignore when objects are compared.
4. Choose the options and click **OK**.
5. The command line reports how many objects were checked, removed and modified.

## Options
| Option | What it does |
|---|---|
| **Tolerance** | How far apart two values may be and still count as equal. |
| **Ignore object property** | Properties left out of the comparison: color, layer, linetype, linetype scale, lineweight, thickness, transparency, plot style, material. **Select All** and **Clear All** set them all at once. |
| **Optimize segments within polylines** | Also cleans up duplicate and overlapping segments inside polylines. |
| **Combine co-linear objects that partially overlap** | Merges overlapping collinear objects into one. |
| **Combine co-linear objects when aligned end to end** | Merges collinear objects that touch end to end. |
| **Maintain associative objects** | Leaves objects that are associative, such as associated dimension or hatch boundary objects, untouched. |

## Command-line form
`-OVERKILL` asks for the same settings on the command line, so it suits scripts.
1. Select objects.
2. Enter an option [Done/Ignore/tOlerance/optimize Plines/combine parTial overlap/combine Endtoend/Associativity] <Done>. The current values are shown above the prompt.
3. **Ignore** (`I`) asks for one property to ignore: None, All, Color, LAyer, Ltype, ltScale, LWeight, Thickness, TRansparency, plotSTyle or Material. **tOlerance** (`O`) asks for a value. **optimize Plines** (`P`), **combine parTial overlap** (`T`), **combine Endtoend** (`E`) and **Associativity** (`A`) ask Yes or No.
4. Press Enter (Done) to run the cleanup.

## Notes
- Of two matching objects, the older one is kept.
- Objects on locked layers are not changed.
- The settings are remembered for the next run.
- The cleanup is one undo step.
