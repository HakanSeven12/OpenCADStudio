---
summary: Removes unused blocks, layers, linetypes and styles, empty groups, zero-length geometry and empty text from the drawing.
related: FINDNONPURGEABLE, OVERKILL, AUDIT
ribbon: Manage tab › Cleanup panel
---

## Usage
1. Run the command. The Purge dialog lists everything that can be removed, grouped by kind with a count: blocks, layers, linetypes, text, dimension, multileader, table and multiline styles, empty groups, materials, zero-length geometry and empty text objects.
2. Tick the items you want to remove. Ticking a group ticks all of its items.
3. Click **Purge Checked Items**, or **Purge All** to remove everything listed.
4. The dialog stays open and shows what is left. Click **Close** when you are done.

If nothing can be removed, the dialog says so.

## Options
| Option | What it does |
|---|---|
| **Purge nested items** | Also removes what becomes unused once the chosen items are gone, such as a layer used only inside a purged block. On by default. |

## Command-line form
`-PURGE` asks on the command line, which suits scripts:
1. `Enter type of unused objects to purge [Blocks/DEtailviewstyles/Dimstyles/Groups/LAyers/LTypes/MAterials/MLeaderstyles/Mlinestyles/Plotstyles/SHapes/textSTyles/Tablestyles/Visualstyles/Regapps/Zero-length geometry/Empty text objects/Orphaned data/All]:`
2. `Enter name(s) to purge <*>:` type names or wildcards, separated by commas, or press Enter for all.
3. `Verify each name to be purged? [Yes/No] <Y>:` the items are removed without further questions either way.

**DEtailviewstyles**, **Plotstyles**, **SHapes**, **Visualstyles**, **Regapps** and **Orphaned data** are not purged: data the program carries without reading it can refer to them, so removing one could damage the file. They report that, so a script keeps running.

You can also type `PURGE` with a type and an optional name list, for example `PURGE LAYERS A*,TMP`. The types are `ALL`, `BLOCKS`, `LAYERS`, `LINETYPES`, `STYLES`, `DIMSTYLES`, `MLEADERSTYLES`, `TABLESTYLES`, `MLINESTYLES`, `GROUPS`, `MATERIALS`, `ZEROLENGTH` and `EMPTYTEXT`.

**All** removes every unused named item but no geometry: zero-length geometry and empty text go only when you choose them, by type or in the dialog.

## Notes
- Only items that are not used anywhere in the drawing are offered. The current layer and styles, the Standard styles, the Global, ByLayer and ByBlock materials and items that belong to an external reference always stay.
- A dimension style with overrides (`NAME$0`) goes after its overrides.
- Zero-length geometry means lines, arcs, circles and lightweight polylines of no length; empty text means text and multiline text with nothing but spaces. Only objects in model space and on sheets are offered, and objects on locked layers stay.
- Removing a block can free the layers, linetypes, styles and nested blocks it used. One purge repeats until nothing more can be removed.
- Draw-order data left behind by removed blocks is cleaned up too.
- The command line reports how many items of each kind were removed.
- A purge is one undo step.
- PURGE and -PURGE cannot be used inside the block editor.
